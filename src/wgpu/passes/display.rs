use iced::wgpu::{
    BindGroup, BindGroupLayout, BlendState, Buffer, Device, PrimitiveTopology, RenderPass,
    RenderPipeline, Sampler, ShaderStages, TextureFormat, TextureView,
};

use crate::wgpu::gpu;

pub struct DisplayPass {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
}

impl DisplayPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bind_group_layout = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("display-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/display.wgsl"),
            Some("display-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::ALPHA_BLENDING,
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
        texture_view: &TextureView,
        sampler: &Sampler,
        label: Option<&str>,
    ) -> BindGroup {
        gpu::standard_bind_group(
            device,
            &self.bind_group_layout,
            uniform_buffer,
            texture_view,
            sampler,
            label,
        )
    }

    pub fn draw<'a>(&'a self, pass: &mut RenderPass<'a>, bind_group: &'a BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}
