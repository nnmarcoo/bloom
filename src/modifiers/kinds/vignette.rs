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
pub struct Vignette {
    pub strength: f32,
    pub size: f32,
    pub softness: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            strength: 0.5,
            size: 0.5,
            softness: 0.5,
        }
    }
}

impl ModifierImpl for Vignette {
    fn name(&self) -> &'static str {
        "Vignette"
    }

    fn has_effect(&self) -> bool {
        self.strength != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::VignetteStrength(v) => self.strength = v,
            ModifierParam::VignetteSize(v) => self.size = v,
            ModifierParam::VignetteSoftness(v) => self.softness = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::VIGNETTE,
            &[self.strength, self.size, self.softness],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let dx = uv[0] - 0.5;
        let dy = uv[1] - 0.5;
        let dist = (dx * dx + dy * dy).sqrt() * 2.0;
        let inner = (self.size - self.softness).max(0.0);
        let t = ((dist - inner) / (self.size + 0.0001 - inner)).clamp(0.0, 1.0);
        let vignette = 1.0 - t * t * (3.0 - 2.0 * t);
        let factor = (1.0 - self.strength).max(0.0) * (1.0 - vignette) + vignette;
        c[0] *= factor;
        c[1] *= factor;
        c[2] *= factor;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        12u8.hash(hasher);
        hash_f32(self.strength, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.softness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Strength",
                self.strength,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::VignetteStrength(v)).into(),
            ),
            value_row("Size", self.size, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::VignetteSize(v)).into()
            },),
            value_row(
                "Softness",
                self.softness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::VignetteSoftness(v)).into(),
            ),
        ])
    }
}
