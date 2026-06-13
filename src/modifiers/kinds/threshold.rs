use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{clamped_luma, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Threshold {
    pub cutoff: f32,
}

impl Default for Threshold {
    fn default() -> Self {
        Self { cutoff: 0.5 }
    }
}

impl ModifierImpl for Threshold {
    fn name(&self) -> &'static str {
        "Threshold"
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::ThresholdCutoff(v) = param {
            self.cutoff = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::THRESHOLD, &[self.cutoff]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let luma = clamped_luma(c);
        let v = if luma >= self.cutoff { 1.0 } else { 0.0 };
        c[0] = v;
        c[1] = v;
        c[2] = v;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        15u8.hash(hasher);
        hash_f32(self.cutoff, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Cutoff",
            self.cutoff,
            0.0..=1.0,
            0.01,
            Fmt::num(2),
            move |v| EditMsg::Update(index, ModifierParam::ThresholdCutoff(v)).into(),
        )])
    }
}
