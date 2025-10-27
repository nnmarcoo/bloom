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
pub struct Controls {
    scale_index: usize,
    pub pos: Vec2,
    pub image: ImageData,
}

impl Controls {
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

    pub fn to_ndc(&self, res: &Rectangle) -> Vec2 {
        todo!("GAH")
    }
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            scale_index: 11,
            pos: vec2(0., 0.),
            image: ImageData::new(),
        }
    }
}

#[derive(Debug)]
pub struct FragmentShaderPrimitive {
    controls: Controls,
}

impl FragmentShaderPrimitive {
    pub fn new(controls: Controls) -> Self {
        Self { controls }
    }
}

impl Primitive for FragmentShaderPrimitive {
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
            queue,
            &Uniforms {
                res: vec2(bounds.width, bounds.height),
                pos: self.controls.pos,
                scale: SCALE_STEPS[self.controls.scale_index],
                _pad: f32::default(),
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
