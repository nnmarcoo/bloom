use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct RadialBlur {
    pub amount: f32,
}

impl Default for RadialBlur {
    fn default() -> Self {
        Self { amount: 10.0 }
    }
}

impl ModifierImpl for RadialBlur {
    fn name(&self) -> &'static str {
        "Radial Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::RadialBlurAmount(v) = param {
            self.amount = v;
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        9u8.hash(hasher);
        hash_f32(self.amount, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Amount",
            self.amount,
            0.0..=100.0,
            0.5,
            Fmt::num(0),
            move |v| EditMsg::Update(index, ModifierParam::RadialBlurAmount(v)).into(),
        )])
    }
}
