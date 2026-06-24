use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::gradient::Linear;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::mouse;
use iced::{
    Background, Border, Color, Element, Event, Gradient, Length, Radians, Rectangle, Renderer,
    Size, Theme,
};

use crate::styles::radius;
use crate::widgets::field_editor::{self, Op};

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
        let mut s = self.render_bare(value);
        s.push_str(self.suffix);
        s
    }

    fn render_bare(&self, value: f32) -> String {
        if self.signed {
            format!("{:+.*}", self.decimals as usize, value)
        } else {
            format!("{:.*}", self.decimals as usize, value)
        }
    }
}

const DRAG_THRESHOLD: f32 = 3.0;
const FINE_SENSITIVITY: f32 = 0.2;

const fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

static HUE_STOPS: [(f32, Color); 7] = [
    (0.0, rgb(1.0, 0.0, 0.0)),
    (1.0 / 6.0, rgb(1.0, 1.0, 0.0)),
    (2.0 / 6.0, rgb(0.0, 1.0, 0.0)),
    (3.0 / 6.0, rgb(0.0, 1.0, 1.0)),
    (4.0 / 6.0, rgb(0.0, 0.0, 1.0)),
    (5.0 / 6.0, rgb(1.0, 0.0, 1.0)),
    (1.0, rgb(1.0, 0.0, 0.0)),
];
static CYAN_RED: [(f32, Color); 2] = [(0.0, rgb(0.0, 0.8, 0.9)), (1.0, rgb(0.95, 0.15, 0.15))];
static MAGENTA_GREEN: [(f32, Color); 2] = [(0.0, rgb(0.9, 0.1, 0.8)), (1.0, rgb(0.1, 0.8, 0.2))];
static YELLOW_BLUE: [(f32, Color); 2] = [(0.0, rgb(0.95, 0.85, 0.1)), (1.0, rgb(0.15, 0.3, 0.95))];

#[derive(Debug, Clone, Copy)]
pub enum Track {
    Fill,
    Gradient(&'static [(f32, Color)]),
}

impl Track {
    pub fn hue() -> Self {
        Track::Gradient(&HUE_STOPS)
    }

    pub fn cyan_red() -> Self {
        Track::Gradient(&CYAN_RED)
    }

    pub fn magenta_green() -> Self {
        Track::Gradient(&MAGENTA_GREEN)
    }

    pub fn yellow_blue() -> Self {
        Track::Gradient(&YELLOW_BLUE)
    }
}

pub struct ValueSlider<Message> {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    fmt: Fmt,
    track: Track,
    on_change: Box<dyn Fn(f32) -> Message>,
    on_change_end: Option<Message>,
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
            track: Track::Fill,
            on_change: Box::new(on_change),
            on_change_end: None,
            height: 16.0,
            text_size: 10.0,
        }
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn on_change_end(mut self, on_change_end: Message) -> Self {
        self.on_change_end = Some(on_change_end);
        self
    }

    pub fn format(mut self, fmt: Fmt) -> Self {
        self.fmt = fmt;
        self
    }

    pub fn track(mut self, track: Track) -> Self {
        self.track = track;
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
        needs_focus: bool,
    },
}

impl Mode {
    fn is_editing(&self) -> bool {
        matches!(self, Self::Editing { .. })
    }

    fn is_dragging(&self) -> bool {
        matches!(self, Self::Dragging { .. })
    }

    fn buffer(&self) -> &str {
        match self {
            Self::Editing { buffer, .. } => buffer,
            _ => "",
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for ValueSlider<Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(field_editor::input("", self.text_size))]
    }

    fn diff(&self, tree: &mut Tree) {
        let buffer = tree.state.downcast_ref::<State>().mode.buffer().to_owned();
        tree.diff_children(&[field_editor::input(&buffer, self.text_size)]);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = layout::atomic(limits, Length::Fill, self.height);
        let size = node.bounds().size();
        let buffer = tree.state.downcast_ref::<State>().mode.buffer().to_owned();
        let editor = field_editor::layout(tree, renderer, &buffer, self.text_size, size);
        layout::Node::with_children(size, vec![editor])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            tree.state.downcast_mut::<State>().shift = modifiers.shift();
        }

        if tree.state.downcast_ref::<State>().mode.is_editing() {
            self.update_editing(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            return;
        }

        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds)
                    && let Some(pos) = cursor.position()
                {
                    state.mode = Mode::Pending { origin_x: pos.x };
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => match state.mode {
                Mode::Pending { .. } => {
                    state.mode = Mode::Editing {
                        buffer: self.fmt.render_bare(self.value),
                        needs_focus: true,
                    };
                    shell.invalidate_layout();
                    shell.request_redraw();
                    shell.capture_event();
                }
                Mode::Dragging { .. } => {
                    state.mode = Mode::Idle;
                    self.publish_end(shell);
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

            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 16.0,
                };
                let increment = if self.step > 0.0 { self.step } else { 0.01 };
                let next = self.sanitize(self.value + lines * increment);
                if next != self.value {
                    shell.publish((self.on_change)(next));
                    self.publish_end(shell);
                }
                shell.capture_event();
            }

            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

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

        if editing {
            field_editor::draw(
                tree,
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
                state.mode.buffer(),
                self.text_size,
            );
            return;
        }

        match self.track {
            Track::Fill => {
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
            Track::Gradient(stops) => {
                let mut linear = Linear::new(Radians(std::f32::consts::FRAC_PI_2));
                for (offset, color) in stops {
                    linear = linear.add_stop(*offset, *color);
                }
                renderer.fill_quad(
                    Quad {
                        bounds,
                        border: Border {
                            radius: radius().into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Gradient(Gradient::Linear(linear)),
                );

                let marker_x = (bounds.x + bounds.width * self.fraction())
                    .min(bounds.x + bounds.width - 2.0)
                    .max(bounds.x + 2.0);
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: marker_x - 2.0,
                            y: bounds.y,
                            width: 4.0,
                            height: bounds.height,
                        },
                        border: Border {
                            radius: 1.0.into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
                );
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: marker_x - 1.0,
                            y: bounds.y + 1.0,
                            width: 2.0,
                            height: bounds.height - 2.0,
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::WHITE),
                );

                let label = self.fmt.render(self.value);
                let pill_w = label.chars().count() as f32 * self.text_size * 0.62 + 8.0;
                let pill_h = self.text_size + 4.0;
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: (bounds.center_x() - pill_w / 2.0).round(),
                            y: (bounds.center_y() - pill_h / 2.0).round(),
                            width: pill_w,
                            height: pill_h,
                        },
                        border: Border {
                            radius: 3.0.into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(palette.background.base.color.scale_alpha(0.7)),
                );
            }
        }

        let display = self.fmt.render(self.value);
        field_editor::draw_centered_text(
            renderer,
            &display,
            bounds,
            self.text_size,
            palette.background.base.text,
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
    #[allow(clippy::too_many_arguments)]
    fn update_editing(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Some(editor_layout) = layout.children().next() else {
            return;
        };

        if let Mode::Editing {
            buffer,
            needs_focus,
        } = &mut tree.state.downcast_mut::<State>().mode
            && *needs_focus
        {
            *needs_focus = false;
            let buffer = buffer.clone();
            field_editor::focus_and_select(tree, renderer, editor_layout, &buffer, self.text_size);
            shell.request_redraw();
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && !cursor.is_over(layout.bounds())
        {
            self.commit(tree, shell);
            return;
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) = event
        {
            tree.state.downcast_mut::<State>().mode = Mode::Idle;
            shell.invalidate_layout();
            shell.request_redraw();
            shell.capture_event();
            return;
        }

        let buffer = tree.state.downcast_ref::<State>().mode.buffer().to_owned();
        let ops = field_editor::forward(
            tree,
            event,
            editor_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
            &buffer,
            self.text_size,
        );

        for op in ops {
            match op {
                Op::Input(s) => {
                    let filtered = field_editor::filter_number(
                        &s,
                        self.allows_decimal(),
                        self.allows_minus(),
                        8,
                    );
                    if let Mode::Editing { buffer, .. } =
                        &mut tree.state.downcast_mut::<State>().mode
                    {
                        *buffer = filtered.clone();
                    }
                    self.publish_buffer(&filtered, shell);
                    shell.request_redraw();
                }
                Op::Submit => self.commit(tree, shell),
            }
        }
    }

    fn publish_buffer(&self, buffer: &str, shell: &mut Shell<'_, Message>) {
        if let Ok(parsed) = buffer.parse::<f32>() {
            shell.publish((self.on_change)(self.sanitize(parsed)));
        }
    }

    fn publish_end(&self, shell: &mut Shell<'_, Message>) {
        if let Some(on_change_end) = &self.on_change_end {
            shell.publish(on_change_end.clone());
        }
    }

    fn commit(&self, tree: &mut Tree, shell: &mut Shell<'_, Message>) {
        let buffer = tree.state.downcast_ref::<State>().mode.buffer().to_owned();
        self.publish_buffer(&buffer, shell);
        tree.state.downcast_mut::<State>().mode = Mode::Idle;
        self.publish_end(shell);
        shell.invalidate_layout();
        shell.request_redraw();
    }
}

impl<'a, Message> From<ValueSlider<Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: ValueSlider<Message>) -> Self {
        Self::new(widget)
    }
}
