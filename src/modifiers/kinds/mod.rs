//! The modifier implementations, plus the shared row builders their panels use.
//!
//! value_row, toggle_row, and picker_row exist so a parameter's control matches
//! the shape of its data: a magnitude gets a slider, a state gets a switch, and
//! a choice from a closed set gets a dropdown. Rendering all three as sliders
//! read wrong for the latter two.

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
mod resize;
mod sepia;
mod solarize;
mod temperature;
mod text;
mod threshold;
mod trim;
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
pub use resize::{Resize, ResizeFilter, ResizeMode};
pub use sepia::Sepia;
pub use solarize::Solarize;
pub use temperature::Temperature;
pub use text::Text;
pub use threshold::Threshold;
pub use trim::Trim;
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

fn toggle_row<'a>(
    label: &'a str,
    value: bool,
    on_change: impl Fn(bool) -> Message + 'static,
) -> Element<'a, Message> {
    const TRACK_W: f32 = 26.0;
    const TRACK_H: f32 = 14.0;
    const KNOB: f32 = 10.0;

    let knob = iced::widget::container(
        iced::widget::Space::new()
            .width(Length::Fixed(KNOB))
            .height(Length::Fixed(KNOB)),
    )
    .style(move |theme: &iced::Theme| {
        let palette = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(iced::Background::Color(if value {
                palette.primary.base.text
            } else {
                palette.background.base.text.scale_alpha(0.75)
            })),
            border: iced::border::rounded(KNOB / 2.0),
            ..Default::default()
        }
    });

    let track_inner = if value {
        row![iced::widget::Space::new().width(Length::Fill), knob]
    } else {
        row![knob, iced::widget::Space::new().width(Length::Fill)]
    };

    let track = iced::widget::container(track_inner.align_y(Vertical::Center))
        .width(Length::Fixed(TRACK_W))
        .height(Length::Fixed(TRACK_H))
        .padding([2, 2])
        .style(move |theme: &iced::Theme| {
            let palette = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(iced::Background::Color(if value {
                    palette.primary.base.color
                } else {
                    palette.background.strong.color
                })),
                border: iced::border::rounded(TRACK_H / 2.0),
                ..Default::default()
            }
        });

    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        iced::widget::button(track)
            .padding(0)
            .on_press_with(move || on_change(!value))
            .style(|_theme, _status| iced::widget::button::Style::default()),
    ]
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn picker_row<'a, T: Copy + PartialEq + 'a>(
    label: &'a str,
    options: &'a [(T, &'a str)],
    selected: T,
    on_change: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        iced::widget::text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        crate::widgets::option_picker::OptionPicker::new(options, selected, on_change),
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
