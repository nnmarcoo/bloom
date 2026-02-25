use std::sync::Arc;

use glam::{Vec2, vec2};
use iced::{
    Rectangle,
    wgpu::{CommandEncoder, Device, Queue, TextureView},
    widget::shader::{Primitive, Viewport},
};

use crate::wgpu::{
    media::image_data::ImageData,
    view_pipeline::{Uniforms, ViewPipeline},
};

#[derive(Debug)]
pub struct ViewPrimitive {
    pub uniforms: Uniforms,
    pub image: Option<Arc<ImageData>>,
    pub scale: f32,
    pub pan_ndc: Vec2,
    pub bounds: Rectangle,
    pub lanczos_enabled: bool,
}

impl Primitive for ViewPrimitive {
    type Pipeline = ViewPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &Device,
        queue: &Queue,
        _bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        if let Some(image) = &self.image {
            if pipeline.needs_upload(image.id) {
                if let Err(e) = pipeline.upload_image(device, queue, image) {
                    eprintln!("upload_image failed: {e}");
                    return;
                }
            }
        }
        pipeline.update(
            device,
            queue,
            self.scale,
            viewport.scale_factor(),
            &self.uniforms,
            vec2(self.bounds.width, self.bounds.height),
            self.pan_ndc,
            self.lanczos_enabled,
        );
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        pipeline.render_display(encoder, target, clip_bounds, &self.bounds);
    }
}
