use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use iced::Rectangle;
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState,
    Buffer, BufferBindingType, CommandEncoder, Device, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, ShaderStages, StoreOp,
    TextureFormat, TextureView,
};

use crate::wgpu::gpu;

#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct PixelGridUniforms {
    pub screen_to_img: Mat4,
    pub viewport: [f32; 4],
    pub bounds_img: [f32; 4],
}

pub struct PixelGridPass {
    pipeline: RenderPipeline,
    uniform_buffer: Buffer,
    bind_group: BindGroup,
}

impl PixelGridPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grid-bgl"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/pixel_grid.wgsl"),
            Some("grid-pipeline"),
            iced::wgpu::PrimitiveTopology::TriangleStrip,
            format,
            BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            },
            &bind_group_layout,
        );

        let uniform_buffer = gpu::uniform_buffer::<PixelGridUniforms>(device, Some("grid-uniform"));

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grid-bg"),
            layout: &bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
        }
    }

    pub fn update(&self, queue: &Queue, uniforms: &PixelGridUniforms) {
        gpu::write_uniform(queue, &self.uniform_buffer, uniforms);
    }

    pub fn draw(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &iced::Rectangle,
        scale_factor: f32,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grid-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_viewport(
            bounds.x * scale_factor,
            bounds.y * scale_factor,
            bounds.width * scale_factor,
            bounds.height * scale_factor,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}
