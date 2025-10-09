use glam::{Vec2, Vec4, vec2, vec4};
use iced::{
    Rectangle,
    widget::shader::{
        Primitive, Storage, Viewport,
        wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView},
    },
};

use crate::wgpu::pipeline::{Pipeline, Uniforms};

const SCALE_STEPS: &[f32] = &[
    0.05, 0.10, 0.15, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 1.00, 1.25, 1.50, 1.75, 2.00,
    2.50, 3.00, 3.50, 4.00, 5.00, 6.00, 7.00, 8.00, 10.0, 12.0, 15.0, 18.0, 21.0, 25.0, 30.0, 35.0,
];

#[derive(Debug, Clone, Copy)]
pub struct Controls {
    pub scale_index: usize,
    pub pos: Vec2,
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

    pub fn to_ndc(&self, res: &Rectangle) -> Vec4 {
        let ndc_x = (self.pos.x / res.width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (self.pos.y / res.height) * 2.0;
        Vec4::new(ndc_x, ndc_y, 0.0, 1.0)
    }
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            scale_index: 11,
            pos: vec2(0., 0.),
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
