use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone, Default)]
pub struct Exposure {
    pub exposure: f32,
}

impl ModifierImpl for Exposure {
    fn name(&self) -> &'static str {
        "Exposure"
    }

    fn has_effect(&self) -> bool {
        self.exposure != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::Exposure(v) = param {
            self.exposure = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::EXPOSURE, &[self.exposure]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let scale = 2.0f32.powf(self.exposure);
        c[0] *= scale;
        c[1] *= scale;
        c[2] *= scale;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        4u8.hash(hasher);
        hash_f32(self.exposure, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Exposure",
            self.exposure,
            -5.0..=5.0,
            0.01,
            Fmt::signed(2),
            move |v| EditMsg::Update(index, ModifierParam::Exposure(v)).into(),
        )])
    }
}
