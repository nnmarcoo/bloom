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
pub struct Solarize {
    pub threshold: f32,
}

impl Default for Solarize {
    fn default() -> Self {
        Self { threshold: 0.5 }
    }
}

fn solarize_channel(v: f32, threshold: f32) -> f32 {
    if v >= threshold { 1.0 - v } else { v }
}

impl ModifierImpl for Solarize {
    fn name(&self) -> &'static str {
        "Solarize"
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::SolarizeThreshold(v) = param {
            self.threshold = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::SOLARIZE, &[self.threshold]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        for ch in c.iter_mut().take(3) {
            *ch = solarize_channel(*ch, self.threshold);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        24u8.hash(hasher);
        hash_f32(self.threshold, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Threshold",
            self.threshold,
            0.0..=1.0,
            0.01,
            Fmt::num(2),
            move |v| EditMsg::Update(index, ModifierParam::SolarizeThreshold(v)).into(),
        )])
    }
}
