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
    wgpu::{
        image_data::ImageData,
        pipeline::{Pipeline, Uniforms},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct ViewState {
    scale_index: usize,
    pub pan: Vec2,
    pub image: ImageData,
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
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scale_index: 11,
            pan: Vec2::ZERO,
            image: ImageData::new(vec2(2048., 2048.)),
        }
    }
}

#[derive(Debug)]
pub struct ImagePrimitive {
    view_state: ViewState,
}

impl ImagePrimitive {
    pub fn new(view_state: ViewState) -> Self {
        Self { view_state }
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

        pipeline.update(
            device,
            queue,
            &Uniforms {
                viewport_size: vec2(bounds.width, bounds.height),
                pan: self.view_state.pan,
                scale: self.view_state.scale(),
                _pad: 0.0,
                image_size: self.view_state.image.original_size,
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
