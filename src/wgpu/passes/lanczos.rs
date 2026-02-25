use glam::Vec2;
use iced::wgpu::{
    BindGroup, BindGroupLayout, BlendState, Buffer, Device, PrimitiveTopology, RenderPass,
    RenderPipeline, Sampler, ShaderStages, TextureFormat, TextureView,
};

use crate::wgpu::gpu;

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct LanczosUniforms {
    pub src_size: Vec2,
    pub scale: f32,
    pub _pad: f32,
}

pub struct LanczosPass {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
}

impl LanczosPass {
    pub fn new(device: &Device, output_format: TextureFormat, shader_src: &str) -> Self {
        let bind_group_layout =
            gpu::standard_bind_group_layout(device, ShaderStages::FRAGMENT, Some("lanczos-bgl"));
        let pipeline = gpu::fullscreen_pipeline(
            device,
            shader_src,
            Some("lanczos-pipeline"),
            PrimitiveTopology::TriangleList,
            output_format,
            BlendState::REPLACE,
            &bind_group_layout,
        );

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &Device,
        uniform_buffer: &Buffer,
        input_view: &TextureView,
        sampler: &Sampler,
        label: Option<&str>,
    ) -> BindGroup {
        gpu::standard_bind_group(
            device,
            &self.bind_group_layout,
            uniform_buffer,
            input_view,
            sampler,
            label,
        )
    }

    pub fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, bind_group: &'a BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}
