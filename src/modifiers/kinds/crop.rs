//! Crop: selects a rectangle of its input and outputs only that.
//!
//! The rect is stored in the pixels of the stage the crop sits at, so a crop
//! placed after a resize is measured in the resized image. That is what lets a
//! stack crop, apply an effect, and crop again: the second crop names a region
//! of what the first one produced, which a fraction of the final document
//! cannot express.
//!
//! rect_in resolves the stored rect against the stage's real input, so a crop
//! that outlives an upstream size change still names a region that exists.
//! output_spec is that rect's extent, which is how the planner learns the
//! document shrank.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::plan::ImageSpec;
use crate::modifiers::{ModifierImpl, ModifierParam, ViewCtx};
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
            width: f32::MAX,
            height: f32::MAX,
        }
    }
}

impl Crop {
    pub fn rect_in(&self, input: ImageSpec) -> (f32, f32, f32, f32) {
        let (iw, ih) = (input.w as f32, input.h as f32);
        let x = self.x.max(0.0).min((iw - 1.0).max(0.0));
        let y = self.y.max(0.0).min((ih - 1.0).max(0.0));
        let w = self.width.max(1.0).min(iw - x);
        let h = self.height.max(1.0).min(ih - y);
        (x, y, w, h)
    }
}

impl ModifierImpl for Crop {
    fn name(&self) -> &'static str {
        "Crop"
    }

    fn output_spec(&self, input: ImageSpec) -> ImageSpec {
        let (_, _, w, h) = self.rect_in(input);
        ImageSpec::new(w.round() as u32, h.round() as u32)
    }

    fn changes_geometry(&self) -> bool {
        true
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

    fn view(&self, index: usize, ctx: ViewCtx) -> Element<'_, Message> {
        let (cx, cy, cw, ch) = (self.x, self.y, self.width, self.height);
        let (iw, ih) = ctx
            .image_size
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((cx + cw, cy + ch));
        let swapped = ctx.rotation % 2 == 1;
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
