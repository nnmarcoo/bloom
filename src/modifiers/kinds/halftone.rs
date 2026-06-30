use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::value_slider::Fmt;

use super::{clamped_luma, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Halftone {
    pub size: f32,
    pub angle: f32,
}

impl Default for Halftone {
    fn default() -> Self {
        Self {
            size: 10.0,
            angle: 45.0,
        }
    }
}

impl ModifierImpl for Halftone {
    fn name(&self) -> &'static str {
        "Halftone"
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::HalftoneSize(v) => self.size = v,
            ModifierParam::HalftoneAngle(v) => self.angle = v,
            _ => {}
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::HALFTONE,
            &[
                self.size / tile.full_w.min(tile.full_h) as f32,
                self.angle * PI / 180.0,
                0.0,
                0.0,
                0.0,
                0.0,
                self.size,
            ],
        ))
    }

    fn apply_cpu(&self, img_w: u32, img_h: u32, uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let angle_rad = self.angle * PI / 180.0;
        let cs = angle_rad.cos();
        let sn = angle_rad.sin();
        let period = (self.size / img_w.min(img_h) as f32).max(0.001);
        let rot_x = (uv[0] * cs - uv[1] * sn) / period;
        let rot_y = (uv[0] * sn + uv[1] * cs) / period;
        let cell_x = rot_x.floor() + 0.5;
        let cell_y = rot_y.floor() + 0.5;
        let dist = ((rot_x - cell_x).powi(2) + (rot_y - cell_y).powi(2)).sqrt();
        let luma = clamped_luma(c);
        let radius = luma.sqrt() * 0.5;
        let aa = 1.0 / self.size.max(1.0);
        let t = ((dist - (radius - aa)) / (2.0 * aa)).clamp(0.0, 1.0);
        let v = 1.0 - t * t * (3.0 - 2.0 * t);
        c[0] = v;
        c[1] = v;
        c[2] = v;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        10u8.hash(hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.angle, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row("Size", self.size, 2.0..=50.0, 0.1, Fmt::num(0), move |v| {
                EditMsg::Update(index, ModifierParam::HalftoneSize(v)).into()
            },),
            value_row(
                "Angle",
                self.angle,
                0.0..=90.0,
                0.5,
                Fmt::num(0).suffix("\u{00b0}"),
                move |v| EditMsg::Update(index, ModifierParam::HalftoneAngle(v)).into(),
            ),
        ])
    }
}
