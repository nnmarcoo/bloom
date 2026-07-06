mod brightness_contrast;
mod chromatic_aberration;
mod color_balance;
mod crop;
mod drawing;
mod duotone;
mod exposure;
mod gaussian_blur;
mod grain;
mod grayscale;
mod halftone;
mod hue_saturation;
mod invert;
mod levels;
mod motion_blur;
mod pixel_sort;
mod posterize;
mod radial_blur;
mod sepia;
mod solarize;
mod temperature;
mod text;
mod threshold;
mod vibrance;
mod vignette;

pub use brightness_contrast::BrightnessContrast;
pub use chromatic_aberration::ChromaticAberration;
pub use color_balance::ColorBalance;
pub use crop::Crop;
pub use drawing::{Drawing, Stroke};
pub use duotone::Duotone;
pub use exposure::Exposure;
pub use gaussian_blur::GaussianBlur;
pub use grain::Grain;
pub use grayscale::Grayscale;
pub use halftone::Halftone;
pub use hue_saturation::HueSaturation;
pub use invert::Invert;
pub use levels::Levels;
pub use motion_blur::{MotionBlur, motion_blur_samples};
pub use pixel_sort::PixelSort;
pub use posterize::Posterize;
pub use radial_blur::RadialBlur;
pub use sepia::Sepia;
pub use solarize::Solarize;
pub use temperature::Temperature;
pub use text::Text;
pub use threshold::Threshold;
pub use vibrance::Vibrance;
pub use vignette::Vignette;

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::ops::RangeInclusive;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::{Column, row};
use iced::{Element, Length};

use crate::app::Message;
use crate::widgets::angle_dial::AngleDial;
use crate::widgets::number_entry::NumberEntry;
use crate::widgets::value_slider::{Fmt, Track, ValueSlider};

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

fn hash_f32(v: f32, hasher: &mut DefaultHasher) {
    v.to_bits().hash(hasher);
}

fn clamped_luma(c: [f32; 4]) -> f32 {
    c[0].clamp(0.0, 1.0) * LUMA[0] + c[1].clamp(0.0, 1.0) * LUMA[1] + c[2].clamp(0.0, 1.0) * LUMA[2]
}

fn finish(col: Column<'_, Message>) -> Element<'_, Message> {
    col.spacing(6).into()
}

fn value_row<'a>(
    label: &'a str,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    fmt: Fmt,
    on_change: impl Fn(f32) -> Message + 'static,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        ValueSlider::new(value, range, on_change)
            .step(step)
            .format(fmt),
    ]
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn number_row<'a>(
    label: &'a str,
    value: f32,
    min: f32,
    step: f32,
    suffix: &'static str,
    on_change: impl Fn(f32) -> Message + 'static,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        iced::widget::container(
            NumberEntry::new(value, on_change)
                .range(min, f32::INFINITY)
                .step(step)
                .suffix(suffix)
                .width(70.0)
        )
        .center_x(Length::Fill),
    ]
    .width(Length::Fill)
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn gradient_row<'a>(
    label: &'a str,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    fmt: Fmt,
    track: Track,
    on_change: impl Fn(f32) -> Message + 'static,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        ValueSlider::new(value, range, on_change)
            .step(step)
            .format(fmt)
            .track(track),
    ]
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn color_row<'a>(
    label: &'a str,
    rgb: [f32; 3],
    on_change: impl Fn([f32; 3]) -> Message + 'static,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        iced::widget::container(crate::widgets::color_swatch::ColorSwatch::new(
            rgb[0], rgb[1], rgb[2], on_change,
        ))
        .center_x(Length::Fill),
    ]
    .width(Length::Fill)
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn angle_row<'a>(
    label: &'a str,
    value: f32,
    range: RangeInclusive<f32>,
    on_change: impl Fn(f32) -> Message + Clone + 'static,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        AngleDial::new(value, on_change.clone()),
        ValueSlider::new(value, range, on_change)
            .step(0.5)
            .format(Fmt::num(0).suffix("\u{00b0}")),
    ]
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}
