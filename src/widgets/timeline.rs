use iced::advanced::layout;
use iced::advanced::renderer::Quad;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::window;
use iced::{Background, Border, Element, Event, Length, Rectangle, Renderer, Size};

use crate::styles::radius;

pub struct Timeline<Message> {
    playing: bool,
    position: f32,
    total: usize,
    on_seek: Box<dyn Fn(usize) -> Message>,
    on_drag_start: Option<Message>,
    on_drag_end: Option<Message>,
    height: f32,
}

impl<Message> Timeline<Message> {
    pub fn new(
        playing: bool,
        position: f32,
        total: usize,
        on_seek: impl Fn(usize) -> Message + 'static,
    ) -> Self {
        Self {
            playing,
            position,
            total,
            on_seek: Box::new(on_seek),
            on_drag_start: None,
            on_drag_end: None,
            height: 28.0,
        }
    }

    pub fn on_drag_start(mut self, msg: Message) -> Self {
        self.on_drag_start = Some(msg);
        self
    }

    pub fn on_drag_end(mut self, msg: Message) -> Self {
        self.on_drag_end = Some(msg);
        self
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

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            if self.playing && state.drag_x.is_none() {
                shell.request_redraw();
            }
            return;
        }

        let bounds = layout.bounds();

        let frame_from_x = |x: f32| -> usize {
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
                    if let Some(msg) = self.on_drag_start.clone() {
                        shell.publish(msg);
                    }
                    shell.publish((self.on_seek)(frame_from_x(pos.x)));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.drag_x.is_some() {
                    state.drag_x = Some(position.x);
                    shell.publish((self.on_seek)(frame_from_x(position.x)));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag_x.take().is_some() {
                    if let Some(msg) = self.on_drag_end.clone() {
                        shell.publish(msg);
                    }
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
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: radius().into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.background.base.color),
        );

        let track_h = 4.0_f32;
        let track_y = bounds.center_y() - track_h / 2.0;

        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: track_h,
                },
                border: Border::default(),
                ..Quad::default()
            },
            Background::Color(palette.background.strong.color),
        );

        let progress = if let Some(x) = state.drag_x {
            ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
        } else {
            self.position
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
                    border: Border::default(),
                    ..Quad::default()
                },
                Background::Color(palette.primary.base.color),
            );
        }

        if self.total > 1 {
            let max_ticks = (bounds.width / 4.0) as usize;
            let step = ((self.total - 1) / max_ticks.max(1)).max(1);
            let tick_h_major = 5.0_f32;
            let tick_h_minor = 3.0_f32;
            let tick_w = 1.0_f32;
            let tick_top = track_y - tick_h_major - 1.0;
            let color_minor = palette.background.base.text.scale_alpha(0.25);
            let color_major = palette.background.base.text.scale_alpha(0.45);

            for i in (step..self.total.saturating_sub(step)).step_by(step) {
                let t = i as f32 / (self.total - 1) as f32;
                let x = (bounds.x + bounds.width * t).round();
                let is_major = i % (step * 5) == 0;
                let tick_h = if is_major { tick_h_major } else { tick_h_minor };
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: x - tick_w / 2.0,
                            y: tick_top + (tick_h_major - tick_h),
                            width: tick_w,
                            height: tick_h,
                        },
                        border: Border::default(),
                        ..Quad::default()
                    },
                    Background::Color(if is_major { color_major } else { color_minor }),
                );
            }
        }

        let thumb_w = 4.0_f32;
        let thumb_cx = (bounds.x + bounds.width * progress)
            .clamp(
                bounds.x + thumb_w / 2.0,
                bounds.x + bounds.width - thumb_w / 2.0,
            )
            .round();
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: thumb_cx - thumb_w / 2.0,
                    y: bounds.y,
                    width: thumb_w,
                    height: bounds.height,
                },
                border: Border {
                    radius: radius().into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.primary.strong.color),
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
