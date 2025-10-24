use glam::{Vec2, vec2};

use crate::constants::SCALE_STEPS;

pub struct ImageData {
    pos: Vec2,
    size: Vec2,
    scale_i: usize,
}

impl ImageData {
    fn new() -> Self {
        Self {
            pos: vec2(0., 0.),
            size: vec2(0., 0.),
            scale_i: 11,
        }
    }

    pub fn scale_up(&mut self) {
        self.scale_i = (self.scale_i + 1).min(SCALE_STEPS.len() - 1);
    }

    pub fn scale_down(&mut self) {
        self.scale_i = self.scale_i.saturating_sub(1);
    }

    pub fn pan(&mut self, deltaP: Vec2) {
        self.pos += deltaP;
    }
}
