use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Background, Border, Element, Event, Length, Rectangle, Renderer, Size};

pub struct AngleDial<Message> {
    value: f32,
    on_change: Box<dyn Fn(f32) -> Message>,
    size: f32,
}

impl<Message> AngleDial<Message> {
    pub fn new(value: f32, on_change: impl Fn(f32) -> Message + 'static) -> Self {
        Self {
            value,
            on_change: Box::new(on_change),
            size: 40.0,
        }
    }

    fn angle_at(&self, x: f32, y: f32, bounds: Rectangle) -> f32 {
        let dx = x - bounds.center_x();
        let dy = y - bounds.center_y();
        dy.atan2(dx).to_degrees().rem_euclid(360.0).round()
    }
}

#[derive(Default)]
struct State {
    dragging: bool,
}

impl<Message> Widget<Message, iced::Theme, Renderer> for AngleDial<Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size),
            height: Length::Fixed(self.size),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.size, self.size)
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

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                let Some(pos) = cursor.position() else {
                    return;
                };
                state.dragging = true;
                shell.publish((self.on_change)(self.angle_at(pos.x, pos.y, bounds)));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                shell.publish((self.on_change)(
                    self.angle_at(position.x, position.y, bounds),
                ));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let active = state.dragging || cursor.is_over(bounds);

        let ring = Rectangle {
            x: bounds.x + 1.0,
            y: bounds.y + 1.0,
            width: bounds.width - 2.0,
            height: bounds.height - 2.0,
        };
        renderer.fill_quad(
            Quad {
                bounds: ring,
                border: Border {
                    color: if active {
                        palette.primary.base.color
                    } else {
                        palette.background.strong.color
                    },
                    width: 1.5,
                    radius: (ring.width / 2.0).into(),
                },
                ..Quad::default()
            },
            Background::Color(palette.background.weak.color),
        );

        let cx = bounds.center_x();
        let cy = bounds.center_y();
        let theta = self.value.to_radians();
        let arm = ring.width / 2.0 - 6.0;
        let dot = 6.0;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: cx + theta.cos() * arm - dot / 2.0,
                    y: cy + theta.sin() * arm - dot / 2.0,
                    width: dot,
                    height: dot,
                },
                border: Border {
                    radius: (dot / 2.0).into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.primary.base.color),
        );

        let center = 3.0;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: cx - center / 2.0,
                    y: cy - center / 2.0,
                    width: center,
                    height: center,
                },
                border: Border {
                    radius: (center / 2.0).into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.background.strong.color),
        );
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
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message> From<AngleDial<Message>> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: AngleDial<Message>) -> Self {
        Self::new(widget)
    }
}
