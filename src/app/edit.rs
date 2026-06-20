use iced::Task;

use crate::{
    app::Message,
    components::notifications::Notification,
    modifiers::{
        Modifier, ModifierKind, ModifierParam, ModifierType,
        kinds::{Crop, Text},
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

pub fn update(state: &mut EditState, program: &mut ViewProgram, msg: EditMsg) -> Task<Message> {
    match msg {
        EditMsg::SelectTool(tool) => {
            let was_crop = state.selected_tool == Tool::Crop;
            let is_crop = tool == Tool::Crop;
            let is_text = tool == Tool::Text;
            state.selected_tool = tool;
            program.crop_tool_active = is_crop;
            if is_crop {
                if let Some(idx) = program
                    .modifiers
                    .iter()
                    .position(|m| m.kind.as_crop().is_some())
                {
                    state.active = Some(idx);
                } else {
                    let (iw, ih) = program
                        .image_size()
                        .map(|(w, h)| (w as f32, h as f32))
                        .unwrap_or((1.0, 1.0));
                    let idx = program.modifiers.len();
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
        }
        EditMsg::Add(t) => {
            let is_crop = matches!(t, ModifierType::Crop);
            let is_text = matches!(t, ModifierType::Text);
            let already_has_crop =
                is_crop && program.modifiers.iter().any(|m| m.kind.as_crop().is_some());
            if already_has_crop {
                return Task::done(Message::Notify(Notification::warning(
                    "Only one Crop modifier is allowed.",
                )));
            }
            let kind = if is_crop {
                let (iw, ih) = program
                    .image_size()
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
            let img_size = program.image_size();
            if let Some(m) = program.modifiers_mut().get_mut(i) {
                m.apply_param(param, img_size);
            }
            program.mark_dirty();
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
