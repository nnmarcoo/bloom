use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::Hash;
use std::ops::RangeInclusive;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::{Column, column, row, text, text_input};
use iced::{Element, Length};

use crate::app::Message;
use crate::modifiers::cpu::{hash21, hsl_to_rgb, rgb_to_hsl};
use crate::modifiers::gpu::{ModEntry, TileInfo, make_entry};
use crate::modifiers::{ModifierImpl, ModifierParam, ids};
use crate::widgets::angle_dial::AngleDial;
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
        text(label)
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
        text(label)
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

fn angle_row<'a>(
    label: &'a str,
    value: f32,
    range: RangeInclusive<f32>,
    on_change: impl Fn(f32) -> Message + Clone + 'static,
) -> Element<'a, Message> {
    row![
        text(label)
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

#[derive(Debug, Clone)]
pub struct Levels {
    pub shadows: f32,
    pub midtones: f32,
    pub highlights: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            midtones: 1.0,
            highlights: 1.0,
        }
    }
}

impl ModifierImpl for Levels {
    fn name(&self) -> &'static str {
        "Levels"
    }

    fn has_effect(&self) -> bool {
        self.shadows != 0.0 || self.midtones != 1.0 || self.highlights != 1.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::LevelsShadows(v) => self.shadows = v,
            ModifierParam::LevelsMidtones(v) => self.midtones = v,
            ModifierParam::LevelsHighlights(v) => self.highlights = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::LEVELS,
            &[self.shadows, self.midtones, self.highlights],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let hi = self.highlights.max(self.shadows + 0.001);
        let range = hi - self.shadows;
        for v in c.iter_mut().take(3) {
            *v = ((*v - self.shadows) / range).clamp(0.0, 1.0);
        }
        let gamma = self.midtones.max(0.001);
        for v in c.iter_mut().take(3) {
            *v = v.powf(1.0 / gamma);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        1u8.hash(hasher);
        hash_f32(self.shadows, hasher);
        hash_f32(self.midtones, hasher);
        hash_f32(self.highlights, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Shadows",
                self.shadows,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::LevelsShadows(v)),
            ),
            value_row(
                "Midtones",
                self.midtones,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::LevelsMidtones(v)),
            ),
            value_row(
                "Highlights",
                self.highlights,
                0.0..=2.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::LevelsHighlights(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct BrightnessContrast {
    pub brightness: f32,
    pub contrast: f32,
}

impl ModifierImpl for BrightnessContrast {
    fn name(&self) -> &'static str {
        "Brightness / Contrast"
    }

    fn has_effect(&self) -> bool {
        self.brightness != 0.0 || self.contrast != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Brightness(v) => self.brightness = v,
            ModifierParam::Contrast(v) => self.contrast = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::BRIGHTNESS_CONTRAST,
            &[self.brightness, self.contrast],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        for v in c.iter_mut().take(3) {
            *v = (*v + self.brightness - 0.5) * (1.0 + self.contrast) + 0.5;
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        2u8.hash(hasher);
        hash_f32(self.brightness, hasher);
        hash_f32(self.contrast, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Brightness",
                self.brightness,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::Brightness(v)),
            ),
            value_row(
                "Contrast",
                self.contrast,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::Contrast(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct HueSaturation {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

impl ModifierImpl for HueSaturation {
    fn name(&self) -> &'static str {
        "Hue / Saturation"
    }

    fn has_effect(&self) -> bool {
        self.hue != 0.0 || self.saturation != 0.0 || self.lightness != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Hue(v) => self.hue = v,
            ModifierParam::Saturation(v) => self.saturation = v,
            ModifierParam::Lightness(v) => self.lightness = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::HUE_SATURATION,
            &[self.hue, self.saturation, self.lightness],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let [h, s, l] = rgb_to_hsl([
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        ]);
        let rgb = hsl_to_rgb([
            (h + self.hue / 360.0).rem_euclid(1.0),
            (s + self.saturation).clamp(0.0, 1.0),
            (l + self.lightness).clamp(0.0, 1.0),
        ]);
        c[0] = rgb[0];
        c[1] = rgb[1];
        c[2] = rgb[2];
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        3u8.hash(hasher);
        hash_f32(self.hue, hasher);
        hash_f32(self.saturation, hasher);
        hash_f32(self.lightness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            gradient_row(
                "Hue",
                self.hue,
                -180.0..=180.0,
                0.5,
                Fmt::signed(0).suffix("\u{00b0}"),
                Track::hue(),
                move |v| Message::UpdateModifier(index, ModifierParam::Hue(v)),
            ),
            value_row(
                "Saturation",
                self.saturation,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::Saturation(v)),
            ),
            value_row(
                "Lightness",
                self.lightness,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::Lightness(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct Exposure {
    pub exposure: f32,
}

impl ModifierImpl for Exposure {
    fn name(&self) -> &'static str {
        "Exposure"
    }

    fn has_effect(&self) -> bool {
        self.exposure != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::Exposure(v) = param {
            self.exposure = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::EXPOSURE, &[self.exposure]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let scale = 2.0f32.powf(self.exposure);
        c[0] *= scale;
        c[1] *= scale;
        c[2] *= scale;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        4u8.hash(hasher);
        hash_f32(self.exposure, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Exposure",
            self.exposure,
            -5.0..=5.0,
            0.01,
            Fmt::signed(2),
            move |v| Message::UpdateModifier(index, ModifierParam::Exposure(v)),
        )])
    }
}

#[derive(Debug, Clone, Default)]
pub struct Vibrance {
    pub vibrance: f32,
    pub saturation: f32,
}

impl ModifierImpl for Vibrance {
    fn name(&self) -> &'static str {
        "Vibrance"
    }

    fn has_effect(&self) -> bool {
        self.vibrance != 0.0 || self.saturation != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::Vibrance(v) => self.vibrance = v,
            ModifierParam::VibranceSaturation(v) => self.saturation = v,
            _ => {}
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::VIBRANCE, &[self.vibrance, self.saturation]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let cc = [
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        ];
        let luma = cc[0] * LUMA[0] + cc[1] * LUMA[1] + cc[2] * LUMA[2];
        let max_c = cc[0].max(cc[1]).max(cc[2]);
        let sat_proxy = max_c - cc[0].min(cc[1]).min(cc[2]);
        let vib_amount = self.vibrance * (1.0 - sat_proxy);
        for (v, &base) in c.iter_mut().take(3).zip(cc.iter()) {
            let after_vib = luma + (base - luma) * (1.0 + vib_amount);
            *v = luma + (after_vib - luma) * (1.0 + self.saturation);
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        5u8.hash(hasher);
        hash_f32(self.vibrance, hasher);
        hash_f32(self.saturation, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Vibrance",
                self.vibrance,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::Vibrance(v)),
            ),
            value_row(
                "Saturation",
                self.saturation,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                move |v| Message::UpdateModifier(index, ModifierParam::VibranceSaturation(v)),
            ),
        ])
    }
}

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
                move |v| Message::UpdateModifier(index, ModifierParam::ColorBalanceCyanRed(v)),
            ),
            gradient_row(
                "Mag / Green",
                self.magenta_green,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                Track::magenta_green(),
                move |v| Message::UpdateModifier(index, ModifierParam::ColorBalanceMagentaGreen(v)),
            ),
            gradient_row(
                "Yel / Blue",
                self.yellow_blue,
                -1.0..=1.0,
                0.01,
                Fmt::signed(2),
                Track::yellow_blue(),
                move |v| Message::UpdateModifier(index, ModifierParam::ColorBalanceYellowBlue(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct GaussianBlur {
    pub radius: f32,
}

impl Default for GaussianBlur {
    fn default() -> Self {
        Self { radius: 5.0 }
    }
}

impl ModifierImpl for GaussianBlur {
    fn name(&self) -> &'static str {
        "Gaussian Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::GaussianBlurRadius(v) = param {
            self.radius = v;
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        7u8.hash(hasher);
        hash_f32(self.radius, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Radius",
            self.radius,
            0.0..=100.0,
            0.5,
            Fmt::num(1),
            move |v| Message::UpdateModifier(index, ModifierParam::GaussianBlurRadius(v)),
        )])
    }
}

#[derive(Debug, Clone)]
pub struct MotionBlur {
    pub angle: f32,
    pub distance: f32,
}

impl Default for MotionBlur {
    fn default() -> Self {
        Self {
            angle: 0.0,
            distance: 20.0,
        }
    }
}

impl ModifierImpl for MotionBlur {
    fn name(&self) -> &'static str {
        "Motion Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::MotionBlurAngle(v) => self.angle = v,
            ModifierParam::MotionBlurDistance(v) => self.distance = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        8u8.hash(hasher);
        hash_f32(self.angle, hasher);
        hash_f32(self.distance, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            angle_row("Angle", self.angle, 0.0..=360.0, move |v| {
                Message::UpdateModifier(index, ModifierParam::MotionBlurAngle(v))
            }),
            value_row(
                "Distance",
                self.distance,
                0.0..=200.0,
                0.5,
                Fmt::num(0),
                move |v| Message::UpdateModifier(index, ModifierParam::MotionBlurDistance(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct RadialBlur {
    pub amount: f32,
}

impl Default for RadialBlur {
    fn default() -> Self {
        Self { amount: 10.0 }
    }
}

impl ModifierImpl for RadialBlur {
    fn name(&self) -> &'static str {
        "Radial Blur"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::RadialBlurAmount(v) = param {
            self.amount = v;
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        9u8.hash(hasher);
        hash_f32(self.amount, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Amount",
            self.amount,
            0.0..=100.0,
            0.5,
            Fmt::num(0),
            move |v| Message::UpdateModifier(index, ModifierParam::RadialBlurAmount(v)),
        )])
    }
}

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
                tile.tile_x as f32 / tile.full_w as f32,
                tile.tile_y as f32 / tile.full_h as f32,
                tile.tile_w as f32 / tile.full_w as f32,
                tile.tile_h as f32 / tile.full_h as f32,
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
                Message::UpdateModifier(index, ModifierParam::HalftoneSize(v))
            },),
            value_row(
                "Angle",
                self.angle,
                0.0..=90.0,
                0.5,
                Fmt::num(0).suffix("\u{00b0}"),
                move |v| Message::UpdateModifier(index, ModifierParam::HalftoneAngle(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct PixelSort {
    pub threshold: f32,
    pub angle: f32,
}

impl Default for PixelSort {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            angle: 90.0,
        }
    }
}

impl ModifierImpl for PixelSort {
    fn name(&self) -> &'static str {
        "Pixel Sort"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::PixelSortThreshold(v) => self.threshold = v,
            ModifierParam::PixelSortAngle(v) => self.angle = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        11u8.hash(hasher);
        hash_f32(self.threshold, hasher);
        hash_f32(self.angle, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Threshold",
                self.threshold,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::PixelSortThreshold(v)),
            ),
            angle_row("Angle", self.angle, 0.0..=360.0, move |v| {
                Message::UpdateModifier(index, ModifierParam::PixelSortAngle(v))
            }),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct Vignette {
    pub strength: f32,
    pub size: f32,
    pub softness: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            strength: 0.5,
            size: 0.5,
            softness: 0.5,
        }
    }
}

impl ModifierImpl for Vignette {
    fn name(&self) -> &'static str {
        "Vignette"
    }

    fn has_effect(&self) -> bool {
        self.strength != 0.0
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::VignetteStrength(v) => self.strength = v,
            ModifierParam::VignetteSize(v) => self.size = v,
            ModifierParam::VignetteSoftness(v) => self.softness = v,
            _ => {}
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::VIGNETTE,
            &[
                self.strength,
                self.size,
                self.softness,
                tile.tile_x as f32 / tile.full_w as f32,
                tile.tile_y as f32 / tile.full_h as f32,
                tile.tile_w as f32 / tile.full_w as f32,
                tile.tile_h as f32 / tile.full_h as f32,
            ],
        ))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let dx = uv[0] - 0.5;
        let dy = uv[1] - 0.5;
        let dist = (dx * dx + dy * dy).sqrt() * 2.0;
        let inner = (self.size - self.softness).max(0.0);
        let t = ((dist - inner) / (self.size + 0.0001 - inner)).clamp(0.0, 1.0);
        let vignette = 1.0 - t * t * (3.0 - 2.0 * t);
        let factor = (1.0 - self.strength).max(0.0) * (1.0 - vignette) + vignette;
        c[0] *= factor;
        c[1] *= factor;
        c[2] *= factor;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        12u8.hash(hasher);
        hash_f32(self.strength, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.softness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Strength",
                self.strength,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::VignetteStrength(v)),
            ),
            value_row("Size", self.size, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::VignetteSize(v))
            },),
            value_row(
                "Softness",
                self.softness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::VignetteSoftness(v)),
            ),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct ChromaticAberration {
    pub amount: f32,
}

impl Default for ChromaticAberration {
    fn default() -> Self {
        Self { amount: 5.0 }
    }
}

impl ModifierImpl for ChromaticAberration {
    fn name(&self) -> &'static str {
        "Chromatic Aberration"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    fn is_resampling(&self) -> bool {
        true
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::ChromaticAberrationAmount(v) = param {
            self.amount = v;
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::CHROMATIC_ABERRATION,
            &[
                self.amount / tile.full_w as f32,
                tile.tile_x as f32 / tile.full_w as f32,
                tile.tile_y as f32 / tile.full_h as f32,
                tile.tile_w as f32 / tile.full_w as f32,
                tile.tile_h as f32 / tile.full_h as f32,
            ],
        ))
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        13u8.hash(hasher);
        hash_f32(self.amount, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Amount",
            self.amount,
            0.0..=50.0,
            0.1,
            Fmt::num(1),
            move |v| Message::UpdateModifier(index, ModifierParam::ChromaticAberrationAmount(v)),
        )])
    }
}

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
            move |v| Message::UpdateModifier(
                index,
                ModifierParam::PosterizeLevels(v.round() as u32)
            ),
        )])
    }
}

#[derive(Debug, Clone)]
pub struct Threshold {
    pub cutoff: f32,
}

impl Default for Threshold {
    fn default() -> Self {
        Self { cutoff: 0.5 }
    }
}

impl ModifierImpl for Threshold {
    fn name(&self) -> &'static str {
        "Threshold"
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::ThresholdCutoff(v) = param {
            self.cutoff = v;
        }
    }

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(ids::THRESHOLD, &[self.cutoff]))
    }

    fn apply_cpu(&self, _w: u32, _h: u32, _uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let luma = clamped_luma(c);
        let v = if luma >= self.cutoff { 1.0 } else { 0.0 };
        c[0] = v;
        c[1] = v;
        c[2] = v;
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        15u8.hash(hasher);
        hash_f32(self.cutoff, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![value_row(
            "Cutoff",
            self.cutoff,
            0.0..=1.0,
            0.01,
            Fmt::num(2),
            move |v| Message::UpdateModifier(index, ModifierParam::ThresholdCutoff(v)),
        )])
    }
}

#[derive(Debug, Clone)]
pub struct Grain {
    pub amount: f32,
    pub size: f32,
    pub roughness: f32,
    pub seed: f32,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            amount: 0.2,
            size: 1.0,
            roughness: 0.5,
            seed: 0.0,
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
            ModifierParam::GrainRoughness(v) => self.roughness = v,
            ModifierParam::GrainSeed(v) => self.seed = v,
            _ => {}
        }
    }

    fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        Some(make_entry(
            ids::GRAIN,
            &[
                self.amount,
                self.size,
                self.roughness,
                self.seed,
                tile.tile_x as f32,
                tile.tile_y as f32,
                tile.tile_w as f32,
                tile.tile_h as f32,
            ],
        ))
    }

    fn apply_cpu(&self, img_w: u32, img_h: u32, uv: [f32; 2], mut c: [f32; 4]) -> [f32; 4] {
        let gx = uv[0] * img_w as f32 / self.size.max(0.5);
        let gy = uv[1] * img_h as f32 / self.size.max(0.5);
        let iseed = self.seed as i32;
        let (cx, cy) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx.fract(), gy.fract());
        let n00 = hash21(cx as i32, cy as i32, iseed);
        let n10 = hash21(cx as i32 + 1, cy as i32, iseed);
        let n01 = hash21(cx as i32, cy as i32 + 1, iseed);
        let n11 = hash21(cx as i32 + 1, cy as i32 + 1, iseed);
        let t = self.roughness.clamp(0.0, 1.0);
        let wx = fx * fx * (3.0 - 2.0 * fx) * (1.0 - t) + if fx >= 0.5 { 1.0 } else { 0.0 } * t;
        let wy = fy * fy * (3.0 - 2.0 * fy) * (1.0 - t) + if fy >= 0.5 { 1.0 } else { 0.0 } * t;
        let noise = (n00 * (1.0 - wx) + n10 * wx) * (1.0 - wy) + (n01 * (1.0 - wx) + n11 * wx) * wy;
        let luma = clamped_luma(c);
        let luma_weight = 4.0 * luma * (1.0 - luma);
        let grain = (noise - 0.5) * self.amount * luma_weight;
        for v in c.iter_mut().take(3) {
            *v += grain;
        }
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        16u8.hash(hasher);
        hash_f32(self.amount, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.roughness, hasher);
        hash_f32(self.seed, hasher);
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
                move |v| Message::UpdateModifier(index, ModifierParam::GrainAmount(v)),
            ),
            value_row(
                "Size",
                self.size,
                0.5..=32.0,
                0.5,
                Fmt::num(1).suffix("px"),
                move |v| Message::UpdateModifier(index, ModifierParam::GrainSize(v)),
            ),
            value_row(
                "Roughness",
                self.roughness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::GrainRoughness(v)),
            ),
            value_row("Seed", self.seed, 0.0..=99.0, 1.0, Fmt::num(0), move |v| {
                Message::UpdateModifier(index, ModifierParam::GrainSeed(v))
            },),
        ])
    }
}

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
            Message::UpdateModifier(
                index,
                if swapped {
                    ModifierParam::CropHeight(v)
                } else {
                    ModifierParam::CropWidth(v)
                },
            )
        };
        let h_msg = move |v| {
            Message::UpdateModifier(
                index,
                if swapped {
                    ModifierParam::CropWidth(v)
                } else {
                    ModifierParam::CropHeight(v)
                },
            )
        };
        finish(column![
            value_row(
                "X",
                cx,
                0.0..=(iw - 1.0).max(0.0),
                1.0,
                Fmt::num(0),
                move |v| Message::UpdateModifier(index, ModifierParam::CropX(v)),
            ),
            value_row(
                "Y",
                cy,
                0.0..=(ih - 1.0).max(0.0),
                1.0,
                Fmt::num(0),
                move |v| Message::UpdateModifier(index, ModifierParam::CropY(v)),
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

#[derive(Debug, Clone)]
pub struct Text {
    pub content: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub rotation: f32,
    pub opacity: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            content: String::new(),
            x: 0.5,
            y: 0.5,
            size: 48.0,
            rotation: 0.0,
            opacity: 1.0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }
    }
}

impl ModifierImpl for Text {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::TextContent(v) => self.content = v,
            ModifierParam::TextX(v) => self.x = v,
            ModifierParam::TextY(v) => self.y = v,
            ModifierParam::TextSize(v) => self.size = v,
            ModifierParam::TextRotation(v) => self.rotation = v,
            ModifierParam::TextOpacity(v) => self.opacity = v,
            ModifierParam::TextR(v) => self.r = v,
            ModifierParam::TextG(v) => self.g = v,
            ModifierParam::TextB(v) => self.b = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        18u8.hash(hasher);
        self.content.hash(hasher);
        hash_f32(self.x, hasher);
        hash_f32(self.y, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.rotation, hasher);
        hash_f32(self.opacity, hasher);
        hash_f32(self.r, hasher);
        hash_f32(self.g, hasher);
        hash_f32(self.b, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            text_input("Type something...", &self.content)
                .on_input(move |v| Message::UpdateModifier(index, ModifierParam::TextContent(v)))
                .size(11)
                .padding([4, 6]),
            value_row("X", self.x, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextX(v))
            }),
            value_row("Y", self.y, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextY(v))
            }),
            value_row("Size", self.size, 4.0..=200.0, 0.5, Fmt::num(0), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextSize(v))
            },),
            value_row(
                "Rotation",
                self.rotation,
                -180.0..=180.0,
                0.5,
                Fmt::num(0).suffix("\u{00b0}"),
                move |v| Message::UpdateModifier(index, ModifierParam::TextRotation(v)),
            ),
            value_row(
                "Opacity",
                self.opacity,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::TextOpacity(v)),
            ),
            value_row("R", self.r, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextR(v))
            }),
            value_row("G", self.g, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextG(v))
            }),
            value_row("B", self.b, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                Message::UpdateModifier(index, ModifierParam::TextB(v))
            }),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct Drawing {
    pub opacity: f32,
    pub size: f32,
    pub hardness: f32,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            size: 10.0,
            hardness: 0.8,
        }
    }
}

impl ModifierImpl for Drawing {
    fn name(&self) -> &'static str {
        "Drawing"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::DrawingOpacity(v) => self.opacity = v,
            ModifierParam::DrawingSize(v) => self.size = v,
            ModifierParam::DrawingHardness(v) => self.hardness = v,
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        19u8.hash(hasher);
        hash_f32(self.opacity, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.hardness, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        finish(column![
            value_row(
                "Opacity",
                self.opacity,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::DrawingOpacity(v)),
            ),
            value_row("Size", self.size, 1.0..=100.0, 0.5, Fmt::num(0), move |v| {
                Message::UpdateModifier(index, ModifierParam::DrawingSize(v))
            },),
            value_row(
                "Hardness",
                self.hardness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| Message::UpdateModifier(index, ModifierParam::DrawingHardness(v)),
            ),
        ])
    }
}
