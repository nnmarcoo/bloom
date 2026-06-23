use std::collections::hash_map::DefaultHasher;

use iced::Element;

use crate::app::Message;
use crate::modifiers::gpu::{ModEntry, TileInfo};
use crate::modifiers::kinds::{
    BrightnessContrast, ChromaticAberration, ColorBalance, Crop, Drawing, Exposure, GaussianBlur,
    Grain, Halftone, HueSaturation, Levels, MotionBlur, PixelSort, Posterize, RadialBlur, Text,
    Threshold, Vibrance, Vignette,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputClass {
    Pointwise,
    NonPointwise,
}

pub trait ModifierImpl {
    fn name(&self) -> &'static str;

    fn has_effect(&self) -> bool {
        true
    }

    fn input_class(&self) -> InputClass {
        InputClass::Pointwise
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

macro_rules! define_modifiers {
    ($($variant:ident),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum ModifierType {
            $($variant,)*
        }

        #[derive(Debug, Clone)]
        pub enum ModifierKind {
            $($variant($variant),)*
        }

        impl ModifierKind {
            fn as_impl(&self) -> &dyn ModifierImpl {
                match self {
                    $(ModifierKind::$variant(m) => m,)*
                }
            }

            fn as_impl_mut(&mut self) -> &mut dyn ModifierImpl {
                match self {
                    $(ModifierKind::$variant(m) => m,)*
                }
            }
        }

        impl From<ModifierType> for ModifierKind {
            fn from(t: ModifierType) -> Self {
                match t {
                    $(ModifierType::$variant => ModifierKind::$variant($variant::default()),)*
                }
            }
        }
    };
}

define_modifiers!(
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
);

impl ModifierKind {
    pub fn name(&self) -> &'static str {
        self.as_impl().name()
    }

    pub fn has_effect(&self) -> bool {
        self.as_impl().has_effect()
    }

    pub fn input_class(&self) -> InputClass {
        self.as_impl().input_class()
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
    TextFont(String),
    TextX(f32),
    TextY(f32),
    TextSize(f32),
    TextRotation(f32),
    TextOpacity(f32),
    TextColor([f32; 3]),
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
    pub const HALFTONE: u32 = 16;
}
