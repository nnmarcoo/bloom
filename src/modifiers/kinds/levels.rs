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
pub struct Levels {
    pub shadows: f32,
    pub midtones: f32,
    pub highlights: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            midtones: 1.0,
            highlights: 1.0,
        }
    }
}

impl ModifierImpl for Levels {
    fn name(&self) -> &'static str {
        "Levels"
    }

    fn has_effect(&self) -> bool {
        self.shadows != 0.0 || self.midtones != 1.0 || self.highlights != 1.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::LevelsShadows(v) => self.shadows = v,
            ModifierParam::LevelsMidtones(v) => self.midtones = v,
            ModifierParam::LevelsHighlights(v) => self.highlights = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::LEVELS,
            &[self.shadows, self.midtones, self.highlights],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let hi = self.highlights.max(self.shadows + 0.001);
        let range = hi - self.shadows;
        for v in c.iter_mut().take(3) {
            *v = ((*v - self.shadows) / range).clamp(0.0, 1.0);
        }
        let gamma = self.midtones.max(0.001);
        for v in c.iter_mut().take(3) {
            *v = v.powf(1.0 / gamma);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        1u8.hash(hasher);
        hash_f32(self.shadows, hasher);
        hash_f32(self.midtones, hasher);
        hash_f32(self.highlights, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Shadows",
                self.shadows,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::LevelsShadows(v)).into(),
            ),
            value_row(
                "Midtones",
                self.midtones,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::LevelsMidtones(v)).into(),
            ),
            value_row(
                "Highlights",
                self.highlights,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::LevelsHighlights(v)).into(),
            ),
        ])
    }
}
