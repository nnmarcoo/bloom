use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grab {
    Move,
    Scale,
    Rotate,
}

struct DragState {
    grab: Grab,
    start_cursor: Vec2,
    start_x: f32,
    start_y: f32,
    start_size: f32,
    start_rotation: f32,
}

#[derive(Default)]
struct State {
    drag: Option<DragState>,
}

pub struct TextOverlay {
    program: ViewProgram,
    idx: usize,
    x: f32,
    y: f32,
    size: f32,
    rotation: f32,
    // measured block size in image pixels at `size`
    block_w: f32,
    block_h: f32,
}

impl TextOverlay {
    pub fn new(program: ViewProgram, idx: usize, text: &Text) -> Self {
        let (block_w, block_h) = text_render::measure_block(text);
        Self {
            program,
            idx,
            x: text.x,
            y: text.y,
            size: text.size,
            rotation: text.rotation,
            block_w,
            block_h,
        }
    }

    fn anchor_screen(&self) -> Option<Vec2> {
        self.program.image_uv_to_screen(vec2(self.x, self.y))
    }

    // Half-extents of the box in screen pixels. When there's no measurable text
    // (empty content), fall back to a size-proportional placeholder box so scaling
    // is still visible.
    fn half_extents(&self) -> Vec2 {
        let scale = self.program.scale();
        let (bw, bh) = if self.block_w > 0.0 && self.block_h > 0.0 {
            (self.block_w, self.block_h)
        } else {
            (self.size * 0.6, self.size)
        };
        vec2((bw * scale * 0.5).max(6.0), (bh * scale * 0.5).max(6.0))
    }

    fn rotate(&self, v: Vec2) -> Vec2 {
        let (s, c) = self.rotation.to_radians().sin_cos();
        vec2(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    /// The four box corners (TL, TR, BR, BL) in screen space, rotated about the anchor.
    fn corners(&self, anchor: Vec2) -> [Vec2; 4] {
        let h = self.half_extents();
        [
            anchor + self.rotate(vec2(-h.x, -h.y)),
            anchor + self.rotate(vec2(h.x, -h.y)),
            anchor + self.rotate(vec2(h.x, h.y)),
            anchor + self.rotate(vec2(-h.x, h.y)),
        ]
    }

    /// Scale handle (bottom-right corner) and rotate handle (above the top edge).
    fn handle_positions(&self, anchor: Vec2) -> (Vec2, Vec2) {
        let h = self.half_extents();
        let scale_h = anchor + self.rotate(vec2(h.x, h.y));
        let rot_h = anchor + self.rotate(vec2(0.0, -(h.y + 24.0)));
        (scale_h, rot_h)
    }

    fn hit(&self, local: Vec2) -> Option<Grab> {
        let anchor = self.anchor_screen()?;
        let (scale_h, rot_h) = self.handle_positions(anchor);
        if (local - rot_h).length() <= HANDLE_HIT {
            return Some(Grab::Rotate);
        }
        if (local - scale_h).length() <= HANDLE_HIT {
            return Some(Grab::Scale);
        }
        // inside the (unrotated) box test: transform local into box space.
        let rel = local - anchor;
        let (s, c) = (-self.rotation.to_radians()).sin_cos();
        let unrot = vec2(rel.x * c - rel.y * s, rel.x * s + rel.y * c);
        let h = self.half_extents();
        if unrot.x.abs() <= h.x + HANDLE_HIT && unrot.y.abs() <= h.y + HANDLE_HIT {
            return Some(Grab::Move);
        }
        None
    }

    fn publish_drag(&self, drag: &DragState, local: Vec2, shell: &mut Shell<'_, Message>) {
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
                // Only a lower floor: dragging the cursor onto the anchor sends
                // cur_dist→0 (size→0), which is degenerate. No upper bound.
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
                while deg > 180.0 {
                    deg -= 360.0;
                }
                while deg < -180.0 {
                    deg += 360.0;
                }
                shell.publish(EditMsg::Update(self.idx, ModifierParam::TextRotation(deg)).into());
            }
        }
    }
}

const OUTLINE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

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
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let local = cursor.position_in(bounds).map(|p| vec2(p.x, p.y));

        if let Event::Keyboard(keyboard::Event::KeyPressed { text, key, .. }) = event {
            if let keyboard::Key::Named(keyboard::key::Named::Backspace) = key {
                shell.publish(EditMsg::TextBackspace(self.idx).into());
                shell.capture_event();
                return;
            }
            if let Some(t) = text
                && !t.is_empty()
                && t.chars().all(|c| !c.is_control())
            {
                shell.publish(EditMsg::TextAppend(self.idx, t.to_string()).into());
                shell.capture_event();
            }
            return;
        }

        let Event::Mouse(mouse_event) = event else {
            return;
        };

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(local) = local else { return };
                if let Some(grab) = self.hit(local) {
                    state.drag = Some(DragState {
                        grab,
                        start_cursor: local,
                        start_x: self.x,
                        start_y: self.y,
                        start_size: self.size,
                        start_rotation: self.rotation,
                    });
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::CursorMoved { .. } => {
                if let (Some(drag), Some(local)) = (&state.drag, local) {
                    self.publish_drag(drag, local, shell);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if state.drag.take().is_some() {
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let Some(anchor) = self.anchor_screen() else {
            return;
        };
        let widget_bounds = layout.bounds();
        // All geometry is in coords local to the widget bounds; the renderer is
        // translated to the bounds origin (same pattern the canvas widget uses).
        let corners = self.corners(anchor);
        let (scale_h, rot_h) = self.handle_positions(anchor);
        let top_mid = (corners[0] + corners[1]) * 0.5;

        let white = Color::WHITE;
        let accent = Color {
            r: 0.3,
            g: 0.6,
            b: 1.0,
            a: 1.0,
        };

        // Solid outline at any angle via the geometry/canvas API. fill_quad can
        // only draw axis-aligned rects, so the box + guide line go through a Frame
        // which strokes arbitrary polylines. Black underlay then white core.
        let pt = |v: Vec2| iced::Point::new(v.x, v.y);
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
        frame.stroke(
            &box_path,
            Stroke::default().with_color(OUTLINE).with_width(3.0),
        );
        frame.stroke(&box_path, Stroke::default().with_color(white).with_width(1.5));

        // Handles in the same frame so they layer ON TOP of the box (fill_quad and
        // draw_geometry render on separate layers and don't respect call order).
        let mut handle = |c: Vec2, r: f32, fill: Color| {
            let path = Path::circle(pt(c), r);
            frame.fill(&path, fill);
            frame.stroke(&path, Stroke::default().with_color(OUTLINE).with_width(1.5));
        };
        for c in corners {
            handle(c, HANDLE_R * 0.55, white);
        }
        handle(scale_h, HANDLE_R, accent);
        handle(rot_h, HANDLE_R, accent);

        let geometry = frame.into_geometry();
        let translation = iced::Vector::new(widget_bounds.x, widget_bounds.y);
        renderer.with_translation(translation, |renderer| {
            renderer.draw_geometry(geometry);
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
                _ => mouse::Interaction::Crosshair,
            };
        }
        let Some(local) = cursor.position_in(layout.bounds()).map(|p| vec2(p.x, p.y)) else {
            return mouse::Interaction::None;
        };
        match self.hit(local) {
            Some(Grab::Move) => mouse::Interaction::Grab,
            Some(_) => mouse::Interaction::Crosshair,
            None => mouse::Interaction::None,
        }
    }
}

impl<'a> From<TextOverlay> for Element<'a, Message> {
    fn from(o: TextOverlay) -> Self {
        Element::new(o)
    }
}
