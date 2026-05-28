use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, BlendState,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::{
    modifiers::{
        Modifier,
        gpu::{ModUniforms, TileInfo, build_mod_uniforms},
    },
    wgpu::{
        gpu,
        tiled_source::{Tile, TiledSource},
    },
};

struct CombinedPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    uniform_buffer: iced::wgpu::Buffer,
}

impl CombinedPass {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("combined-modifiers-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("shaders/combined_modifiers.wgsl"),
            Some("combined-modifiers-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let uniform_buffer =
            gpu::uniform_buffer::<ModUniforms>(device, Some("combined-modifiers-uniform"));
        Self {
            pipeline,
            bgl,
            uniform_buffer,
        }
    }

    fn write_uniforms(&self, queue: &Queue, uniforms: &ModUniforms) {
        gpu::write_uniform(queue, &self.uniform_buffer, uniforms);
    }

    fn run(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        src: &TextureView,
        dst: &TextureView,
        sampler: &Sampler,
    ) {
        let bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            &self.uniform_buffer,
            src,
            sampler,
            Some("combined-modifiers-bg"),
        );
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("combined-modifiers-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: dst,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(iced::wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..4, 0..1);
    }
}

struct TileOutput {
    _tex: Texture,
    view: TextureView,
    valid: bool,
    width: u32,
    height: u32,
}

pub struct ModifierPipeline {
    tile_outputs: Vec<Option<TileOutput>>,
    tile_display_bgs_linear: Vec<Option<BindGroup>>,
    tile_display_bgs_nearest: Vec<Option<BindGroup>>,

    combined: CombinedPass,
    display_bgl: BindGroupLayout,
    trilinear_sampler: Sampler,
    linear_sampler: Sampler,
    nearest_sampler: Sampler,

    format: TextureFormat,
    pub width: u32,
    pub height: u32,
}

impl ModifierPipeline {
    pub fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        let display_bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("modifier-display-bgl"),
        );

        let trilinear_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-trilinear-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            mipmap_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let linear_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-linear-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-nearest-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Nearest,
            min_filter: iced::wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            tile_outputs: Vec::new(),
            tile_display_bgs_linear: Vec::new(),
            tile_display_bgs_nearest: Vec::new(),
            combined: CombinedPass::new(device, format),
            display_bgl,
            trilinear_sampler,
            linear_sampler,
            nearest_sampler,
            format,
            width,
            height,
        }
    }

    pub fn tile_display_bg(&self, i: usize, nearest: bool) -> Option<&BindGroup> {
        if nearest {
            self.tile_display_bgs_nearest.get(i)?.as_ref()
        } else {
            self.tile_display_bgs_linear.get(i)?.as_ref()
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        modifiers: &[Modifier],
        dirty_from: Option<usize>,
    ) {
        let n_tiles = source.tiles.len();

        self.tile_outputs.resize_with(n_tiles, || None);
        self.tile_display_bgs_linear.resize_with(n_tiles, || None);
        self.tile_display_bgs_nearest.resize_with(n_tiles, || None);

        if dirty_from.is_some() {
            for o in self.tile_outputs.iter_mut().flatten() {
                o.valid = false;
            }
        }

        let physical_scale = source.physical_scale;
        let downscale = physical_scale < 1.0;

        for ti in 0..n_tiles {
            let tile = &source.tiles[ti];

            if !is_tile_visible(tile) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }

            let eff_w = if downscale {
                ((tile.width as f32 * physical_scale).ceil() as u32).max(1)
            } else {
                tile.width
            };
            let eff_h = if downscale {
                ((tile.height as f32 * physical_scale).ceil() as u32).max(1)
            } else {
                tile.height
            };

            let needs_alloc = self.tile_outputs[ti]
                .as_ref()
                .is_none_or(|o| o.width != eff_w || o.height != eff_h);

            if needs_alloc {
                let tex = gpu::texture_2d(
                    device,
                    eff_w,
                    eff_h,
                    self.format,
                    TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    Some(&format!("modifier-tile{ti}-output")),
                );
                let view = tex.create_view(&Default::default());
                self.tile_outputs[ti] = Some(TileOutput {
                    _tex: tex,
                    view,
                    valid: false,
                    width: eff_w,
                    height: eff_h,
                });
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }

            let needs_reprocess = !self.tile_outputs[ti].as_ref().unwrap().valid;
            let needs_bg_rebuild = self.tile_display_bgs_linear[ti].is_none() || needs_reprocess;

            if !needs_bg_rebuild {
                continue;
            }

            if needs_reprocess {
                let tile_info = TileInfo {
                    tile_x: tile.x,
                    tile_y: tile.y,
                    tile_w: tile.width,
                    tile_h: tile.height,
                    full_w: source.full_width,
                    full_h: source.full_height,
                };

                let uniforms = build_mod_uniforms(modifiers, &tile_info);
                self.combined.write_uniforms(queue, &uniforms);

                let mut encoder =
                    device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                        label: Some(&format!("modifier-tile{ti}-encoder")),
                    });

                let output_view = &self.tile_outputs[ti].as_ref().unwrap().view;

                self.combined.run(
                    device,
                    &mut encoder,
                    &tile.source_view,
                    output_view,
                    &self.trilinear_sampler,
                );

                self.tile_outputs[ti].as_mut().unwrap().valid = true;
                queue.submit([encoder.finish()]);
            }

            let output_view = &self.tile_outputs[ti].as_ref().unwrap().view;

            let make_bg = |sampler: &Sampler, label: &str| {
                device.create_bind_group(&BindGroupDescriptor {
                    label: Some(label),
                    layout: &self.display_bgl,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: tile.uniform_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(output_view),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: BindingResource::Sampler(sampler),
                        },
                    ],
                })
            };

            self.tile_display_bgs_linear[ti] = Some(make_bg(
                &self.linear_sampler,
                &format!("modifier-tile{ti}-display-linear"),
            ));
            self.tile_display_bgs_nearest[ti] = Some(make_bg(
                &self.nearest_sampler,
                &format!("modifier-tile{ti}-display-nearest"),
            ));
        }
    }
}

fn is_tile_visible(tile: &Tile) -> bool {
    match tile.last_ndc_rect {
        None => true,
        Some((min_ndc, max_ndc)) => {
            !(max_ndc.x < -1.0 || min_ndc.x > 1.0 || max_ndc.y < -1.0 || min_ndc.y > 1.0)
        }
    }
}
