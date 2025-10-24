use glam::{vec2, vec4, Vec2, Vec4};
use iced::Rectangle;

use crate::constants::SCALE_STEPS;

// add custom option
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

        pub fn to_ndc(&self, screen_size: &Rectangle) -> [Vec4; 4] {
        let scaled_size = self.image_size * self.scale;
        let top_left = self.pos;
        let bottom_right = self.pos + scaled_size;

        // Convert corners to NDC (Vec2)
        let ndc_top_left = Self::screen_to_ndc(top_left, screen_size);
        let ndc_bottom_right = Self::screen_to_ndc(bottom_right, screen_size);

        // Convert to Vec4 positions for wgpu (z=0, w=1)
        [
            vec4(ndc_top_left.x,     ndc_top_left.y,     0.0, 1.0), // top-left
            vec4(ndc_bottom_right.x, ndc_top_left.y,     0.0, 1.0), // top-right
            vec4(ndc_top_left.x,     ndc_bottom_right.y, 0.0, 1.0), // bottom-left
            vec4(ndc_bottom_right.x, ndc_bottom_right.y, 0.0, 1.0), // bottom-right
        ]
    }

    fn screen_to_ndc(pos: Vec2, screen_size: &Rectangle) -> Vec2 {
        vec2(
            2.0 * pos.x / screen_size.x - 1.0,
            1.0 - 2.0 * pos.y / screen_size.y,
        )
    }
}
