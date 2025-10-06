use iced::widget::shader::wgpu::{BindGroup, Buffer, Device, RenderPipeline};

struct Pipeline {
    pipeline: RenderPipeline,
    vertices: Buffer,
    uniforms: Buffer,
    uniform_bind_group: BindGroup,
}

impl Pipeline {
    pub fn new(device: &Device,) -> Self {

    }
}
