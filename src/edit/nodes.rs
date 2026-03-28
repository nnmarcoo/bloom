/// Parameters for each edit node type.
/// All values use normalised ranges where practical so shaders can consume them directly.

#[derive(Debug, Clone, PartialEq)]
pub struct BrightnessContrastParams {
    /// -1.0 … 1.0  (0.0 = no change)
    pub brightness: f32,
    /// -1.0 … 1.0  (0.0 = no change)
    pub contrast: f32,
}

impl Default for BrightnessContrastParams {
    fn default() -> Self {
        Self { brightness: 0.0, contrast: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HueSaturationParams {
    /// -180.0 … 180.0 degrees
    pub hue: f32,
    /// -1.0 … 1.0  (0.0 = no change)
    pub saturation: f32,
    /// -1.0 … 1.0  (0.0 = no change)
    pub lightness: f32,
}

impl Default for HueSaturationParams {
    fn default() -> Self {
        Self { hue: 0.0, saturation: 0.0, lightness: 0.0 }
    }
}

/// Per-channel tone curve defined as a set of control points.
/// Points are in 0.0–1.0 input/output space.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveChannel {
    /// Sorted by x. Always includes (0,0) and (1,1).
    pub points: Vec<[f32; 2]>,
}

impl Default for CurveChannel {
    fn default() -> Self {
        Self { points: vec![[0.0, 0.0], [1.0, 1.0]] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurvesParams {
    pub rgb: CurveChannel,
    pub r: CurveChannel,
    pub g: CurveChannel,
    pub b: CurveChannel,
}

impl Default for CurvesParams {
    fn default() -> Self {
        Self {
            rgb: CurveChannel::default(),
            r: CurveChannel::default(),
            g: CurveChannel::default(),
            b: CurveChannel::default(),
        }
    }
}

/// Non-destructive crop — stores a normalised rect within the source image.
/// (0,0) = top-left, (1,1) = bottom-right. Does not change canvas size;
/// pixels outside the rect are masked to transparent.
#[derive(Debug, Clone, PartialEq)]
pub struct CropParams {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for CropParams {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }
    }
}

/// A single brush stroke. Points are in normalised image space (0.0–1.0).
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub color: [f32; 4],
    /// Brush radius as a fraction of image width.
    pub size_frac: f32,
    /// 0.0–1.0
    pub opacity: f32,
    /// 0.0 = soft, 1.0 = hard
    pub hardness: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaintParams {
    pub strokes: Vec<Stroke>,
}

/// A node in the non-destructive edit stack.
#[derive(Debug, Clone, PartialEq)]
pub struct EditNode {
    pub id: u64,
    pub enabled: bool,
    pub op: EditOp,
}

impl EditNode {
    pub fn new(id: u64, op: EditOp) -> Self {
        Self { id, enabled: true, op }
    }

    pub fn label(&self) -> &'static str {
        self.op.label()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    BrightnessContrast(BrightnessContrastParams),
    HueSaturation(HueSaturationParams),
    Curves(CurvesParams),
    Crop(CropParams),
    Paint(PaintParams),
}

impl EditOp {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BrightnessContrast(_) => "Brightness / Contrast",
            Self::HueSaturation(_) => "Hue / Saturation",
            Self::Curves(_) => "Curves",
            Self::Crop(_) => "Crop",
            Self::Paint(_) => "Paint",
        }
    }
}
