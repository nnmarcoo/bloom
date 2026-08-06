//! Execution planning, shared by every render backend.
//!
//! A plan is the modifier stack reduced to the units a backend actually
//! executes. Two rules define it, and both are load-bearing:
//!
//! * Modifiers with no visible effect are dropped.
//! * Adjacent pointwise modifiers are fused into one item; anything else
//!   becomes its own item.
//!
//! Planning is deliberately **order-preserving**. It never reorders, dedupes, or
//! rewrites the stack, so "the render looks like a sequential render" holds by
//! construction rather than by argument. Fusing adjacent pointwise modifiers is
//! safe precisely because they are per-pixel functions evaluated at the same
//! coordinate, so composing them cannot change the result.
//!
//! This lives in `modifiers` rather than `wgpu` because it is backend-agnostic:
//! the GPU pipeline and the CPU export path consume the same plan, which is what
//! keeps the two from drifting apart.

use crate::modifiers::{Modifier, ModifierKind};

/// One unit of execution.
#[derive(Debug)]
pub enum PlanItem<'a> {
    /// Adjacent pointwise modifiers, evaluated together in one pass.
    Fused(Vec<&'a Modifier>),
    /// A modifier that needs a pass of its own, with its index in the original
    /// stack so backends can find per-modifier side data (text and drawing
    /// rasters are stored positionally).
    Step(usize, &'a Modifier),
}

/// The pixel dimensions a stage operates on.
///
/// This is **document geometry**: the size the modifier stack says an image is,
/// independent of any runtime quality scaling a backend applies to go faster.
/// Keeping the two apart is what lets a downscaled preview and a full-resolution
/// export run the same plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    pub w: u32,
    pub h: u32,
}

impl ImageSpec {
    pub fn new(w: u32, h: u32) -> Self {
        // A zero-sized stage would make every downstream division by width or
        // height a divide-by-zero, so clamp at construction.
        Self {
            w: w.max(1),
            h: h.max(1),
        }
    }

    /// This spec reduced by a runtime quality factor.
    ///
    /// The result is a *device* size, not document geometry: it says how large a
    /// texture to allocate, never how large the image is. Keeping the scale
    /// outside `ImageSpec` is deliberate — a spec that carried its own quality
    /// factor could let a preview's reduction leak into an export, which is the
    /// mistake this type exists to prevent.
    pub fn scaled(self, scale: f32) -> Self {
        Self::new(
            (self.w as f32 * scale).round() as u32,
            (self.h as f32 * scale).round() as u32,
        )
    }
}

/// The input and output geometry of one plan item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSpec {
    pub input: ImageSpec,
    pub output: ImageSpec,
}

impl StageSpec {
    /// True when the stage hands its input through at the same size.
    pub fn is_passthrough(&self) -> bool {
        self.input == self.output
    }
}

/// The dimensions a modifier produces, or `None` when it preserves its input.
///
/// Every modifier is currently dimension-preserving: crop is still hoisted out
/// of the stack and applied as a sampling window after the chain, and no resize
/// modifier exists yet. This function is where both of those become ordinary
/// stages.
fn output_spec(kind: &ModifierKind, input: ImageSpec) -> Option<ImageSpec> {
    let _ = (kind, input);
    None
}

/// Resolves the geometry of every plan item, given the source dimensions.
///
/// Returns one [`StageSpec`] per plan item, in plan order, so
/// `specs[i]` describes `plan[i]`.
pub fn infer_specs(source: ImageSpec, plan: &[PlanItem]) -> Vec<StageSpec> {
    let mut cur = source;
    plan.iter()
        .map(|item| {
            // A fused run is pointwise by construction, so it cannot resize.
            let output = match item {
                PlanItem::Fused(_) => cur,
                PlanItem::Step(_, m) => output_spec(&m.kind, cur).unwrap_or(cur),
            };
            let spec = StageSpec { input: cur, output };
            cur = output;
            spec
        })
        .collect()
}

/// The dimensions the whole plan produces from `source`.
pub fn chain_output_spec(source: ImageSpec, plan: &[PlanItem]) -> ImageSpec {
    infer_specs(source, plan)
        .last()
        .map_or(source, |s| s.output)
}

/// Reduces a modifier stack to its execution plan.
pub fn plan_modifiers(modifiers: &[Modifier]) -> Vec<PlanItem<'_>> {
    let mut plan: Vec<PlanItem> = Vec::new();
    let mut current: Vec<&Modifier> = Vec::new();
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        if !m.kind.effect_class().is_pointwise() {
            if !current.is_empty() {
                plan.push(PlanItem::Fused(std::mem::take(&mut current)));
            }
            plan.push(PlanItem::Step(i, m));
        } else {
            current.push(m);
        }
    }
    if !current.is_empty() {
        plan.push(PlanItem::Fused(current));
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::{
        ChromaticAberration, Drawing, Exposure, GaussianBlur, MotionBlur, PixelSort, Posterize,
        Stroke, Text,
    };

    fn m(kind: ModifierKind) -> Modifier {
        Modifier::new(kind)
    }

    fn exposure() -> Modifier {
        m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))
    }

    fn blur() -> Modifier {
        m(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 }))
    }

    /// Compact plan shape, for asserting structure without naming modifiers:
    /// `F(n)` is a fused run of n, `S(i)` is a standalone step at stack index i.
    fn shape(plan: &[PlanItem]) -> Vec<String> {
        plan.iter()
            .map(|p| match p {
                PlanItem::Fused(seg) => format!("F({})", seg.len()),
                PlanItem::Step(i, _) => format!("S({i})"),
            })
            .collect()
    }

    #[test]
    fn empty_stack_plans_nothing() {
        assert!(plan_modifiers(&[]).is_empty());
    }

    const SRC: ImageSpec = ImageSpec { w: 1920, h: 1080 };

    #[test]
    fn image_spec_never_collapses_to_zero() {
        assert_eq!(ImageSpec::new(0, 0), ImageSpec { w: 1, h: 1 });
    }

    #[test]
    fn an_empty_plan_outputs_the_source_size() {
        assert_eq!(chain_output_spec(SRC, &[]), SRC);
    }

    /// Until a resize modifier exists, geometry is constant through the chain.
    /// This is the invariant that makes Phase 2 byte-neutral; when resize lands
    /// it should fail, and the failure is the signal to update it.
    #[test]
    fn every_stage_is_currently_passthrough() {
        let mods = vec![
            exposure(),
            blur(),
            m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.5,
                angle: 0.0,
            })),
            m(ModifierKind::ChromaticAberration(ChromaticAberration {
                amount: 5.0,
            })),
            exposure(),
        ];
        let plan = plan_modifiers(&mods);
        let specs = infer_specs(SRC, &plan);

        assert_eq!(specs.len(), plan.len(), "one spec per plan item");
        for (i, s) in specs.iter().enumerate() {
            assert!(
                s.is_passthrough(),
                "stage {i} resizes, but no modifier declares an output spec yet"
            );
            assert_eq!(s.input, SRC);
        }
        assert_eq!(chain_output_spec(SRC, &plan), SRC);
    }

    /// Specs must chain: each stage's input is the previous stage's output.
    /// Trivially true while everything is passthrough, but this is the property
    /// resize will rely on, so it is worth pinning before the behavior exists.
    #[test]
    fn stage_inputs_chain_from_previous_outputs() {
        let mods = vec![exposure(), blur(), exposure()];
        let plan = plan_modifiers(&mods);
        let specs = infer_specs(SRC, &plan);

        assert_eq!(specs[0].input, SRC, "first stage reads the source");
        for pair in specs.windows(2) {
            assert_eq!(
                pair[0].output, pair[1].input,
                "a stage must read exactly what the previous one produced"
            );
        }
    }

    #[test]
    fn adjacent_pointwise_modifiers_fuse() {
        let mods = vec![
            exposure(),
            m(ModifierKind::Posterize(Posterize { levels: 5 })),
            exposure(),
        ];
        assert_eq!(shape(&plan_modifiers(&mods)), ["F(3)"]);
    }

    #[test]
    fn a_kernel_step_splits_the_fused_runs_around_it() {
        let mods = vec![exposure(), blur(), exposure()];
        assert_eq!(shape(&plan_modifiers(&mods)), ["F(1)", "S(1)", "F(1)"]);
    }

    #[test]
    fn consecutive_non_pointwise_steps_stay_separate() {
        let mods = vec![blur(), blur()];
        assert_eq!(
            shape(&plan_modifiers(&mods)),
            ["S(0)", "S(1)"],
            "blurs must not be fused with each other"
        );
    }

    #[test]
    fn step_indices_refer_to_the_original_stack() {
        // The disabled modifier is dropped, but indices must still point into
        // the *input* stack: backends use them to look up positional side data.
        let mut disabled = exposure();
        disabled.enabled = false;
        let mods = vec![disabled, exposure(), blur()];
        assert_eq!(shape(&plan_modifiers(&mods)), ["F(1)", "S(2)"]);
    }

    #[test]
    fn disabled_modifiers_are_dropped() {
        let mut off = blur();
        off.enabled = false;
        let mods = vec![exposure(), off, exposure()];
        assert_eq!(
            shape(&plan_modifiers(&mods)),
            ["F(2)"],
            "a disabled step must not split the fused run around it"
        );
    }

    #[test]
    fn modifiers_that_report_no_effect_are_dropped() {
        // PixelSort is inert at threshold >= 1.0, so `has_effect` is state
        // dependent, not a per-type constant.
        let inert = m(ModifierKind::PixelSort(PixelSort {
            threshold: 1.0,
            angle: 0.0,
        }));
        let mods = vec![exposure(), inert, exposure()];
        assert_eq!(shape(&plan_modifiers(&mods)), ["F(2)"]);
    }

    #[test]
    fn ordering_is_preserved_exactly() {
        let mods = vec![
            exposure(),
            blur(),
            m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.5,
                angle: 0.0,
            })),
            exposure(),
            m(ModifierKind::ChromaticAberration(ChromaticAberration {
                amount: 5.0,
            })),
        ];
        assert_eq!(
            shape(&plan_modifiers(&mods)),
            ["F(1)", "S(1)", "S(2)", "F(1)", "S(4)"]
        );
    }

    /// Every modifier type, classified by the planner.
    ///
    /// The CPU backend historically branched on `ModifierKind` directly while
    /// the GPU backend branched on `effect_class()`. Those two rules must agree
    /// for every type, or a modifier gets fused on one backend and split on the
    /// other — which shows up as a rendering difference, not a compile error.
    /// This walks `ModifierType::ALL` so a newly added type cannot skip the
    /// check.
    #[test]
    fn planner_classification_covers_every_modifier_type() {
        use crate::modifiers::ModifierType;

        // Types the CPU backend gives a dedicated (non-fused) branch to. Keep
        // in sync with the match in `cpu::render_full`.
        const CPU_DEDICATED: &[&str] = &[
            "Gaussian Blur",
            "Chromatic Aberration",
            "Motion Blur",
            "Text",
            "Drawing",
            "Pixel Sort",
        ];

        for t in ModifierType::ALL {
            let kind = ModifierKind::from(t.clone());
            let name = kind.name();
            let pointwise = kind.effect_class().is_pointwise();
            let cpu_dedicated = CPU_DEDICATED.contains(&name);
            assert_eq!(
                !pointwise,
                cpu_dedicated,
                "{name}: planner says pointwise={pointwise} but the CPU path \
                 {} give it a dedicated branch. The two backends would segment \
                 this modifier differently.",
                if cpu_dedicated { "does" } else { "does not" }
            );
        }
    }

    #[test]
    fn every_non_pointwise_kind_gets_its_own_step() {
        // Locks the partition the CPU and GPU backends both branch on. A new
        // modifier that is non-pointwise but missing from a backend's match
        // would show up here as a shape change.
        let mods = vec![
            blur(),
            m(ModifierKind::MotionBlur(MotionBlur {
                angle: 10.0,
                distance: 8.0,
            })),
            m(ModifierKind::ChromaticAberration(ChromaticAberration {
                amount: 5.0,
            })),
            m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.5,
                angle: 0.0,
            })),
            m(ModifierKind::Text(Text {
                content: "x".into(),
                ..Text::default()
            })),
            m(ModifierKind::Drawing(Drawing {
                strokes: vec![Stroke {
                    points: vec![[0.1, 0.1], [0.9, 0.9]],
                    size: 5.0,
                    hardness: 0.5,
                    opacity: 1.0,
                    color: [1.0, 0.0, 0.0],
                }],
                ..Drawing::default()
            })),
        ];
        assert_eq!(
            shape(&plan_modifiers(&mods)),
            ["S(0)", "S(1)", "S(2)", "S(3)", "S(4)", "S(5)"]
        );
    }
}
