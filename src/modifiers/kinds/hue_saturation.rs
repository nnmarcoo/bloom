use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::cpu::{hsl_to_rgb, rgb_to_hsl};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::{Fmt, Track};

use super::{finish, gradient_row, hash_f32, value_row};

#[derive(Debug, Clone, Default)]
pub struct HueSaturation {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

impl ModifierImpl for HueSaturation {
    fn name(&self) -> &'static str {
        "Hue / Saturation"
    }

    fn has_effect(&self) -> bool {
        self.hue != 0.0 || self.saturation != 0.0 || self.lightness != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Hue(v) => self.hue = v,
            ModifierParam::Saturation(v) => self.saturation = v,
            ModifierParam::Lightness(v) => self.lightness = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::HUE_SATURATION,
            &[self.hue, self.saturation, self.lightness],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let [h, s, l] = rgb_to_hsl([
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        ]);
        let rgb = hsl_to_rgb([
            (h + self.hue / 360.0).rem_euclid(1.0),
            (s + self.saturation).clamp(0.0, 1.0),
            (l + self.lightness).clamp(0.0, 1.0),
        ]);
        c[0] = rgb[0];
        c[1] = rgb[1];
        c[2] = rgb[2];
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        3u8.hash(hasher);
        hash_f32(self.hue, hasher);
        hash_f32(self.saturation, hasher);
        hash_f32(self.lightness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            gradient_row(
                "Hue",
                self.hue,
                -180.0..=180.0,
                0.5,
                Fmt::signed(0).suffix("\u{00b0}"),
                Track::hue(),
                move |v| EditMsg::Update(index, ModifierParam::Hue(v)).into(),
            ),
            value_row(
                "Saturation",
                self.saturation,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::Saturation(v)).into(),
            ),
            value_row(
                "Lightness",
                self.lightness,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::Lightness(v)).into(),
            ),
        ])
    }
}
