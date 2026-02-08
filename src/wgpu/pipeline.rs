use std::borrow::Cow;

use bytemuck::bytes_of;
use glam::Vec2;
use iced::{
    Rectangle,
    advanced::graphics::image::image_rs::load_from_memory,
    widget::shader::wgpu::{
        AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
        BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, Buffer,
        BufferBindingType, BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites,
        CommandEncoder, Device, Extent3d, FilterMode, FragmentState, LoadOp, MultisampleState,
        Operations, PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, Queue,
        RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
        Sampler, SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
        ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
        TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension,
        VertexState,
        util::{DeviceExt, TextureDataOrder},
    },
};

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    pub viewport_size: Vec2,
    pub pan: Vec2,
    pub scale: f32,
    pub _pad: f32,
    pub image_size: Vec2,
}

const INTERMEDIATE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

struct RenderTarget {
    #[allow(dead_code)]
    texture: Texture,
    view: TextureView,
    size: (u32, u32),
}

impl RenderTarget {
    fn new(device: &Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: INTERMEDIATE_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size: (width.max(1), height.max(1)),
        }
    }
}

pub struct Pipeline {
    lanczos_h_pipeline: RenderPipeline,
    lanczos_v_pipeline: RenderPipeline,
    blit_pipeline: RenderPipeline,

    bind_group_layout: BindGroupLayout,
    uniform_buffer: Buffer,
    sampler: Sampler,

    #[allow(dead_code)]
    source_texture: Texture,
    source_view: TextureView,
    source_size: (u32, u32),
    source_bind_group: BindGroup,

    h_filtered: RenderTarget,
    h_filtered_bind_group: BindGroup,

    hv_filtered: RenderTarget,
    blit_bind_group: BindGroup,

    cached_scale: f32,
}

impl Pipeline {
    pub fn new(device: &Device, queue: &Queue, target_format: TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let load_shader = |label, source| {
            device.create_shader_module(ShaderModuleDescriptor {
                label: Some(label),
                source: ShaderSource::Wgsl(Cow::Borrowed(source)),
            })
        };

        let lanczos_h_shader = load_shader("Lanczos H", include_str!("shader/lanczos_h.wgsl"));
        let lanczos_v_shader = load_shader("Lanczos V", include_str!("shader/lanczos_v.wgsl"));
        let blit_shader = load_shader("Blit", include_str!("shader/blit.wgsl"));

        let create_pipeline = |label, shader: &_, output_format| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: MultisampleState::default(),
                fragment: Some(FragmentState {
                    module: shader,
                    entry_point: "fs_main",
                    targets: &[Some(ColorTargetState {
                        format: output_format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview: None,
            })
        };

        let lanczos_h_pipeline =
            create_pipeline("Lanczos H", &lanczos_h_shader, INTERMEDIATE_FORMAT);
        let lanczos_v_pipeline =
            create_pipeline("Lanczos V", &lanczos_v_shader, INTERMEDIATE_FORMAT);
        let blit_pipeline = create_pipeline("Blit", &blit_shader, target_format);

        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            ..Default::default()
        });

        let (source_texture, source_view, source_size) =
            Self::load_image(device, queue, include_bytes!("../assets/debug.jpg"));

        let source_bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &source_view,
            &sampler,
        );

        let h_filtered = RenderTarget::new(device, 1, 1);
        let h_filtered_bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &h_filtered.view,
            &sampler,
        );

        let hv_filtered = RenderTarget::new(device, 1, 1);
        let blit_bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &hv_filtered.view,
            &sampler,
        );

        Self {
            lanczos_h_pipeline,
            lanczos_v_pipeline,
            blit_pipeline,
            bind_group_layout,
            uniform_buffer,
            sampler,
            source_texture,
            source_view,
            source_size,
            source_bind_group,
            h_filtered,
            h_filtered_bind_group,
            hv_filtered,
            blit_bind_group,
            cached_scale: 0.0,
        }
    }

    pub fn update(&mut self, device: &Device, queue: &Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytes_of(uniforms));

        let scale = uniforms.scale;
        if scale == self.cached_scale {
            return;
        }
        self.cached_scale = scale;

        if scale >= 1.0 {
            self.blit_bind_group = Self::create_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.source_view,
                &self.sampler,
            );
            return;
        }

        let dst_w = ((self.source_size.0 as f32 * scale).round() as u32).max(1);
        let dst_h = ((self.source_size.1 as f32 * scale).round() as u32).max(1);

        let h_needed = (dst_w, self.source_size.1);
        if h_needed != self.h_filtered.size {
            self.h_filtered = RenderTarget::new(device, h_needed.0, h_needed.1);
            self.h_filtered_bind_group = Self::create_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.h_filtered.view,
                &self.sampler,
            );
        }

        let hv_needed = (dst_w, dst_h);
        if hv_needed != self.hv_filtered.size {
            self.hv_filtered = RenderTarget::new(device, hv_needed.0, hv_needed.1);
            self.blit_bind_group = Self::create_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.hv_filtered.view,
                &self.sampler,
            );
        }
    }

    pub fn render(&self, screen: &TextureView, encoder: &mut CommandEncoder, clip: Rectangle<u32>) {
        if self.cached_scale < 1.0 {
            self.render_to_texture(
                encoder,
                &self.lanczos_h_pipeline,
                &self.h_filtered.view,
                self.h_filtered.size,
                &self.source_bind_group,
            );

            self.render_to_texture(
                encoder,
                &self.lanczos_v_pipeline,
                &self.hv_filtered.view,
                self.hv_filtered.size,
                &self.h_filtered_bind_group,
            );
        }

        self.blit_to_screen(encoder, screen, clip);
    }

    fn render_to_texture(
        &self,
        encoder: &mut CommandEncoder,
        pipeline: &RenderPipeline,
        target: &TextureView,
        target_size: (u32, u32),
        bind_group: &BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(iced::widget::shader::wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_viewport(0., 0., target_size.0 as f32, target_size.1 as f32, 0., 1.);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..4, 0..1);
    }

    fn blit_to_screen(
        &self,
        encoder: &mut CommandEncoder,
        screen: &TextureView,
        clip: Rectangle<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: screen,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.blit_pipeline);
        pass.set_viewport(
            clip.x as f32,
            clip.y as f32,
            clip.width as f32,
            clip.height as f32,
            0.,
            1.,
        );
        pass.set_bind_group(0, &self.blit_bind_group, &[]);
        pass.draw(0..4, 0..1);
    }

    #[allow(dead_code)]
    pub fn set_image(&mut self, device: &Device, queue: &Queue, image_bytes: &[u8]) {
        let (texture, view, size) = Self::load_image(device, queue, image_bytes);
        self.source_texture = texture;
        self.source_view = view;
        self.source_size = size;
        self.cached_scale = 0.0;
        self.source_bind_group = Self::create_bind_group(
            device,
            &self.bind_group_layout,
            &self.uniform_buffer,
            &self.source_view,
            &self.sampler,
        );
    }

    fn load_image(
        device: &Device,
        queue: &Queue,
        image_bytes: &[u8],
    ) -> (Texture, TextureView, (u32, u32)) {
        let img = load_from_memory(image_bytes)
            .expect("invalid image file")
            .to_rgba8();
        let (width, height) = img.dimensions();

        let texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: None,
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &img.into_raw(),
        );

        let view = texture.create_view(&TextureViewDescriptor::default());
        (texture, view, (width, height))
    }

    fn create_bind_group(
        device: &Device,
        layout: &BindGroupLayout,
        uniform_buffer: &Buffer,
        texture_view: &TextureView,
        sampler: &Sampler,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(texture_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
