use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{LUMA, finish, hash_f32, value_row};

#[derive(Debug, Clone, Default)]
pub struct Vibrance {
    pub vibrance: f32,
    pub saturation: f32,
}

impl ModifierImpl for Vibrance {
    fn name(&self) -> &'static str {
        "Vibrance"
    }

    fn has_effect(&self) -> bool {
        self.vibrance != 0.0 || self.saturation != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Vibrance(v) => self.vibrance = v,
            ModifierParam::VibranceSaturation(v) => self.saturation = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::VIBRANCE, &[self.vibrance, self.saturation]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let cc = [
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        ];
        let luma = cc[0] * LUMA[0] + cc[1] * LUMA[1] + cc[2] * LUMA[2];
        let max_c = cc[0].max(cc[1]).max(cc[2]);
        let sat_proxy = max_c - cc[0].min(cc[1]).min(cc[2]);
        let vib_amount = self.vibrance * (1.0 - sat_proxy);
        for (v, &base) in c.iter_mut().take(3).zip(cc.iter()) {
            let after_vib = luma + (base - luma) * (1.0 + vib_amount);
            *v = luma + (after_vib - luma) * (1.0 + self.saturation);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        5u8.hash(hasher);
        hash_f32(self.vibrance, hasher);
        hash_f32(self.saturation, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Vibrance",
                self.vibrance,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::Vibrance(v)).into(),
            ),
            value_row(
                "Saturation",
                self.saturation,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::VibranceSaturation(v)).into(),
            ),
        ])
    }
}
