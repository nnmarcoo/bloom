use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroupLayout,
    BlendState, Buffer, CommandEncoder,
    Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, TextureFormat, TextureView,
};

use crate::modifiers::motion_blur_samples;
use crate::{modifiers::gpu::UvRect, wgpu::gpu};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MbUniforms {
    dir: [f32; 2],
    samples: f32,
    _pad0: f32,
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
}

pub struct MotionBlurPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl MotionBlurPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::FRAGMENT,
            Some("motion-blur-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/motion_blur.wgsl"),
            Some("motion-blur-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("motion-blur-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bgl,
            sampler,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform_buffer: &Buffer,
        angle: f32,
        distance: f32,
        full_w: f32,
        full_h: f32,
        proc: UvRect,
        src: UvRect,
        input: &TextureView,
        output: &TextureView,
    ) {
        let rad = angle.to_radians();
        let dist_u = distance / full_w;
        let dist_v = distance / full_h;
        let dir = [rad.cos() * dist_u, rad.sin() * dist_v];
        let uniforms = MbUniforms {
            dir,
            samples: motion_blur_samples(distance) as f32,
            _pad0: 0.0,
            proc_origin: proc.origin,
            proc_size: proc.size,
            src_origin: src.origin,
            src_size: src.size,
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);
        let bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            uniform_buffer,
            input,
            &self.sampler,
            Some("motion-blur-bg"),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("motion-blur-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: output,
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

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<MbUniforms>(device, Some("motion-blur-uniform"))
    }
}


