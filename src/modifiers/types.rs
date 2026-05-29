use std::collections::hash_map::DefaultHasher;

use iced::Element;

use crate::app::Message;
use crate::modifiers::gpu::{ModEntry, TileInfo};
use crate::modifiers::kinds::{
    BrightnessContrast, ChromaticAberration, ColorBalance, Crop, Drawing, Exposure, GaussianBlur,
    Grain, Halftone, HueSaturation, Levels, MotionBlur, PixelSort, Posterize, RadialBlur, Text,
    Threshold, Vibrance, Vignette,
};

pub trait ModifierImpl {
    fn name(&self) -> &'static str;

    fn has_effect(&self) -> bool {
        true
    }

    fn is_resampling(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, img_size: Option<(u32, u32)>);

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        None
    }

    fn apply_cpu(&self, _img_w: u32, _img_h: u32, _uv: [f32; 2], c: [f32; 4]) -> [f32; 4] {
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher);

    fn view(
        &self,
        index: usize,
        image_size: Option<(u32, u32)>,
        rotation: u8,
    ) -> Element<'_, Message>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModifierType {
    Levels,
    BrightnessContrast,
    HueSaturation,
    Exposure,
    Vibrance,
    ColorBalance,
    GaussianBlur,
    MotionBlur,
    RadialBlur,
    Halftone,
    PixelSort,
    Vignette,
    ChromaticAberration,
    Posterize,
    Threshold,
    Grain,
    Crop,
    Text,
    Drawing,
}

#[derive(Debug, Clone)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub enabled: bool,
    pub expanded: bool,
}

impl Modifier {
    pub fn new(kind: ModifierKind) -> Self {
        Self {
            kind,
            enabled: true,
            expanded: true,
        }
    }

    pub fn has_visible_effect(&self) -> bool {
        self.enabled && self.kind.has_effect()
    }

    pub fn apply_param(&mut self, param: ModifierParam, img_size: Option<(u32, u32)>) {
        self.kind.apply_param(param, img_size);
    }
}

#[derive(Debug, Clone)]
pub enum ModifierKind {
    Levels(Levels),
    BrightnessContrast(BrightnessContrast),
    HueSaturation(HueSaturation),
    Exposure(Exposure),
    Vibrance(Vibrance),
    ColorBalance(ColorBalance),
    GaussianBlur(GaussianBlur),
    MotionBlur(MotionBlur),
    RadialBlur(RadialBlur),
    Halftone(Halftone),
    PixelSort(PixelSort),
    Vignette(Vignette),
    ChromaticAberration(ChromaticAberration),
    Posterize(Posterize),
    Threshold(Threshold),
    Grain(Grain),
    Crop(Crop),
    Text(Text),
    Drawing(Drawing),
}

impl ModifierKind {
    fn as_impl(&self) -> &dyn ModifierImpl {
        match self {
            ModifierKind::Levels(m) => m,
            ModifierKind::BrightnessContrast(m) => m,
            ModifierKind::HueSaturation(m) => m,
            ModifierKind::Exposure(m) => m,
            ModifierKind::Vibrance(m) => m,
            ModifierKind::ColorBalance(m) => m,
            ModifierKind::GaussianBlur(m) => m,
            ModifierKind::MotionBlur(m) => m,
            ModifierKind::RadialBlur(m) => m,
            ModifierKind::Halftone(m) => m,
            ModifierKind::PixelSort(m) => m,
            ModifierKind::Vignette(m) => m,
            ModifierKind::ChromaticAberration(m) => m,
            ModifierKind::Posterize(m) => m,
            ModifierKind::Threshold(m) => m,
            ModifierKind::Grain(m) => m,
            ModifierKind::Crop(m) => m,
            ModifierKind::Text(m) => m,
            ModifierKind::Drawing(m) => m,
        }
    }

    fn as_impl_mut(&mut self) -> &mut dyn ModifierImpl {
        match self {
            ModifierKind::Levels(m) => m,
            ModifierKind::BrightnessContrast(m) => m,
            ModifierKind::HueSaturation(m) => m,
            ModifierKind::Exposure(m) => m,
            ModifierKind::Vibrance(m) => m,
            ModifierKind::ColorBalance(m) => m,
            ModifierKind::GaussianBlur(m) => m,
            ModifierKind::MotionBlur(m) => m,
            ModifierKind::RadialBlur(m) => m,
            ModifierKind::Halftone(m) => m,
            ModifierKind::PixelSort(m) => m,
            ModifierKind::Vignette(m) => m,
            ModifierKind::ChromaticAberration(m) => m,
            ModifierKind::Posterize(m) => m,
            ModifierKind::Threshold(m) => m,
            ModifierKind::Grain(m) => m,
            ModifierKind::Crop(m) => m,
            ModifierKind::Text(m) => m,
            ModifierKind::Drawing(m) => m,
        }
    }

    pub fn name(&self) -> &'static str {
        self.as_impl().name()
    }

    pub fn has_effect(&self) -> bool {
        self.as_impl().has_effect()
    }

    pub fn is_resampling(&self) -> bool {
        self.as_impl().is_resampling()
    }

    pub fn apply_param(&mut self, param: ModifierParam, img_size: Option<(u32, u32)>) {
        self.as_impl_mut().apply_param(param, img_size);
    }

    pub fn pack(&self, tile: &TileInfo) -> Option<ModEntry> {
        self.as_impl().pack(tile)
    }

    pub fn apply_cpu(&self, img_w: u32, img_h: u32, uv: [f32; 2], c: [f32; 4]) -> [f32; 4] {
        self.as_impl().apply_cpu(img_w, img_h, uv, c)
    }

    pub fn hash_into(&self, hasher: &mut DefaultHasher) {
        self.as_impl().hash(hasher);
    }

    pub fn view(
        &self,
        index: usize,
        image_size: Option<(u32, u32)>,
        rotation: u8,
    ) -> Element<'_, Message> {
        self.as_impl().view(index, image_size, rotation)
    }

    pub fn as_crop(&self) -> Option<&Crop> {
        match self {
            ModifierKind::Crop(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_crop_mut(&mut self) -> Option<&mut Crop> {
        match self {
            ModifierKind::Crop(c) => Some(c),
            _ => None,
        }
    }
}

impl From<ModifierType> for ModifierKind {
    fn from(t: ModifierType) -> Self {
        match t {
            ModifierType::Levels => ModifierKind::Levels(Levels::default()),
            ModifierType::BrightnessContrast => {
                ModifierKind::BrightnessContrast(BrightnessContrast::default())
            }
            ModifierType::HueSaturation => ModifierKind::HueSaturation(HueSaturation::default()),
            ModifierType::Exposure => ModifierKind::Exposure(Exposure::default()),
            ModifierType::Vibrance => ModifierKind::Vibrance(Vibrance::default()),
            ModifierType::ColorBalance => ModifierKind::ColorBalance(ColorBalance::default()),
            ModifierType::GaussianBlur => ModifierKind::GaussianBlur(GaussianBlur::default()),
            ModifierType::MotionBlur => ModifierKind::MotionBlur(MotionBlur::default()),
            ModifierType::RadialBlur => ModifierKind::RadialBlur(RadialBlur::default()),
            ModifierType::Halftone => ModifierKind::Halftone(Halftone::default()),
            ModifierType::PixelSort => ModifierKind::PixelSort(PixelSort::default()),
            ModifierType::Vignette => ModifierKind::Vignette(Vignette::default()),
            ModifierType::ChromaticAberration => {
                ModifierKind::ChromaticAberration(ChromaticAberration::default())
            }
            ModifierType::Posterize => ModifierKind::Posterize(Posterize::default()),
            ModifierType::Threshold => ModifierKind::Threshold(Threshold::default()),
            ModifierType::Grain => ModifierKind::Grain(Grain::default()),
            ModifierType::Crop => ModifierKind::Crop(Crop::default()),
            ModifierType::Text => ModifierKind::Text(Text::default()),
            ModifierType::Drawing => ModifierKind::Drawing(Drawing::default()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModifierParam {
    LevelsShadows(f32),
    LevelsMidtones(f32),
    LevelsHighlights(f32),
    Brightness(f32),
    Contrast(f32),
    Hue(f32),
    Saturation(f32),
    Lightness(f32),
    Exposure(f32),
    Vibrance(f32),
    VibranceSaturation(f32),
    ColorBalanceCyanRed(f32),
    ColorBalanceMagentaGreen(f32),
    ColorBalanceYellowBlue(f32),
    GaussianBlurRadius(f32),
    MotionBlurAngle(f32),
    MotionBlurDistance(f32),
    RadialBlurAmount(f32),
    HalftoneSize(f32),
    HalftoneAngle(f32),
    PixelSortThreshold(f32),
    PixelSortAngle(f32),
    VignetteStrength(f32),
    VignetteSize(f32),
    VignetteSoftness(f32),
    ChromaticAberrationAmount(f32),
    PosterizeLevels(u32),
    ThresholdCutoff(f32),
    GrainAmount(f32),
    GrainSize(f32),
    GrainSeed(f32),
    GrainColor(f32),
    GrainResponse(f32),
    CropX(f32),
    CropY(f32),
    CropWidth(f32),
    CropHeight(f32),
    TextContent(String),
    TextX(f32),
    TextY(f32),
    TextSize(f32),
    TextRotation(f32),
    TextOpacity(f32),
    TextR(f32),
    TextG(f32),
    TextB(f32),
    DrawingOpacity(f32),
    DrawingSize(f32),
    DrawingHardness(f32),
}

pub mod ids {
    pub const EXPOSURE: u32 = 1;
    pub const LEVELS: u32 = 2;
    pub const BRIGHTNESS_CONTRAST: u32 = 3;
    pub const HUE_SATURATION: u32 = 4;
    pub const VIGNETTE: u32 = 5;
    pub const POSTERIZE: u32 = 6;
    pub const THRESHOLD: u32 = 7;
    pub const VIBRANCE: u32 = 8;
    pub const COLOR_BALANCE: u32 = 9;
    pub const GRAIN: u32 = 10;
    pub const CHROMATIC_ABERRATION: u32 = 11;
    pub const HALFTONE: u32 = 16;
}
