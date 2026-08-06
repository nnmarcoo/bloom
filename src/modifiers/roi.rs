use crate::modifiers::{InputRequest, ModifierKind};

pub type RegionPx = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepClass {
    Pointwise,
    Kernel { apron_px: f32, separable: bool },
    Scanline { dir: (i32, i32) },
    WholeFrame,
}

/// Derived from the modifier's own [`InputRequest`] so the ROI taxonomy cannot
/// drift from what the modifier actually reads. Previously this matched on
/// `ModifierKind` and re-derived each apron by hand, which let the two
/// disagree — chromatic aberration declared `FullFrame` here but was given a
/// bounded apron, under-fetching its input by orders of magnitude at the image
/// corners.
pub fn step_class(kind: &ModifierKind) -> StepClass {
    match kind.input_request() {
        InputRequest::SamplePoint => StepClass::Pointwise,
        InputRequest::Neighborhood {
            radius_px,
            separable,
        } => StepClass::Kernel {
            apron_px: radius_px.abs(),
            separable,
        },
        InputRequest::ScanLines { step } => StepClass::Scanline { dir: step },
        InputRequest::FullFrame => StepClass::WholeFrame,
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
    use crate::modifiers::kinds::{
        ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort,
    };

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
        // Half the distance: the sweep is centred on the output pixel.
        assert_eq!(
            step_class(&ModifierKind::MotionBlur(MotionBlur {
                angle: 30.0,
                distance: 12.0
            })),
            StepClass::Kernel {
                apron_px: 6.0,
                separable: false
            }
        );
        // Not a kernel at any radius — see `ChromaticAberration::input_request`.
        assert_eq!(
            step_class(&ModifierKind::ChromaticAberration(ChromaticAberration {
                amount: 5.0
            })),
            StepClass::WholeFrame
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

    /// The defect this phase fixed, pinned so it cannot return.
    ///
    /// Chromatic aberration's displacement grows with distance from the image
    /// centre, so on a large image a centre-ish tile still needs input from far
    /// away. The old taxonomy dilated by `amount` (5px here) and produced a
    /// region that missed almost all of it.
    #[test]
    fn chromatic_aberration_fetches_beyond_a_bounded_apron() {
        const BIG: f32 = 4000.0;
        let ca = ModifierKind::ChromaticAberration(ChromaticAberration { amount: 5.0 });
        let tile = [3000.0, 3000.0, 3200.0, 3200.0];

        let needed = input_needed(step_class(&ca), tile, BIG, BIG);
        assert_eq!(needed, [0.0, 0.0, BIG, BIG]);

        // What the previous hand-written mapping would have fetched.
        let old = input_needed(
            StepClass::Kernel {
                apron_px: 5.0,
                separable: false,
            },
            tile,
            BIG,
            BIG,
        );
        assert_eq!(old, [2995.0, 2995.0, 3205.0, 3205.0]);
    }

    /// Mirrors the backward walk `execute_kernel_chain` performs to decide how
    /// much source it must gather (`executor.rs`, "for k in (0..n).rev()").
    ///
    /// This is where CA's classification is actually observable. The GPU
    /// goldens cannot see it: `chromatic_aberration.wgsl` clamps its sample
    /// coordinates into the fetched rect, so an under-fetch reads as an edge
    /// colour rather than as a visible error. Asserting on the region itself
    /// sidesteps that.
    #[test]
    fn chain_gather_region_follows_the_widest_stage() {
        const W: f32 = 2048.0;
        const H: f32 = 2048.0;
        let out = [900.0, 900.0, 1100.0, 1100.0];

        // Blur alone: a bounded apron, so the gather stays local.
        let blur = step_class(&ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 }));
        assert_eq!(
            input_needed(blur, out, W, H),
            [894.0, 894.0, 1106.0, 1106.0]
        );

        // Any FullFrame stage in the chain forces a full gather, regardless of
        // where it sits or how tight the stages around it are.
        let ca = step_class(&ModifierKind::ChromaticAberration(ChromaticAberration {
            amount: 8.0,
        }));
        let chain = [blur, ca, StepClass::Pointwise];
        let mut cur = out;
        for c in chain.iter().rev() {
            cur = input_needed(*c, cur, W, H);
        }
        assert_eq!(
            cur,
            [0.0, 0.0, W, H],
            "a FullFrame stage must widen the whole chain's gather"
        );
    }

    /// `StepClass` and `EffectClass` are two views of one declaration. Walking
    /// every modifier type catches a new kind that classifies inconsistently,
    /// which is what previously let chromatic aberration claim a bounded apron
    /// in the ROI taxonomy while declaring `FullFrame` to everyone else.
    #[test]
    fn step_class_agrees_with_effect_class_for_every_kind() {
        use crate::modifiers::ModifierType;

        for t in ModifierType::ALL {
            let kind = ModifierKind::from(t.clone());
            let name = kind.name();
            let step = step_class(&kind);
            let effect = kind.effect_class();

            let agrees = match (step, effect) {
                (StepClass::Pointwise, c) => c.is_pointwise(),
                (StepClass::WholeFrame, c) => c.is_fragment(),
                (StepClass::Kernel { apron_px, .. }, c) => c.separable_apron() == Some(apron_px),
                (StepClass::Scanline { .. }, c) => c.is_compute_scanline(),
            };
            assert!(
                agrees,
                "{name}: StepClass {step:?} and EffectClass {effect:?} describe \
                 different input reaches, so ROI and the backend would disagree \
                 about how much input this stage needs."
            );
        }
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
