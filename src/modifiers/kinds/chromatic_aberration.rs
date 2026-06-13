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
pub struct ChromaticAberration {
    pub amount: f32,
}

impl Default for ChromaticAberration {
    fn default() -> Self {
        Self { amount: 5.0 }
    }
}

impl ModifierImpl for ChromaticAberration {
    fn name(&self) -> &'static str {
        "Chromatic Aberration"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    fn is_resampling(&self) -> bool {
        true
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::ChromaticAberrationAmount(v) = param {
            self.amount = v;
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::CHROMATIC_ABERRATION,
            &[
                self.amount / tile.full_w as f32,
                tile.tile_x as f32 / tile.full_w as f32,
                tile.tile_y as f32 / tile.full_h as f32,
                tile.tile_w as f32 / tile.full_w as f32,
                tile.tile_h as f32 / tile.full_h as f32,
            ],
        ))
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        13u8.hash(hasher);
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
            0.0..=50.0,
            0.1,
            Fmt::num(1),
            move |v| EditMsg::Update(index, ModifierParam::ChromaticAberrationAmount(v)).into(),
        )])
    }
}
