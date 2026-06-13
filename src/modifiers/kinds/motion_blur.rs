use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{angle_row, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct MotionBlur {
    pub angle: f32,
    pub distance: f32,
}

impl Default for MotionBlur {
    fn default() -> Self {
        Self {
            angle: 0.0,
            distance: 20.0,
        }
    }
}

impl ModifierImpl for MotionBlur {
    fn name(&self) -> &'static str {
        "Motion Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::MotionBlurAngle(v) => self.angle = v,
            ModifierParam::MotionBlurDistance(v) => self.distance = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        8u8.hash(hasher);
        hash_f32(self.angle, hasher);
        hash_f32(self.distance, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            angle_row("Angle", self.angle, 0.0..=360.0, move |v| {
                EditMsg::Update(index, ModifierParam::MotionBlurAngle(v)).into()
            }),
            value_row(
                "Distance",
                self.distance,
                0.0..=200.0,
                0.5,
                Fmt::num(0),
                move |v| EditMsg::Update(index, ModifierParam::MotionBlurDistance(v)).into(),
            ),
        ])
    }
}
