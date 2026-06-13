use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{finish, value_row};

#[derive(Debug, Clone)]
pub struct Posterize {
    pub levels: u32,
}

impl Default for Posterize {
    fn default() -> Self {
        Self { levels: 4 }
    }
}

impl ModifierImpl for Posterize {
    fn name(&self) -> &'static str {
        "Posterize"
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::PosterizeLevels(v) = param {
            self.levels = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::POSTERIZE, &[self.levels as f32]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let l = (self.levels as f32 - 1.0).max(1.0);
        for v in c.iter_mut().take(3) {
            *v = ((*v).clamp(0.0, 1.0) * l + 0.5).floor() / l;
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        14u8.hash(hasher);
        self.levels.hash(hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Levels",
            self.levels as f32,
            2.0..=32.0,
            1.0,
            Fmt::num(0),
            move |v| EditMsg::Update(index, ModifierParam::PosterizeLevels(v.round() as u32))
                .into(),
        )])
    }
}
