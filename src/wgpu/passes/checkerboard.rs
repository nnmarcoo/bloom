use bytemuck::{Pod, Zeroable};
use iced::Rectangle;
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, CommandEncoder,
    Device, LoadOp, Operations, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, ShaderStages, StoreOp, TextureFormat, TextureView,
};

use crate::wgpu::gpu;

#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct CheckerboardUniforms {
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub tile_size: f32,
    pub _pad: [f32; 3],
}

pub struct CheckerboardPass {
    pipeline: RenderPipeline,
    uniform_buffer: Buffer,
    bind_group: BindGroup,
}

impl CheckerboardPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("checker-bgl"),
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
            include_str!("../shaders/checkerboard.wgsl"),
            Some("checker-pipeline"),
            iced::wgpu::PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bind_group_layout,
        );

        let uniform_buffer =
            gpu::uniform_buffer::<CheckerboardUniforms>(device, Some("checker-uniform"));

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("checker-bg"),
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

    pub fn update_colors(&self, queue: &Queue, uniforms: &CheckerboardUniforms) {
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
            label: Some("checker-pass"),
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
