use glam::{Vec2, vec2};
use iced::{
    Rectangle,
    widget::shader::{
        Primitive, Storage, Viewport,
        wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView},
    },
};

use crate::wgpu::pipeline::{Pipeline, Uniforms};

#[derive(Debug, Clone, Copy)]
pub struct Controls {
    pub zoom: f32,
    pub center: Vec2,
}

impl Controls {
    // change this to be a proper scaling
    pub fn scale(&self) -> f32 {
        1.0 / 2.0_f32.powf(self.zoom) / 200.
    }
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            zoom: 1.,
            center: vec2(0., 0.),
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
                resolution: vec2(bounds.width, bounds.height),
                center: self.controls.center,
                scale: self.controls.scale(),
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
