use std::borrow::Cow;

use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
use iced::advanced::clipboard::Kind as ClipboardKind;
use iced::advanced::graphics::geometry::{Frame, Path, Renderer as GeometryRenderer, Stroke};
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::app::{EditMsg, Message};
use crate::modifiers::ModifierParam;
use crate::modifiers::kinds::Text;
use crate::modifiers::text_render;
use crate::wgpu::view_program::ViewProgram;

const HANDLE_R: f32 = 6.0;
const HANDLE_HIT: f32 = 12.0;
const EDGE_BAND: f32 = 10.0;
const DRAG_THRESHOLD: f32 = 2.0;
const ROTATION_SNAP: f32 = 15.0;

const OUTLINE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn clamp_caret(s: &str, caret: usize) -> usize {
    let caret = caret.min(s.len());
    if s.is_char_boundary(caret) {
        caret
    } else {
        s[..caret]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

fn sanitize_paste(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grab {
    Move,
    Scale,
    Rotate,
    Text,
}

struct DragState {
    grab: Grab,
    start_cursor: Vec2,
    start_x: f32,
    start_y: f32,
    start_size: f32,
    start_rotation: f32,
    text_anchor: usize,
    moved: bool,
}

#[derive(Default)]
struct State {
    drag: Option<DragState>,
    caret: usize,
    caret_idx: Option<usize>,
    selection: Option<usize>,
    pending: Option<String>,
    shift: bool,
}

impl State {
    fn sel_range(&self) -> Option<(usize, usize)> {
        let a = self.selection?;
        let (lo, hi) = if a <= self.caret {
            (a, self.caret)
        } else {
            (self.caret, a)
        };
        (lo != hi).then_some((lo, hi))
    }
}

struct OtherText {
    idx: usize,
    text: Text,
    block_w: f32,
    block_h: f32,
}

pub struct TextOverlay {
    program: ViewProgram,
    active: bool,
    idx: usize,
    text: Text,
    block_w: f32,
    block_h: f32,
    others: Vec<OtherText>,
}

impl TextOverlay {
    pub fn new(
        program: ViewProgram,
        active: Option<(usize, &Text)>,
        others: Vec<(usize, Text)>,
    ) -> Self {
        let (idx, text) = active
            .map(|(i, t)| (i, t.clone()))
            .unwrap_or((usize::MAX, Text::default()));
        let (block_w, block_h) = text_render::measure_block(&text);
        let others = others
            .into_iter()
            .map(|(idx, text)| {
                let (block_w, block_h) = text_render::measure_block(&text);
                OtherText {
                    idx,
                    text,
                    block_w,
                    block_h,
                }
            })
            .collect();
        Self {
            program,
            active: active.is_some(),
            idx,
            text,
            block_w,
            block_h,
            others,
        }
    }

    fn other_box(&self, other: &OtherText) -> Option<(Vec2, Vec2, [Vec2; 4])> {
        let t = &other.text;
        let anchor = self.program.image_uv_to_screen(vec2(t.x, t.y))?;
        let h = self.half_extents_of(t, other.block_w, other.block_h);
        let (s, c) = t.rotation.to_radians().sin_cos();
        let rot = |v: Vec2| vec2(v.x * c - v.y * s, v.x * s + v.y * c);
        let corners = [
            anchor + rot(vec2(-h.x, -h.y)),
            anchor + rot(vec2(h.x, -h.y)),
            anchor + rot(vec2(h.x, h.y)),
            anchor + rot(vec2(-h.x, h.y)),
        ];
        Some((anchor, h, corners))
    }

    fn other_hit(&self, local: Vec2) -> Option<usize> {
        for other in &self.others {
            let Some((anchor, h, _)) = self.other_box(other) else {
                continue;
            };
            let (s, c) = (-other.text.rotation.to_radians()).sin_cos();
            let d = local - anchor;
            let unrot = vec2(d.x * c - d.y * s, d.x * s + d.y * c);
            if unrot.x.abs() <= h.x + EDGE_BAND && unrot.y.abs() <= h.y + EDGE_BAND {
                return Some(other.idx);
            }
        }
        None
    }

    fn content(&self) -> &str {
        &self.text.content
    }

    fn effective(&self, state: &State) -> (Cow<'_, Text>, f32, f32) {
        match &state.pending {
            Some(s) if *s != self.text.content => {
                let mut t = self.text.clone();
                t.content = s.clone();
                let (bw, bh) = text_render::measure_block(&t);
                (Cow::Owned(t), bw, bh)
            }
            _ => (Cow::Borrowed(&self.text), self.block_w, self.block_h),
        }
    }

    fn caret_segment(&self, anchor: Vec2, text: &Text, h: Vec2, caret: usize) -> (Vec2, Vec2) {
        let scale = self.program.scale();
        let (cx, cy, line_h) = text_render::caret_offset(text, caret);
        let box_h = h.y * 2.0;
        let caret_h = (line_h * scale).min(box_h);
        let x = -h.x + cx * scale;
        let mut y = -h.y + cy * scale;
        if y + caret_h > h.y {
            y = h.y - caret_h;
        }
        let p_top = vec2(x, y);
        let p_bot = p_top + vec2(0.0, caret_h);
        (anchor + self.rotate(p_top), anchor + self.rotate(p_bot))
    }

    fn anchor_screen(&self) -> Option<Vec2> {
        self.program
            .image_uv_to_screen(vec2(self.text.x, self.text.y))
    }

    fn half_extents_of(&self, text: &Text, block_w: f32, block_h: f32) -> Vec2 {
        let scale = self.program.scale();
        let (bw, bh) = if block_w > 0.0 && block_h > 0.0 {
            (block_w, block_h)
        } else {
            (text.size * 0.6, text.size)
        };
        vec2((bw * scale * 0.5).max(6.0), (bh * scale * 0.5).max(6.0))
    }

    fn half_extents_for(&self, block_w: f32, block_h: f32) -> Vec2 {
        self.half_extents_of(&self.text, block_w, block_h)
    }

    fn half_extents(&self) -> Vec2 {
        self.half_extents_for(self.block_w, self.block_h)
    }

    fn rotate(&self, v: Vec2) -> Vec2 {
        let (s, c) = self.text.rotation.to_radians().sin_cos();
        vec2(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    fn corners_for(&self, anchor: Vec2, h: Vec2) -> [Vec2; 4] {
        [
            anchor + self.rotate(vec2(-h.x, -h.y)),
            anchor + self.rotate(vec2(h.x, -h.y)),
            anchor + self.rotate(vec2(h.x, h.y)),
            anchor + self.rotate(vec2(-h.x, h.y)),
        ]
    }

    fn handle_positions_for(&self, anchor: Vec2, h: Vec2) -> (Vec2, Vec2) {
        let scale_h = anchor + self.rotate(vec2(h.x, h.y));
        let rot_h = anchor + self.rotate(vec2(0.0, -(h.y + 24.0)));
        (scale_h, rot_h)
    }

    fn handle_positions(&self, anchor: Vec2) -> (Vec2, Vec2) {
        self.handle_positions_for(anchor, self.half_extents())
    }

    fn hit(&self, local: Vec2) -> Option<Grab> {
        if !self.active {
            return None;
        }
        let anchor = self.anchor_screen()?;
        let (scale_h, rot_h) = self.handle_positions(anchor);
        if (local - rot_h).length() <= HANDLE_HIT {
            return Some(Grab::Rotate);
        }
        if (local - scale_h).length() <= HANDLE_HIT {
            return Some(Grab::Scale);
        }
        let unrot = self.unrotate(local - anchor);
        let h = self.half_extents();
        let outer = h + EDGE_BAND;
        if unrot.x.abs() > outer.x || unrot.y.abs() > outer.y {
            return None;
        }
        let inner = vec2((h.x - EDGE_BAND).max(0.0), (h.y - EDGE_BAND).max(0.0));
        if unrot.x.abs() >= inner.x || unrot.y.abs() >= inner.y {
            Some(Grab::Move)
        } else {
            Some(Grab::Text)
        }
    }

    fn unrotate(&self, v: Vec2) -> Vec2 {
        let (s, c) = (-self.text.rotation.to_radians()).sin_cos();
        vec2(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    fn publish_drag(
        &self,
        drag: &DragState,
        local: Vec2,
        shift: bool,
        shell: &mut Shell<'_, Message>,
    ) {
        match drag.grab {
            Grab::Move => {
                if let (Some(start_uv), Some(cur_uv)) = (
                    self.program.screen_to_image_uv(drag.start_cursor),
                    self.program.screen_to_image_uv(local),
                ) {
                    let nx = drag.start_x + cur_uv.x - start_uv.x;
                    let ny = drag.start_y + cur_uv.y - start_uv.y;
                    shell.publish(EditMsg::Update(self.idx, ModifierParam::TextX(nx)).into());
                    shell.publish(EditMsg::Update(self.idx, ModifierParam::TextY(ny)).into());
                }
            }
            Grab::Scale => {
                let Some(anchor) = self.anchor_screen() else {
                    return;
                };
                let start_dist = (drag.start_cursor - anchor).length().max(1.0);
                let cur_dist = (local - anchor).length().max(1.0);
                let new_size = (drag.start_size * cur_dist / start_dist).max(1.0);
                shell.publish(EditMsg::Update(self.idx, ModifierParam::TextSize(new_size)).into());
            }
            Grab::Rotate => {
                let Some(anchor) = self.anchor_screen() else {
                    return;
                };
                let a0 = drag.start_cursor - anchor;
                let a1 = local - anchor;
                let delta = a1.y.atan2(a1.x) - a0.y.atan2(a0.x);
                let mut deg = drag.start_rotation + delta.to_degrees();
                if shift {
                    deg = (deg / ROTATION_SNAP).round() * ROTATION_SNAP;
                }
                while deg > 180.0 {
                    deg -= 360.0;
                }
                while deg < -180.0 {
                    deg += 360.0;
                }
                shell.publish(EditMsg::Update(self.idx, ModifierParam::TextRotation(deg)).into());
            }
            Grab::Text => {}
        }
    }

    fn publish_content(&self, new_content: &str, shell: &mut Shell<'_, Message>) {
        shell.publish(
            EditMsg::Update(
                self.idx,
                ModifierParam::TextContent(new_content.to_string()),
            )
            .into(),
        );
    }

    fn prev_boundary(s: &str, caret: usize) -> usize {
        s[..caret.min(s.len())]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next_boundary(s: &str, caret: usize) -> usize {
        let caret = caret.min(s.len());
        s[caret..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| caret + i)
            .unwrap_or(s.len())
    }

    fn line_start(s: &str, caret: usize) -> usize {
        s[..caret.min(s.len())]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    fn line_end(s: &str, caret: usize) -> usize {
        let caret = caret.min(s.len());
        s[caret..].find('\n').map(|i| caret + i).unwrap_or(s.len())
    }

    fn move_caret(state: &mut State, target: usize, shift: bool) {
        if shift {
            if state.selection.is_none() {
                state.selection = Some(state.caret);
            }
        } else {
            state.selection = None;
        }
        state.caret = target;
    }

    fn block_to_screen(&self, anchor: Vec2, h: Vec2, lx: f32, ly: f32) -> Vec2 {
        let scale = self.program.scale();
        let p = vec2(-h.x + lx * scale, -h.y + ly * scale);
        anchor + self.rotate(p)
    }

    fn caret_from_cursor(&self, anchor: Vec2, local: Vec2) -> usize {
        let scale = self.program.scale().max(1e-4);
        let h = self.half_extents();
        let block = self.unrotate(local - anchor) + h;
        text_render::caret_at_point(&self.text, block.x / scale, block.y / scale)
    }

    fn handle_keyboard(
        &self,
        state: &mut State,
        text: Option<&str>,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        use keyboard::key::Named;
        let caret = state.caret;
        let shift = modifiers.shift();
        let ctrl = modifiers.command();
        let sel = state.sel_range();
        let base = state
            .pending
            .clone()
            .unwrap_or_else(|| self.content().to_string());

        let replace_sel = |state: &mut State, repl: &str, shell: &mut Shell<'_, Message>| -> bool {
            let Some((lo, hi)) = sel else { return false };
            let mut s = base.clone();
            s.replace_range(lo..hi, repl);
            state.caret = lo + repl.len();
            state.selection = None;
            state.pending = Some(s.clone());
            self.publish_content(&s, shell);
            true
        };

        let insert_at_caret = |state: &mut State, repl: &str, shell: &mut Shell<'_, Message>| {
            let mut s = base.clone();
            s.insert_str(caret, repl);
            state.caret = caret + repl.len();
            state.selection = None;
            state.pending = Some(s.clone());
            self.publish_content(&s, shell);
        };

        match key {
            keyboard::Key::Named(Named::Backspace) => {
                if !replace_sel(state, "", shell) && caret > 0 {
                    let prev = Self::prev_boundary(&base, caret);
                    let mut s = base;
                    s.replace_range(prev..caret, "");
                    state.caret = prev;
                    state.selection = None;
                    state.pending = Some(s.clone());
                    self.publish_content(&s, shell);
                }
            }
            keyboard::Key::Named(Named::Delete) => {
                if !replace_sel(state, "", shell) && caret < base.len() {
                    let next = Self::next_boundary(&base, caret);
                    let mut s = base;
                    s.replace_range(caret..next, "");
                    state.selection = None;
                    state.pending = Some(s.clone());
                    self.publish_content(&s, shell);
                }
            }
            keyboard::Key::Named(Named::ArrowLeft) => {
                let target = match sel.filter(|_| !shift) {
                    Some((lo, _)) => lo,
                    None => Self::prev_boundary(&base, caret),
                };
                Self::move_caret(state, target, shift);
            }
            keyboard::Key::Named(Named::ArrowRight) => {
                let target = match sel.filter(|_| !shift) {
                    Some((_, hi)) => hi,
                    None => Self::next_boundary(&base, caret),
                };
                Self::move_caret(state, target, shift);
            }
            keyboard::Key::Named(Named::Home) => {
                Self::move_caret(state, Self::line_start(&base, caret), shift);
            }
            keyboard::Key::Named(Named::End) => {
                Self::move_caret(state, Self::line_end(&base, caret), shift);
            }
            keyboard::Key::Character(c) if ctrl && c.as_str() == "a" => {
                state.selection = Some(0);
                state.caret = base.len();
            }
            keyboard::Key::Character(c) if ctrl && (c.as_str() == "c" || c.as_str() == "x") => {
                if let Some((lo, hi)) = sel {
                    clipboard.write(ClipboardKind::Standard, base[lo..hi].to_string());
                    if c.as_str() == "x" {
                        replace_sel(state, "", shell);
                    }
                }
            }
            keyboard::Key::Character(c) if ctrl && c.as_str() == "v" => {
                if let Some(paste) = clipboard
                    .read(ClipboardKind::Standard)
                    .map(|s| sanitize_paste(&s))
                    .filter(|s| !s.is_empty())
                    && !replace_sel(state, &paste, shell)
                {
                    insert_at_caret(state, &paste, shell);
                }
            }
            keyboard::Key::Named(Named::Enter) => {
                if !replace_sel(state, "\n", shell) {
                    insert_at_caret(state, "\n", shell);
                }
            }
            _ => {
                if !ctrl
                    && let Some(t) = text
                    && !t.is_empty()
                    && t.chars().all(|c| !c.is_control())
                    && !replace_sel(state, t, shell)
                {
                    insert_at_caret(state, t, shell);
                }
            }
        }
    }
}

impl Widget<Message, Theme, Renderer> for TextOverlay {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let local = cursor.position_in(bounds).map(|p| vec2(p.x, p.y));

        if state.caret_idx != Some(self.idx) {
            state.caret_idx = Some(self.idx);
            state.caret = self.content().len();
            state.selection = None;
            state.pending = None;
        }
        if state.pending.as_deref() == Some(self.content()) {
            state.pending = None;
        }
        let clamp_against = state.pending.as_deref().unwrap_or(self.content());
        state.caret = clamp_caret(clamp_against, state.caret);
        if let Some(sel) = state.selection {
            state.selection = Some(clamp_caret(clamp_against, sel));
        }

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.shift = modifiers.shift();
            return;
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            text,
            key,
            modifiers,
            ..
        }) = event
        {
            if !self.active {
                return;
            }
            state.shift = modifiers.shift();
            self.handle_keyboard(state, text.as_deref(), key, *modifiers, clipboard, shell);
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        let Event::Mouse(mouse_event) = event else {
            return;
        };

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(local) = local else { return };
                let Some(grab) = self.hit(local) else {
                    if let Some(other_idx) = self.other_hit(local) {
                        shell.publish(EditMsg::SetActive(other_idx).into());
                        shell.capture_event();
                        shell.request_redraw();
                    } else if self.active {
                        shell.publish(EditMsg::ClearActive.into());
                        shell.request_redraw();
                    }
                    return;
                };
                let text_anchor = if grab == Grab::Text
                    && let Some(anchor) = self.anchor_screen()
                {
                    let c = self.caret_from_cursor(anchor, local);
                    state.caret = c;
                    state.selection = Some(c);
                    c
                } else {
                    state.caret
                };
                state.drag = Some(DragState {
                    grab,
                    start_cursor: local,
                    start_x: self.text.x,
                    start_y: self.text.y,
                    start_size: self.text.size,
                    start_rotation: self.text.rotation,
                    text_anchor,
                    moved: false,
                });
                shell.capture_event();
                shell.request_redraw();
            }
            mouse::Event::CursorMoved { .. } => {
                let Some(local) = local else { return };
                let Some(drag) = &mut state.drag else { return };
                if (local - drag.start_cursor).length() > DRAG_THRESHOLD {
                    drag.moved = true;
                }
                if drag.grab == Grab::Text {
                    if let Some(anchor) = self.anchor_screen() {
                        let anchor_byte = drag.text_anchor;
                        state.caret = self.caret_from_cursor(anchor, local);
                        state.selection = Some(anchor_byte);
                    }
                } else {
                    let shift = state.shift;
                    let drag = state.drag.as_ref().unwrap();
                    self.publish_drag(drag, local, shift, shell);
                }
                shell.capture_event();
                shell.request_redraw();
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(drag) = state.drag.take() {
                    if drag.grab == Grab::Text && !drag.moved {
                        state.selection = None;
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let accent = theme.extended_palette().primary.base.color;
        let widget_bounds = layout.bounds();
        let pt = |v: Vec2| iced::Point::new(v.x, v.y);
        let translation = iced::Vector::new(widget_bounds.x, widget_bounds.y);

        let mut others_frame = Frame::new(renderer, widget_bounds.size());
        for other in &self.others {
            if let Some((_, _, corners)) = self.other_box(other) {
                let path = Path::new(|b| {
                    b.move_to(pt(corners[0]));
                    b.line_to(pt(corners[1]));
                    b.line_to(pt(corners[2]));
                    b.line_to(pt(corners[3]));
                    b.close();
                });
                others_frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(OUTLINE.scale_alpha(0.5))
                        .with_width(1.5),
                );
                others_frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(Color::WHITE.scale_alpha(0.5))
                        .with_width(1.0),
                );
            }
        }

        let anchor = match (self.active, self.anchor_screen()) {
            (true, Some(a)) => a,
            _ => {
                let others_geometry = others_frame.into_geometry();
                renderer.with_translation(translation, |renderer| {
                    renderer.draw_geometry(others_geometry);
                });
                return;
            }
        };

        let (eff_text, block_w, block_h) = self.effective(state);
        let h = self.half_extents_for(block_w, block_h);
        let corners = self.corners_for(anchor, h);
        let (scale_h, rot_h) = self.handle_positions_for(anchor, h);
        let top_mid = (corners[0] + corners[1]) * 0.5;

        let box_path = Path::new(|b| {
            b.move_to(pt(corners[0]));
            b.line_to(pt(corners[1]));
            b.line_to(pt(corners[2]));
            b.line_to(pt(corners[3]));
            b.close();
            b.move_to(pt(top_mid));
            b.line_to(pt(rot_h));
        });
        let mut frame = Frame::new(renderer, widget_bounds.size());

        let is_active = state.caret_idx == Some(self.idx);

        if is_active && let Some((lo, hi)) = state.sel_range() {
            let sel_fill = accent.scale_alpha(0.35);
            for (rx, ry, rw, rh) in text_render::selection_rects(&eff_text, lo, hi) {
                let quad = Path::new(|b| {
                    b.move_to(pt(self.block_to_screen(anchor, h, rx, ry)));
                    b.line_to(pt(self.block_to_screen(anchor, h, rx + rw, ry)));
                    b.line_to(pt(self.block_to_screen(anchor, h, rx + rw, ry + rh)));
                    b.line_to(pt(self.block_to_screen(anchor, h, rx, ry + rh)));
                    b.close();
                });
                frame.fill(&quad, sel_fill);
            }
        }

        frame.stroke(
            &box_path,
            Stroke::default().with_color(OUTLINE).with_width(3.0),
        );
        frame.stroke(
            &box_path,
            Stroke::default().with_color(Color::WHITE).with_width(1.5),
        );

        if is_active && state.sel_range().is_none() {
            let caret = clamp_caret(&eff_text.content, state.caret);
            let (top, bot) = self.caret_segment(anchor, &eff_text, h, caret);
            let caret_path = Path::new(|b| {
                b.move_to(pt(top));
                b.line_to(pt(bot));
            });
            frame.stroke(
                &caret_path,
                Stroke::default().with_color(OUTLINE).with_width(3.5),
            );
            frame.stroke(
                &caret_path,
                Stroke::default().with_color(accent).with_width(2.0),
            );
        }

        let mut handle_frame = Frame::new(renderer, widget_bounds.size());
        let mut handle = |c: Vec2, r: f32, fill: Color| {
            let path = Path::circle(pt(c), r);
            handle_frame.fill(&path, fill);
            handle_frame.stroke(&path, Stroke::default().with_color(OUTLINE).with_width(1.5));
        };
        for c in corners {
            handle(c, HANDLE_R, Color::WHITE);
        }
        handle(scale_h, HANDLE_R, accent);
        handle(rot_h, HANDLE_R, accent);

        let others_geometry = others_frame.into_geometry();
        let geometry = frame.into_geometry();
        let handle_geometry = handle_frame.into_geometry();
        renderer.with_translation(translation, |renderer| {
            renderer.draw_geometry(others_geometry);
            renderer.draw_geometry(geometry);
            renderer.draw_geometry(handle_geometry);
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if let Some(drag) = &state.drag {
            return match drag.grab {
                Grab::Move => mouse::Interaction::Grabbing,
                Grab::Text => mouse::Interaction::Text,
                _ => mouse::Interaction::Crosshair,
            };
        }
        let Some(local) = cursor.position_in(layout.bounds()).map(|p| vec2(p.x, p.y)) else {
            return mouse::Interaction::None;
        };
        match self.hit(local) {
            Some(Grab::Move) => mouse::Interaction::Grab,
            Some(Grab::Text) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Crosshair,
            None => {
                if self.other_hit(local).is_some() {
                    mouse::Interaction::Pointer
                } else {
                    mouse::Interaction::None
                }
            }
        }
    }
}

impl<'a> From<TextOverlay> for Element<'a, Message> {
    fn from(o: TextOverlay) -> Self {
        Element::new(o)
    }
}
