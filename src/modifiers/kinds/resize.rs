//! Resize: the one modifier that changes the image's dimensions.
//!
//! The target is stored as a mode plus two numbers, never as resolved pixels,
//! so a percentage stays a percentage and follows an upstream size change.
//! output_for resolves it at plan time against whatever the stage's actual
//! input turns out to be.
//!
//! The filter is chosen per instance rather than inferred from the scale
//! direction: a lo-fi upscale wants Nearest precisely because it is blocky,
//! and that intent cannot be recovered from the numbers.
//!
//! Both backends honor a resize at any position in the chain. The GPU preview
//! renders it as a stage like any other, so the viewport shows the resized
//! document rather than the source size, and the view refits when an edit
//! changes those dimensions.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::plan::ImageSpec;
use crate::modifiers::{InputRequest, ModifierImpl, ModifierParam, ViewCtx};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, picker_row, toggle_row, value_row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeMode {
    Pixels,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeFilter {
    Nearest,
    Bilinear,
    Lanczos,
}

impl ResizeFilter {
    pub const ALL: [ResizeFilter; 3] = [
        ResizeFilter::Nearest,
        ResizeFilter::Bilinear,
        ResizeFilter::Lanczos,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ResizeFilter::Nearest => "Nearest",
            ResizeFilter::Bilinear => "Bilinear",
            ResizeFilter::Lanczos => "Lanczos",
        }
    }

    pub fn radius(&self) -> f32 {
        match self {
            ResizeFilter::Nearest => 0.0,
            ResizeFilter::Bilinear => 1.0,
            ResizeFilter::Lanczos => 3.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resize {
    pub mode: ResizeMode,
    pub width: f32,
    pub height: f32,
    pub filter: ResizeFilter,
    pub lock_aspect: bool,
}

impl Default for Resize {
    fn default() -> Self {
        Self {
            mode: ResizeMode::Percent,
            width: 100.0,
            height: 100.0,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }
    }
}

impl Resize {
    pub fn output_for(&self, input: ImageSpec) -> ImageSpec {
        let (w, h) = match self.mode {
            ResizeMode::Pixels => (self.width, self.height),
            ResizeMode::Percent => (
                input.w as f32 * self.width / 100.0,
                input.h as f32 * self.height / 100.0,
            ),
        };
        let w = w.round().max(1.0) as u32;
        let h = if self.lock_aspect {
            let ratio = input.h as f32 / input.w.max(1) as f32;
            (w as f32 * ratio).round().max(1.0) as u32
        } else {
            h.round().max(1.0) as u32
        };
        ImageSpec::new(w, h)
    }

    fn aspect_ratio(&self, img_size: Option<(u32, u32)>) -> f32 {
        match self.mode {
            ResizeMode::Percent => 1.0,
            ResizeMode::Pixels => match img_size {
                Some((iw, ih)) if iw > 0 => ih as f32 / iw as f32,
                _ if self.width > 0.0 => self.height / self.width,
                _ => 1.0,
            },
        }
    }

    pub fn is_identity_for(&self, input: ImageSpec) -> bool {
        self.output_for(input) == input
    }
}

impl ModifierImpl for Resize {
    fn name(&self) -> &'static str {
        "Resize"
    }

    fn has_effect(&self) -> bool {
        true
    }

    fn input_request(&self) -> InputRequest {
        InputRequest::FullFrame
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::ResizeWidth(v) => {
                let v = v.max(1.0);
                if self.lock_aspect {
                    let ratio = self.aspect_ratio(_img_size);
                    self.height = (v * ratio).max(1.0);
                }
                self.width = v;
            }
            ModifierParam::ResizeHeight(v) => {
                let v = v.max(1.0);
                if self.lock_aspect {
                    let ratio = self.aspect_ratio(_img_size);
                    if ratio > 0.0 {
                        self.width = (v / ratio).max(1.0);
                    }
                }
                self.height = v;
            }
            ModifierParam::ResizeFilter(f) => self.filter = f,
            ModifierParam::ResizeMode(m) if m != self.mode => {
                match (m, _img_size) {
                    (ResizeMode::Pixels, Some((iw, ih))) => {
                        self.width = (iw as f32 * self.width / 100.0).max(1.0).round();
                        self.height = (ih as f32 * self.height / 100.0).max(1.0).round();
                    }
                    (ResizeMode::Percent, Some((iw, ih))) => {
                        self.width = (self.width / iw.max(1) as f32 * 100.0).max(0.1);
                        self.height = (self.height / ih.max(1) as f32 * 100.0).max(0.1);
                    }
                    (ResizeMode::Percent, None) => {
                        self.width = 100.0;
                        self.height = 100.0;
                    }
                    (ResizeMode::Pixels, None) => {}
                }
                self.mode = m;
            }
            ModifierParam::ResizeLockAspect(v) => self.lock_aspect = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        20u8.hash(hasher);
        (self.mode as u8).hash(hasher);
        hash_f32(self.width, hasher);
        hash_f32(self.height, hasher);
        (self.filter as u8).hash(hasher);
        self.lock_aspect.hash(hasher);
    }

    fn view(&self, index: usize, ctx: ViewCtx) -> Element<'_, Message> {
        let (max_w, max_h) = match self.mode {
            ResizeMode::Pixels => {
                let (iw, ih) = ctx.image_size.unwrap_or((8192, 8192));
                ((iw as f32 * 4.0).max(1.0), (ih as f32 * 4.0).max(1.0))
            }
            ResizeMode::Percent => (400.0, 400.0),
        };
        let fmt = match self.mode {
            ResizeMode::Pixels => Fmt::num(0),
            ResizeMode::Percent => Fmt::num(1),
        };
        let unit = match self.mode {
            ResizeMode::Pixels => "Width (px)",
            ResizeMode::Percent => "Width (%)",
        };

        const MODES: [(ResizeMode, &str); 2] =
            [(ResizeMode::Percent, "%"), (ResizeMode::Pixels, "px")];
        const FILTERS: [(ResizeFilter, &str); 3] = [
            (ResizeFilter::Nearest, "Nearest"),
            (ResizeFilter::Bilinear, "Bilinear"),
            (ResizeFilter::Lanczos, "Lanczos"),
        ];

        let mut rows = column![
            picker_row("Units", &MODES, self.mode, move |m| {
                EditMsg::Update(index, ModifierParam::ResizeMode(m)).into()
            }),
            value_row(unit, self.width, 1.0..=max_w, 1.0, fmt, move |v| {
                EditMsg::Update(index, ModifierParam::ResizeWidth(v)).into()
            }),
        ];

        let height_label = match self.mode {
            ResizeMode::Pixels => "Height (px)",
            ResizeMode::Percent => "Height (%)",
        };
        rows = rows.push(value_row(
            height_label,
            self.height,
            1.0..=max_h,
            1.0,
            fmt,
            move |v| EditMsg::Update(index, ModifierParam::ResizeHeight(v)).into(),
        ));

        rows = rows.push(toggle_row("Lock ratio", self.lock_aspect, move |v| {
            EditMsg::Update(index, ModifierParam::ResizeLockAspect(v)).into()
        }));

        rows = rows.push(picker_row("Filter", &FILTERS, self.filter, move |f| {
            EditMsg::Update(index, ModifierParam::ResizeFilter(f)).into()
        }));

        finish(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::ModifierImpl;

    fn pct(w: f32, h: f32, lock: bool) -> Resize {
        Resize {
            mode: ResizeMode::Percent,
            width: w,
            height: h,
            filter: ResizeFilter::Lanczos,
            lock_aspect: lock,
        }
    }

    #[test]
    fn percent_scales_both_axes() {
        let r = pct(50.0, 50.0, false);
        assert_eq!(
            r.output_for(ImageSpec::new(800, 600)),
            ImageSpec::new(400, 300)
        );
    }

    #[test]
    fn locked_aspect_derives_height_from_width() {
        let r = pct(50.0, 999.0, true);
        assert_eq!(
            r.output_for(ImageSpec::new(800, 600)),
            ImageSpec::new(400, 300)
        );
    }

    #[test]
    fn unlocked_aspect_honours_both_fields() {
        let r = pct(50.0, 25.0, false);
        assert_eq!(
            r.output_for(ImageSpec::new(800, 600)),
            ImageSpec::new(400, 150)
        );
    }

    #[test]
    fn pixel_mode_is_absolute() {
        let r = Resize {
            mode: ResizeMode::Pixels,
            width: 320.0,
            height: 240.0,
            filter: ResizeFilter::Bilinear,
            lock_aspect: false,
        };
        assert_eq!(
            r.output_for(ImageSpec::new(800, 600)),
            ImageSpec::new(320, 240)
        );
    }

    #[test]
    fn output_never_collapses_to_zero() {
        let r = pct(0.001, 0.001, false);
        assert_eq!(r.output_for(ImageSpec::new(10, 10)), ImageSpec::new(1, 1));
    }

    #[test]
    fn switching_units_preserves_the_resulting_size() {
        let src = ImageSpec::new(800, 600);
        let mut r = pct(50.0, 50.0, false);
        let before = r.output_for(src);

        r.apply_param(
            ModifierParam::ResizeMode(ResizeMode::Pixels),
            Some((800, 600)),
        );
        assert_eq!(r.mode, ResizeMode::Pixels);
        assert_eq!(
            r.output_for(src),
            before,
            "percent -> pixels changed the size"
        );

        r.apply_param(
            ModifierParam::ResizeMode(ResizeMode::Percent),
            Some((800, 600)),
        );
        assert_eq!(r.mode, ResizeMode::Percent);
        assert_eq!(
            r.output_for(src),
            before,
            "pixels -> percent changed the size"
        );
    }

    #[test]
    fn setting_the_same_mode_is_inert() {
        let mut r = pct(50.0, 50.0, false);
        r.apply_param(
            ModifierParam::ResizeMode(ResizeMode::Percent),
            Some((800, 600)),
        );
        assert_eq!(r.width, 50.0);
        assert_eq!(r.height, 50.0);
    }

    #[test]
    fn locked_width_edit_drives_height_in_percent() {
        let mut r = pct(100.0, 100.0, true);
        r.apply_param(ModifierParam::ResizeWidth(50.0), Some((800, 600)));
        assert_eq!(r.width, 50.0);
        assert_eq!(r.height, 50.0, "percent fields are equal scale factors");
    }

    #[test]
    fn locked_height_edit_drives_width_in_percent() {
        let mut r = pct(100.0, 100.0, true);
        r.apply_param(ModifierParam::ResizeHeight(25.0), Some((800, 600)));
        assert_eq!(r.height, 25.0);
        assert_eq!(r.width, 25.0);
    }

    #[test]
    fn locked_edits_follow_the_image_ratio_in_pixels() {
        let mut r = Resize {
            mode: ResizeMode::Pixels,
            width: 800.0,
            height: 600.0,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        };
        r.apply_param(ModifierParam::ResizeWidth(400.0), Some((800, 600)));
        assert_eq!(r.width, 400.0);
        assert_eq!(r.height, 300.0, "3:4 ratio preserved");

        r.apply_param(ModifierParam::ResizeHeight(150.0), Some((800, 600)));
        assert_eq!(r.height, 150.0);
        assert_eq!(r.width, 200.0);
    }

    #[test]
    fn unlocked_edits_do_not_couple() {
        let mut r = pct(100.0, 100.0, false);
        r.apply_param(ModifierParam::ResizeWidth(50.0), Some((800, 600)));
        assert_eq!(r.width, 50.0);
        assert_eq!(r.height, 100.0, "height must not move while unlocked");
    }

    #[test]
    fn lock_aspect_is_settable() {
        let mut r = pct(50.0, 50.0, true);
        r.apply_param(ModifierParam::ResizeLockAspect(false), None);
        assert!(!r.lock_aspect);
        r.apply_param(ModifierParam::ResizeLockAspect(true), None);
        assert!(r.lock_aspect);
    }

    #[test]
    fn filter_is_settable() {
        let mut r = pct(50.0, 50.0, true);
        for f in ResizeFilter::ALL {
            r.apply_param(ModifierParam::ResizeFilter(f), None);
            assert_eq!(r.filter, f);
        }
    }

    #[test]
    fn identity_resize_reports_itself() {
        let src = ImageSpec::new(640, 480);
        assert!(pct(100.0, 100.0, true).is_identity_for(src));
        assert!(!pct(50.0, 50.0, true).is_identity_for(src));
    }
}
