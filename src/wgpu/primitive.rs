use glam::{Vec2, vec2};
use iced::Rectangle;
use iced::widget::shader::{Primitive, Viewport};
use iced::wgpu::{CommandEncoder, Device, Queue, TextureView};

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::{
    constants::SCALE_STEPS,
    wgpu::pipeline::{Pipeline, Uniforms},
};

#[derive(Debug, Clone, Copy)]
pub struct ViewState {
    pub scale: f32,
    pub pan: Vec2,
    pub image_size: Vec2,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scale: 0.0,
            pan: Vec2::ZERO,
            image_size: Vec2::ZERO,
        }
    }
}

impl ViewState {
    pub fn scale_up(&mut self) {
        if let Some(&s) = SCALE_STEPS.iter().find(|&&s| s > self.scale) {
            self.scale = s;
        }
    }

    pub fn scale_down(&mut self) {
        if let Some(&s) = SCALE_STEPS.iter().rev().find(|&&s| s < self.scale) {
            self.scale = s;
        }
    }

    pub fn clamp_pan(&mut self) {
        self.pan = self.pan.clamp(-self.image_size, self.image_size);
    }
}

pub struct ImagePrimitive {
    view_state: ViewState,
    pending_image: Option<Vec<u8>>,
    scale_out: Arc<AtomicU32>,
}

impl ImagePrimitive {
    pub fn new(
        view_state: ViewState,
        pending_image: Option<Vec<u8>>,
        scale_out: Arc<AtomicU32>,
    ) -> Self {
        Self {
            view_state,
            pending_image,
            scale_out,
        }
    }
}

impl std::fmt::Debug for ImagePrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagePrimitive")
            .field("view_state", &self.view_state)
            .field(
                "pending_image",
                &self.pending_image.as_ref().map(|b| b.len()),
            )
            .finish()
    }
}

impl Primitive for ImagePrimitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &Device,
        queue: &Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        if let Some(bytes) = &self.pending_image {
            pipeline.set_image(device, queue, bytes);
        }

        let image_size = pipeline.image_size();
        let viewport = vec2(bounds.width, bounds.height);

        let scale = if self.view_state.scale == 0.0
            && image_size != Vec2::ZERO
            && viewport != Vec2::ZERO
        {
            let fit = (viewport.x / image_size.x).min(viewport.y / image_size.y);
            self.scale_out.store(fit.to_bits(), Ordering::Relaxed);
            fit
        } else {
            self.view_state.scale
        };

        pipeline.update(
            device,
            queue,
            &Uniforms {
                viewport_size: viewport,
                pan: self.view_state.pan,
                scale,
                _pad: 0.0,
                image_size,
            },
        );
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        pipeline.render(target, encoder, *clip_bounds);
    }
}
