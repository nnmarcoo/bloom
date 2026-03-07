use iced::advanced::layout;
use iced::advanced::renderer::Quad;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Background, Border, Element, Event, Length, Rectangle, Renderer, Size};

pub struct Timeline<Message> {
    frame: usize,
    total: usize,
    on_seek: Box<dyn Fn(usize) -> Message>,
    height: f32,
}

impl<Message> Timeline<Message> {
    pub fn new(frame: usize, total: usize, on_seek: impl Fn(usize) -> Message + 'static) -> Self {
        Self {
            frame,
            total,
            on_seek: Box::new(on_seek),
            height: 20.0,
        }
    }
}

#[derive(Default)]
struct State {
    drag_x: Option<f32>,
}

impl<Message> Widget<Message, iced::Theme, Renderer> for Timeline<Message>
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
            width: Length::Fill,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, limits.max().width, self.height)
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

        let seek_from_x = |x: f32| -> usize {
            if self.total <= 1 {
                return 0;
            }
            let t = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
            (t * (self.total - 1) as f32).round() as usize
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    state.drag_x = Some(pos.x);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.drag_x.is_some() {
                    state.drag_x = Some(position.x);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(x) = state.drag_x.take() {
                    shell.publish((self.on_seek)(seek_from_x(x)));
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let is_active = cursor.is_over(bounds) || state.drag_x.is_some();

        let track_h = if is_active { 6.0_f32 } else { 4.0_f32 };
        let track_y = bounds.center_y() - track_h / 2.0;
        let track_radius = track_h / 2.0;

        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: track_h,
                },
                border: Border {
                    radius: track_radius.into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.background.strong.color),
        );

        let progress = if let Some(x) = state.drag_x {
            ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
        } else if self.total <= 1 {
            0.0
        } else {
            self.frame as f32 / (self.total - 1) as f32
        };

        if progress > 0.0 {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: track_y,
                        width: bounds.width * progress,
                        height: track_h,
                    },
                    border: Border {
                        radius: track_radius.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(palette.primary.base.color),
            );
        }

        if is_active {
            let thumb_r = 7.0_f32;
            let thumb_cx = bounds.x + bounds.width * progress;
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: thumb_cx - thumb_r,
                        y: bounds.center_y() - thumb_r,
                        width: thumb_r * 2.0,
                        height: thumb_r * 2.0,
                    },
                    border: Border {
                        radius: thumb_r.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(palette.primary.strong.color),
            );
        }
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
        if state.drag_x.is_some() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message> From<Timeline<Message>> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: Timeline<Message>) -> Self {
        Self::new(widget)
    }
}
