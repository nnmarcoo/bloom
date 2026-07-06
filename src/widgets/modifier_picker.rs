use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::widget::operation::focusable;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget, layout, text};
use iced::alignment::Vertical;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, text as text_widget, text_input};
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme,
    Vector, mouse, overlay,
};

use crate::modifiers::ModifierType;
use crate::styles::radius;
use crate::widgets::menu::{SubMenuSide, menu_item, styled_menu, sub_menu};

const TRIGGER_H: f32 = 28.0;
const SUBMENU_W: f32 = 210.0;
const SEARCH_V_PAD: f32 = 8.0;
const SEARCH_TEXT_SIZE: f32 = 12.0;
const SEARCH_H: f32 = SEARCH_TEXT_SIZE + 2.0 * SEARCH_V_PAD;
const ITEM_HEIGHT: f32 = 28.0;
const MAX_VISIBLE_ROWS: f32 = 14.0;
const ITEM_PADDING_H: f32 = 8.0;
const PADDING: f32 = 6.0;
const GAP: f32 = 4.0;
const TEXT_SIZE: f32 = 13.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_GUTTER: f32 = 4.0;
const TRIGGER_LABEL: &str = "+ Add Modifier";
const SEARCH_PLACEHOLDER: &str = "Search modifiers\u{2026}";
const SEARCH_ID: &str = "modifier_picker_search";

#[derive(Clone)]
enum Op {
    Query(String),
    Pick(ModifierType),
}

#[derive(Default)]
struct State {
    open: bool,
    needs_focus: bool,
    query: String,
    built_for: Option<String>,
    content: Option<Element<'static, Op, Theme, Renderer>>,
    menu_tree: Option<Tree>,
    // fly-out picks reach the real shell directly; this flags a close on the next update
    picked: std::rc::Rc<std::cell::Cell<bool>>,
}

pub struct ModifierPicker<Message> {
    on_select: Box<dyn Fn(ModifierType) -> Message>,
    width: Length,
}

impl<Message> ModifierPicker<Message> {
    pub fn new(on_select: impl Fn(ModifierType) -> Message + 'static) -> Self {
        Self {
            on_select: Box::new(on_select),
            width: Length::Fill,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for ModifierPicker<Message> {
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
                state.picked.set(false);
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

        renderer.fill_text(
            text::Text {
                content: TRIGGER_LABEL.to_owned(),
                bounds: Size::new(bounds.width, bounds.height),
                size: SEARCH_TEXT_SIZE.into(),
                line_height: text::LineHeight::default(),
                font: renderer.default_font(),
                align_x: text::Alignment::Center,
                align_y: Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            ),
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

        if state.built_for.as_ref() != Some(&state.query) {
            let content = build_content(&state.query);
            let tree = state.menu_tree.get_or_insert_with(|| Tree::new(&content));
            tree.diff(&content);
            state.content = Some(content);
            state.built_for = Some(state.query.clone());
        }

        Some(overlay::Element::new(Box::new(PickerOverlay {
            state,
            on_select: self.on_select.as_ref(),
            mapper: None,
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

fn build_content<'a>(query: &str) -> Element<'a, Op, Theme, Renderer> {
    let search = text_input(SEARCH_PLACEHOLDER, query)
        .id(SEARCH_ID)
        .on_input(Op::Query)
        .size(SEARCH_TEXT_SIZE)
        .padding([SEARCH_V_PAD, ITEM_PADDING_H])
        .style(search_style);

    let body: Element<'a, Op, Theme, Renderer> = if query.is_empty() {
        submenu_body()
    } else {
        filtered_body(&query.to_lowercase())
    };

    container(column![body, search].spacing(GAP).width(Length::Fill))
        .padding(PADDING)
        .width(Length::Fill)
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

fn categories() -> Vec<(&'static str, Vec<&'static ModifierType>)> {
    let mut cats: Vec<(&'static str, Vec<&'static ModifierType>)> = Vec::new();
    for t in ModifierType::ALL.iter().filter(|t| t.in_menu()) {
        match cats.last_mut() {
            Some((cat, items)) if *cat == t.category() => items.push(t),
            _ => cats.push((t.category(), vec![t])),
        }
    }
    cats
}

fn submenu_body<'a>() -> Element<'a, Op, Theme, Renderer> {
    let mut col = column![].spacing(2).width(Length::Fill);
    for (cat, items) in categories() {
        let mut group = column![].spacing(2);
        for t in items {
            group = group.push(menu_item(t.label(), Op::Pick(t.clone())));
        }
        col = col.push(sub_menu(cat, styled_menu(group, SUBMENU_W)).side(SubMenuSide::Left));
    }
    col.into()
}

fn filtered_body<'a>(query_lower: &str) -> Element<'a, Op, Theme, Renderer> {
    let mut col = column![].spacing(2).width(Length::Fill);
    let mut count = 0usize;
    for t in ModifierType::ALL.iter().filter(|t| t.in_menu()) {
        if t.label().to_lowercase().contains(query_lower) {
            count += 1;
            col = col.push(result_row(t));
        }
    }
    if count == 0 {
        col = col.push(
            container(
                iced::widget::text("No modifiers found")
                    .size(TEXT_SIZE)
                    .color(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
            )
            .padding([0.0, ITEM_PADDING_H])
            .height(Length::Fixed(ITEM_HEIGHT))
            .align_y(Vertical::Center),
        );
    }

    let rows = count.max(1) as f32;
    let spacing = 2.0 * (rows - 1.0).max(0.0);
    let content_h = rows * ITEM_HEIGHT + spacing;
    let list_h = content_h.min(MAX_VISIBLE_ROWS * ITEM_HEIGHT);

    let gutter = SCROLLBAR_WIDTH + SCROLLBAR_GUTTER;
    iced::widget::scrollable(col.padding(iced::Padding::ZERO.right(gutter)))
        .width(Length::Fill)
        .height(Length::Fixed(list_h))
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(0.0),
        ))
        .into()
}

fn result_row<'a>(t: &'static ModifierType) -> Element<'a, Op, Theme, Renderer> {
    button(
        text_widget(t.label())
            .size(TEXT_SIZE)
            .wrapping(text::Wrapping::None)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(ITEM_HEIGHT))
    .padding([0.0, ITEM_PADDING_H])
    .style(result_row_style)
    .on_press(Op::Pick(t.clone()))
    .into()
}

fn result_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(palette.background.strong.color)),
        text_color: palette.background.base.text,
        border: Border {
            radius: radius().into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

struct PickerOverlay<'a, 'b, Message> {
    state: &'b mut State,
    on_select: &'b (dyn Fn(ModifierType) -> Message + 'a),
    #[allow(clippy::type_complexity)]
    mapper: Option<Box<dyn Fn(Op) -> Message + 'b>>,
    anchor: Rectangle,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for PickerOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let max_h = PADDING + SEARCH_H + GAP + ITEM_HEIGHT * MAX_VISIBLE_ROWS + PADDING;

        let (Some(content), Some(menu_tree)) =
            (self.state.content.as_mut(), self.state.menu_tree.as_mut())
        else {
            return layout::Node::new(Size::ZERO);
        };

        let w = self.anchor.width.min(bounds.width);
        let node = content.as_widget_mut().layout(
            menu_tree,
            renderer,
            &layout::Limits::new(Size::new(w, 0.0), Size::new(w, max_h)),
        );
        let size = node.bounds().size();

        let x = self
            .anchor
            .x
            .clamp(0.0, (bounds.width - size.width).max(0.0));
        let y =
            (self.anchor.y - size.height - GAP).clamp(0.0, (bounds.height - size.height).max(0.0));
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
        if self.state.picked.get() {
            self.close();
            shell.request_redraw();
            return;
        }

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

        // cursor is Unavailable when a fly-out is under the pointer, so a genuine
        // outside-click is one where the cursor is available but over nothing
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.position().is_some()
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
                Op::Pick(t) => {
                    shell.publish((self.on_select)(t));
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

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'c, Message, Theme, Renderer>> {
        let on_select = self.on_select;
        let picked = self.state.picked.clone();
        self.mapper = Some(Box::new(move |op| match op {
            Op::Pick(t) => {
                picked.set(true);
                on_select(t)
            }
            Op::Query(_) => unreachable!("submenu overlays never emit a query op"),
        }));
        let mapper = self.mapper.as_deref().unwrap();

        let (content, menu_tree) = (self.state.content.as_mut()?, self.state.menu_tree.as_mut()?);
        let child = content.as_widget_mut().overlay(
            menu_tree,
            layout,
            renderer,
            &layout.bounds(),
            Vector::ZERO,
        )?;
        Some(child.map(mapper))
    }
}

impl<Message> PickerOverlay<'_, '_, Message> {
    fn close(&mut self) {
        self.state.open = false;
        self.state.query.clear();
        self.state.content = None;
        self.state.menu_tree = None;
        self.state.built_for = None;
        self.state.picked.set(false);
    }
}

impl<'a, Message: Clone + 'a> From<ModifierPicker<Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(picker: ModifierPicker<Message>) -> Self {
        Self::new(picker)
    }
}
