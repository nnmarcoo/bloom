//! Modifier definitions and the registry macro.
//!
//! define_modifiers! is the single place a modifier is declared; the kind enum,
//! dispatch, and parameter plumbing are all generated from it.
//!
//! Each modifier declares an InputRequest describing how far it reads from its
//! input. The ROI taxonomy is derived from that declaration rather than
//! restated, so the two cannot disagree.
//!
//! output_spec is the size a modifier produces, identity unless it overrides.
//! changes_geometry is deliberately separate: it must hold for every input, not
//! the one a caller happens to have, because a crop currently spanning the whole
//! image still changes geometry and a plan built at that moment would fuse it
//! and stop being able to shrink. Anything true there is barred from a fused
//! run however cheap its per-pixel work is -- fusing a crop made it plan, size,
//! and then render as a passthrough with the whole suite green.
//!
//! stage_transform is the third geometric fact, and the one output_spec cannot
//! supply: two stages can report the same output size while mapping their input
//! to it in incompatible ways. A resize stretches, so a point maps by the size
//! ratio; a crop translates, so a point maps by the rect's origin at ratio one.
//! Every geometry walk -- the ROI walks in both backends, the document offset,
//! the kept source extent, the banded row mapping -- has to pick one of those
//! rules, and each used to pick it by asking whether the modifier was a Crop.
//! Declaring it here is what lets a new geometry modifier land without editing
//! those six walks.
//!
//! tool_target answers which modifier a tool edits when the stack holds several
//! of a kind: the selected one, else the last. It lives here because the tool's
//! message handling and the overlay that draws it must reach the same one; when
//! they disagreed the overlay edited a crop the user had not selected.

use std::collections::hash_map::DefaultHasher;

use iced::Element;

use crate::app::Message;
use crate::modifiers::gpu::{ModEntry, TileInfo};
use crate::modifiers::kinds::{
    BrightnessContrast, ChromaticAberration, ColorBalance, Crop, Drawing, Duotone, Exposure,
    GaussianBlur, Grain, Grayscale, Halftone, HueSaturation, Invert, Levels, MotionBlur, PixelSort,
    Posterize, RadialBlur, Resize, Sepia, Solarize, Temperature, Text, Threshold, Trim, Vibrance,
    Vignette,
};
use crate::modifiers::plan::ImageSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// How a stage maps a point in its input space to its output space.
///
/// Scale is the default and covers every modifier whose output stands for the
/// whole of its input, whether or not it resized it. Translate is for a stage
/// that outputs a sub-rectangle of its input, where the mapping is a pure shift
/// and the size ratio would be wrong: a crop's output is not a squeezed input.
///
/// Chosen per modifier rather than inferred from the origin being nonzero,
/// because a crop anchored at (0, 0) still translates rather than scales.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StageTransform {
    Scale,
    Translate { x: f32, y: f32 },
}

impl StageTransform {
    pub fn origin(&self) -> (f32, f32) {
        match self {
            StageTransform::Scale => (0.0, 0.0),
            StageTransform::Translate { x, y } => (*x, *y),
        }
    }

    pub fn is_translate(&self) -> bool {
        matches!(self, StageTransform::Translate { .. })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MediaTiming {
    pub duration: std::time::Duration,
    pub frame_count: u64,
}

impl MediaTiming {
    pub fn frame_at(&self, t: std::time::Duration) -> u64 {
        let total = self.duration.as_secs_f64();
        if total <= 0.0 || self.frame_count == 0 {
            return 0;
        }
        let frac = (t.as_secs_f64() / total).clamp(0.0, 1.0);
        ((frac * self.frame_count as f64).round() as u64).min(self.frame_count)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ViewCtx {
    pub image_size: Option<(u32, u32)>,
    pub rotation: u8,
    pub timing: Option<MediaTiming>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputRequest {
    SamplePoint,
    Neighborhood { radius_px: f32, separable: bool },
    ScanLines { step: (i32, i32) },
    FullFrame,
}

impl InputRequest {
    pub fn is_pointwise(&self) -> bool {
        matches!(self, InputRequest::SamplePoint)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectClass {
    Pointwise,
    Fragment,
    Separable { apron_px: f32 },
    ComputeScanline { axis: Axis },
}

impl EffectClass {
    pub fn from_input_request(req: InputRequest) -> Self {
        match req {
            InputRequest::SamplePoint => EffectClass::Pointwise,
            InputRequest::FullFrame => EffectClass::Fragment,
            InputRequest::Neighborhood { radius_px, .. } => EffectClass::Separable {
                apron_px: radius_px,
            },
            InputRequest::ScanLines { step: (dx, dy) } => EffectClass::ComputeScanline {
                axis: if dy.abs() > dx.abs() {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                },
            },
        }
    }

    pub fn is_pointwise(&self) -> bool {
        matches!(self, EffectClass::Pointwise)
    }

    pub fn is_fragment(&self) -> bool {
        matches!(self, EffectClass::Fragment)
    }

    pub fn separable_apron(&self) -> Option<f32> {
        match self {
            EffectClass::Separable { apron_px } => Some(*apron_px),
            _ => None,
        }
    }

    pub fn is_compute_scanline(&self) -> bool {
        matches!(self, EffectClass::ComputeScanline { .. })
    }
}

pub trait ModifierImpl {
    fn name(&self) -> &'static str;

    fn has_effect(&self) -> bool {
        true
    }

    fn input_request(&self) -> InputRequest {
        InputRequest::SamplePoint
    }

    fn effect_class(&self) -> EffectClass {
        EffectClass::from_input_request(self.input_request())
    }

    fn output_spec(&self, input: ImageSpec) -> ImageSpec {
        input
    }

    fn changes_geometry(&self) -> bool {
        false
    }

    fn stage_transform(&self, _input: ImageSpec) -> StageTransform {
        StageTransform::Scale
    }

    fn apply_param(&mut self, param: ModifierParam, img_size: Option<(u32, u32)>);

    fn pack(&self, _tile: &TileInfo) -> Option<ModEntry> {
        None
    }

    fn apply_cpu(&self, _img_w: u32, _img_h: u32, _uv: [f32; 2], c: [f32; 4]) -> [f32; 4] {
        c
    }

    fn hash(&self, hasher: &mut DefaultHasher);

    fn view(&self, index: usize, ctx: ViewCtx) -> Element<'_, Message>;
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

pub fn tool_target(
    modifiers: &[Modifier],
    active: Option<usize>,
    is_kind: impl Fn(&Modifier) -> bool,
) -> Option<usize> {
    active
        .filter(|i| modifiers.get(*i).is_some_and(&is_kind))
        .or_else(|| modifiers.iter().rposition(&is_kind))
}

macro_rules! define_modifiers {
    ($($variant:ident => $label:literal @ $category:literal),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum ModifierType {
            $($variant,)*
        }

        impl ModifierType {
            pub const ALL: &'static [ModifierType] = &[$(ModifierType::$variant,)*];

            pub fn label(&self) -> &'static str {
                match self {
                    $(ModifierType::$variant => $label,)*
                }
            }

            pub fn category(&self) -> &'static str {
                match self {
                    $(ModifierType::$variant => $category,)*
                }
            }

            pub fn in_menu(&self) -> bool {
                !matches!(self, ModifierType::RadialBlur)
            }

            pub fn enabled_for(&self, timed: bool) -> bool {
                match self {
                    ModifierType::Trim => timed,
                    _ => true,
                }
            }

            pub fn disabled_reason(&self) -> &'static str {
                match self {
                    ModifierType::Trim => "Only for animations and video",
                    _ => "",
                }
            }
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
    Levels => "Levels" @ "Adjustments",
    BrightnessContrast => "Brightness & Contrast" @ "Adjustments",
    HueSaturation => "Hue & Saturation" @ "Adjustments",
    Exposure => "Exposure" @ "Adjustments",
    Vibrance => "Vibrance" @ "Adjustments",
    ColorBalance => "Color Balance" @ "Adjustments",
    Temperature => "Temperature" @ "Adjustments",
    Grayscale => "Grayscale" @ "Adjustments",
    Invert => "Invert" @ "Adjustments",
    Posterize => "Posterize" @ "Adjustments",
    Threshold => "Threshold" @ "Adjustments",
    Sepia => "Sepia" @ "Stylize",
    Duotone => "Duotone" @ "Stylize",
    Solarize => "Solarize" @ "Stylize",
    Vignette => "Vignette" @ "Stylize",
    ChromaticAberration => "Chromatic Aberration" @ "Stylize",
    Grain => "Grain" @ "Stylize",
    GaussianBlur => "Gaussian Blur" @ "Blur",
    MotionBlur => "Motion Blur" @ "Blur",
    RadialBlur => "Radial Blur" @ "Blur",
    Halftone => "Halftone" @ "Distort",
    PixelSort => "Pixel Sort" @ "Distort",
    Crop => "Crop" @ "Transform",
    Resize => "Resize" @ "Transform",
    Trim => "Trim" @ "Time",
    Text => "Text" @ "Create",
    Drawing => "Drawing" @ "Create",
);

impl ModifierKind {
    pub fn name(&self) -> &'static str {
        self.as_impl().name()
    }

    pub fn has_effect(&self) -> bool {
        self.as_impl().has_effect()
    }

    pub fn input_request(&self) -> InputRequest {
        self.as_impl().input_request()
    }

    pub fn effect_class(&self) -> EffectClass {
        self.as_impl().effect_class()
    }

    pub fn output_spec(&self, input: ImageSpec) -> ImageSpec {
        self.as_impl().output_spec(input)
    }

    pub fn changes_geometry(&self) -> bool {
        self.as_impl().changes_geometry()
    }

    pub fn stage_transform(&self, input: ImageSpec) -> StageTransform {
        self.as_impl().stage_transform(input)
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

    pub fn view(&self, index: usize, ctx: ViewCtx) -> Element<'_, Message> {
        self.as_impl().view(index, ctx)
    }

    pub fn as_crop(&self) -> Option<&Crop> {
        match self {
            ModifierKind::Crop(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_resize(&self) -> Option<&Resize> {
        match self {
            ModifierKind::Resize(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_resize_mut(&mut self) -> Option<&mut Resize> {
        match self {
            ModifierKind::Resize(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_crop_mut(&mut self) -> Option<&mut Crop> {
        match self {
            ModifierKind::Crop(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_trim(&self) -> Option<&Trim> {
        match self {
            ModifierKind::Trim(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_trim_mut(&mut self) -> Option<&mut Trim> {
        match self {
            ModifierKind::Trim(t) => Some(t),
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
    TemperatureTemp(f32),
    TemperatureTint(f32),
    GrayscaleAmount(f32),
    InvertAmount(f32),
    SepiaIntensity(f32),
    SolarizeThreshold(f32),
    DuotoneShadow([f32; 3]),
    DuotoneHighlight([f32; 3]),
    DuotoneAmount(f32),
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
    ResizeWidth(f32),
    ResizeHeight(f32),
    ResizeMode(crate::modifiers::kinds::ResizeMode),
    ResizeFilter(crate::modifiers::kinds::ResizeFilter),
    ResizeLockAspect(bool),
    TrimStart(f32, std::time::Duration),
    TrimEnd(f32, std::time::Duration),
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
    DrawingColor([f32; 3]),
    DrawingStrokeStart([f32; 2]),
    DrawingStrokeExtend([f32; 2]),
    DrawingUndoStroke,
    DrawingClear,
}

#[cfg(test)]
mod tool_target_tests {
    use super::*;
    use crate::modifiers::kinds::{Crop, Exposure};

    fn crop() -> Modifier {
        Modifier::new(ModifierKind::Crop(Crop::default()))
    }

    fn exposure() -> Modifier {
        Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.2 }))
    }

    fn is_crop(m: &Modifier) -> bool {
        m.enabled && m.kind.as_crop().is_some()
    }

    #[test]
    fn the_selected_one_wins_over_the_first() {
        let mods = vec![crop(), exposure(), crop()];
        assert_eq!(
            tool_target(&mods, Some(2), is_crop),
            Some(2),
            "the tool must edit the crop the user selected, not whichever comes \
             first in the stack"
        );
    }

    #[test]
    fn the_last_one_is_the_fallback() {
        let mods = vec![crop(), exposure(), crop()];
        assert_eq!(tool_target(&mods, None, is_crop), Some(2));
    }

    #[test]
    fn a_selection_of_another_kind_falls_back() {
        let mods = vec![crop(), exposure(), crop()];
        assert_eq!(
            tool_target(&mods, Some(1), is_crop),
            Some(2),
            "an exposure is selected, so the crop tool takes the last crop"
        );
    }

    #[test]
    fn a_disabled_crop_is_not_a_target() {
        let mut off = crop();
        off.enabled = false;
        let mods = vec![crop(), off];
        assert_eq!(
            tool_target(&mods, Some(1), is_crop),
            Some(0),
            "a disabled crop draws no overlay, so it cannot be the target even \
             when selected"
        );
    }

    #[test]
    fn nothing_matches_gives_nothing() {
        assert_eq!(tool_target(&[exposure()], Some(0), is_crop), None);
        assert_eq!(tool_target(&[], None, is_crop), None);
    }

    #[test]
    fn an_out_of_range_selection_falls_back() {
        let mods = vec![crop()];
        assert_eq!(tool_target(&mods, Some(9), is_crop), Some(0));
    }
}

#[cfg(test)]
mod menu_gating_tests {
    use super::*;

    #[test]
    fn trim_is_listed_but_disabled_for_stills() {
        assert!(
            ModifierType::Trim.in_menu(),
            "Trim should stay visible so users can see it exists"
        );
        assert!(!ModifierType::Trim.enabled_for(false));
        assert!(ModifierType::Trim.enabled_for(true));
    }

    #[test]
    fn disabled_trim_explains_itself() {
        assert!(!ModifierType::Trim.disabled_reason().is_empty());
    }

    #[test]
    fn pixel_modifiers_are_always_enabled() {
        for t in ModifierType::ALL {
            if matches!(t, ModifierType::Trim) {
                continue;
            }
            assert!(
                t.enabled_for(false) && t.enabled_for(true),
                "{} should not depend on media kind",
                t.label()
            );
        }
    }

    #[test]
    fn radial_blur_stays_hidden() {
        assert!(!ModifierType::RadialBlur.in_menu());
    }
}

#[cfg(test)]
mod effect_class_tests {
    use super::*;
    use crate::modifiers::kinds::{
        ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort, Text,
    };

    fn class(k: ModifierKind) -> EffectClass {
        k.effect_class()
    }

    #[test]
    fn class_matches_input_request_partition() {
        assert!(class(ModifierKind::Exposure(Exposure::default())).is_pointwise());

        assert!(
            class(ModifierKind::ChromaticAberration(
                ChromaticAberration::default()
            ))
            .is_fragment()
        );

        assert_eq!(
            class(ModifierKind::Text(Text::default())).separable_apron(),
            Some(0.0)
        );

        assert_eq!(
            class(ModifierKind::MotionBlur(MotionBlur {
                angle: 0.0,
                distance: 20.0,
            }))
            .separable_apron(),
            Some(10.0)
        );

        let blur = ModifierKind::GaussianBlur(GaussianBlur { radius: 7.0 });
        assert_eq!(blur.effect_class().separable_apron(), Some(7.0));

        let sort = ModifierKind::PixelSort(PixelSort {
            threshold: 0.5,
            angle: 90.0,
        });
        assert!(sort.effect_class().is_compute_scanline());
        assert!(matches!(
            sort.effect_class(),
            EffectClass::ComputeScanline {
                axis: Axis::Vertical
            }
        ));
    }
}

macro_rules! shader_ids {
    ($($name:ident = $value:expr),+ $(,)?) => {
        pub mod ids {
            $(pub const $name: u32 = $value;)+

            #[allow(dead_code, reason = "drives the id/shader-arm parity tests")]
            pub const ALL: &[(&str, u32)] = &[$((stringify!($name), $value)),+];
        }
    };
}

shader_ids! {
    EXPOSURE = 1,
    LEVELS = 2,
    BRIGHTNESS_CONTRAST = 3,
    HUE_SATURATION = 4,
    VIGNETTE = 5,
    POSTERIZE = 6,
    THRESHOLD = 7,
    VIBRANCE = 8,
    COLOR_BALANCE = 9,
    GRAIN = 10,
    INVERT = 11,
    GRAYSCALE = 12,
    TEMPERATURE = 13,
    SEPIA = 14,
    SOLARIZE = 15,
    HALFTONE = 16,
    DUOTONE = 17,
}

#[cfg(test)]
mod shader_id_tests {
    use super::ids;
    use std::collections::BTreeSet;

    const SHADER: &str = include_str!("../wgpu/shaders/combined_modifiers.wgsl");

    fn shader_cases() -> BTreeSet<u32> {
        SHADER
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix("case ")?;
                let num = rest.strip_suffix("u: {")?;
                num.parse().ok()
            })
            .collect()
    }

    #[test]
    fn every_id_has_a_shader_arm_and_the_reverse() {
        let declared: BTreeSet<u32> = ids::ALL.iter().map(|(_, v)| *v).collect();
        let implemented = shader_cases();

        let missing: Vec<_> = declared.difference(&implemented).collect();
        assert!(
            missing.is_empty(),
            "ids with no `case Nu:` in combined_modifiers.wgsl: {missing:?}. \
             These modifiers would fall through to the default arm and render \
             as a passthrough."
        );

        let orphaned: Vec<_> = implemented.difference(&declared).collect();
        assert!(
            orphaned.is_empty(),
            "shader arms with no id constant: {orphaned:?}. Either an id was \
             deleted without its arm, or an arm was added without registering \
             the id."
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut seen: Vec<(u32, &str)> = ids::ALL.iter().map(|(n, v)| (*v, *n)).collect();
        seen.sort_unstable();
        for pair in seen.windows(2) {
            assert_ne!(
                pair[0].0, pair[1].0,
                "{} and {} share id {}, so one renders as the other",
                pair[0].1, pair[1].1, pair[0].0
            );
        }
    }
}

#[cfg(test)]
mod docs_tests {
    use super::ModifierType;

    fn docs_page() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guide/modifiers.html");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
    }

    fn docs_alias(name: &str) -> &str {
        match name {
            "Brightness & Contrast" => "Brightness / Contrast",
            "Hue & Saturation" => "Hue / Saturation",
            other => other,
        }
    }

    #[test]
    fn every_modifier_is_listed_in_the_docs() {
        let html = docs_page();
        let missing: Vec<&str> = ModifierType::ALL
            .iter()
            .map(|t| docs_alias(t.label()))
            .filter(|label| !html.contains(&format!(">{label}<")))
            .collect();
        assert!(
            missing.is_empty(),
            "docs/guide/modifiers.html does not list {missing:?}. A modifier \
             shipped without a docs entry is invisible to anyone reading the \
             site, and the count in the search placeholder goes stale with it."
        );
    }

    #[test]
    fn the_docs_do_not_list_modifiers_that_do_not_exist() {
        let html = docs_page();
        let listed: Vec<&str> = html
            .match_indices("<span class=\"fmt-name\">")
            .filter_map(|(i, m)| {
                let rest = &html[i + m.len()..];
                rest.find('<').map(|e| &rest[..e])
            })
            .collect();
        let real: Vec<&str> = ModifierType::ALL
            .iter()
            .map(|t| docs_alias(t.label()))
            .collect();
        let phantom: Vec<&&str> = listed.iter().filter(|l| !real.contains(l)).collect();
        assert!(
            phantom.is_empty(),
            "docs/guide/modifiers.html advertises {phantom:?}, which no \
             ModifierType provides. The site promises a feature the app does \
             not have."
        );
        assert_eq!(
            listed.len(),
            real.len(),
            "the docs list {} modifiers but the app has {}",
            listed.len(),
            real.len()
        );
    }

    #[test]
    fn the_search_placeholder_states_the_real_count() {
        let html = docs_page();
        let want = format!("Search {} modifiers", ModifierType::ALL.len());
        assert!(
            html.contains(&want),
            "the search box should say {want:?}; it drifts every time a \
             modifier is added and nobody updates the number by hand"
        );
    }

    #[test]
    fn no_page_advertises_a_stale_modifier_count() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let n = ModifierType::ALL.len();
        let claims: &[(&str, &str)] = &[
            (
                "docs/index.html",
                r#"<span class="n">{}</span><span class="l">modifiers</span>"#,
            ),
            ("docs/index.html", "All {} modifiers"),
            (
                "docs/guide/index.html",
                "non-destructive pipeline of {} effects.",
            ),
            ("docs/guide/modifiers.html", "bloom's {} non-destructive"),
            (
                "README.md",
                "**Non-destructive modifiers:** {} stackable effects",
            ),
        ];
        for (rel, shape) in claims {
            let text = std::fs::read_to_string(root.join(rel))
                .unwrap_or_else(|e| panic!("reading {rel}: {e}"));
            let want = shape.replace("{}", &n.to_string());
            assert!(
                text.contains(&want),
                "{rel} should contain {want:?}. Every one of these is a \
                 hand-written count of the modifiers the app ships, and each \
                 goes stale silently the next time one is added."
            );
        }
    }
}
