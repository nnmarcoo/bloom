use glam::{Vec2, Vec4, vec2, vec4};
use iced::Rectangle;

use crate::constants::SCALE_STEPS;

pub enum ScaleDirection {
    UP,
    DOWN,
    CUSTOM,
}

// Add texture data ?
#[derive(Debug, Clone, Copy)]
pub struct ImageData {
    pub pos: Vec2,
    pub image_size: Vec2,
    pub scale_i: usize,
    pub scale: f32,
}

impl ImageData {
    pub fn new() -> Self {
        Self {
            pos: vec2(0., 0.),
            image_size: vec2(0., 0.),
            scale_i: 11,
            scale: 1., // TODO: calculate to fit on screen
        }
    }

    pub fn scale(&mut self, dir: ScaleDirection, cursor: Vec2) {
        let prev_scale = self.scale;

        self.set_scale(dir);

        let factor = self.scale / prev_scale;
        self.pos = cursor - (cursor - self.pos) * factor;
    }

    fn set_scale(&mut self, dir: ScaleDirection) {
        match dir {
            ScaleDirection::UP => self.scale_i = (self.scale_i + 1).min(SCALE_STEPS.len() - 1),
            ScaleDirection::DOWN => self.scale_i = self.scale_i.saturating_sub(1),
            ScaleDirection::CUSTOM => {
                /* TODO: calculate new scale_i for later*/
                return;
            }
        }

        self.scale = SCALE_STEPS[self.scale_i];
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.pos += 2. * delta / SCALE_STEPS[self.scale_i];
    }
}
