use glam::vec2;
use iced::wgpu::{
    BindGroup, Color, CommandEncoderDescriptor, Device, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, Sampler, StoreOp, Texture, TextureFormat,
    TextureUsages, TextureView,
};

use crate::wgpu::{
    gpu,
    passes::{
        display::DisplayPass,
        lanczos::{LanczosPass, LanczosUniforms},
    },
    tiled_source::TiledSource,
};

// LOD scale ratios for the Lanczos pyramid.
pub const LOD_SCALES: &[f32] = &[1.0, 0.5, 0.25, 0.125, 0.0625];

pub fn compute_lanczos_mip_count(w: u32, h: u32) -> u32 {
    let mut count = 1u32;
    let mut pw = w;
    let mut ph = h;
    for _ in 1..LOD_SCALES.len() {
        let nw = ((pw as f32) * 0.5).round().max(1.0) as u32;
        let nh = ((ph as f32) * 0.5).round().max(1.0) as u32;
        if nw < 4 || nh < 4 {
            break;
        }
        count += 1;
        pw = nw;
        ph = nh;
    }
    count
}

// Per-tile result kept until all tiles are done, then flushed into `TiledSource`.
struct TileResult {
    texture: Texture,
    bind_group: BindGroup,
}

pub struct LanczosBuildState {
    results: Vec<Option<TileResult>>,
    next_tile: usize,
}

impl LanczosBuildState {
    pub fn new(tile_count: usize) -> Self {
        Self {
            results: (0..tile_count).map(|_| None).collect(),
            next_tile: 0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.next_tile >= self.results.len()
    }

    // Process one tile per call. Returns `true` when all tiles are done,
    // at which point results have been written into `source.tiles`.
    pub fn step(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &mut TiledSource,
        display: &DisplayPass,
        lanczos_h: &LanczosPass,
        lanczos_v: &LanczosPass,
        trilinear_sampler: &Sampler,
    ) -> bool {
        if self.is_done() {
            return true;
        }

        let tile_idx = self.next_tile;
        let tile = &source.tiles[tile_idx];
        let tw = tile.width;
        let th = tile.height;
        let label = format!("tile[{}]:lanczos", tile_idx);
        let mip_count = compute_lanczos_mip_count(tw, th);

        let (mip_widths, mip_heights): (Vec<u32>, Vec<u32>) = (0..mip_count)
            .map(|m| {
                let s = LOD_SCALES[m as usize];
                (
                    ((tw as f32 * s).round() as u32).max(1),
                    ((th as f32 * s).round() as u32).max(1),
                )
            })
            .unzip();

        let mip_texture = gpu::texture_2d_mipmapped(
            device,
            tw,
            th,
            mip_count,
            TextureFormat::Rgba16Float,
            TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            Some(&format!("{label}:mip-pyramid")),
        );

        let mip_all_view = mip_texture.create_view(&iced::wgpu::TextureViewDescriptor {
            label: Some(&format!("{label}:mip-all")),
            format: None,
            dimension: Some(iced::wgpu::TextureViewDimension::D2),
            usage: None,
            aspect: iced::wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(mip_count),
            base_array_layer: 0,
            array_layer_count: None,
        });

        let level_views: Vec<TextureView> = (0..mip_count)
            .map(|m| {
                mip_texture.create_view(&iced::wgpu::TextureViewDescriptor {
                    label: Some(&format!("{label}:mip{m}")),
                    base_mip_level: m,
                    mip_level_count: Some(1),
                    dimension: Some(iced::wgpu::TextureViewDimension::D2),
                    ..Default::default()
                })
            })
            .collect();

        let source_view = tile._source_texture.create_view(&Default::default());

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some(&format!("{label}:encoder")),
        });

        // Mip 0: blit source (Rgba8Unorm → Rgba16Float) using 1:1 Lanczos.
        {
            let w = mip_widths[0];
            let h = mip_heights[0];
            let inter_tex = gpu::texture_2d(
                device,
                w,
                h,
                TextureFormat::Rgba16Float,
                TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                Some(&format!("{label}:mip0:inter")),
            );
            let inter_view = inter_tex.create_view(&Default::default());

            let h_buf = gpu::uniform_buffer::<LanczosUniforms>(device, None);
            gpu::write_uniform(
                queue,
                &h_buf,
                &LanczosUniforms {
                    src_size: vec2(w as f32, h as f32),
                    scale: 1.0,
                    _pad: 0.0,
                },
            );
            encode_lanczos_pass(
                &mut encoder,
                &inter_view,
                &lanczos_h.create_bind_group(device, &h_buf, &source_view, trilinear_sampler, None),
                lanczos_h,
                &format!("{label}:mip0:h"),
            );

            let v_buf = gpu::uniform_buffer::<LanczosUniforms>(device, None);
            gpu::write_uniform(
                queue,
                &v_buf,
                &LanczosUniforms {
                    src_size: vec2(w as f32, h as f32),
                    scale: 1.0,
                    _pad: 0.0,
                },
            );
            encode_lanczos_pass(
                &mut encoder,
                &level_views[0],
                &lanczos_v.create_bind_group(device, &v_buf, &inter_view, trilinear_sampler, None),
                lanczos_v,
                &format!("{label}:mip0:v"),
            );
        }

        // Mips 1..N: downsample each level from the previous.
        for mip in 1..mip_count as usize {
            let prev_w = mip_widths[mip - 1];
            let prev_h = mip_heights[mip - 1];
            let out_w = mip_widths[mip];
            let out_h = mip_heights[mip];

            let inter_tex = gpu::texture_2d(
                device,
                out_w,
                prev_h,
                TextureFormat::Rgba16Float,
                TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                Some(&format!("{label}:mip{mip}:inter")),
            );
            let inter_view = inter_tex.create_view(&Default::default());

            let h_buf = gpu::uniform_buffer::<LanczosUniforms>(device, None);
            gpu::write_uniform(
                queue,
                &h_buf,
                &LanczosUniforms {
                    src_size: vec2(prev_w as f32, prev_h as f32),
                    scale: out_w as f32 / prev_w as f32,
                    _pad: 0.0,
                },
            );
            encode_lanczos_pass(
                &mut encoder,
                &inter_view,
                &lanczos_h.create_bind_group(
                    device,
                    &h_buf,
                    &level_views[mip - 1],
                    trilinear_sampler,
                    None,
                ),
                lanczos_h,
                &format!("{label}:mip{mip}:h"),
            );

            let v_buf = gpu::uniform_buffer::<LanczosUniforms>(device, None);
            gpu::write_uniform(
                queue,
                &v_buf,
                &LanczosUniforms {
                    src_size: vec2(out_w as f32, prev_h as f32),
                    scale: out_h as f32 / prev_h as f32,
                    _pad: 0.0,
                },
            );
            encode_lanczos_pass(
                &mut encoder,
                &level_views[mip],
                &lanczos_v.create_bind_group(device, &v_buf, &inter_view, trilinear_sampler, None),
                lanczos_v,
                &format!("{label}:mip{mip}:v"),
            );
        }

        queue.submit(std::iter::once(encoder.finish()));

        let tile = &source.tiles[tile_idx];
        let bind_group = display.create_bind_group(
            device,
            &tile.uniform_buffer,
            &mip_all_view,
            trilinear_sampler,
            Some(&format!("{label}:lanczos-bg")),
        );

        self.results[tile_idx] = Some(TileResult {
            texture: mip_texture,
            bind_group,
        });
        self.next_tile += 1;

        // If fully done, flush results into tiles.
        if self.is_done() {
            for (i, tile) in source.tiles.iter_mut().enumerate() {
                let result = self.results[i].take().unwrap();
                tile._lanczos_texture = Some(result.texture);
                tile.lanczos_bind_group = Some(result.bind_group);
            }
            return true;
        }

        false
    }
}

fn encode_lanczos_pass(
    encoder: &mut iced::wgpu::CommandEncoder,
    target: &TextureView,
    bind_group: &iced::wgpu::BindGroup,
    pass: &LanczosPass,
    label: &str,
) {
    let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::TRANSPARENT),
                store: StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.draw(&mut rpass, bind_group);
}
