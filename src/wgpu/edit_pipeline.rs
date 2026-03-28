use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
    ColorTargetState, ColorWrites, CommandEncoder, Device, Extent3d, FilterMode, FragmentState,
    LoadOp, MultisampleState, Operations, PipelineCompilationOptions, PipelineLayoutDescriptor,
    PrimitiveState, PrimitiveTopology, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension, VertexState,
};

use crate::edit::nodes::{CurveChannel, EditNode, EditOp};
use crate::wgpu::gpu;

const EDIT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const LUT_SIZE: u32 = 256;

// ── Uniform structs (must match shader layouts) ───────────────────────────────

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct BrightnessContrastUniforms {
    brightness: f32,
    contrast: f32,
    _pad: [f32; 2],
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct HueSaturationUniforms {
    hue: f32,
    saturation: f32,
    lightness: f32,
    _pad: f32,
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct CropUniforms {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

// ── Per-pass pipeline helpers ─────────────────────────────────────────────────

/// A simple fullscreen pass that reads one texture + one uniform buffer.
struct SimplePass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
}

impl SimplePass {
    fn new(device: &Device, shader_src: &str, label: &str) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::FRAGMENT,
            Some(&format!("{label}-bgl")),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            shader_src,
            Some(&format!("{label}-pipeline")),
            PrimitiveTopology::TriangleStrip,
            EDIT_FORMAT,
            BlendState::REPLACE,
            &bgl,
        );
        Self { pipeline, bgl }
    }

    fn make_bind_group(
        &self,
        device: &Device,
        uniform: &Buffer,
        view: &TextureView,
        sampler: &Sampler,
        label: &str,
    ) -> BindGroup {
        gpu::standard_bind_group(device, &self.bgl, uniform, view, sampler, Some(label))
    }

    fn run(
        &self,
        encoder: &mut CommandEncoder,
        bind_group: &BindGroup,
        target: &TextureView,
        label: &str,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
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

// ── Curves pass (special: 4 LUT textures + input texture, no uniform) ────────

struct CurvesPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
}

impl CurvesPass {
    fn new(device: &Device) -> Self {
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("curves-bgl"),
            entries: &[
                // binding 0: input texture
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 1: input sampler
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                // bindings 2–5: LUT textures (rgb, r, g, b)
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 6: LUT sampler
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("curves-shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "shaders/edit_curves.wgsl"
            ))),
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("curves-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("curves-pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: EDIT_FORMAT,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            cache: None,
            multiview: None,
        });

        Self { pipeline, bgl }
    }

    fn make_bind_group(
        &self,
        device: &Device,
        input_view: &TextureView,
        input_sampler: &Sampler,
        lut_rgb: &TextureView,
        lut_r: &TextureView,
        lut_g: &TextureView,
        lut_b: &TextureView,
        lut_sampler: &Sampler,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("curves-bg"),
            layout: &self.bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(input_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(input_sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(lut_rgb),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(lut_r),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(lut_g),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(lut_b),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::Sampler(lut_sampler),
                },
            ],
        })
    }

    fn run(&self, encoder: &mut CommandEncoder, bind_group: &BindGroup, target: &TextureView) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("curves-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
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

// ── LUT helpers ───────────────────────────────────────────────────────────────

/// Evaluate a piecewise-linear curve defined by control points at `t` (0.0–1.0).
fn eval_curve(points: &[[f32; 2]], t: f32) -> f32 {
    if points.len() < 2 {
        return t;
    }
    if t <= points[0][0] {
        return points[0][1];
    }
    if t >= points[points.len() - 1][0] {
        return points[points.len() - 1][1];
    }
    for w in points.windows(2) {
        let (x0, y0) = (w[0][0], w[0][1]);
        let (x1, y1) = (w[1][0], w[1][1]);
        if t >= x0 && t <= x1 {
            let alpha = (t - x0) / (x1 - x0);
            return y0 + alpha * (y1 - y0);
        }
    }
    t
}

fn build_lut(channel: &CurveChannel) -> Vec<u8> {
    (0..LUT_SIZE)
        .flat_map(|i| {
            let t = i as f32 / (LUT_SIZE - 1) as f32;
            let v = (eval_curve(&channel.points, t) * 255.0).clamp(0.0, 255.0) as u8;
            [v, 0, 0, 255]
        })
        .collect()
}

fn upload_lut(device: &Device, queue: &Queue, data: &[u8]) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("curves-lut"),
        size: Extent3d {
            width: LUT_SIZE,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        data,
        iced::wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LUT_SIZE * 4),
            rows_per_image: None,
        },
        Extent3d {
            width: LUT_SIZE,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

// ── EditPipeline ──────────────────────────────────────────────────────────────

pub struct EditPipeline {
    brightness_contrast: SimplePass,
    hue_saturation: SimplePass,
    curves: CurvesPass,
    crop: SimplePass,

    /// Blit pipeline compiled for EDIT_FORMAT (Rgba16Float) — used to seed ping-pong.
    seed_pipeline: RenderPipeline,
    seed_bgl: BindGroupLayout,

    linear_sampler: Sampler,
    lut_sampler: Sampler,

    /// Ping-pong textures. Index 0 and 1 alternate as source/target.
    ping_pong: Option<PingPong>,
}

struct PingPong {
    textures: [Texture; 2],
    views: [TextureView; 2],
    width: u32,
    height: u32,
    /// Which slot holds the current output (starts as 0 after first pass).
    current: usize,
}

impl PingPong {
    fn new(device: &Device, width: u32, height: u32) -> Self {
        let make = |label: &str| {
            let t = gpu::texture_2d(
                device,
                width,
                height,
                EDIT_FORMAT,
                TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                Some(label),
            );
            let v = t.create_view(&TextureViewDescriptor::default());
            (t, v)
        };
        let (t0, v0) = make("edit-ping");
        let (t1, v1) = make("edit-pong");
        Self {
            textures: [t0, t1],
            views: [v0, v1],
            width,
            height,
            current: 0,
        }
    }

    fn source(&self) -> &TextureView {
        &self.views[1 - self.current]
    }

    fn target(&self) -> &TextureView {
        &self.views[self.current]
    }

    fn swap(&mut self) {
        self.current = 1 - self.current;
    }

    /// Output is the last target that was written (opposite of current source slot).
    fn output(&self) -> &TextureView {
        &self.views[1 - self.current]
    }
}

impl EditPipeline {
    pub fn new(device: &Device) -> Self {
        let brightness_contrast = SimplePass::new(
            device,
            include_str!("shaders/edit_brightness_contrast.wgsl"),
            "brightness-contrast",
        );
        let hue_saturation = SimplePass::new(
            device,
            include_str!("shaders/edit_hue_saturation.wgsl"),
            "hue-saturation",
        );
        let curves = CurvesPass::new(device);
        let crop = SimplePass::new(
            device,
            include_str!("shaders/edit_crop.wgsl"),
            "crop",
        );

        let (seed_pipeline, seed_bgl) = gpu::blit_pipeline(device, EDIT_FORMAT);

        let linear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("edit-linear-sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("edit-lut-sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        Self {
            brightness_contrast,
            hue_saturation,
            curves,
            crop,
            seed_pipeline,
            seed_bgl,
            linear_sampler,
            lut_sampler,
            ping_pong: None,
        }
    }

    /// Call when a new image is loaded. Reallocates ping-pong textures at image resolution.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        match &self.ping_pong {
            Some(pp) if pp.width == width && pp.height == height => {}
            _ => self.ping_pong = Some(PingPong::new(device, width, height)),
        }
    }

    /// Blit the source texture into ping-pong slot 0 so passes can read from it.
    /// `source_view` should be the Rgba8Unorm tile from TiledSource (or a resolved composite).
    fn seed(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &Device,
        source_view: &TextureView,
    ) {
        let pp = match &mut self.ping_pong {
            Some(p) => p,
            None => return,
        };
        // Always write into slot (1 - current) so first pass reads from it.
        let seed_slot = 1 - pp.current;
        let target = &pp.views[seed_slot];

        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("edit-seed-bg"),
            layout: &self.seed_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("edit-seed-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
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
        pass.set_pipeline(&self.seed_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..4, 0..1);
    }

    /// Run all enabled nodes and return the final output view.
    /// Returns `None` if there are no ping-pong textures (no image loaded).
    pub fn run(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &Device,
        queue: &Queue,
        source_view: &TextureView,
        nodes: &[EditNode],
    ) -> Option<&TextureView> {
        self.seed(encoder, device, source_view);

        let enabled: Vec<&EditNode> = nodes.iter().filter(|n| n.enabled).collect();
        if enabled.is_empty() {
            return self.ping_pong.as_ref().map(|pp| pp.source());
        }

        for node in enabled {
            let pp = self.ping_pong.as_mut()?;
            let src = pp.source();
            let dst = pp.target();

            match &node.op {
                EditOp::BrightnessContrast(p) => {
                    let uniform = gpu::uniform_buffer::<BrightnessContrastUniforms>(
                        device,
                        Some("bc-uniform"),
                    );
                    gpu::write_uniform(
                        queue,
                        &uniform,
                        &BrightnessContrastUniforms {
                            brightness: p.brightness,
                            contrast: p.contrast,
                            _pad: [0.0; 2],
                        },
                    );
                    let bg = self.brightness_contrast.make_bind_group(
                        device,
                        &uniform,
                        src,
                        &self.linear_sampler,
                        "bc-bg",
                    );
                    self.brightness_contrast.run(encoder, &bg, dst, "bc-pass");
                }

                EditOp::HueSaturation(p) => {
                    let uniform = gpu::uniform_buffer::<HueSaturationUniforms>(
                        device,
                        Some("hs-uniform"),
                    );
                    gpu::write_uniform(
                        queue,
                        &uniform,
                        &HueSaturationUniforms {
                            hue: p.hue,
                            saturation: p.saturation,
                            lightness: p.lightness,
                            _pad: 0.0,
                        },
                    );
                    let bg = self.hue_saturation.make_bind_group(
                        device,
                        &uniform,
                        src,
                        &self.linear_sampler,
                        "hs-bg",
                    );
                    self.hue_saturation.run(encoder, &bg, dst, "hs-pass");
                }

                EditOp::Curves(p) => {
                    let (_, v_rgb) = upload_lut(device, queue, &build_lut(&p.rgb));
                    let (_, v_r) = upload_lut(device, queue, &build_lut(&p.r));
                    let (_, v_g) = upload_lut(device, queue, &build_lut(&p.g));
                    let (_, v_b) = upload_lut(device, queue, &build_lut(&p.b));
                    let bg = self.curves.make_bind_group(
                        device,
                        src,
                        &self.linear_sampler,
                        &v_rgb,
                        &v_r,
                        &v_g,
                        &v_b,
                        &self.lut_sampler,
                    );
                    self.curves.run(encoder, &bg, dst);
                }

                EditOp::Crop(p) => {
                    let uniform =
                        gpu::uniform_buffer::<CropUniforms>(device, Some("crop-uniform"));
                    gpu::write_uniform(
                        queue,
                        &uniform,
                        &CropUniforms {
                            x: p.x,
                            y: p.y,
                            w: p.width,
                            h: p.height,
                        },
                    );
                    let bg = self.crop.make_bind_group(
                        device,
                        &uniform,
                        src,
                        &self.linear_sampler,
                        "crop-bg",
                    );
                    self.crop.run(encoder, &bg, dst, "crop-pass");
                }

                EditOp::Paint(_) => {
                    // Paint is handled separately via the paint pass — skip here.
                    continue;
                }
            }

            self.ping_pong.as_mut()?.swap();
        }

        self.ping_pong.as_ref().map(|pp| pp.output())
    }
}
