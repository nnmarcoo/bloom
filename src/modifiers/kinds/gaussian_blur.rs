use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct GaussianBlur {
    pub radius: f32,
}

impl Default for GaussianBlur {
    fn default() -> Self {
        Self { radius: 5.0 }
    }
}

impl ModifierImpl for GaussianBlur {
    fn name(&self) -> &'static str {
        "Gaussian Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::GaussianBlurRadius(v) = param {
            self.radius = v;
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        7u8.hash(hasher);
        hash_f32(self.radius, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Radius",
            self.radius,
            0.0..=100.0,
            0.5,
            Fmt::num(1),
            move |v| EditMsg::Update(index, ModifierParam::GaussianBlurRadius(v)).into(),
        )])
    }
}
