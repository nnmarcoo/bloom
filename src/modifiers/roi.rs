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

pub fn union(a: RegionPx, b: RegionPx) -> RegionPx {
    if is_empty(a) {
        return b;
    }
    if is_empty(b) {
        return a;
    }
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
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

pub struct RoiPlan {
    pub need: Vec<RegionPx>,
    pub source: RegionPx,
}

pub fn backward_roi(classes: &[StepClass], display_need: RegionPx, w: f32, h: f32) -> RoiPlan {
    let mut need = vec![[0.0; 4]; classes.len()];
    let mut cur = clamp_region(display_need, w, h);
    for (k, class) in classes.iter().enumerate().rev() {
        need[k] = cur;
        cur = input_needed(*class, cur, w, h);
    }
    RoiPlan { need, source: cur }
}

pub fn crop_to_source(r: RegionPx, crop: RegionPx) -> RegionPx {
    let t = [
        r[0] + crop[0],
        r[1] + crop[1],
        r[2] + crop[0],
        r[3] + crop[1],
    ];
    [
        t[0].clamp(crop[0], crop[2]),
        t[1].clamp(crop[1], crop[3]),
        t[2].clamp(crop[0], crop[2]),
        t[3].clamp(crop[1], crop[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::kinds::{Exposure, GaussianBlur, MotionBlur, PixelSort};

    const W: f32 = 1000.0;
    const H: f32 = 800.0;

    #[test]
    fn pointwise_chain_is_identity() {
        let classes = [StepClass::Pointwise, StepClass::Pointwise];
        let d = [100.0, 50.0, 300.0, 250.0];
        let plan = backward_roi(&classes, d, W, H);
        assert_eq!(plan.need, vec![d, d]);
        assert_eq!(plan.source, d);
    }

    #[test]
    fn kernel_chain_accumulates_aprons_backward() {
        let classes = [
            StepClass::Kernel {
                apron_px: 4.0,
                separable: true,
            },
            StepClass::Pointwise,
            StepClass::Kernel {
                apron_px: 2.0,
                separable: false,
            },
        ];
        let d = [100.0, 100.0, 200.0, 200.0];
        let plan = backward_roi(&classes, d, W, H);
        assert_eq!(plan.need[2], d);
        assert_eq!(plan.need[1], [98.0, 98.0, 202.0, 202.0]);
        assert_eq!(plan.need[0], [98.0, 98.0, 202.0, 202.0]);
        assert_eq!(plan.source, [94.0, 94.0, 206.0, 206.0]);
    }

    #[test]
    fn kernel_dilation_clamps_to_image() {
        let classes = [StepClass::Kernel {
            apron_px: 50.0,
            separable: false,
        }];
        let plan = backward_roi(&classes, [10.0, 10.0, 990.0, 790.0], W, H);
        assert_eq!(plan.source, [0.0, 0.0, W, H]);
    }

    #[test]
    fn row_scanline_needs_full_rows() {
        let classes = [StepClass::Scanline { dir: (1, 0) }];
        let plan = backward_roi(&classes, [100.0, 50.0, 300.0, 250.0], W, H);
        assert_eq!(plan.source, [0.0, 50.0, W, 250.0]);
    }

    #[test]
    fn col_scanline_needs_full_cols() {
        let classes = [StepClass::Scanline { dir: (0, 1) }];
        let plan = backward_roi(&classes, [100.0, 50.0, 300.0, 250.0], W, H);
        assert_eq!(plan.source, [100.0, 0.0, 300.0, H]);
    }

    #[test]
    fn diagonal_scanline_is_conservative_full_image() {
        let classes = [StepClass::Scanline { dir: (1, 1) }];
        let plan = backward_roi(&classes, [100.0, 50.0, 300.0, 250.0], W, H);
        assert_eq!(plan.source, [0.0, 0.0, W, H]);
    }

    #[test]
    fn whole_frame_needs_everything_upstream_of_it_only() {
        let classes = [
            StepClass::Pointwise,
            StepClass::WholeFrame,
            StepClass::Kernel {
                apron_px: 3.0,
                separable: true,
            },
        ];
        let d = [400.0, 400.0, 500.0, 500.0];
        let plan = backward_roi(&classes, d, W, H);
        assert_eq!(plan.need[2], d);
        assert_eq!(plan.need[1], [397.0, 397.0, 503.0, 503.0]);
        assert_eq!(plan.need[0], [0.0, 0.0, W, H]);
        assert_eq!(plan.source, [0.0, 0.0, W, H]);
    }

    #[test]
    fn scanline_after_kernel_composes() {
        let classes = [
            StepClass::Kernel {
                apron_px: 5.0,
                separable: true,
            },
            StepClass::Scanline { dir: (1, 0) },
        ];
        let d = [100.0, 100.0, 200.0, 200.0];
        let plan = backward_roi(&classes, d, W, H);
        assert_eq!(plan.need[1], d);
        assert_eq!(plan.need[0], [0.0, 100.0, W, 200.0]);
        assert_eq!(plan.source, [0.0, 95.0, W, 205.0]);
    }

    #[test]
    fn crop_translates_and_clamps() {
        let crop = [200.0, 100.0, 700.0, 500.0];
        assert_eq!(
            crop_to_source([50.0, 50.0, 150.0, 150.0], crop),
            [250.0, 150.0, 350.0, 250.0]
        );
        assert_eq!(
            crop_to_source([400.0, 300.0, 600.0, 500.0], crop),
            [600.0, 400.0, 700.0, 500.0]
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
    fn empty_display_region_stays_empty() {
        let classes = [StepClass::Kernel {
            apron_px: 10.0,
            separable: false,
        }];
        let plan = backward_roi(&classes, [50.0, 50.0, 50.0, 90.0], W, H);
        assert!(is_empty(plan.source));
    }
}
