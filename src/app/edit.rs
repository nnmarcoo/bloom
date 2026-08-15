//! Edit-mode state: the active tool and the modifier stack's message handling.
//!
//! Modifiers live in an ordered list the user can reorder freely; nothing here
//! restricts what may sit where. Geometry-changing modifiers are handled by the
//! renderer's planning, not by constraining the stack.
//!
//! An edit is resolved against the size that modifier *receives*, via
//! stage_input_size, not against the source. The two differ as soon as anything
//! upstream resizes or crops, and a parameter clamped against the wrong one
//! produces a document the panel never showed. A modifier about to be appended
//! asks for index len(), which is the chain's current output.
//!
//! A stack may hold any number of crops. The old one-crop rule was not a
//! product decision but a consequence of crop being a single display-time
//! window; as a chain stage each crop reframes whatever the one before it
//! produced, which is what makes crop, effect, crop possible.
//!
//! update refits the view by comparing document_size across the edit, rather
//! than listing the messages that resize. The list form only named Resize's
//! width and height, so editing a crop, dragging the crop overlay, disabling a
//! resize, or removing one all left the view scaled for the previous document.
//! Any new geometry-changing modifier would have joined them. Comparing sizes
//! also leaves a size-preserving edit alone, which matters because a refit
//! fights a user who zoomed in deliberately.
//!
//! Opening and closing the crop tool is the one refit this cannot see: it
//! switches the view between the cropped and uncropped extents while the
//! document keeps its size, so SelectTool refits explicitly.

use iced::Task;

use crate::{
    app::Message,
    components::notifications::Notification,
    modifiers::{
        Modifier, ModifierKind, ModifierParam, ModifierType,
        kinds::{Crop, Drawing, Text},
        tool_target,
    },
    wgpu::view_program::ViewProgram,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Draw,
    Text,
}

#[derive(Debug, Clone)]
pub enum EditMsg {
    SelectTool(Tool),
    Add(ModifierType),
    Remove(usize),
    ToggleExpanded(usize),
    ToggleEnabled(usize),
    Update(usize, ModifierParam),
    SetActive(usize),
    ClearActive,
    DragStart(usize),
    DragHover(usize),
    DragEnd,
    SetCropRect(usize, f32, f32, f32, f32),
}

pub struct EditState {
    pub selected_tool: Tool,
    pub active: Option<usize>,
    pub dragging: Option<usize>,
    pub drag_hover: Option<usize>,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            selected_tool: Tool::Select,
            active: None,
            dragging: None,
            drag_hover: None,
        }
    }
}

pub fn update(
    state: &mut EditState,
    program: &mut ViewProgram,
    timed: bool,
    msg: EditMsg,
) -> Task<Message> {
    let doc_before = program.document_size();
    let task = update_inner(state, program, timed, msg);
    if program.document_size() != doc_before {
        program.fit();
    }
    task
}

fn update_inner(
    state: &mut EditState,
    program: &mut ViewProgram,
    timed: bool,
    msg: EditMsg,
) -> Task<Message> {
    match msg {
        EditMsg::SelectTool(tool) => {
            let was_crop = state.selected_tool == Tool::Crop;
            let is_crop = tool == Tool::Crop;
            let is_text = tool == Tool::Text;
            let is_draw = tool == Tool::Draw;
            state.selected_tool = tool;
            program.crop_tool_active = is_crop;
            if is_crop {
                let existing = tool_target(&program.modifiers, state.active, |m| {
                    m.enabled && m.kind.as_crop().is_some()
                });
                if let Some(idx) = existing {
                    state.active = Some(idx);
                } else {
                    let idx = program.modifiers.len();
                    let (iw, ih) = program
                        .stage_input_size(idx)
                        .or_else(|| program.image_size())
                        .map(|(w, h)| (w as f32, h as f32))
                        .unwrap_or((1.0, 1.0));
                    program
                        .modifiers_mut()
                        .push(Modifier::new(ModifierKind::Crop(Crop {
                            x: 0.0,
                            y: 0.0,
                            width: iw,
                            height: ih,
                        })));
                    state.active = Some(idx);
                    program.mark_dirty();
                }
                program.fit();
            } else if was_crop {
                program.fit();
            }
            if is_text {
                if let Some(idx) = program
                    .modifiers
                    .iter()
                    .rposition(|m| matches!(m.kind, ModifierKind::Text(_)))
                {
                    state.active = Some(idx);
                } else {
                    let idx = program.modifiers.len();
                    program
                        .modifiers_mut()
                        .push(Modifier::new(ModifierKind::Text(Text::default())));
                    state.active = Some(idx);
                    program.mark_dirty();
                }
            }
            if is_draw {
                if let Some(idx) = program
                    .modifiers
                    .iter()
                    .rposition(|m| m.enabled && matches!(m.kind, ModifierKind::Drawing(_)))
                {
                    state.active = Some(idx);
                } else {
                    let idx = program.modifiers.len();
                    program
                        .modifiers_mut()
                        .push(Modifier::new(ModifierKind::Drawing(Drawing::default())));
                    state.active = Some(idx);
                    program.mark_dirty();
                }
            }
        }
        EditMsg::Add(t) => {
            let is_crop = matches!(t, ModifierType::Crop);
            let is_text = matches!(t, ModifierType::Text);
            let is_draw = matches!(t, ModifierType::Drawing);
            if matches!(t, ModifierType::Trim) {
                if program.modifiers.iter().any(|m| m.kind.as_trim().is_some()) {
                    return Task::done(Message::Notify(Notification::warning(
                        "Only one Trim modifier is allowed.",
                    )));
                }
                if !timed {
                    return Task::done(Message::Notify(Notification::warning(
                        "Trim only applies to animations and video.",
                    )));
                }
            }
            let kind = if is_crop {
                let (iw, ih) = program
                    .stage_input_size(program.modifiers.len())
                    .or_else(|| program.image_size())
                    .map(|(w, h)| (w as f32, h as f32))
                    .unwrap_or((1.0, 1.0));
                ModifierKind::Crop(Crop {
                    x: 0.0,
                    y: 0.0,
                    width: iw,
                    height: ih,
                })
            } else {
                ModifierKind::from(t)
            };
            program.modifiers_mut().push(Modifier::new(kind));
            let idx = program.modifiers.len() - 1;
            state.active = Some(idx);
            if is_text {
                state.selected_tool = Tool::Text;
                program.crop_tool_active = false;
            }
            if is_draw {
                state.selected_tool = Tool::Draw;
                program.crop_tool_active = false;
            }
            program.mark_dirty();
        }
        EditMsg::Remove(i) => {
            if i < program.modifiers.len() {
                program.mark_dirty();
                program.modifiers_mut().remove(i);
                state.active = match state.active {
                    Some(a) if a == i => None,
                    Some(a) if a > i => Some(a - 1),
                    other => other,
                };
            }
        }
        EditMsg::ToggleExpanded(i) => {
            if let Some(m) = program.modifiers_mut().get_mut(i) {
                m.expanded = !m.expanded;
            }
        }
        EditMsg::ToggleEnabled(i) => {
            if let Some(m) = program.modifiers_mut().get_mut(i) {
                m.enabled = !m.enabled;
            }
            program.mark_dirty();
        }
        EditMsg::Update(i, param) => {
            let stroke_edit = matches!(
                param,
                ModifierParam::DrawingStrokeStart(_) | ModifierParam::DrawingStrokeExtend(_)
            );
            let img_size = program.stage_input_size(i);
            if let Some(m) = program.modifiers_mut().get_mut(i) {
                m.apply_param(param, img_size);
            }
            if !stroke_edit {
                program.mark_dirty();
            }
        }
        EditMsg::SetActive(i) => {
            if i < program.modifiers.len() {
                state.active = Some(i);
            }
        }
        EditMsg::ClearActive => {
            state.active = None;
        }
        EditMsg::DragStart(i) => {
            state.dragging = Some(i);
            state.drag_hover = Some(i);
        }
        EditMsg::DragHover(i) => {
            if state.dragging.is_some() {
                state.drag_hover = Some(i);
            }
        }
        EditMsg::DragEnd => {
            let source = state.dragging.take();
            let target = state.drag_hover.take();
            if let (Some(src), Some(tgt)) = (source, target)
                && src != tgt
            {
                let m = program.modifiers_mut().remove(src);
                let insert_at = if tgt > src { tgt - 1 } else { tgt };
                program.modifiers_mut().insert(insert_at, m);
                program.mark_dirty();
                if let Some(active) = state.active {
                    state.active = Some(if active == src {
                        insert_at
                    } else {
                        let after_remove = if active > src { active - 1 } else { active };
                        if after_remove >= insert_at {
                            after_remove + 1
                        } else {
                            after_remove
                        }
                    });
                }
            }
        }
        EditMsg::SetCropRect(i, x, y, w, h) => {
            if let Some(m) = program.modifiers_mut().get_mut(i)
                && let Some(crop) = m.kind.as_crop_mut()
            {
                crop.x = x;
                crop.y = y;
                crop.width = w;
                crop.height = h;
            }
            program.mark_dirty();
        }
    }
    Task::none()
}

#[cfg(test)]
mod fit_on_resize_tests {
    use super::*;
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::wgpu::media::image_data::ImageData;
    use glam::Vec2;
    use iced::Rectangle;

    const SRC_W: u32 = 800;
    const SRC_H: u32 = 600;

    fn program_with_resize() -> ViewProgram {
        let mut p = ViewProgram::default();
        p.set_image(ImageData::new(
            vec![255u8; (SRC_W * SRC_H * 4) as usize],
            SRC_W,
            SRC_H,
        ));
        p.set_bounds(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        });
        p.modifiers_mut()
            .push(Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 100.0,
                height: 100.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            })));
        p.fit();
        p
    }

    #[test]
    fn a_huge_image_refits_across_the_whole_slider_range() {
        const HUGE: u32 = 30000;
        let mut p = ViewProgram::default();
        p.set_image(ImageData::new(Vec::new(), HUGE, HUGE));
        p.set_bounds(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        });
        p.modifiers_mut()
            .push(Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 100.0,
                height: 100.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            })));
        p.fit();

        for pct in [100.0f32, 75.0, 50.0, 10.0, 1.0] {
            edit(&mut p, ModifierParam::ResizeWidth(pct));

            let spec = crate::modifiers::plan::chain_output_spec(
                crate::modifiers::plan::ImageSpec::new(HUGE, HUGE),
                &crate::modifiers::plan::plan_modifiers(&p.modifiers),
            );
            let doc = Vec2::new(spec.w as f32, spec.h as f32);
            let on_screen = doc * p.scale();

            assert!(
                p.scale().is_finite() && p.scale() > 0.0,
                "{pct}%: scale is {}",
                p.scale()
            );
            assert!(
                on_screen.x <= 1601.0 && on_screen.y <= 901.0,
                "{pct}%: doc {doc:?} renders {on_screen:?}, overflowing the \
                 1600x900 viewport"
            );
            assert!(
                on_screen.x >= 1599.0 || on_screen.y >= 899.0,
                "{pct}%: doc {doc:?} renders {on_screen:?}, touching neither \
                 edge of the 1600x900 viewport"
            );
        }
    }

    #[test]
    fn every_edit_that_changes_the_document_refits() {
        // Both families that can resize the document, not just Resize's own
        // params: a list holding one kind is what let the crop and toggle
        // paths ship without a refit.
        let cases: Vec<(&str, ModifierParam)> = vec![
            ("width", ModifierParam::ResizeWidth(37.0)),
            ("height", ModifierParam::ResizeHeight(37.0)),
            ("crop-width", ModifierParam::CropWidth(120.0)),
            ("crop-height", ModifierParam::CropHeight(90.0)),
            ("crop-x", ModifierParam::CropX(40.0)),
            ("crop-y", ModifierParam::CropY(30.0)),
        ];

        let doc_of = |p: &ViewProgram| -> Vec2 {
            let spec = crate::modifiers::plan::chain_output_spec(
                crate::modifiers::plan::ImageSpec::new(SRC_W, SRC_H),
                &crate::modifiers::plan::plan_modifiers(&p.modifiers),
            );
            Vec2::new(spec.w as f32, spec.h as f32)
        };

        for (label, param) in cases {
            let is_crop = label.starts_with("crop-");
            let mut p = if is_crop {
                program_with_crop()
            } else {
                program_with_resize()
            };
            let before_doc = doc_of(&p);
            let before_scale = p.scale();

            edit(&mut p, param);

            let after_doc = doc_of(&p);
            assert_ne!(
                after_doc, before_doc,
                "{label}: the document did not change, so this case proves \
                 nothing about refitting. Fix the case, do not skip it."
            );

            let on_screen = after_doc * p.scale();
            let filled = (on_screen.x / 1000.0).max(on_screen.y / 800.0);
            assert!(
                on_screen.x <= 1000.0 + 1.0 && on_screen.y <= 800.0 + 1.0 && filled > 0.99,
                "{label}: the document changed from {before_doc:?} to \
                 {after_doc:?} but the view was not refitted (scale stayed \
                 {before_scale}), so it now renders {on_screen:?} inside the \
                 1000x800 viewport"
            );
        }
    }

    fn edit(p: &mut ViewProgram, param: ModifierParam) {
        let _ = update(
            &mut EditState::default(),
            p,
            false,
            EditMsg::Update(0, param),
        );
    }

    #[test]
    fn a_second_crop_can_be_added_and_reframes_what_the_first_produced() {
        use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

        let mut p = program_with_resize();
        p.modifiers_mut().clear();
        let mut st = EditState::default();

        let _ = update(&mut st, &mut p, false, EditMsg::Add(ModifierType::Crop));
        let _ = update(
            &mut st,
            &mut p,
            false,
            EditMsg::SetCropRect(0, 0.0, 0.0, 400.0, 300.0),
        );
        let _ = update(&mut st, &mut p, false, EditMsg::Add(ModifierType::Crop));
        let _ = update(
            &mut st,
            &mut p,
            false,
            EditMsg::SetCropRect(1, 10.0, 20.0, 100.0, 80.0),
        );

        assert_eq!(
            p.modifiers
                .iter()
                .filter(|m| m.kind.as_crop().is_some())
                .count(),
            2,
            "the second crop was refused"
        );
        assert_eq!(
            chain_output_spec(ImageSpec::new(SRC_W, SRC_H), &plan_modifiers(&p.modifiers)),
            ImageSpec::new(100, 80),
            "the second crop takes 100x80 of the 400x300 the first produced"
        );
    }

    #[test]
    fn a_crop_added_after_a_resize_starts_at_the_resized_size() {
        let mut p = program_with_resize();
        let mut st = EditState::default();
        let _ = update(
            &mut st,
            &mut p,
            false,
            EditMsg::Update(0, ModifierParam::ResizeWidth(50.0)),
        );
        let _ = update(&mut st, &mut p, false, EditMsg::Add(ModifierType::Crop));

        let crop = p.modifiers[1].kind.as_crop().expect("crop was added");
        assert_eq!(
            (crop.width, crop.height),
            (SRC_W as f32 * 0.5, SRC_H as f32 * 0.5),
            "a fresh crop spans the image it receives, not the source"
        );
    }

    #[test]
    fn an_edit_resolves_against_the_stage_input_not_the_source() {
        use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

        let mut p = program_with_resize();
        p.modifiers_mut()
            .push(Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Pixels,
                width: 800.0,
                height: 600.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: false,
            })));
        let _ = update(
            &mut EditState::default(),
            &mut p,
            false,
            EditMsg::Update(0, ModifierParam::ResizeWidth(50.0)),
        );

        let _ = update(
            &mut EditState::default(),
            &mut p,
            false,
            EditMsg::Update(1, ModifierParam::ResizeWidth(800.0)),
        );

        let doc = chain_output_spec(ImageSpec::new(SRC_W, SRC_H), &plan_modifiers(&p.modifiers));
        assert_eq!(
            doc.w, 400,
            "the second resize receives a 400px-wide image, so its width must \
             clamp to 400. Clamping against the 800px source instead lets it \
             ask for an upscale the panel presents as a limit, and the \
             document comes out {}px wide.",
            doc.w
        );
    }

    #[test]
    fn a_resize_from_a_zoomed_in_view_refits() {
        let mut p = program_with_resize();
        p.set_scale(4.0, Vec2::ZERO);

        edit(&mut p, ModifierParam::ResizeWidth(60.0));

        assert!(p.fit_active(), "the view is not in fit mode after a resize");
        assert!(
            p.scale() != 4.0,
            "the view stayed at {} after the document changed size; the user \
             still has to zoom to see what they just did",
            p.scale()
        );
    }

    #[test]
    fn a_downscale_refits_the_view() {
        let mut p = program_with_resize();
        let before = p.scale();

        edit(&mut p, ModifierParam::ResizeHeight(50.0));

        assert!(p.fit_active());
        assert!(
            p.scale() > before,
            "a half-size document should fill the viewport at a larger scale, \
             but the scale stayed at {}",
            p.scale()
        );
    }

    fn fits_the_viewport(p: &ViewProgram, bounds: Vec2) -> bool {
        use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};
        let doc = chain_output_spec(ImageSpec::new(SRC_W, SRC_H), &plan_modifiers(&p.modifiers));
        let on_screen = Vec2::new(doc.w as f32, doc.h as f32) * p.scale();
        let filled = (on_screen.x / bounds.x).max(on_screen.y / bounds.y);
        on_screen.x <= bounds.x * 1.01 && on_screen.y <= bounds.y * 1.01 && filled > 0.99
    }

    fn program_with_crop() -> ViewProgram {
        let mut p = ViewProgram::default();
        p.set_image(ImageData::new(
            vec![255u8; (SRC_W * SRC_H * 4) as usize],
            SRC_W,
            SRC_H,
        ));
        p.set_bounds(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        });
        p.modifiers_mut().push(Modifier::new(ModifierKind::Crop(
            crate::modifiers::kinds::Crop {
                x: 0.0,
                y: 0.0,
                width: SRC_W as f32,
                height: SRC_H as f32,
            },
        )));
        p.fit();
        p
    }

    #[test]
    fn editing_a_crop_refits_the_view() {
        let mut p = program_with_crop();
        edit(&mut p, ModifierParam::CropWidth(200.0));
        edit(&mut p, ModifierParam::CropHeight(150.0));

        assert!(
            fits_the_viewport(&p, Vec2::new(1000.0, 800.0)),
            "a 200x150 crop is not fitted to the viewport; scale is {} so the \
             document draws at {}x{} inside 1000x800",
            p.scale(),
            200.0 * p.scale(),
            150.0 * p.scale()
        );
    }

    #[test]
    fn dragging_the_crop_overlay_refits_the_view() {
        let mut p = program_with_crop();
        let _ = update(
            &mut EditState::default(),
            &mut p,
            false,
            EditMsg::SetCropRect(0, 0.0, 0.0, 200.0, 150.0),
        );

        assert!(
            fits_the_viewport(&p, Vec2::new(1000.0, 800.0)),
            "dragging the crop overlay left the view at scale {}",
            p.scale()
        );
    }

    #[test]
    fn disabling_a_resize_refits_the_view() {
        let mut p = program_with_resize();
        edit(&mut p, ModifierParam::ResizeWidth(25.0));

        let _ = update(
            &mut EditState::default(),
            &mut p,
            false,
            EditMsg::ToggleEnabled(0),
        );

        assert!(
            fits_the_viewport(&p, Vec2::new(1000.0, 800.0)),
            "disabling the resize restored the 800x600 document but the view \
             stayed at scale {}, which was fitted to the 200x150 one",
            p.scale()
        );
    }

    #[test]
    fn removing_a_resize_refits_the_view() {
        let mut p = program_with_resize();
        edit(&mut p, ModifierParam::ResizeWidth(25.0));

        let _ = update(&mut EditState::default(), &mut p, false, EditMsg::Remove(0));

        assert!(
            fits_the_viewport(&p, Vec2::new(1000.0, 800.0)),
            "removing the resize restored the 800x600 document but the view \
             stayed at scale {}",
            p.scale()
        );
    }

    #[test]
    fn changing_the_filter_leaves_the_view_alone() {
        let mut p = program_with_resize();
        p.set_scale(4.0, Vec2::ZERO);

        edit(&mut p, ModifierParam::ResizeFilter(ResizeFilter::Nearest));

        assert_eq!(
            p.scale(),
            4.0,
            "changing the filter refit the view, which would fight a user who \
             zoomed in to compare filters"
        );
    }

    #[test]
    fn opening_the_crop_tool_refits_even_though_the_document_is_unchanged() {
        // The tool shows the uncropped extent, so the displayed size changes
        // while document_size() does not. The refit in SelectTool carries this
        // case; the document-size comparison in update cannot see it.
        let mut p = program_with_crop();
        let mut state = EditState::default();
        let _ = update(
            &mut state,
            &mut p,
            false,
            EditMsg::Update(0, ModifierParam::CropWidth(200.0)),
        );
        let cropped_scale = p.scale();

        let _ = update(&mut state, &mut p, false, EditMsg::SelectTool(Tool::Crop));

        assert_ne!(
            p.scale(),
            cropped_scale,
            "opening the crop tool switched the view to the uncropped extent \
             but left the scale fitted to the cropped one"
        );
    }

    #[test]
    fn a_size_preserving_edit_leaves_the_view_alone() {
        // Mode conversion restates the same document in other units and
        // lock_aspect only sets a flag, so neither may disturb a manual zoom.
        for (label, param) in [
            ("mode", ModifierParam::ResizeMode(ResizeMode::Pixels)),
            ("lock-aspect", ModifierParam::ResizeLockAspect(false)),
        ] {
            let mut p = program_with_resize();
            p.set_scale(4.0, Vec2::ZERO);

            edit(&mut p, param);

            assert_eq!(
                p.scale(),
                4.0,
                "{label}: the document kept its size but the view refit anyway, \
                 which would fight a user who zoomed in"
            );
        }
    }
}
