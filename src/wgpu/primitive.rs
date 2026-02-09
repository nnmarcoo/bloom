use glam::{Vec2, vec2};
use iced::{
    Rectangle,
    widget::shader::{
        Primitive, Storage, Viewport,
        wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView},
    },
};

use crate::{
    constants::SCALE_STEPS,
    wgpu::pipeline::{Pipeline, Uniforms},
};

const DEFAULT_SCALE_INDEX: usize = 11; // 1.0x in SCALE_STEPS

#[derive(Debug, Clone, Copy)]
pub struct ViewState {
    scale_index: usize,
    pub pan: Vec2,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scale_index: DEFAULT_SCALE_INDEX,
            pan: Vec2::ZERO,
        }
    }
}

impl ViewState {
    pub fn scale(&self) -> f32 {
        SCALE_STEPS[self.scale_index]
    }

    pub fn scale_up(&mut self) {
        if self.scale_index + 1 < SCALE_STEPS.len() {
            self.scale_index += 1;
        }
    }

    pub fn scale_down(&mut self) {
        if self.scale_index > 0 {
            self.scale_index -= 1;
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct ImagePrimitive {
    view_state: ViewState,
    pending_image: Option<Vec<u8>>,
}

impl ImagePrimitive {
    pub fn new(view_state: ViewState, pending_image: Option<Vec<u8>>) -> Self {
        Self {
            view_state,
            pending_image,
        }
    }
}

impl std::fmt::Debug for ImagePrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagePrimitive")
            .field("view_state", &self.view_state)
            .field("pending_image", &self.pending_image.as_ref().map(|b| b.len()))
            .finish()
    }
}

impl Primitive for ImagePrimitive {
    fn prepare(
        &self,
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        storage: &mut Storage,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        if !storage.has::<Pipeline>() {
            storage.store(Pipeline::new(device, queue, format));
        }

        let pipeline = storage.get_mut::<Pipeline>().unwrap();

        if let Some(bytes) = &self.pending_image {
            pipeline.set_image(device, queue, bytes);
        }

        let image_size = pipeline.image_size();

        pipeline.update(
            device,
            queue,
            &Uniforms {
                viewport_size: vec2(bounds.width, bounds.height),
                pan: self.view_state.pan,
                scale: self.view_state.scale(),
                _pad: 0.0,
                image_size,
            },
        );
    }

    fn render(
        &self,
        encoder: &mut CommandEncoder,
        storage: &Storage,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let pipeline = storage.get::<Pipeline>().unwrap();
        pipeline.render(target, encoder, *clip_bounds);
    }
}
