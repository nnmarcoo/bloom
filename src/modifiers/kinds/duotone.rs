use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{LUMA, color_row, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Duotone {
    pub shadow: [f32; 3],
    pub highlight: [f32; 3],
    pub amount: f32,
}

impl Default for Duotone {
    fn default() -> Self {
        Self {
            shadow: [0.1, 0.15, 0.4],
            highlight: [1.0, 0.95, 0.8],
            amount: 1.0,
        }
    }
}

impl ModifierImpl for Duotone {
    fn name(&self) -> &'static str {
        "Duotone"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::DuotoneShadow(v) => self.shadow = v,
            ModifierParam::DuotoneHighlight(v) => self.highlight = v,
            ModifierParam::DuotoneAmount(v) => self.amount = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::DUOTONE,
            &[
                self.shadow[0],
                self.shadow[1],
                self.shadow[2],
                self.highlight[0],
                self.highlight[1],
                self.highlight[2],
                self.amount,
            ],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let luma = c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2];
        for ((ch, &lo), &hi) in c.iter_mut().zip(&self.shadow).zip(&self.highlight) {
            let toned = lo + luma * (hi - lo);
            *ch += self.amount * (toned - *ch);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        25u8.hash(hasher);
        for v in self.shadow.iter().chain(&self.highlight) {
            hash_f32(*v, hasher);
        }
        hash_f32(self.amount, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            color_row("Shadows", self.shadow, move |rgb| EditMsg::Update(
                index,
                ModifierParam::DuotoneShadow(rgb)
            )
            .into()),
            color_row("Highlights", self.highlight, move |rgb| EditMsg::Update(
                index,
                ModifierParam::DuotoneHighlight(rgb)
            )
            .into()),
            value_row(
                "Amount",
                self.amount,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::DuotoneAmount(v)).into(),
            ),
        ])
    }
}
