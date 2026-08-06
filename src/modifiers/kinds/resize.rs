use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::plan::ImageSpec;
use crate::modifiers::{InputRequest, ModifierImpl, ModifierParam, ViewCtx};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

/// How a resize decides its target dimensions.
///
/// Stored as a mode rather than as resolved pixels so a percentage stays a
/// percentage: if an earlier stage changes size, a `Percent` resize follows it,
/// while `Pixels` pins an absolute target. Resolution happens in
/// [`Resize::output_for`], at plan time, against whatever the stage's actual
/// input turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeMode {
    Pixels,
    Percent,
}

/// The resampling kernel, chosen per instance.
///
/// Deliberately not inferred from the scale direction: downscaling usually
/// wants `Lanczos`, but a lo-fi upscale wants `Nearest` precisely *because* it
/// is blocky, and that intent cannot be recovered from the numbers.
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
}

#[derive(Debug, Clone)]
pub struct Resize {
    pub mode: ResizeMode,
    /// Target width in pixels (`Pixels`) or percent of input (`Percent`).
    pub width: f32,
    pub height: f32,
    pub filter: ResizeFilter,
    /// Preserve the input's aspect ratio, deriving height from width.
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
    /// The dimensions this resize produces from `input`.
    ///
    /// Resolved late and never cached: a `Percent` resize must track upstream
    /// changes, and pinning its output at edit time would silently decouple it.
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
            // Derive from width so the ratio survives rounding on both axes.
            let ratio = input.h as f32 / input.w.max(1) as f32;
            (w as f32 * ratio).round().max(1.0) as u32
        } else {
            h.round().max(1.0) as u32
        };
        ImageSpec::new(w, h)
    }

    /// True when this resize would leave its input untouched.
    ///
    /// Only used to skip work; it is never used to *remove* a resize from the
    /// chain, because two resizes that individually cancel out do not cancel
    /// jointly -- 50% then 200% is not a no-op, and the information lost in
    /// between is usually the point.
    pub fn is_identity_for(&self, input: ImageSpec) -> bool {
        self.output_for(input) == input
    }
}

impl ModifierImpl for Resize {
    fn name(&self) -> &'static str {
        "Resize"
    }

    fn has_effect(&self) -> bool {
        // Geometry-only: the dimension change is declared through the plan's
        // `output_spec`, not produced by a pixel pass here. A resize whose
        // target equals its input is still kept in the plan so the chain's
        // geometry stays explicit.
        true
    }

    fn input_request(&self) -> InputRequest {
        // Resampling reads a neighbourhood whose extent depends on the scale
        // factor, which is not known here. `FullFrame` is the honest answer:
        // a resize is a chain-wide geometry change, not a local kernel.
        InputRequest::FullFrame
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::ResizeWidth(v) => self.width = v.max(1.0),
            ModifierParam::ResizeHeight(v) => self.height = v.max(1.0),
            ModifierParam::ResizeFilter(f) => self.filter = f,
            ModifierParam::ResizeMode(m) => {
                // Switching units keeps the visual size rather than the number,
                // which is what "100" meaning two different things demands.
                self.mode = m;
                match m {
                    ResizeMode::Percent => {
                        self.width = 100.0;
                        self.height = 100.0;
                    }
                    ResizeMode::Pixels => {}
                }
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

        let mut rows = column![value_row(
            "Width",
            self.width,
            1.0..=max_w,
            1.0,
            fmt,
            move |v| EditMsg::Update(index, ModifierParam::ResizeWidth(v)).into(),
        )];
        if !self.lock_aspect {
            rows = rows.push(value_row(
                "Height",
                self.height,
                1.0..=max_h,
                1.0,
                fmt,
                move |v| EditMsg::Update(index, ModifierParam::ResizeHeight(v)).into(),
            ));
        }
        finish(rows)
    }
}
