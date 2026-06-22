use std::cell::RefCell;
use std::sync::OnceLock;

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget, layout, text};
use iced::alignment::Vertical;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, row, scrollable, text as text_widget};
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
const SEARCH_H: f32 = 28.0;
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

#[derive(Default)]
struct State {
    open: bool,
    query: String,
    menu_tree: RefCell<Option<Tree>>,
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

        let list = build_list(&state.query, &self.selected, self.on_select.as_ref());

        Some(overlay::Element::new(Box::new(PickerOverlay {
            state,
            list,
            anchor: Rectangle {
                x: position.x,
                y: position.y,
                width: bounds.width,
                height: bounds.height,
            },
        })))
    }
}

fn build_list<'a, Message: Clone + 'a>(
    query: &str,
    selected: &str,
    on_select: &(dyn Fn(String) -> Message + 'a),
) -> Element<'a, Message, Theme, Renderer> {
    let query_lower = query.to_lowercase();
    let mut col = column![].width(Length::Fill);

    if query.is_empty() {
        col = col.push(font_row(
            DEFAULT_LABEL,
            Font::DEFAULT,
            String::new(),
            selected.is_empty(),
            on_select,
        ));
    }

    let mut found = false;
    for name in matches(&query_lower) {
        found = true;
        let is_selected = name == selected;
        col = col.push(font_row(
            name,
            font_of(name),
            name.to_owned(),
            is_selected,
            on_select,
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
    scrollable(col.padding(iced::Padding::ZERO.right(gutter)))
        .width(Length::Fill)
        .height(Length::Shrink)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(0.0),
        ))
        .into()
}

fn font_row<'a, Message: Clone + 'a>(
    label: &'static str,
    font: Font,
    value: String,
    is_selected: bool,
    on_select: &(dyn Fn(String) -> Message + 'a),
) -> Element<'a, Message, Theme, Renderer> {
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
    .on_press(on_select(value))
    .into()
}

struct PickerOverlay<'a, 'b, Message> {
    state: &'b mut State,
    list: Element<'a, Message, Theme, Renderer>,
    anchor: Rectangle,
}

impl<Message> PickerOverlay<'_, '_, Message> {
    fn search_rect(&self, origin: Point, width: f32) -> Rectangle {
        Rectangle {
            x: origin.x + PADDING,
            y: origin.y + PADDING,
            width: width - 2.0 * PADDING,
            height: SEARCH_H,
        }
    }

    fn list_limits(&self, width: f32) -> Size {
        let visible_h = ROW_HEIGHT * MAX_VISIBLE_ITEMS as f32;
        Size::new(width - 2.0 * PADDING, visible_h)
    }
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for PickerOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let width = POPUP_W;
        let list_size = self.list_limits(width);

        let mut menu_cell = self.state.menu_tree.borrow_mut();
        let menu_tree = menu_cell.get_or_insert_with(|| Tree::new(&self.list));
        menu_tree.diff(&self.list);

        let list_node = self.list.as_widget_mut().layout(
            menu_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, list_size),
        );
        let list_h = list_node.bounds().height.min(list_size.height);

        let total_h = PADDING + SEARCH_H + GAP + list_h + PADDING;
        let size = Size::new(width, total_h);

        let x = self
            .anchor
            .x
            .clamp(0.0, (bounds.width - size.width).max(0.0));
        let y = (self.anchor.y + self.anchor.height + GAP)
            .clamp(0.0, (bounds.height - size.height).max(0.0));
        let origin = Point::new(x, y);

        let positioned = list_node.move_to(Point::new(PADDING, PADDING + SEARCH_H + GAP));

        layout::Node::with_children(size, vec![positioned]).move_to(origin)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let origin = bounds.position();
        let palette = theme.extended_palette();

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
            Background::Color(palette.background.weak.color),
        );

        let search = self.search_rect(origin, bounds.width);
        renderer.fill_quad(
            Quad {
                bounds: search,
                border: Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: radius().into(),
                },
                ..Quad::default()
            },
            Background::Color(palette.background.base.color),
        );

        let has_query = !self.state.query.is_empty();
        let content = if has_query {
            self.state.query.clone()
        } else {
            SEARCH_PLACEHOLDER.to_owned()
        };
        let text_color = if has_query {
            palette.background.base.text
        } else {
            Color {
                a: 0.5,
                ..palette.background.base.text
            }
        };
        renderer.fill_text(
            text::Text {
                content,
                bounds: Size::new(search.width - 2.0 * ITEM_PADDING_H, search.height),
                size: SEARCH_TEXT_SIZE.into(),
                line_height: text::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: text::Alignment::Left,
                align_y: Vertical::Center,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            Point::new(search.x + ITEM_PADDING_H, search.y + search.height / 2.0),
            text_color,
            search,
        );

        if has_query {
            let caret_x = (search.x + ITEM_PADDING_H + self.query_width()).round();
            let cy = search.y + search.height / 2.0;
            let ch = SEARCH_TEXT_SIZE;
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: caret_x,
                        y: cy - ch / 2.0,
                        width: 1.5,
                        height: ch,
                    },
                    ..Quad::default()
                },
                Background::Color(palette.primary.base.color),
            );
        }

        let list_layout = layout.children().next().unwrap();
        let menu_cell = self.state.menu_tree.borrow();
        if let Some(menu_tree) = menu_cell.as_ref() {
            let viewport = list_layout.bounds();
            self.list.as_widget().draw(
                menu_tree,
                renderer,
                theme,
                style,
                list_layout,
                cursor,
                &viewport,
            );
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            text,
            modifiers,
            ..
        }) = event
        {
            match key {
                Key::Named(Named::Escape) => {
                    self.close();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                Key::Named(Named::Backspace) => {
                    if self.state.query.pop().is_some() {
                        shell.capture_event();
                        shell.request_redraw();
                    }
                    return;
                }
                _ if !modifiers.command() && !modifiers.control() && !modifiers.alt() => {
                    if let Some(s) = text.as_ref() {
                        let mut changed = false;
                        for ch in s.chars() {
                            if !ch.is_control() {
                                self.state.query.push(ch);
                                changed = true;
                            }
                        }
                        if changed {
                            shell.capture_event();
                            shell.request_redraw();
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            self.close();
            shell.request_redraw();
            return;
        }

        let list_layout = layout.children().next().unwrap();
        let viewport = list_layout.bounds();
        let mut menu_cell = self.state.menu_tree.borrow_mut();
        let Some(menu_tree) = menu_cell.as_mut() else {
            return;
        };
        let had_messages = !shell.is_empty();
        self.list.as_widget_mut().update(
            menu_tree,
            event,
            list_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &viewport,
        );
        if !had_messages && !shell.is_empty() {
            drop(menu_cell);
            self.close();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let list_layout = layout.children().next().unwrap();
        let viewport = list_layout.bounds();
        let menu_cell = self.state.menu_tree.borrow();
        if let Some(menu_tree) = menu_cell.as_ref() {
            self.list.as_widget().mouse_interaction(
                menu_tree,
                list_layout,
                cursor,
                &viewport,
                renderer,
            )
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message> PickerOverlay<'_, '_, Message> {
    fn close(&mut self) {
        self.state.open = false;
        self.state.query.clear();
        *self.state.menu_tree.borrow_mut() = None;
    }

    fn query_width(&self) -> f32 {
        use iced::advanced::text::Paragraph as _;
        let para = <Renderer as text::Renderer>::Paragraph::with_text(text::Text {
            content: &self.state.query,
            bounds: Size::INFINITE,
            size: SEARCH_TEXT_SIZE.into(),
            line_height: text::LineHeight::default(),
            font: Font::DEFAULT,
            align_x: text::Alignment::Left,
            align_y: Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        });
        para.min_bounds().width
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
