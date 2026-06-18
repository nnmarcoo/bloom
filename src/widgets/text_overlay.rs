use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::app::{EditMsg, Message};
use crate::modifiers::ModifierParam;
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
}

impl TextOverlay {
    pub fn new(
        program: ViewProgram,
        idx: usize,
        x: f32,
        y: f32,
        size: f32,
        rotation: f32,
    ) -> Self {
        Self {
            program,
            idx,
            x,
            y,
            size,
            rotation,
        }
    }

    fn anchor_screen(&self) -> Option<Vec2> {
        self.program.image_uv_to_screen(vec2(self.x, self.y))
    }

    // Half-extent of the gizmo in screen pixels, derived from font size and zoom.
    fn extent_px(&self) -> f32 {
        (self.size * self.program.scale() * 0.5).max(12.0)
    }

    fn handle_positions(&self, anchor: Vec2) -> (Vec2, Vec2) {
        let e = self.extent_px();
        let rot = self.rotation.to_radians();
        let (s, c) = rot.sin_cos();
        // scale handle: down-right; rotate handle: straight up.
        let scale_local = vec2(e, e);
        let rot_local = vec2(0.0, -(e + 24.0));
        let rotate = |v: Vec2| vec2(v.x * c - v.y * s, v.x * s + v.y * c);
        (anchor + rotate(scale_local), anchor + rotate(rot_local))
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
        if (local - anchor).length() <= self.extent_px() + HANDLE_HIT {
            return Some(Grab::Move);
        }
        None
    }

    fn publish_drag(&self, drag: &DragState, local: Vec2, shell: &mut Shell<'_, Message>) {
        match drag.grab {
            Grab::Move => {
                if let Some(uv) = self.program.screen_to_image_uv(local) {
                    shell.publish(
                        EditMsg::Update(self.idx, ModifierParam::TextX(uv.x.clamp(0.0, 1.0))).into(),
                    );
                    shell.publish(
                        EditMsg::Update(self.idx, ModifierParam::TextY(uv.y.clamp(0.0, 1.0))).into(),
                    );
                }
            }
            Grab::Scale => {
                let Some(anchor) = self.anchor_screen() else {
                    return;
                };
                let start_dist = (drag.start_cursor - anchor).length().max(1.0);
                let cur_dist = (local - anchor).length().max(1.0);
                let new_size = (drag.start_size * cur_dist / start_dist).clamp(4.0, 2000.0);
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

fn fill_circle(renderer: &mut Renderer, c: Vec2, r: f32, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: c.x - r,
                y: c.y - r,
                width: r * 2.0,
                height: r * 2.0,
            },
            border: Border {
                radius: r.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Background::Color(color),
    );
}

fn stroke_line(renderer: &mut Renderer, a: Vec2, b: Vec2, color: Color) {
    // thin quad between two points (axis-aligned approximation is wrong for angles;
    // draw a 1.5px-wide segment via bounding using midpoint — acceptable for a guide line).
    let mid = (a + b) * 0.5;
    let len = (b - a).length();
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: mid.x - 0.75,
                y: a.y.min(b.y),
                width: 1.5,
                height: len.max(1.0),
            },
            ..Default::default()
        },
        Background::Color(color),
    );
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
        let off = vec2(widget_bounds.x, widget_bounds.y);
        let a = anchor + off;
        let (scale_h, rot_h) = self.handle_positions(anchor);
        let scale_h = scale_h + off;
        let rot_h = rot_h + off;

        renderer.with_layer(widget_bounds, |renderer| {
            let white = Color::WHITE;
            let accent = Color {
                r: 0.3,
                g: 0.6,
                b: 1.0,
                a: 1.0,
            };
            stroke_line(renderer, a, rot_h, white);
            fill_circle(renderer, a, HANDLE_R * 0.7, white);
            fill_circle(renderer, scale_h, HANDLE_R, accent);
            fill_circle(renderer, rot_h, HANDLE_R, accent);
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
