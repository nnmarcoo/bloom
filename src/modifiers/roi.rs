//! Region-of-interest math: how much input a stage must read to produce a
//! given piece of its output.
//!
//! StepClass is derived from each modifier's own InputRequest so the taxonomy
//! cannot drift from what the modifier actually reads. Deriving it by hand let
//! the two disagree once, with chromatic aberration declaring a full-frame
//! reach while being given a bounded apron.
//!
//! input_needed and unmap_region compose, in that order, when walking a chain
//! backward: unmap first to cross a stage that changes dimensions, then dilate
//! by the apron in the input's own space. Reversing them would apply a
//! half-size apron in a full-size space.
//!
//! unmap_region carries a scale and no offset, which is everything a resize
//! needs. A crop is the other half of that: same scale, pure offset. unmap_offset
//! crosses it, and crop_stage_feasibility shows the two together reproduce the
//! tile culling the display-time crop does today -- which is what a crop stage
//! would have to preserve.

use crate::modifiers::{InputRequest, ModifierKind};

pub type RegionPx = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepClass {
    Pointwise,
    Kernel { apron_px: f32, separable: bool },
    Scanline { dir: (i32, i32) },
    WholeFrame,
}

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

pub fn step_class_for(kind: &ModifierKind, in_h: u32, out_h: u32) -> StepClass {
    if let Some(r) = kind.as_resize() {
        let scale = if in_h == 0 {
            1.0
        } else {
            out_h as f32 / in_h as f32
        };
        let widen = if scale > 0.0 && scale < 1.0 {
            1.0 / scale
        } else {
            1.0
        };
        return StepClass::Kernel {
            apron_px: r.filter.radius() * widen,
            separable: true,
        };
    }
    step_class(kind)
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

pub fn unmap_region(from: (f32, f32), to: (f32, f32), r: RegionPx) -> RegionPx {
    if from.0 <= 0.0 || from.1 <= 0.0 {
        return r;
    }
    let (sx, sy) = (to.0 / from.0, to.1 / from.1);
    [r[0] * sx, r[1] * sy, r[2] * sx, r[3] * sy]
}

/// Cross a stage that *translates* its input rather than scaling it.
///
/// unmap_region carries a scale but no offset, which is all a resize needs. A
/// crop is the opposite: same scale, pure offset, where output pixel (0, 0)
/// comes from input pixel (origin). A backward walk that only scales cannot
/// express it, so a crop stage would report the wrong source region and tile
/// culling would read the wrong tiles.
#[allow(
    dead_code,
    reason = "proves the crop-as-a-stage walk before the stage exists; \
              crop_stage_feasibility is its only caller until then"
)]
pub fn unmap_offset(origin: (f32, f32), r: RegionPx) -> RegionPx {
    [
        r[0] + origin.0,
        r[1] + origin.1,
        r[2] + origin.0,
        r[3] + origin.1,
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
    fn unmap_is_identity_when_the_size_is_unchanged() {
        let r = [100.0, 50.0, 300.0, 250.0];
        assert_eq!(unmap_region((W, H), (W, H), r), r);
    }

    #[test]
    fn unmap_scales_a_region_into_a_smaller_input() {
        let r = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(
            unmap_region((500.0, 400.0), (1000.0, 800.0), r),
            [20.0, 40.0, 60.0, 80.0]
        );
    }

    #[test]
    fn unmap_scales_a_region_into_a_larger_input() {
        let r = [20.0, 40.0, 60.0, 80.0];
        assert_eq!(
            unmap_region((1000.0, 800.0), (500.0, 400.0), r),
            [10.0, 20.0, 30.0, 40.0]
        );
    }

    #[test]
    fn unmap_handles_axes_scaled_differently() {
        let r = [10.0, 10.0, 20.0, 20.0];
        assert_eq!(
            unmap_region((100.0, 200.0), (300.0, 200.0), r),
            [30.0, 10.0, 60.0, 20.0]
        );
    }

    #[test]
    fn unmap_of_a_zero_sized_space_is_inert() {
        let r = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(unmap_region((0.0, 0.0), (100.0, 100.0), r), r);
    }

    #[test]
    fn unmap_then_apron_dilates_in_the_input_space() {
        let out = [100.0, 100.0, 200.0, 200.0];
        let in_space = unmap_region((500.0, 400.0), (1000.0, 800.0), out);
        assert_eq!(in_space, [200.0, 200.0, 400.0, 400.0]);
        let class = StepClass::Kernel {
            apron_px: 4.0,
            separable: true,
        };
        assert_eq!(
            input_needed(class, in_space, W, H),
            [196.0, 196.0, 404.0, 404.0]
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
                apron_px: 6.0,
                separable: false
            }
        );
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

    #[test]
    fn chromatic_aberration_fetches_beyond_a_bounded_apron() {
        const BIG: f32 = 4000.0;
        let ca = ModifierKind::ChromaticAberration(ChromaticAberration { amount: 5.0 });
        let tile = [3000.0, 3000.0, 3200.0, 3200.0];

        let needed = input_needed(step_class(&ca), tile, BIG, BIG);
        assert_eq!(needed, [0.0, 0.0, BIG, BIG]);

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

    #[test]
    fn chain_gather_region_follows_the_widest_stage() {
        const W: f32 = 2048.0;
        const H: f32 = 2048.0;
        let out = [900.0, 900.0, 1100.0, 1100.0];

        let blur = step_class(&ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 }));
        assert_eq!(
            input_needed(blur, out, W, H),
            [894.0, 894.0, 1106.0, 1106.0]
        );

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

    /// Step 2 of the crop-as-a-stage feasibility check.
    ///
    /// Today a crop culls tiles at display time: view_pipeline intersects the
    /// crop rect with each tile and skips the ones that miss. If crop became a
    /// chain stage that culling has to fall out of the backward ROI walk
    /// instead, or large documents would start reading tiles they do not need.
    ///
    /// These tests answer whether the walk can reproduce it. They are pure
    /// geometry -- no crop stage exists yet -- so they gate the feature rather
    /// than test it.
    mod crop_stage_feasibility {
        use super::*;

        const FULL_W: f32 = 4096.0;
        const FULL_H: f32 = 2731.0;
        const TILE: f32 = 1024.0;

        // The crop the user drew, in source pixels.
        const CROP: RegionPx = [1500.0, 800.0, 2600.0, 1900.0];

        fn tiles() -> Vec<RegionPx> {
            let mut v = Vec::new();
            let mut y = 0.0;
            while y < FULL_H {
                let mut x = 0.0;
                while x < FULL_W {
                    v.push([x, y, (x + TILE).min(FULL_W), (y + TILE).min(FULL_H)]);
                    x += TILE;
                }
                y += TILE;
            }
            v
        }

        fn intersects(a: RegionPx, b: RegionPx) -> bool {
            a[0].max(b[0]) < a[2].min(b[2]) && a[1].max(b[1]) < a[3].min(b[3])
        }

        /// What view_pipeline does today: intersect the crop with each tile.
        fn culled_today() -> Vec<bool> {
            tiles().iter().map(|t| !intersects(CROP, *t)).collect()
        }

        /// What a crop stage would do: ask the backward walk which source
        /// region feeds the crop's whole output, then cull against that.
        fn culled_by_walk(chain_after_crop: &[StepClass]) -> Vec<bool> {
            let (cw, ch) = (CROP[2] - CROP[0], CROP[3] - CROP[1]);

            // Start from the crop stage's entire output and walk back through
            // anything downstream of it, in the cropped space.
            let mut cur = [0.0, 0.0, cw, ch];
            for c in chain_after_crop.iter().rev() {
                cur = input_needed(*c, cur, cw, ch);
            }

            // Cross the crop itself: pure translation, no scale.
            let src = clamp_region(unmap_offset((CROP[0], CROP[1]), cur), FULL_W, FULL_H);
            tiles().iter().map(|t| !intersects(src, *t)).collect()
        }

        #[test]
        fn a_crop_stage_culls_exactly_the_tiles_the_display_crop_culls() {
            assert_eq!(
                culled_by_walk(&[]),
                culled_today(),
                "the backward walk through a crop stage must reach the same \
                 source region the display-time crop culls against. If it does \
                 not, moving crop into the chain loses tile culling and large \
                 documents read tiles they do not need."
            );
        }

        #[test]
        fn culling_survives_a_pointwise_stage_after_the_crop() {
            assert_eq!(culled_by_walk(&[StepClass::Pointwise]), culled_today());
        }

        #[test]
        fn a_blur_after_the_crop_widens_the_read_but_not_past_the_apron() {
            let with_blur = culled_by_walk(&[StepClass::Kernel {
                apron_px: 32.0,
                separable: true,
            }]);
            let plain = culled_today();

            // A blur legitimately reads a little outside the crop, so it may
            // keep tiles the display crop dropped -- but never fewer.
            for (i, (blurred_culls, plain_culls)) in with_blur.iter().zip(&plain).enumerate() {
                let dropped_a_needed_tile = *blurred_culls && !*plain_culls;
                assert!(
                    !dropped_a_needed_tile,
                    "tile {i}: the blurred chain culled a tile the plain crop \
                     kept, so the apron was applied in the wrong direction"
                );
            }
            assert!(
                with_blur.iter().filter(|c| **c).count() > 0,
                "a 32px apron around a {}x{} crop of a {FULL_W}x{FULL_H} image \
                 should still cull most tiles; culling nothing means the walk \
                 widened to the whole frame",
                CROP[2] - CROP[0],
                CROP[3] - CROP[1]
            );
        }

        #[test]
        fn a_whole_frame_stage_after_the_crop_still_only_reads_the_crop() {
            // WholeFrame widens to the stage's own frame, which after a crop is
            // the cropped frame -- not the source. That containment is the
            // whole point of cropping early.
            assert_eq!(
                culled_by_walk(&[StepClass::WholeFrame]),
                culled_today(),
                "a full-frame effect placed after a crop reads the cropped \
                 frame. If this widened to the source, cropping early would \
                 stop being an optimisation."
            );
        }

        #[test]
        fn the_offset_unmap_is_what_makes_this_work() {
            // Without the offset the walk lands at the origin and culls the
            // wrong tiles -- this is the failure the feature has to avoid.
            let (cw, ch) = (CROP[2] - CROP[0], CROP[3] - CROP[1]);
            let scale_only = clamp_region(
                unmap_region((cw, ch), (cw, ch), [0.0, 0.0, cw, ch]),
                FULL_W,
                FULL_H,
            );
            let wrong: Vec<bool> = tiles()
                .iter()
                .map(|t| !intersects(scale_only, *t))
                .collect();
            assert_ne!(
                wrong,
                culled_today(),
                "if a scale-only unmap already reproduced the culling, the \
                 offset would be unnecessary; this pins why it is needed"
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
