use std::borrow::Cow;

use bytemuck::bytes_of;
use glam::Vec2;
use iced::{
    Rectangle,
    advanced::graphics::image::image_rs::load_from_memory,
    widget::shader::wgpu::{
        AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer,
        BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device,
        Extent3d, FilterMode, FragmentState, LoadOp, MultisampleState, Operations, PrimitiveState,
        PrimitiveTopology, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
        RenderPipelineDescriptor, Sampler, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
        StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        TextureView, TextureViewDescriptor, VertexState,
        util::{DeviceExt, TextureDataOrder},
    },
};

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    pub res: Vec2,
    pub pos: Vec2,
    pub scale: f32,
    pub _pad: f32,
}

pub struct Pipeline {
    pipeline: RenderPipeline,
    uniform_buffer: Buffer,
    bind_group: BindGroup,

    // why am I storing these
    texture: Texture,
    texture_view: TextureView,
    sampler: Sampler,
}

impl Pipeline {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader/simple.wgsl"))),
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Pipeline"),
            layout: None,
            vertex: VertexState {
                module: &shader,
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
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });

        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let (texture, texture_view, bind_group) = Self::create_texture_bind_group(
            device,
            queue,
            &pipeline,
            &uniform_buffer,
            &sampler,
            include_bytes!("../assets/debug.jpg"),
            "Texture",
        );

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            texture,
            texture_view,
            sampler,
        }
    }

    pub fn update(&mut self, queue: &Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytes_of(uniforms));
    }

    pub fn render(
        &self,
        target: &TextureView,
        encoder: &mut CommandEncoder,
        viewport: Rectangle<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("Render Pass Test"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
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

        pass.set_pipeline(&self.pipeline);
        pass.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.width as f32,
            viewport.height as f32,
            0.,
            1.,
        );
        pass.set_bind_group(0, &self.bind_group, &[]);

        pass.draw(0..4, 0..1);
    }

    fn create_texture_bind_group(
        device: &Device,
        queue: &Queue,
        pipeline: &RenderPipeline,
        uniform_buffer: &Buffer,
        sampler: &Sampler,
        image_bytes: &[u8],
        label: &str,
    ) -> (Texture, TextureView, BindGroup) {
        let img = load_from_memory(image_bytes)
            .expect("invalid image file")
            .to_rgba8();
        let (width, height) = img.dimensions();
        let data = img.into_raw();

        let texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: Some(label),
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
            &data,
        );

        let texture_view = texture.create_view(&TextureViewDescriptor::default());

        let layout = pipeline.get_bind_group_layout(0);
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Image Bind Group"),
            layout: &layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&texture_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        });

        (texture, texture_view, bind_group)
    }

    pub fn set_texture(&mut self, device: &Device, queue: &Queue, image_bytes: &[u8]) {
        let (texture, texture_view, bind_group) = Self::create_texture_bind_group(
            device,
            queue,
            &self.pipeline,
            &self.uniform_buffer,
            &self.sampler,
            image_bytes,
            "Texture",
        );

        self.texture = texture;
        self.texture_view = texture_view;
        self.bind_group = bind_group;
    }
}
