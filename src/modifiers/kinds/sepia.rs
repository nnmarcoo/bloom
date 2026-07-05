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
pub struct Sepia {
    pub intensity: f32,
}

impl Default for Sepia {
    fn default() -> Self {
        Self { intensity: 1.0 }
    }
}

fn sepia_map(c: [f32; 4]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    [
        r * 0.393 + g * 0.769 + b * 0.189,
        r * 0.349 + g * 0.686 + b * 0.168,
        r * 0.272 + g * 0.534 + b * 0.131,
    ]
}

impl ModifierImpl for Sepia {
    fn name(&self) -> &'static str {
        "Sepia"
    }

    fn has_effect(&self) -> bool {
        self.intensity != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::SepiaIntensity(v) = param {
            self.intensity = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::SEPIA, &[self.intensity]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let s = sepia_map(c);
        for i in 0..3 {
            c[i] += self.intensity * (s[i] - c[i]);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        23u8.hash(hasher);
        hash_f32(self.intensity, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Intensity",
            self.intensity,
            0.0..=1.0,
            0.01,
            Fmt::num(2),
            move |v| EditMsg::Update(index, ModifierParam::SepiaIntensity(v)).into(),
        )])
    }
}
