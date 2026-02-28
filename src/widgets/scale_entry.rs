use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Paragraph, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::mouse;
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size,
};

use crate::styles::WIDGET_RADIUS;

pub struct ScaleEntry<Message> {
    value: f32,
    on_change: Box<dyn Fn(f32) -> Message>,
    width: f32,
    height: f32,
    text_size: f32,
}

impl<Message> ScaleEntry<Message> {
    pub fn new(value: f32, on_change: impl Fn(f32) -> Message + 'static) -> Self {
        Self {
            value,
            on_change: Box::new(on_change),
            width: 58.0,
            height: 24.0,
            text_size: 12.0,
        }
    }
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Editing {
        buffer: String,
        fresh: bool,
    },
}

impl State {
    fn is_editing(&self) -> bool {
        matches!(self, Self::Editing { .. })
    }
}

impl<Message> Widget<Message, iced::Theme, Renderer> for ScaleEntry<Message>
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
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
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
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    if !state.is_editing() {
                        *state = State::Editing {
                            buffer: format!("{}", (self.value * 100.0).round() as i32),
                            fresh: true,
                        };
                        shell.capture_event();
                    }
                } else if state.is_editing() {
                    self.commit(state, shell);
                }
            }

            Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. })
                if state.is_editing() =>
            {
                match key {
                    Key::Named(Named::Enter) => {
                        self.commit(state, shell);
                        shell.capture_event();
                    }
                    Key::Named(Named::Escape) => {
                        *state = State::Idle;
                        shell.capture_event();
                    }
                    Key::Named(Named::Backspace) => {
                        if let State::Editing { buffer, fresh } = state {
                            if *fresh {
                                buffer.clear();
                                *fresh = false;
                            } else {
                                buffer.pop();
                            }
                        }
                        shell.capture_event();
                    }
                    _ => {
                        if let Some(ch) = text.as_ref().and_then(|s| s.chars().next()) {
                            if ch.is_ascii_digit() {
                                if let State::Editing { buffer, fresh } = state {
                                    if *fresh {
                                        buffer.clear();
                                        *fresh = false;
                                    }
                                    if buffer.len() < 4 {
                                        buffer.push(ch);
                                    }
                                }
                                shell.capture_event();
                            }
                        }
                    }
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;
        use advanced::text::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let is_hovered = cursor.is_over(bounds);

        if is_hovered || state.is_editing() {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        color: if state.is_editing() {
                            palette.primary.base.color
                        } else {
                            Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: WIDGET_RADIUS.into(),
                    },
                    ..Quad::default()
                },
                Background::Color(palette.background.weak.color),
            );
        }

        let (display, selected, show_caret) = match state {
            State::Idle => (
                format!("{}%", (self.value * 100.0).round() as i32),
                false,
                false,
            ),
            State::Editing {
                buffer,
                fresh: true,
            } => (buffer.clone(), true, false),
            State::Editing { buffer, .. } => (buffer.clone(), false, true),
        };

        if selected {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        radius: WIDGET_RADIUS.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(palette.primary.base.color),
            );
        }

        let text_color = if selected {
            palette.primary.base.text
        } else {
            palette.background.base.text
        };

        renderer.fill_text(
            Text {
                content: display.clone(),
                bounds: Size::new(bounds.width, bounds.height),
                size: Pixels(self.text_size),
                line_height: text::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center.into(),
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.center_x(), bounds.center_y()),
            text_color,
            bounds,
        );

        if show_caret {
            let para = <Renderer as advanced::text::Renderer>::Paragraph::with_text(Text {
                content: display.as_str(),
                bounds: Size::new(f32::INFINITY, f32::INFINITY),
                size: Pixels(self.text_size),
                line_height: text::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top.into(),
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            });
            let text_width = para.min_bounds().width;
            let caret_x = if text_width > 0.0 {
                (bounds.center_x() + text_width / 2.0 + 2.0).round()
            } else {
                bounds.center_x().round()
            };
            let caret_h = self.text_size + 2.0;
            let caret_y = (bounds.center_y() - caret_h / 2.0).round();
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: caret_x,
                        y: caret_y,
                        width: 1.5,
                        height: caret_h,
                    },
                    ..Quad::default()
                },
                Background::Color(text_color),
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
        if state.is_editing() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message: Clone> ScaleEntry<Message> {
    fn commit(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        if let State::Editing { buffer, .. } = state {
            if let Ok(pct) = buffer.parse::<u32>() {
                if pct > 0 {
                    shell.publish((self.on_change)(pct as f32 / 100.0));
                }
            }
        }
        *state = State::Idle;
    }
}

impl<'a, Message> From<ScaleEntry<Message>> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: ScaleEntry<Message>) -> Self {
        Self::new(widget)
    }
}
