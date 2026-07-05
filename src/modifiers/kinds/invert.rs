use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Invert {
    pub amount: f32,
}

impl Default for Invert {
    fn default() -> Self {
        Self { amount: 1.0 }
    }
}

impl ModifierImpl for Invert {
    fn name(&self) -> &'static str {
        "Invert"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::InvertAmount(v) = param {
            self.amount = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::INVERT, &[self.amount]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        for ch in c.iter_mut().take(3) {
            *ch += self.amount * (1.0 - 2.0 * *ch);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        20u8.hash(hasher);
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
            0.0..=1.0,
            0.01,
            Fmt::num(2),
            move |v| EditMsg::Update(index, ModifierParam::InvertAmount(v)).into(),
        )])
    }
}
