use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::mouse;
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::styles::radius;
use crate::widgets::field_editor::{self, Op};

pub struct NumberEntry<Message> {
    value: f32,
    on_change: Box<dyn Fn(f32) -> Message>,
    min: f32,
    max: f32,
    step: f32,
    drag_per_px: f32,
    suffix: &'static str,
    width: f32,
    height: f32,
    text_size: f32,
}

impl<Message> NumberEntry<Message> {
    pub fn new(value: f32, on_change: impl Fn(f32) -> Message + 'static) -> Self {
        Self {
            value,
            on_change: Box::new(on_change),
            min: 0.0,
            max: f32::INFINITY,
            step: 1.0,
            drag_per_px: 1.0,
            suffix: "",
            width: 58.0,
            height: 24.0,
            text_size: 12.0,
        }
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn drag_per_px(mut self, v: f32) -> Self {
        self.drag_per_px = v;
        self
    }

    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    fn clamp(&self, v: f32) -> f32 {
        v.clamp(self.min, self.max)
    }

    fn display_value(&self) -> String {
        if self.step.fract() == 0.0 {
            format!("{}", self.value.round() as i64)
        } else {
            format!("{:.1}", self.value)
        }
    }

    fn filter(&self, s: String) -> String {
        field_editor::filter_number(&s, self.step.fract() != 0.0, self.min < 0.0, 6)
    }
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Pending {
        origin_x: f32,
        origin_value: f32,
    },
    Dragging {
        origin_x: f32,
        origin_value: f32,
    },
    Editing {
        buffer: String,
        needs_focus: bool,
    },
}

impl State {
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

impl<Message> Widget<Message, Theme, Renderer> for NumberEntry<Message>
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
        let buffer = tree.state.downcast_ref::<State>().buffer().to_owned();
        tree.diff_children(&[field_editor::input(&buffer, self.text_size)]);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let bounds = layout::atomic(limits, self.width, self.height);
        let buffer = tree.state.downcast_ref::<State>().buffer().to_owned();
        let editor = field_editor::layout(tree, renderer, &buffer, self.text_size, bounds.size());
        layout::Node::with_children(bounds.size(), vec![editor])
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

        if tree.state.downcast_ref::<State>().is_editing() {
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
                    *state = State::Pending {
                        origin_x: pos.x,
                        origin_value: self.value,
                    };
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => match *state {
                State::Pending { .. } => {
                    *state = State::Editing {
                        buffer: self.display_value(),
                        needs_focus: true,
                    };
                    shell.invalidate_layout();
                    shell.request_redraw();
                    shell.capture_event();
                }
                State::Dragging { .. } => {
                    *state = State::Idle;
                    shell.capture_event();
                }
                _ => {}
            },

            Event::Mouse(mouse::Event::CursorMoved { position }) => match *state {
                State::Pending {
                    origin_x,
                    origin_value,
                } => {
                    let delta_x = position.x - origin_x;
                    if delta_x.abs() >= 4.0 {
                        *state = State::Dragging {
                            origin_x,
                            origin_value,
                        };
                        let v = self.clamp(origin_value + delta_x * self.drag_per_px);
                        shell.publish((self.on_change)(v));
                        shell.capture_event();
                    }
                }
                State::Dragging {
                    origin_x,
                    origin_value,
                } => {
                    let delta_x = position.x - origin_x;
                    let v = self.clamp(origin_value + delta_x * self.drag_per_px);
                    shell.publish((self.on_change)(v));
                    shell.capture_event();
                }
                _ => {}
            },

            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 16.0,
                };
                let v = self.clamp(self.value + lines * self.step);
                shell.publish((self.on_change)(v));
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
        let editing = state.is_editing();
        let is_active = cursor.is_over(bounds)
            || editing
            || state.is_dragging()
            || matches!(state, State::Pending { .. });

        if is_active {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        color: if editing {
                            palette.primary.base.color
                        } else {
                            Color::TRANSPARENT
                        },
                        width: 1.5,
                        radius: radius().into(),
                    },
                    ..Quad::default()
                },
                Background::Color(palette.background.weak.color),
            );
        }

        if editing {
            field_editor::draw(
                tree,
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
                state.buffer(),
                self.text_size,
            );
            return;
        }

        let display = format!("{}{}", self.display_value(), self.suffix);
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
        if state.is_dragging() {
            mouse::Interaction::ResizingHorizontally
        } else if state.is_editing() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message: Clone> NumberEntry<Message> {
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

        if let State::Editing {
            buffer,
            needs_focus,
        } = tree.state.downcast_mut::<State>()
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
            *tree.state.downcast_mut::<State>() = State::Idle;
            shell.invalidate_layout();
            shell.request_redraw();
            shell.capture_event();
            return;
        }

        let buffer = tree.state.downcast_ref::<State>().buffer().to_owned();
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
                    let filtered = self.filter(s);
                    if let State::Editing { buffer, .. } = tree.state.downcast_mut::<State>() {
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
        if let Ok(v) = buffer.parse::<f32>() {
            shell.publish((self.on_change)(self.clamp(v)));
        }
    }

    fn commit(&self, tree: &mut Tree, shell: &mut Shell<'_, Message>) {
        let buffer = tree.state.downcast_ref::<State>().buffer().to_owned();
        self.publish_buffer(&buffer, shell);
        *tree.state.downcast_mut::<State>() = State::Idle;
        shell.invalidate_layout();
        shell.request_redraw();
    }
}

impl<'a, Message> From<NumberEntry<Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: NumberEntry<Message>) -> Self {
        Self::new(widget)
    }
}
