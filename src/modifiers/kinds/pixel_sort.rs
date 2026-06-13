use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{angle_row, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct PixelSort {
    pub threshold: f32,
    pub angle: f32,
}

impl Default for PixelSort {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            angle: 90.0,
        }
    }
}

impl ModifierImpl for PixelSort {
    fn name(&self) -> &'static str {
        "Pixel Sort"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::PixelSortThreshold(v) => self.threshold = v,
            ModifierParam::PixelSortAngle(v) => self.angle = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        11u8.hash(hasher);
        hash_f32(self.threshold, hasher);
        hash_f32(self.angle, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Threshold",
                self.threshold,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::PixelSortThreshold(v)).into(),
            ),
            angle_row("Angle", self.angle, 0.0..=360.0, move |v| {
                EditMsg::Update(index, ModifierParam::PixelSortAngle(v)).into()
            }),
        ])
    }
}
