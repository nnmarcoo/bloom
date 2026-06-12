use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, BlendState,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::{
    modifiers::{
        Modifier,
        gpu::{ModUniforms, TileInfo, build_segment_uniforms},
    },
    wgpu::{gpu, tiled_source::TiledSource, view_pipeline::tile_ndc_culled},
};

struct CombinedPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
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
        Self { pipeline, bgl }
    }

    fn run(&self, encoder: &mut CommandEncoder, bind_group: &BindGroup, dst: &TextureView) {
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
        pass.set_bind_group(0, bind_group, &[]);
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

struct ScratchTarget {
    _tex: Texture,
    view: TextureView,
    width: u32,
    height: u32,
}

impl ScratchTarget {
    fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        let tex = gpu::texture_2d(
            device,
            width,
            height,
            format,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            Some("modifier-scratch"),
        );
        let view = tex.create_view(&Default::default());
        Self {
            _tex: tex,
            view,
            width,
            height,
        }
    }
}

fn segment_modifiers(modifiers: &[Modifier]) -> Vec<Vec<&Modifier>> {
    let mut segments: Vec<Vec<&Modifier>> = Vec::new();
    let mut current: Vec<&Modifier> = Vec::new();
    for m in modifiers.iter().filter(|m| m.has_visible_effect()) {
        if m.kind.is_resampling() && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        current.push(m);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

pub struct ModifierPipeline {
    tile_outputs: Vec<Option<TileOutput>>,
    tile_display_bgs_linear: Vec<Option<BindGroup>>,
    tile_display_bgs_nearest: Vec<Option<BindGroup>>,

    scratch_a: Option<ScratchTarget>,
    scratch_b: Option<ScratchTarget>,

    uniform_pool: Vec<iced::wgpu::Buffer>,
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
            scratch_a: None,
            scratch_b: None,
            uniform_pool: Vec::new(),
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

    fn ensure_scratch(&mut self, device: &Device, w: u32, h: u32) {
        let stale =
            |s: &Option<ScratchTarget>| s.as_ref().is_none_or(|t| t.width != w || t.height != h);
        if stale(&self.scratch_a) {
            self.scratch_a = Some(ScratchTarget::new(device, self.format, w, h));
        }
        if stale(&self.scratch_b) {
            self.scratch_b = Some(ScratchTarget::new(device, self.format, w, h));
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        modifiers: &[Modifier],
        dirty: bool,
    ) {
        let n_tiles = source.tiles.len();

        self.tile_outputs.resize_with(n_tiles, || None);
        self.tile_display_bgs_linear.resize_with(n_tiles, || None);
        self.tile_display_bgs_nearest.resize_with(n_tiles, || None);

        if dirty {
            for o in self.tile_outputs.iter_mut().flatten() {
                o.valid = false;
            }
        }

        let physical_scale = source.physical_scale;
        let downscale = physical_scale < 1.0;

        let mut segments: Option<Vec<Vec<&Modifier>>> = None;
        let mut encoder: Option<CommandEncoder> = None;
        let mut pool_used = 0usize;

        for ti in 0..n_tiles {
            let tile = &source.tiles[ti];

            if tile_ndc_culled(tile.last_ndc_rect) {
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

                let segs = segments.get_or_insert_with(|| segment_modifiers(modifiers));
                let n_seg = segs.len();
                if n_seg > 1 {
                    self.ensure_scratch(device, eff_w, eff_h);
                }

                let combined = &self.combined;
                let sampler = &self.trilinear_sampler;
                let output_view = &self.tile_outputs[ti].as_ref().unwrap().view;
                let mut prev: &TextureView = &tile.source_view;
                for (k, seg) in segs.iter().enumerate() {
                    let out: &TextureView = if k == n_seg - 1 {
                        output_view
                    } else if k % 2 == 0 {
                        &self.scratch_a.as_ref().unwrap().view
                    } else {
                        &self.scratch_b.as_ref().unwrap().view
                    };

                    let uniforms = build_segment_uniforms(seg, &tile_info);
                    if pool_used == self.uniform_pool.len() {
                        self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                            device,
                            Some("combined-modifiers-uniform"),
                        ));
                    }
                    let buffer = &self.uniform_pool[pool_used];
                    pool_used += 1;
                    gpu::write_uniform(queue, buffer, &uniforms);
                    let bg = gpu::standard_bind_group(
                        device,
                        &combined.bgl,
                        buffer,
                        prev,
                        sampler,
                        Some("combined-modifiers-bg"),
                    );

                    let enc = encoder.get_or_insert_with(|| {
                        device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                            label: Some("modifier-pipeline-encoder"),
                        })
                    });
                    combined.run(enc, &bg, out);

                    prev = out;
                }

                self.tile_outputs[ti].as_mut().unwrap().valid = true;
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

        if let Some(encoder) = encoder {
            queue.submit([encoder.finish()]);
        }
    }
}
