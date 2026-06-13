use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl ModifierImpl for Crop {
    fn name(&self) -> &'static str {
        "Crop"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::CropX(v) => {
                let right = self.x + self.width;
                self.x = v.round().clamp(0.0, right - 1.0);
                self.width = (right - self.x).max(1.0);
            }
            ModifierParam::CropY(v) => {
                let bottom = self.y + self.height;
                self.y = v.round().clamp(0.0, bottom - 1.0);
                self.height = (bottom - self.y).max(1.0);
            }
            ModifierParam::CropWidth(v) => {
                self.width = v.round().max(1.0);
                if let Some((iw, _)) = img_size {
                    self.width = self.width.min(iw as f32 - self.x);
                }
            }
            ModifierParam::CropHeight(v) => {
                self.height = v.round().max(1.0);
                if let Some((_, ih)) = img_size {
                    self.height = self.height.min(ih as f32 - self.y);
                }
            }
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        17u8.hash(hasher);
        hash_f32(self.x, hasher);
        hash_f32(self.y, hasher);
        hash_f32(self.width, hasher);
        hash_f32(self.height, hasher);
    }

    fn view(
        &self,
        index: usize,
        image_size: Option<(u32, u32)>,
        rotation: u8,
    ) -> Element<'_, Message> {
        let (cx, cy, cw, ch) = (self.x, self.y, self.width, self.height);
        let (iw, ih) = image_size
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((cx + cw, cy + ch));
        let swapped = rotation % 2 == 1;
        let (vis_w, vis_h) = if swapped { (ch, cw) } else { (cw, ch) };
        let (vis_w_max, vis_h_max) = if swapped { (ih, iw) } else { (iw, ih) };
        let w_msg = move |v| {
            EditMsg::Update(
                index,
                if swapped {
                    ModifierParam::CropHeight(v)
                } else {
                    ModifierParam::CropWidth(v)
                },
            )
            .into()
        };
        let h_msg = move |v| {
            EditMsg::Update(
                index,
                if swapped {
                    ModifierParam::CropWidth(v)
                } else {
                    ModifierParam::CropHeight(v)
                },
            )
            .into()
        };
        finish(column![
            value_row(
                "X",
                cx,
                0.0..=(iw - 1.0).max(0.0),
                1.0,
                Fmt::num(0),
                move |v| EditMsg::Update(index, ModifierParam::CropX(v)).into(),
            ),
            value_row(
                "Y",
                cy,
                0.0..=(ih - 1.0).max(0.0),
                1.0,
                Fmt::num(0),
                move |v| EditMsg::Update(index, ModifierParam::CropY(v)).into(),
            ),
            value_row(
                "Width",
                vis_w,
                1.0..=vis_w_max.max(1.0),
                1.0,
                Fmt::num(0),
                w_msg
            ),
            value_row(
                "Height",
                vis_h,
                1.0..=vis_h_max.max(1.0),
                1.0,
                Fmt::num(0),
                h_msg
            ),
        ])
    }
}
