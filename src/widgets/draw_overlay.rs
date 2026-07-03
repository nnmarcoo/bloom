use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
use iced::advanced::graphics::geometry::{Frame, Path, Renderer as GeometryRenderer, Stroke};
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::{EditMsg, Message};
use crate::modifiers::ModifierParam;
use crate::wgpu::view_program::ViewProgram;
use crate::widgets::viewport_nav::{self, NavState};

const MIN_POINT_DIST: f32 = 1.5;
const OUTLINE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.6,
};

#[derive(Default)]
struct State {
    drawing: bool,
    last_screen: Vec2,
    nav: NavState,
}

pub struct DrawOverlay {
    program: ViewProgram,
    modifier_idx: usize,
    brush_size: f32,
    color: [f32; 3],
}

impl DrawOverlay {
    pub fn new(
        program: ViewProgram,
        modifier_idx: usize,
        brush_size: f32,
        color: [f32; 3],
    ) -> Self {
        Self {
            program,
            modifier_idx,
            brush_size,
            color,
        }
    }

    fn brush_screen_radius(&self, uv: Vec2) -> Option<f32> {
        let (img_w, _) = self.program.image_size()?;
        let a = self.program.image_uv_to_screen(uv)?;
        let b = self
            .program
            .image_uv_to_screen(uv + vec2(self.brush_size / img_w as f32, 0.0))?;
        Some(((b - a).length() * 0.5).max(1.0))
    }
}

impl Widget<Message, Theme, Renderer> for DrawOverlay {
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

        if !state.drawing
            && viewport_nav::handle(&mut state.nav, event, bounds, cursor, true, shell)
        {
            shell.request_redraw();
            return;
        }

        let Event::Mouse(mouse_event) = event else {
            return;
        };

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(local) = local else { return };
                let Some(uv) = self.program.screen_to_image_uv(local) else {
                    return;
                };
                if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) {
                    return;
                }
                state.drawing = true;
                state.last_screen = local;
                shell.publish(
                    EditMsg::Update(
                        self.modifier_idx,
                        ModifierParam::DrawingStrokeStart([uv.x, uv.y]),
                    )
                    .into(),
                );
                shell.capture_event();
                shell.request_redraw();
            }
            mouse::Event::CursorMoved { .. } => {
                let Some(local) = local else { return };
                if state.drawing {
                    if (local - state.last_screen).length() >= MIN_POINT_DIST
                        && let Some(uv) = self.program.screen_to_image_uv(local)
                    {
                        state.last_screen = local;
                        shell.publish(
                            EditMsg::Update(
                                self.modifier_idx,
                                ModifierParam::DrawingStrokeExtend([uv.x, uv.y]),
                            )
                            .into(),
                        );
                    }
                    shell.capture_event();
                }
                shell.request_redraw();
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if state.drawing => {
                state.drawing = false;
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if let Some(nav) = state.nav.interaction() {
            return nav;
        }
        if state.drawing {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(pos) = cursor.position_in(bounds) else {
            return;
        };
        let Some(uv) = self.program.screen_to_image_uv(vec2(pos.x, pos.y)) else {
            return;
        };
        let Some(radius) = self.brush_screen_radius(uv) else {
            return;
        };

        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(pos.x, pos.y);
        let circle = Path::circle(center, radius);
        frame.stroke(
            &circle,
            Stroke::default().with_color(OUTLINE).with_width(2.5),
        );
        frame.stroke(
            &circle,
            Stroke::default()
                .with_color(Color::from_rgb(self.color[0], self.color[1], self.color[2]))
                .with_width(1.0),
        );

        let geometry = frame.into_geometry();
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

impl<'a> From<DrawOverlay> for Element<'a, Message> {
    fn from(overlay: DrawOverlay) -> Self {
        Element::new(overlay)
    }
}
