use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Paragraph, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::alignment::{Horizontal, Vertical};
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::mouse;
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size,
};

use crate::styles::radius;

#[derive(Debug, Clone, Copy)]
pub struct Fmt {
    decimals: u8,
    signed: bool,
    suffix: &'static str,
}

impl Fmt {
    pub const fn num(decimals: u8) -> Self {
        Self {
            decimals,
            signed: false,
            suffix: "",
        }
    }

    pub const fn signed(decimals: u8) -> Self {
        Self {
            decimals,
            signed: true,
            suffix: "",
        }
    }

    pub const fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    fn render(&self, value: f32) -> String {
        let mut s = if self.signed {
            format!("{:+.*}", self.decimals as usize, value)
        } else {
            format!("{:.*}", self.decimals as usize, value)
        };
        s.push_str(self.suffix);
        s
    }
}

const DRAG_THRESHOLD: f32 = 3.0;
const FINE_SENSITIVITY: f32 = 0.2;

pub struct ValueSlider<Message> {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    fmt: Fmt,
    on_change: Box<dyn Fn(f32) -> Message>,
    height: f32,
    text_size: f32,
}

impl<Message> ValueSlider<Message> {
    pub fn new(
        value: f32,
        range: std::ops::RangeInclusive<f32>,
        on_change: impl Fn(f32) -> Message + 'static,
    ) -> Self {
        Self {
            value,
            min: *range.start(),
            max: *range.end(),
            step: 0.0,
            fmt: Fmt::num(2),
            on_change: Box::new(on_change),
            height: 16.0,
            text_size: 10.0,
        }
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn format(mut self, fmt: Fmt) -> Self {
        self.fmt = fmt;
        self
    }

    fn allows_minus(&self) -> bool {
        self.min < 0.0
    }

    fn allows_decimal(&self) -> bool {
        self.fmt.decimals > 0
    }

    fn sanitize(&self, value: f32) -> f32 {
        let clamped = value.clamp(self.min, self.max);
        if self.step > 0.0 {
            let snapped = (clamped / self.step).round() * self.step;
            snapped.clamp(self.min, self.max)
        } else {
            clamped
        }
    }

    fn fraction(&self) -> f32 {
        if self.max <= self.min {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    fn value_at(&self, x: f32, bounds: Rectangle) -> f32 {
        let t = if bounds.width > 0.0 {
            ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.sanitize(self.min + t * (self.max - self.min))
    }
}

#[derive(Default)]
struct State {
    mode: Mode,
    shift: bool,
}

#[derive(Default)]
enum Mode {
    #[default]
    Idle,
    Pending {
        origin_x: f32,
    },
    Dragging {
        last_x: f32,
        accum: f32,
    },
    Editing {
        buffer: String,
        fresh: bool,
    },
}

impl Mode {
    fn is_editing(&self) -> bool {
        matches!(self, Self::Editing { .. })
    }

    fn is_dragging(&self) -> bool {
        matches!(self, Self::Dragging { .. })
    }
}

impl<Message> Widget<Message, iced::Theme, Renderer> for ValueSlider<Message>
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
        layout::atomic(limits, Length::Fill, self.height)
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
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift = modifiers.shift();
            }

            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    if !state.mode.is_editing()
                        && let Some(pos) = cursor.position()
                    {
                        state.mode = Mode::Pending { origin_x: pos.x };
                        shell.capture_event();
                    }
                } else if state.mode.is_editing() {
                    self.commit(state, shell);
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => match state.mode {
                Mode::Pending { .. } => {
                    state.mode = Mode::Editing {
                        buffer: self.fmt.render(self.value),
                        fresh: true,
                    };
                    shell.request_redraw();
                    shell.capture_event();
                }
                Mode::Dragging { .. } => {
                    state.mode = Mode::Idle;
                    shell.capture_event();
                }
                _ => {}
            },

            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let shift = state.shift;
                match state.mode {
                    Mode::Pending { origin_x }
                        if (position.x - origin_x).abs() >= DRAG_THRESHOLD =>
                    {
                        let accum = if shift {
                            self.value
                        } else {
                            self.value_at(position.x, bounds)
                        };
                        state.mode = Mode::Dragging {
                            last_x: position.x,
                            accum,
                        };
                        shell.publish((self.on_change)(self.sanitize(accum)));
                        shell.capture_event();
                    }
                    Mode::Dragging { last_x, accum } => {
                        let sens = if shift { FINE_SENSITIVITY } else { 1.0 };
                        let range = self.max - self.min;
                        let new_accum = (accum
                            + (position.x - last_x) / bounds.width.max(1.0) * range * sens)
                            .clamp(self.min, self.max);
                        state.mode = Mode::Dragging {
                            last_x: position.x,
                            accum: new_accum,
                        };
                        let new = self.sanitize(new_accum);
                        if new != self.value {
                            shell.publish((self.on_change)(new));
                        }
                        shell.capture_event();
                    }
                    _ => {}
                }
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) && !state.mode.is_editing() =>
            {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 16.0,
                };
                let increment = if self.step > 0.0 { self.step } else { 0.01 };
                let next = self.sanitize(self.value + lines * increment);
                if next != self.value {
                    shell.publish((self.on_change)(next));
                }
                shell.capture_event();
            }

            Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. })
                if state.mode.is_editing() =>
            {
                match key {
                    Key::Named(Named::Enter) => {
                        self.commit(state, shell);
                        shell.capture_event();
                    }
                    Key::Named(Named::Escape) => {
                        state.mode = Mode::Idle;
                        shell.capture_event();
                    }
                    Key::Named(Named::Backspace) => {
                        if let Mode::Editing { buffer, fresh } = &mut state.mode {
                            if *fresh {
                                buffer.clear();
                                *fresh = false;
                            } else {
                                buffer.pop();
                            }
                            self.publish_buffer(buffer, shell);
                        }
                        shell.capture_event();
                    }
                    _ => {
                        if let Some(ch) = text.as_ref().and_then(|s| s.chars().next())
                            && let Mode::Editing { buffer, fresh } = &mut state.mode
                            && self.accepts_char(ch, buffer)
                        {
                            if *fresh {
                                buffer.clear();
                                *fresh = false;
                            }
                            if buffer.len() < 8 {
                                buffer.push(ch);
                            }
                            self.publish_buffer(buffer, shell);
                            shell.capture_event();
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
        let editing = state.mode.is_editing();
        let active = is_hovered || editing || state.mode.is_dragging();

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: if editing {
                        palette.primary.base.color
                    } else {
                        Color::TRANSPARENT
                    },
                    width: 1.0,
                    radius: radius().into(),
                },
                ..Quad::default()
            },
            Background::Color(palette.background.weak.color),
        );

        if !editing {
            let fill_w = (bounds.width * self.fraction()).round();
            if fill_w > 0.0 {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            width: fill_w,
                            ..bounds
                        },
                        border: Border {
                            radius: radius().into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(if active {
                        palette.primary.base.color.scale_alpha(0.45)
                    } else {
                        palette.primary.base.color.scale_alpha(0.30)
                    }),
                );
            }
        }

        let display = match &state.mode {
            Mode::Editing { buffer, .. } => buffer.clone(),
            _ => self.fmt.render(self.value),
        };
        let text_color = palette.background.base.text;

        let show_selection = matches!(state.mode, Mode::Editing { fresh: true, .. });
        let show_caret = matches!(state.mode, Mode::Editing { fresh: false, .. });

        let caret_x = if show_caret || show_selection {
            let para = <Renderer as advanced::text::Renderer>::Paragraph::with_text(Text {
                content: display.as_str(),
                bounds: Size::new(f32::INFINITY, f32::INFINITY),
                size: Pixels(self.text_size),
                line_height: text::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: Horizontal::Left.into(),
                align_y: Vertical::Top,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            });
            let text_width = para.min_bounds().width;

            if show_selection {
                let sel_h = self.text_size + 4.0;
                let sel_x = (bounds.center_x() - text_width / 2.0 - 2.0).round();
                let sel_y = (bounds.center_y() - sel_h / 2.0).round();
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: sel_x,
                            y: sel_y,
                            width: text_width + 4.0,
                            height: sel_h,
                        },
                        border: Border {
                            radius: 3.0.into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(palette.primary.base.color.scale_alpha(0.35)),
                );
                None
            } else {
                Some(if text_width > 0.0 {
                    (bounds.center_x() + text_width / 2.0 + 2.0).round()
                } else {
                    bounds.center_x().round()
                })
            }
        } else {
            None
        };

        renderer.fill_text(
            Text {
                content: display,
                bounds: Size::new(bounds.width, bounds.height),
                size: Pixels(self.text_size),
                line_height: text::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: Horizontal::Center.into(),
                align_y: Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.center_x(), bounds.center_y()),
            text_color,
            bounds,
        );

        if let Some(caret_x) = caret_x {
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
        if state.mode.is_dragging() {
            mouse::Interaction::ResizingHorizontally
        } else if state.mode.is_editing() {
            mouse::Interaction::Text
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message: Clone> ValueSlider<Message> {
    fn accepts_char(&self, ch: char, buffer: &str) -> bool {
        if ch.is_ascii_digit() {
            return true;
        }
        match ch {
            '.' => self.allows_decimal() && !buffer.contains('.'),
            '-' => self.allows_minus() && buffer.is_empty(),
            _ => false,
        }
    }

    fn publish_buffer(&self, buffer: &str, shell: &mut Shell<'_, Message>) {
        if let Ok(parsed) = buffer.parse::<f32>() {
            shell.publish((self.on_change)(self.sanitize(parsed)));
        }
    }

    fn commit(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        if let Mode::Editing { buffer, .. } = &state.mode {
            self.publish_buffer(buffer, shell);
        }
        state.mode = Mode::Idle;
    }
}

impl<'a, Message> From<ValueSlider<Message>> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: ValueSlider<Message>) -> Self {
        Self::new(widget)
    }
}
