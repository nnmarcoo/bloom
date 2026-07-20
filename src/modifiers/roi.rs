use crate::modifiers::ModifierKind;
use crate::modifiers::pixel_sort::{SortAxis, SortMode};

pub type RegionPx = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepClass {
    Pointwise,
    Kernel { apron_px: f32, separable: bool },
    Scanline { dir: (i32, i32) },
    WholeFrame,
}

pub fn step_class(kind: &ModifierKind) -> StepClass {
    match kind {
        ModifierKind::GaussianBlur(gb) => StepClass::Kernel {
            apron_px: gb.radius.abs(),
            separable: true,
        },
        ModifierKind::ChromaticAberration(ca) => StepClass::Kernel {
            apron_px: ca.amount.abs(),
            separable: false,
        },
        ModifierKind::MotionBlur(mb) => StepClass::Kernel {
            apron_px: mb.distance.abs(),
            separable: false,
        },
        ModifierKind::Text(_) | ModifierKind::Drawing(_) => StepClass::Kernel {
            apron_px: 0.0,
            separable: false,
        },
        ModifierKind::PixelSort(ps) => StepClass::Scanline {
            dir: match SortMode::from_angle(ps.angle) {
                SortMode::Cardinal(SortAxis::Horizontal { .. }) => (1, 0),
                SortMode::Cardinal(SortAxis::Vertical { .. }) => (0, 1),
                SortMode::Diagonal { dx, dy } => (dx, dy),
            },
        },
        _ if kind.effect_class().is_pointwise() => StepClass::Pointwise,
        _ => StepClass::WholeFrame,
    }
}

pub fn is_empty(r: RegionPx) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

pub fn clamp_region(r: RegionPx, w: f32, h: f32) -> RegionPx {
    [
        r[0].clamp(0.0, w),
        r[1].clamp(0.0, h),
        r[2].clamp(0.0, w),
        r[3].clamp(0.0, h),
    ]
}

pub fn dilate(r: RegionPx, d: f32) -> RegionPx {
    [r[0] - d, r[1] - d, r[2] + d, r[3] + d]
}

pub fn input_needed(class: StepClass, out: RegionPx, w: f32, h: f32) -> RegionPx {
    if is_empty(out) {
        return out;
    }
    let r = match class {
        StepClass::Pointwise => out,
        StepClass::Kernel { apron_px, .. } => dilate(out, apron_px.ceil()),
        StepClass::Scanline { dir: (_, 0) } => [0.0, out[1], w, out[3]],
        StepClass::Scanline { dir: (0, _) } => [out[0], 0.0, out[2], h],
        StepClass::Scanline { .. } | StepClass::WholeFrame => [0.0, 0.0, w, h],
    };
    clamp_region(r, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::kinds::{Exposure, GaussianBlur, MotionBlur, PixelSort};

    const W: f32 = 1000.0;
    const H: f32 = 800.0;

    #[test]
    fn pointwise_input_is_identity() {
        let d = [100.0, 50.0, 300.0, 250.0];
        assert_eq!(input_needed(StepClass::Pointwise, d, W, H), d);
    }

    #[test]
    fn kernel_input_dilates_by_apron() {
        let d = [100.0, 100.0, 200.0, 200.0];
        let class = StepClass::Kernel {
            apron_px: 4.0,
            separable: true,
        };
        assert_eq!(input_needed(class, d, W, H), [96.0, 96.0, 204.0, 204.0]);
    }

    #[test]
    fn kernel_dilation_clamps_to_image() {
        let class = StepClass::Kernel {
            apron_px: 50.0,
            separable: false,
        };
        assert_eq!(
            input_needed(class, [10.0, 10.0, 990.0, 790.0], W, H),
            [0.0, 0.0, W, H]
        );
    }

    #[test]
    fn row_scanline_needs_full_rows() {
        let class = StepClass::Scanline { dir: (1, 0) };
        assert_eq!(
            input_needed(class, [100.0, 50.0, 300.0, 250.0], W, H),
            [0.0, 50.0, W, 250.0]
        );
    }

    #[test]
    fn col_scanline_needs_full_cols() {
        let class = StepClass::Scanline { dir: (0, 1) };
        assert_eq!(
            input_needed(class, [100.0, 50.0, 300.0, 250.0], W, H),
            [100.0, 0.0, 300.0, H]
        );
    }

    #[test]
    fn diagonal_scanline_is_conservative_full_image() {
        let class = StepClass::Scanline { dir: (1, 1) };
        assert_eq!(
            input_needed(class, [100.0, 50.0, 300.0, 250.0], W, H),
            [0.0, 0.0, W, H]
        );
    }

    #[test]
    fn whole_frame_needs_everything() {
        let class = StepClass::WholeFrame;
        assert_eq!(
            input_needed(class, [400.0, 400.0, 500.0, 500.0], W, H),
            [0.0, 0.0, W, H]
        );
    }

    #[test]
    fn step_class_maps_kinds() {
        assert_eq!(
            step_class(&ModifierKind::Exposure(Exposure { exposure: 0.5 })),
            StepClass::Pointwise
        );
        assert_eq!(
            step_class(&ModifierKind::GaussianBlur(GaussianBlur { radius: 7.0 })),
            StepClass::Kernel {
                apron_px: 7.0,
                separable: true
            }
        );
        assert_eq!(
            step_class(&ModifierKind::MotionBlur(MotionBlur {
                angle: 30.0,
                distance: 12.0
            })),
            StepClass::Kernel {
                apron_px: 12.0,
                separable: false
            }
        );
        assert_eq!(
            step_class(&ModifierKind::PixelSort(PixelSort {
                threshold: 0.5,
                angle: 0.0
            })),
            StepClass::Scanline { dir: (1, 0) }
        );
        assert_eq!(
            step_class(&ModifierKind::PixelSort(PixelSort {
                threshold: 0.5,
                angle: 90.0
            })),
            StepClass::Scanline { dir: (0, 1) }
        );
    }

    #[test]
    fn empty_output_stays_empty() {
        let class = StepClass::Kernel {
            apron_px: 10.0,
            separable: false,
        };
        assert!(is_empty(input_needed(
            class,
            [50.0, 50.0, 50.0, 90.0],
            W,
            H
        )));
    }
}
