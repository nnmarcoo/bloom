use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::cpu::hash21;
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::number_entry::NumberEntry;
use crate::widgets::value_slider::Fmt;

use super::{clamped_luma, finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Grain {
    pub amount: f32,
    pub size: f32,
    pub seed: f32,
    pub color: f32,
    pub response: f32,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            amount: 0.2,
            size: 1.0,
            seed: 0.0,
            color: 0.0,
            response: 0.5,
        }
    }
}

impl ModifierImpl for Grain {
    fn name(&self) -> &'static str {
        "Grain"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::GrainAmount(v) => self.amount = v,
            ModifierParam::GrainSize(v) => self.size = v,
            ModifierParam::GrainSeed(v) => self.seed = v,
            ModifierParam::GrainColor(v) => self.color = v,
            ModifierParam::GrainResponse(v) => self.response = v,
            _ => {}
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::GRAIN,
            &[
                self.amount,
                self.size,
                self.seed,
                tile.tile_x as f32,
                tile.tile_y as f32,
                tile.tile_w as f32,
                tile.tile_h as f32,
                self.color,
                self.response,
            ],
        ))
    }

    fn apply_cpu(&self, img_w: u32, img_h: u32, uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let gx = uv[0] * img_w as f32 / self.size.max(0.5);
        let gy = uv[1] * img_h as f32 / self.size.max(0.5);
        let iseed = self.seed as i32;
        let (cx, cy) = (gx.floor() as i32, gy.floor() as i32);
        let (fx, fy) = (gx.fract(), gy.fract());
        let wx = fx * fx * (3.0 - 2.0 * fx);
        let wy = fy * fy * (3.0 - 2.0 * fy);
        let sample = |seed: i32| {
            let n00 = hash21(cx, cy, seed);
            let n10 = hash21(cx + 1, cy, seed);
            let n01 = hash21(cx, cy + 1, seed);
            let n11 = hash21(cx + 1, cy + 1, seed);
            (n00 * (1.0 - wx) + n10 * wx) * (1.0 - wy) + (n01 * (1.0 - wx) + n11 * wx) * wy
        };
        let mono = sample(iseed);
        let color = self.color.clamp(0.0, 1.0);
        let noise = [
            mono + (sample(iseed + 101) - mono) * color,
            mono + (sample(iseed + 211) - mono) * color,
            mono + (sample(iseed + 307) - mono) * color,
        ];
        let luma = clamped_luma(c);
        let response = self.response.clamp(0.0, 1.0);
        let luma_weight = 1.0 + (4.0 * luma * (1.0 - luma) - 1.0) * response;
        for (v, n) in c.iter_mut().take(3).zip(noise) {
            *v += (n - 0.5) * self.amount * luma_weight;
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        16u8.hash(hasher);
        hash_f32(self.amount, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.seed, hasher);
        hash_f32(self.color, hasher);
        hash_f32(self.response, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Amount",
                self.amount,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::GrainAmount(v)).into(),
            ),
            value_row(
                "Size",
                self.size,
                0.5..=32.0,
                0.5,
                Fmt::num(1).suffix("px"),
                move |v| EditMsg::Update(index, ModifierParam::GrainSize(v)).into(),
            ),
            value_row(
                "Response",
                self.response,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::GrainResponse(v)).into(),
            ),
            value_row(
                "Color",
                self.color,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::GrainColor(v)).into(),
            ),
            iced::widget::row![
                iced::widget::text("Seed")
                    .size(10)
                    .width(iced::Length::Fixed(58.0)),
                iced::widget::container(
                    NumberEntry::new(self.seed, move |v| EditMsg::Update(
                        index,
                        ModifierParam::GrainSeed(v)
                    )
                    .into())
                    .range(0.0, 9999.0)
                    .step(1.0)
                    .width(70.0)
                )
                .center_x(iced::Length::Fill),
            ]
            .width(iced::Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .spacing(4),
        ])
    }
}
