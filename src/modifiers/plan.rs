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
//!
//! Stage sizes come from each modifier's own output_spec, never from a match on
//! the kind here. A modifier that changes dimensions declares that itself, so
//! adding one does not mean editing the planner.

use crate::modifiers::Modifier;

#[derive(Debug)]
pub enum PlanItem<'a> {
    Fused(Vec<&'a Modifier>),
    Step(usize, &'a Modifier),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    pub w: u32,
    pub h: u32,
}

impl ImageSpec {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w: w.max(1),
            h: h.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSpec {
    pub input: ImageSpec,
    pub output: ImageSpec,
}

impl StageSpec {
    #[allow(dead_code, reason = "used by the plan tests")]
    pub fn is_passthrough(&self) -> bool {
        self.input == self.output
    }
}

pub fn infer_specs(source: ImageSpec, plan: &[PlanItem]) -> Vec<StageSpec> {
    let mut cur = source;
    plan.iter()
        .map(|item| {
            let output = match item {
                PlanItem::Fused(_) => cur,
                PlanItem::Step(_, m) => m.kind.output_spec(cur),
            };
            let spec = StageSpec { input: cur, output };
            cur = output;
            spec
        })
        .collect()
}

pub fn chain_output_spec(source: ImageSpec, plan: &[PlanItem]) -> ImageSpec {
    infer_specs(source, plan)
        .last()
        .map_or(source, |s| s.output)
}

pub fn stage_inputs(source: ImageSpec, modifiers: &[Modifier]) -> Vec<ImageSpec> {
    let mut cur = source;
    modifiers
        .iter()
        .map(|m| {
            let input = cur;
            if m.has_visible_effect() {
                cur = m.kind.output_spec(cur);
            }
            input
        })
        .collect()
}

fn is_fusable(m: &Modifier) -> bool {
    m.kind.effect_class().is_pointwise() && !m.kind.changes_geometry()
}

pub fn plan_modifiers(modifiers: &[Modifier]) -> Vec<PlanItem<'_>> {
    let mut plan: Vec<PlanItem> = Vec::new();
    let mut current: Vec<&Modifier> = Vec::new();
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        if !is_fusable(m) {
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
        Resize, ResizeFilter, ResizeMode, Stroke, Text,
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

    #[test]
    fn stage_sizes_come_from_each_modifiers_own_declaration() {
        use crate::modifiers::ModifierType;

        let src = ImageSpec::new(800, 600);
        for t in ModifierType::ALL {
            let kind = ModifierKind::from(t.clone());
            let name = kind.name();
            let declared = kind.output_spec(src);

            let mods = vec![m(kind)];
            let plan = plan_modifiers(&mods);
            let Some(spec) = infer_specs(src, &plan).into_iter().next() else {
                continue;
            };

            assert_eq!(
                spec.output, declared,
                "{name}: the plan produced {:?} but the modifier declares \
                 {declared:?}. infer_specs must read output_spec rather than \
                 special-case a kind, or a new dimension-changing modifier \
                 renders at the wrong size until the planner is edited too.",
                spec.output
            );
        }
    }

    #[test]
    fn a_geometry_changing_modifier_is_never_fused() {
        use crate::modifiers::ModifierType;

        let src = ImageSpec::new(800, 600);
        for t in ModifierType::ALL {
            let kind = ModifierKind::from(t.clone());
            if !kind.changes_geometry() {
                continue;
            }
            let name = kind.name();

            let mods = vec![exposure(), m(kind), exposure()];
            let plan = plan_modifiers(&mods);

            assert_eq!(
                shape(&plan),
                ["F(1)", "S(1)", "F(1)"],
                "{name} changes geometry but was folded into a fused run. A \
                 fused run is evaluated at one coordinate in one space, so a \
                 stage that moves or resizes its output cannot join one -- it \
                 would be planned and sized and then silently rendered as a \
                 passthrough."
            );

            let specs = infer_specs(src, &plan);
            assert_eq!(
                specs[1].input, src,
                "{name}: the fused run before it must not have changed the size"
            );
        }
    }

    #[test]
    fn a_modifier_that_declares_a_new_size_needs_no_planner_change() {
        let src = ImageSpec::new(800, 600);
        let half = m(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: 50.0,
            height: 50.0,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }));
        assert_eq!(half.kind.output_spec(src), ImageSpec::new(400, 300));

        let plan = plan_modifiers(std::slice::from_ref(&half));
        assert_eq!(
            chain_output_spec(src, &plan),
            half.kind.output_spec(src),
            "the chain's output must be exactly what the modifier declared"
        );
    }

    #[test]
    fn a_modifier_sees_the_size_the_one_before_it_produced() {
        let mods = vec![resize_pct(50.0), blur(), resize_pct(50.0)];
        let inputs = stage_inputs(SRC, &mods);

        assert_eq!(inputs[0], SRC, "the first modifier reads the source");
        assert_eq!(
            inputs[1],
            ImageSpec::new(960, 540),
            "the blur sits after a half resize"
        );
        assert_eq!(
            inputs[2],
            ImageSpec::new(960, 540),
            "the second resize resolves its percentage against the already \
             halved image, not against the source"
        );
    }

    #[test]
    fn stage_inputs_agrees_with_the_planner() {
        let mods = vec![
            exposure(),
            resize_pct(50.0),
            blur(),
            resize_pct(200.0),
            exposure(),
        ];
        let inputs = stage_inputs(SRC, &mods);
        let plan = plan_modifiers(&mods);
        let specs = infer_specs(SRC, &plan);

        for (item, spec) in plan.iter().zip(&specs) {
            if let PlanItem::Step(i, _) = item {
                assert_eq!(
                    inputs[*i], spec.input,
                    "modifier {i}: the panel resolves against {:?} while the \
                     renderer feeds it {:?}. stage_inputs and infer_specs walk \
                     the same declarations, so they must not disagree -- that \
                     gap is what made a panel show one size and the canvas \
                     render another.",
                    inputs[*i], spec.input
                );
            }
        }

        assert_eq!(
            chain_output_spec(SRC, &plan),
            SRC,
            "sanity: halving then doubling returns to the source size"
        );
    }

    #[test]
    fn every_modifier_has_a_stage_input() {
        let mods = vec![exposure(), resize_pct(50.0), blur()];
        assert_eq!(
            stage_inputs(SRC, &mods).len(),
            mods.len(),
            "stage_inputs is indexed by stack position, so every modifier -- \
             including ones the planner drops -- must have an entry"
        );
    }

    #[test]
    fn a_disabled_resize_does_not_move_the_stages_after_it() {
        let mut off = resize_pct(50.0);
        off.enabled = false;
        let mods = vec![off, blur()];
        assert_eq!(
            stage_inputs(SRC, &mods)[1],
            SRC,
            "a disabled resize contributes nothing, so the blur still reads \
             the source"
        );
    }

    #[test]
    fn a_disabled_modifier_still_reports_its_own_input() {
        let mut off = blur();
        off.enabled = false;
        let mods = vec![resize_pct(50.0), off];
        assert_eq!(
            stage_inputs(SRC, &mods)[1],
            ImageSpec::new(960, 540),
            "a disabled modifier still shows a panel, and that panel must \
             resolve against the size it would receive once re-enabled"
        );
    }

    #[test]
    fn only_resize_changes_dimensions() {
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
            assert!(s.is_passthrough(), "stage {i} unexpectedly resizes");
            assert_eq!(s.input, SRC);
        }
        assert_eq!(chain_output_spec(SRC, &plan), SRC);
    }

    fn resize_pct(pct: f32) -> Modifier {
        m(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }))
    }

    #[test]
    fn resize_declares_its_output_and_later_stages_follow() {
        let mods = vec![exposure(), resize_pct(50.0), blur()];
        let plan = plan_modifiers(&mods);
        let specs = infer_specs(SRC, &plan);

        assert_eq!(specs[0].input, SRC);
        assert_eq!(specs[0].output, SRC);
        assert_eq!(specs[1].input, SRC);
        assert_eq!(specs[1].output, ImageSpec::new(SRC.w / 2, SRC.h / 2));
        assert_eq!(specs[2].input, ImageSpec::new(SRC.w / 2, SRC.h / 2));
        assert_eq!(
            chain_output_spec(SRC, &plan),
            ImageSpec::new(SRC.w / 2, SRC.h / 2)
        );
    }

    #[test]
    fn percent_resizes_compound() {
        let mods = vec![resize_pct(50.0), resize_pct(50.0)];
        let plan = plan_modifiers(&mods);
        assert_eq!(
            chain_output_spec(SRC, &plan),
            ImageSpec::new(SRC.w / 4, SRC.h / 4)
        );
    }

    #[test]
    fn opposing_resizes_are_not_collapsed() {
        let mods = vec![resize_pct(50.0), resize_pct(200.0)];
        let plan = plan_modifiers(&mods);
        assert_eq!(plan.len(), 2, "both resizes must remain in the plan");
        assert_eq!(chain_output_spec(SRC, &plan), SRC);
    }

    #[test]
    fn identity_resize_is_retained() {
        let mods = vec![resize_pct(100.0)];
        let plan = plan_modifiers(&mods);
        assert_eq!(plan.len(), 1);
        assert_eq!(chain_output_spec(SRC, &plan), SRC);
    }

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

    /// A modifier needs a dedicated CPU branch exactly when the planner refuses
    /// to fuse it. Being pointwise is not the rule on its own: Crop reads one
    /// sample per output pixel and is Pointwise, but it changes geometry, so it
    /// gets its own step and its own CPU arm. Asserting against is_pointwise
    /// alone passed only because Crop was left out of the list, which made the
    /// list agree with the assertion by cancelling two errors.
    #[test]
    fn planner_classification_covers_every_modifier_type() {
        use crate::modifiers::ModifierType;

        const CPU_DEDICATED: &[&str] = &[
            "Gaussian Blur",
            "Chromatic Aberration",
            "Motion Blur",
            "Text",
            "Drawing",
            "Pixel Sort",
            "Resize",
            "Crop",
        ];

        for t in ModifierType::ALL {
            let kind = ModifierKind::from(t.clone());
            let name = kind.name();
            let m = Modifier::new(kind);
            let fusable = is_fusable(&m);
            let cpu_dedicated = CPU_DEDICATED.contains(&name);
            assert_eq!(
                !fusable,
                cpu_dedicated,
                "{name}: the planner {} fuse this modifier but the CPU path \
                 {} give it a dedicated branch. A modifier the planner splits \
                 into its own step renders as a passthrough unless the CPU \
                 backend has an arm for it.",
                if fusable { "does" } else { "does not" },
                if cpu_dedicated { "does" } else { "does not" }
            );
        }
    }

    #[test]
    fn every_non_pointwise_kind_gets_its_own_step() {
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
