use iced::advanced::text::{self, Text};
use iced::advanced::widget::operation::{focusable, text_input as text_input_op};
use iced::advanced::widget::tree::Tree;
use iced::advanced::{self, Clipboard, Layout, Shell, layout};
use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::text_input;
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size, Theme,
};

const EDITOR_ID: &str = "field_editor";

#[derive(Clone)]
pub enum Op {
    Input(String),
    Submit,
}

pub fn input<'a>(buffer: &str, text_size: f32) -> Element<'a, Op, Theme, Renderer> {
    text_input("", buffer)
        .id(EDITOR_ID)
        .on_input(Op::Input)
        .on_submit(Op::Submit)
        .size(text_size)
        .padding(0)
        .width(Length::Fill)
        .align_x(Horizontal::Center)
        .style(style)
        .into()
}

fn style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let text_color = palette.background.base.text;
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: text_color,
        placeholder: text_color.scale_alpha(0.4),
        value: text_color,
        selection: palette.primary.base.color.scale_alpha(0.35),
    }
}

pub fn layout(
    tree: &mut Tree,
    renderer: &Renderer,
    buffer: &str,
    text_size: f32,
    bounds: Size,
) -> layout::Node {
    let mut editor = input(buffer, text_size);
    let node = editor.as_widget_mut().layout(
        &mut tree.children[0],
        renderer,
        &layout::Limits::new(Size::ZERO, bounds),
    );
    let y = ((bounds.height - node.bounds().height) / 2.0).max(0.0);
    node.move_to(Point::new(0.0, y))
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    tree: &Tree,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
    buffer: &str,
    text_size: f32,
) {
    if let Some(editor_layout) = layout.children().next() {
        input(buffer, text_size).as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            editor_layout,
            cursor,
            viewport,
        );
    }
}

pub fn focus_and_select(
    tree: &mut Tree,
    renderer: &Renderer,
    editor_layout: Layout<'_>,
    buffer: &str,
    text_size: f32,
) {
    let mut editor = input(buffer, text_size);
    let id = iced::advanced::widget::Id::from(EDITOR_ID);
    let mut focus = focusable::focus::<()>(id.clone());
    editor
        .as_widget_mut()
        .operate(&mut tree.children[0], editor_layout, renderer, &mut focus);
    let mut select = text_input_op::select_all::<()>(id);
    editor
        .as_widget_mut()
        .operate(&mut tree.children[0], editor_layout, renderer, &mut select);
}

#[allow(clippy::too_many_arguments)]
pub fn forward<Message>(
    tree: &mut Tree,
    event: &Event,
    editor_layout: Layout<'_>,
    cursor: mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
    viewport: &Rectangle,
    buffer: &str,
    text_size: f32,
) -> Vec<Op> {
    let mut editor = input(buffer, text_size);
    let mut ops: Vec<Op> = Vec::new();
    let mut local = Shell::new(&mut ops);
    editor.as_widget_mut().update(
        &mut tree.children[0],
        event,
        editor_layout,
        cursor,
        renderer,
        clipboard,
        &mut local,
        viewport,
    );
    if local.is_event_captured() {
        shell.capture_event();
    }
    if local.is_layout_invalid() {
        shell.invalidate_layout();
    }
    if local.are_widgets_invalid() {
        shell.invalidate_widgets();
    }
    shell.request_redraw_at(local.redraw_request());
    ops
}

pub fn filter_number(s: &str, allow_decimal: bool, allow_minus: bool, max_len: usize) -> String {
    let mut out = String::new();
    let mut has_dot = false;
    for ch in s.chars() {
        if out.len() >= max_len {
            break;
        }
        if ch.is_ascii_digit() {
            out.push(ch);
        } else if ch == '.' && allow_decimal && !has_dot {
            has_dot = true;
            out.push(ch);
        } else if ch == '-' && allow_minus && out.is_empty() {
            out.push(ch);
        }
    }
    out
}

pub fn draw_centered_text(
    renderer: &mut Renderer,
    content: &str,
    bounds: Rectangle,
    text_size: f32,
    color: Color,
) {
    use advanced::text::Renderer as _;
    renderer.fill_text(
        Text {
            content: content.to_owned(),
            bounds: Size::new(bounds.width, bounds.height),
            size: Pixels(text_size),
            line_height: text::LineHeight::default(),
            font: Font::DEFAULT,
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        },
        Point::new(bounds.center_x(), bounds.center_y()),
        color,
        bounds,
    );
}
