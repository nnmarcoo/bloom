use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::{Fmt, Track};

use super::{finish, gradient_row, hash_f32};

#[derive(Debug, Clone, Default)]
pub struct ColorBalance {
    pub cyan_red: f32,
    pub magenta_green: f32,
    pub yellow_blue: f32,
}

impl ModifierImpl for ColorBalance {
    fn name(&self) -> &'static str {
        "Color Balance"
    }

    fn has_effect(&self) -> bool {
        self.cyan_red != 0.0 || self.magenta_green != 0.0 || self.yellow_blue != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::ColorBalanceCyanRed(v) => self.cyan_red = v,
            ModifierParam::ColorBalanceMagentaGreen(v) => self.magenta_green = v,
            ModifierParam::ColorBalanceYellowBlue(v) => self.yellow_blue = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::COLOR_BALANCE,
            &[self.cyan_red, self.magenta_green, self.yellow_blue],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        c[0] += self.cyan_red;
        c[1] += self.magenta_green;
        c[2] += self.yellow_blue;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        6u8.hash(hasher);
        hash_f32(self.cyan_red, hasher);
        hash_f32(self.magenta_green, hasher);
        hash_f32(self.yellow_blue, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            gradient_row(
                "Cyan / Red",
                self.cyan_red,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                Track::cyan_red(),
                move |v| EditMsg::Update(index, ModifierParam::ColorBalanceCyanRed(v)).into(),
            ),
            gradient_row(
                "Mag / Green",
                self.magenta_green,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                Track::magenta_green(),
                move |v| EditMsg::Update(index, ModifierParam::ColorBalanceMagentaGreen(v)).into(),
            ),
            gradient_row(
                "Yel / Blue",
                self.yellow_blue,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                Track::yellow_blue(),
                move |v| EditMsg::Update(index, ModifierParam::ColorBalanceYellowBlue(v)).into(),
            ),
        ])
    }
}
