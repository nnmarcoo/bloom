use std::fmt;

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

impl ModifierType {
    pub fn name(&self) -> &'static str {
        match self {
            ModifierType::Levels => "Levels",
            ModifierType::BrightnessContrast => "Brightness / Contrast",
            ModifierType::HueSaturation => "Hue / Saturation",
            ModifierType::Exposure => "Exposure",
            ModifierType::Vibrance => "Vibrance",
            ModifierType::ColorBalance => "Color Balance",
            ModifierType::GaussianBlur => "Gaussian Blur",
            ModifierType::MotionBlur => "Motion Blur",
            ModifierType::RadialBlur => "Radial Blur",
            ModifierType::Halftone => "Halftone",
            ModifierType::PixelSort => "Pixel Sort",
            ModifierType::Vignette => "Vignette",
            ModifierType::ChromaticAberration => "Chromatic Aberration",
            ModifierType::Posterize => "Posterize",
            ModifierType::Threshold => "Threshold",
            ModifierType::Grain => "Grain",
            ModifierType::Crop => "Crop",
            ModifierType::Text => "Text",
            ModifierType::Drawing => "Drawing",
        }
    }
}

impl fmt::Display for ModifierType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub enabled: bool,
    pub expanded: bool,
    pub mask_enabled: bool,
    pub mask_x: f32,
    pub mask_y: f32,
    pub mask_w: f32,
    pub mask_h: f32,
    pub feather: f32,
}

impl Modifier {
    pub fn new(kind: ModifierKind) -> Self {
        Self {
            kind,
            enabled: true,
            expanded: true,
            mask_enabled: false,
            mask_x: 0.0,
            mask_y: 0.0,
            mask_w: 1.0,
            mask_h: 1.0,
            feather: 0.0,
        }
    }

    pub fn has_visible_effect(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.kind {
            ModifierKind::Exposure { exposure } => *exposure != 0.0,
            ModifierKind::Levels {
                shadows,
                midtones,
                highlights,
            } => *shadows != 0.0 || *midtones != 1.0 || *highlights != 1.0,
            ModifierKind::BrightnessContrast {
                brightness,
                contrast,
            } => *brightness != 0.0 || *contrast != 0.0,
            ModifierKind::HueSaturation {
                hue,
                saturation,
                lightness,
            } => *hue != 0.0 || *saturation != 0.0 || *lightness != 0.0,
            ModifierKind::Vignette { strength, .. } => *strength != 0.0,
            ModifierKind::Posterize { .. } => true,
            ModifierKind::Threshold { .. } => true,
            ModifierKind::Vibrance {
                vibrance,
                saturation,
            } => *vibrance != 0.0 || *saturation != 0.0,
            ModifierKind::ColorBalance {
                cyan_red,
                magenta_green,
                yellow_blue,
            } => *cyan_red != 0.0 || *magenta_green != 0.0 || *yellow_blue != 0.0,
            ModifierKind::Grain { amount, .. } => *amount != 0.0,
            ModifierKind::ChromaticAberration { amount, .. } => *amount != 0.0,
            ModifierKind::Halftone { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MaskParam {
    X(f32),
    Y(f32),
    Width(f32),
    Height(f32),
    Feather(f32),
}

#[derive(Debug, Clone)]
pub enum ModifierKind {
    Levels {
        shadows: f32,
        midtones: f32,
        highlights: f32,
    },
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
    },
    Exposure {
        exposure: f32,
    },
    Vibrance {
        vibrance: f32,
        saturation: f32,
    },
    ColorBalance {
        cyan_red: f32,
        magenta_green: f32,
        yellow_blue: f32,
    },
    GaussianBlur {
        radius: f32,
    },
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    RadialBlur {
        amount: f32,
    },
    Halftone {
        size: f32,
        angle: f32,
    },
    PixelSort {
        threshold: f32,
        angle: f32,
    },
    Vignette {
        strength: f32,
        size: f32,
        softness: f32,
    },
    ChromaticAberration {
        amount: f32,
        angle: f32,
    },
    Posterize {
        levels: u32,
    },
    Threshold {
        cutoff: f32,
    },
    Grain {
        amount: f32,
        size: f32,
        roughness: f32,
        seed: f32,
    },
    Crop {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation: f32,
    },
    Text {
        content: String,
        x: f32,
        y: f32,
        size: f32,
        rotation: f32,
        opacity: f32,
        r: f32,
        g: f32,
        b: f32,
    },
    Drawing {
        opacity: f32,
        size: f32,
        hardness: f32,
    },
}

impl ModifierKind {
    pub fn name(&self) -> &'static str {
        match self {
            ModifierKind::Levels { .. } => "Levels",
            ModifierKind::BrightnessContrast { .. } => "Brightness / Contrast",
            ModifierKind::HueSaturation { .. } => "Hue / Saturation",
            ModifierKind::Exposure { .. } => "Exposure",
            ModifierKind::Vibrance { .. } => "Vibrance",
            ModifierKind::ColorBalance { .. } => "Color Balance",
            ModifierKind::GaussianBlur { .. } => "Gaussian Blur",
            ModifierKind::MotionBlur { .. } => "Motion Blur",
            ModifierKind::RadialBlur { .. } => "Radial Blur",
            ModifierKind::Halftone { .. } => "Halftone",
            ModifierKind::PixelSort { .. } => "Pixel Sort",
            ModifierKind::Vignette { .. } => "Vignette",
            ModifierKind::ChromaticAberration { .. } => "Chromatic Aberration",
            ModifierKind::Posterize { .. } => "Posterize",
            ModifierKind::Threshold { .. } => "Threshold",
            ModifierKind::Grain { .. } => "Grain",
            ModifierKind::Crop { .. } => "Crop",
            ModifierKind::Text { .. } => "Text",
            ModifierKind::Drawing { .. } => "Drawing",
        }
    }
}

impl From<ModifierType> for ModifierKind {
    fn from(t: ModifierType) -> Self {
        match t {
            ModifierType::Levels => ModifierKind::Levels {
                shadows: 0.0,
                midtones: 1.0,
                highlights: 1.0,
            },
            ModifierType::BrightnessContrast => ModifierKind::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            ModifierType::HueSaturation => ModifierKind::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            ModifierType::Exposure => ModifierKind::Exposure { exposure: 0.0 },
            ModifierType::Vibrance => ModifierKind::Vibrance {
                vibrance: 0.0,
                saturation: 0.0,
            },
            ModifierType::ColorBalance => ModifierKind::ColorBalance {
                cyan_red: 0.0,
                magenta_green: 0.0,
                yellow_blue: 0.0,
            },
            ModifierType::GaussianBlur => ModifierKind::GaussianBlur { radius: 5.0 },
            ModifierType::MotionBlur => ModifierKind::MotionBlur {
                angle: 0.0,
                distance: 20.0,
            },
            ModifierType::RadialBlur => ModifierKind::RadialBlur { amount: 10.0 },
            ModifierType::Halftone => ModifierKind::Halftone {
                size: 10.0,
                angle: 45.0,
            },
            ModifierType::PixelSort => ModifierKind::PixelSort {
                threshold: 0.5,
                angle: 90.0,
            },
            ModifierType::Vignette => ModifierKind::Vignette {
                strength: 0.5,
                size: 0.5,
                softness: 0.5,
            },
            ModifierType::ChromaticAberration => ModifierKind::ChromaticAberration {
                amount: 5.0,
                angle: 0.0,
            },
            ModifierType::Posterize => ModifierKind::Posterize { levels: 4 },
            ModifierType::Threshold => ModifierKind::Threshold { cutoff: 0.5 },
            ModifierType::Grain => ModifierKind::Grain {
                amount: 0.2,
                size: 1.0,
                roughness: 0.5,
                seed: 0.0,
            },
            ModifierType::Crop => ModifierKind::Crop {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                rotation: 0.0,
            },
            ModifierType::Text => ModifierKind::Text {
                content: String::new(),
                x: 0.5,
                y: 0.5,
                size: 48.0,
                rotation: 0.0,
                opacity: 1.0,
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            ModifierType::Drawing => ModifierKind::Drawing {
                opacity: 1.0,
                size: 10.0,
                hardness: 0.8,
            },
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
    ChromaticAberrationAngle(f32),
    PosterizeLevels(u32),
    ThresholdCutoff(f32),
    GrainAmount(f32),
    GrainSize(f32),
    GrainRoughness(f32),
    GrainSeed(f32),
    CropX(f32),
    CropY(f32),
    CropWidth(f32),
    CropHeight(f32),
    CropRotation(f32),
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
