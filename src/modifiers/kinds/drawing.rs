use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Drawing {
    pub opacity: f32,
    pub size: f32,
    pub hardness: f32,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            size: 10.0,
            hardness: 0.8,
        }
    }
}

impl ModifierImpl for Drawing {
    fn name(&self) -> &'static str {
        "Drawing"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::DrawingOpacity(v) => self.opacity = v,
            ModifierParam::DrawingSize(v) => self.size = v,
            ModifierParam::DrawingHardness(v) => self.hardness = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        19u8.hash(hasher);
        hash_f32(self.opacity, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.hardness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Opacity",
                self.opacity,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::DrawingOpacity(v)).into(),
            ),
            value_row("Size", self.size, 1.0..=100.0, 0.5, Fmt::num(0), move |v| {
                EditMsg::Update(index, ModifierParam::DrawingSize(v)).into()
            },),
            value_row(
                "Hardness",
                self.hardness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::DrawingHardness(v)).into(),
            ),
        ])
    }
}
