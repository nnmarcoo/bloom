use std::sync::OnceLock;

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::widget::operation::focusable;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget, layout, text};
use iced::alignment::Vertical;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, row, scrollable, text as text_widget, text_input};
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Point, Rectangle, Renderer, Size,
    Theme, Vector, mouse, overlay,
};

use crate::modifiers::text_render::font_families;
use crate::styles::radius;

const TRIGGER_W: f32 = 220.0;
const TRIGGER_H: f32 = 28.0;
const POPUP_W: f32 = 220.0;
const ROW_HEIGHT: f32 = 28.0;
const SEARCH_V_PAD: f32 = 8.0;
const SEARCH_H: f32 = SEARCH_TEXT_SIZE + 2.0 * SEARCH_V_PAD;
const MAX_VISIBLE_ITEMS: usize = 10;
const ITEM_PADDING_H: f32 = 8.0;
const PADDING: f32 = 6.0;
const GAP: f32 = 4.0;
const TEXT_SIZE: f32 = 13.0;
const TRIGGER_TEXT_SIZE: f32 = 12.0;
const SEARCH_TEXT_SIZE: f32 = 12.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_GUTTER: f32 = 4.0;
const DEFAULT_LABEL: &str = "Default font";
const SEARCH_PLACEHOLDER: &str = "Search fonts\u{2026}";
const SEARCH_ID: &str = "font_picker_search";

fn font_of(name: &'static str) -> Font {
    if name.is_empty() {
        Font::DEFAULT
    } else {
        Font::with_name(name)
    }
}

struct FontEntry {
    name: &'static str,
    lower: String,
}

fn font_index() -> &'static [FontEntry] {
    static INDEX: OnceLock<Vec<FontEntry>> = OnceLock::new();
    INDEX.get_or_init(|| {
        font_families()
            .iter()
            .map(|f| FontEntry {
                name: f.as_str(),
                lower: f.to_lowercase(),
            })
            .collect()
    })
}

fn resolve_static(selected: &str) -> Option<&'static str> {
    if selected.is_empty() {
        return None;
    }
    font_index()
        .iter()
        .find(|e| e.name == selected)
        .map(|e| e.name)
}

fn matches(query_lower: &str) -> impl Iterator<Item = &'static str> {
    font_index().iter().filter_map(move |e| {
        if query_lower.is_empty() || e.lower.contains(query_lower) {
            Some(e.name)
        } else {
            None
        }
    })
}

#[derive(Clone)]
enum Op {
    Query(String),
    Select(String),
}

#[derive(Default)]
struct State {
    open: bool,
    needs_focus: bool,
    query: String,
    built_for: Option<(String, String)>,
    content: Option<Element<'static, Op, Theme, Renderer>>,
    menu_tree: Option<Tree>,
}

pub struct FontPicker<Message> {
    selected: String,
    on_select: Box<dyn Fn(String) -> Message>,
    width: Length,
}

impl<Message> FontPicker<Message> {
    pub fn new(selected: String, on_select: impl Fn(String) -> Message + 'static) -> Self {
        Self {
            selected,
            on_select: Box::new(on_select),
            width: Length::Fixed(TRIGGER_W),
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for FontPicker<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Fixed(TRIGGER_H),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, Length::Fixed(TRIGGER_H))
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.is_over(layout.bounds())
        {
            let state = tree.state.downcast_mut::<State>();
            state.open = !state.open;
            if state.open {
                state.query.clear();
                state.needs_focus = true;
            }
            shell.capture_event();
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let hovered = cursor.is_over(bounds);

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: radius().into(),
                },
                ..Quad::default()
            },
            Background::Color(if hovered {
                palette.background.weak.color
            } else {
                palette.background.base.color
            }),
        );

        let (label, font) = match resolve_static(&self.selected) {
            Some(name) => (name, font_of(name)),
            None => (DEFAULT_LABEL, Font::DEFAULT),
        };

        renderer.fill_text(
            text::Text {
                content: label.to_owned(),
                bounds: Size::new(bounds.width - 2.0 * ITEM_PADDING_H, bounds.height),
                size: TRIGGER_TEXT_SIZE.into(),
                line_height: text::LineHeight::default(),
                font,
                align_x: text::Alignment::Left,
                align_y: Vertical::Center,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.x + ITEM_PADDING_H, bounds.y + bounds.height / 2.0),
            palette.background.base.text,
            bounds,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.open {
            return None;
        }

        let position = layout.position() + translation;
        let bounds = layout.bounds();

        let key = (state.query.clone(), self.selected.clone());
        if state.built_for.as_ref() != Some(&key) {
            let content = build_content(&state.query, &self.selected);
            let tree = state.menu_tree.get_or_insert_with(|| Tree::new(&content));
            tree.diff(&content);
            state.content = Some(content);
            state.built_for = Some(key);
        }

        Some(overlay::Element::new(Box::new(PickerOverlay {
            state,
            on_select: self.on_select.as_ref(),
            anchor: Rectangle {
                x: position.x,
                y: position.y,
                width: bounds.width,
                height: bounds.height,
            },
        })))
    }
}

fn search_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let text_color = palette.background.base.text;
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: palette.primary.base.color,
            width: 1.0,
            radius: radius().into(),
        },
        icon: text_color,
        placeholder: Color {
            a: 0.5,
            ..text_color
        },
        value: text_color,
        selection: palette.primary.base.color.scale_alpha(0.35),
    }
}

fn build_content<'a>(query: &str, selected: &str) -> Element<'a, Op, Theme, Renderer> {
    let search = text_input(SEARCH_PLACEHOLDER, query)
        .id(SEARCH_ID)
        .on_input(Op::Query)
        .size(SEARCH_TEXT_SIZE)
        .padding([SEARCH_V_PAD, ITEM_PADDING_H])
        .style(search_style);

    let query_lower = query.to_lowercase();
    let mut col = column![].width(Length::Fill);

    if query.is_empty() {
        col = col.push(font_row(
            DEFAULT_LABEL,
            Font::DEFAULT,
            String::new(),
            selected.is_empty(),
        ));
    }

    let mut found = false;
    for name in matches(&query_lower) {
        found = true;
        col = col.push(font_row(
            name,
            font_of(name),
            name.to_owned(),
            name == selected,
        ));
    }

    if !found && !query.is_empty() {
        col = col.push(
            container(
                text_widget("No fonts found")
                    .size(TEXT_SIZE)
                    .color(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
            )
            .padding([0.0, ITEM_PADDING_H])
            .height(Length::Fixed(ROW_HEIGHT))
            .align_y(Vertical::Center),
        );
    }

    let gutter = SCROLLBAR_WIDTH + SCROLLBAR_GUTTER;
    let list = scrollable(col.padding(iced::Padding::ZERO.right(gutter)))
        .width(Length::Fill)
        .height(Length::Shrink)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(0.0),
        ));

    container(column![search, list].spacing(GAP).width(Length::Fill))
        .padding(PADDING)
        .width(Length::Fixed(POPUP_W))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color,
            )),
            border: Border {
                color: theme.extended_palette().background.strong.color,
                width: 1.0,
                radius: radius().into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn font_row<'a>(
    label: &'static str,
    font: Font,
    value: String,
    is_selected: bool,
) -> Element<'a, Op, Theme, Renderer> {
    button(
        row![text_widget(label).size(TEXT_SIZE).font(font)]
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .padding([0.0, ITEM_PADDING_H]),
    )
    .width(Length::Fill)
    .height(Length::Fixed(ROW_HEIGHT))
    .padding(0.0)
    .style(move |theme, status| row_style(theme, status, is_selected))
    .on_press(Op::Select(value))
    .into()
}

struct PickerOverlay<'a, 'b, Message> {
    state: &'b mut State,
    on_select: &'b (dyn Fn(String) -> Message + 'a),
    anchor: Rectangle,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for PickerOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let max_h = PADDING + SEARCH_H + GAP + ROW_HEIGHT * MAX_VISIBLE_ITEMS as f32 + PADDING;

        let (Some(content), Some(menu_tree)) =
            (self.state.content.as_mut(), self.state.menu_tree.as_mut())
        else {
            return layout::Node::new(Size::ZERO);
        };

        let node = content.as_widget_mut().layout(
            menu_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(POPUP_W, max_h)),
        );
        let size = node.bounds().size();

        let x = self
            .anchor
            .x
            .clamp(0.0, (bounds.width - size.width).max(0.0));
        let y = (self.anchor.y + self.anchor.height + GAP)
            .clamp(0.0, (bounds.height - size.height).max(0.0));
        node.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        if let (Some(content), Some(menu_tree)) =
            (self.state.content.as_ref(), self.state.menu_tree.as_ref())
        {
            let viewport = layout.bounds();
            content
                .as_widget()
                .draw(menu_tree, renderer, theme, style, layout, cursor, &viewport);
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        if let (Some(content), Some(menu_tree)) =
            (self.state.content.as_mut(), self.state.menu_tree.as_mut())
        {
            content
                .as_widget_mut()
                .operate(menu_tree, layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
    ) {
        if self.state.needs_focus {
            self.state.needs_focus = false;
            if let (Some(content), Some(menu_tree)) =
                (self.state.content.as_mut(), self.state.menu_tree.as_mut())
            {
                let mut op = focusable::focus::<()>(iced::advanced::widget::Id::from(SEARCH_ID));
                content
                    .as_widget_mut()
                    .operate(menu_tree, layout, renderer, &mut op);
            }
            shell.request_redraw();
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) = event
        {
            self.close();
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            self.close();
            shell.request_redraw();
            return;
        }

        let mut ops: Vec<Op> = Vec::new();
        let mut local = Shell::new(&mut ops);
        let viewport = layout.bounds();
        if let (Some(content), Some(menu_tree)) =
            (self.state.content.as_mut(), self.state.menu_tree.as_mut())
        {
            content.as_widget_mut().update(
                menu_tree, event, layout, cursor, renderer, clipboard, &mut local, &viewport,
            );
        }
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

        if matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })) {
            shell.request_redraw();
        }

        for op in ops {
            match op {
                Op::Query(q) => {
                    self.state.query = q;
                    shell.request_redraw();
                }
                Op::Select(value) => {
                    shell.publish((self.on_select)(value));
                    self.close();
                    shell.request_redraw();
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        if let (Some(content), Some(menu_tree)) =
            (self.state.content.as_ref(), self.state.menu_tree.as_ref())
        {
            content
                .as_widget()
                .mouse_interaction(menu_tree, layout, cursor, &viewport, renderer)
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message> PickerOverlay<'_, '_, Message> {
    fn close(&mut self) {
        self.state.open = false;
        self.state.query.clear();
        self.state.content = None;
        self.state.menu_tree = None;
        self.state.built_for = None;
    }
}

fn row_style(theme: &Theme, status: button::Status, is_selected: bool) -> button::Style {
    let palette = theme.extended_palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    let background = if is_selected {
        Some(Background::Color(palette.primary.weak.color))
    } else if hovered {
        Some(Background::Color(palette.background.strong.color))
    } else {
        None
    };

    let text_color = if is_selected {
        palette.primary.base.color
    } else {
        palette.background.base.text
    };

    button::Style {
        background,
        text_color,
        border: Border {
            radius: radius().into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

impl<'a, Message: Clone + 'a> From<FontPicker<Message>> for Element<'a, Message, Theme, Renderer> {
    fn from(picker: FontPicker<Message>) -> Self {
        Self::new(picker)
    }
}
