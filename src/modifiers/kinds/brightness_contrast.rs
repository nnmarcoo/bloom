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
pub struct BrightnessContrast {
    pub brightness: f32,
    pub contrast: f32,
}

impl ModifierImpl for BrightnessContrast {
    fn name(&self) -> &'static str {
        "Brightness & Contrast"
    }

    fn has_effect(&self) -> bool {
        self.brightness != 0.0 || self.contrast != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Brightness(v) => self.brightness = v,
            ModifierParam::Contrast(v) => self.contrast = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::BRIGHTNESS_CONTRAST,
            &[self.brightness, self.contrast],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        for v in c.iter_mut().take(3) {
            *v = (*v + self.brightness - 0.5) * (1.0 + self.contrast) + 0.5;
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        2u8.hash(hasher);
        hash_f32(self.brightness, hasher);
        hash_f32(self.contrast, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Brightness",
                self.brightness,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::Brightness(v)).into(),
            ),
            value_row(
                "Contrast",
                self.contrast,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| EditMsg::Update(index, ModifierParam::Contrast(v)).into(),
            ),
        ])
    }
}
