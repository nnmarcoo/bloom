use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Overlay, Shell, Widget};
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::{Column, container, text};
use iced::{
    Background, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, border,
    overlay,
};

use crate::styles::{menu_container_style, menu_item_hover_color, menu_separator_style, radius};

const ITEM_HEIGHT: f32 = 28.0;
const ITEM_PADDING_H: f32 = 8.0;
const CONTAINER_PADDING: f32 = 6.0;

struct MenuItem<'a, Message> {
    label: &'a str,
    on_press: Message,
}

#[derive(Default)]
struct MenuItemState {
    is_hovered: bool,
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for MenuItem<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MenuItemState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MenuItemState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(ITEM_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, ITEM_HEIGHT)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<MenuItemState>();
        let is_over = cursor.is_over(layout.bounds());

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_hovered != is_over {
                    state.is_hovered = is_over;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if is_over => {
                shell.publish(self.on_press.clone());
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;
        use iced::advanced::text::{self, Renderer as _};

        let state = tree.state.downcast_ref::<MenuItemState>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();

        if state.is_hovered {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: border::rounded(radius()),
                    ..Default::default()
                },
                Background::Color(menu_item_hover_color(theme)),
            );
        }

        renderer.fill_text(
            text::Text {
                content: self.label.to_owned(),
                bounds: Size::new(bounds.width - 2.0 * ITEM_PADDING_H, bounds.height),
                size: renderer.default_size(),
                line_height: text::LineHeight::default(),
                font: renderer.default_font(),
                align_x: text::Alignment::Left,
                align_y: Vertical::Center,
                shaping: text::Shaping::Basic,
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
        layout: Layout,
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
}

impl<'a, Message: Clone + 'a> From<MenuItem<'a, Message>> for Element<'a, Message> {
    fn from(item: MenuItem<'a, Message>) -> Self {
        Element::new(item)
    }
}

pub fn menu_item<'a, Message: Clone + 'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    MenuItem {
        label,
        on_press: msg,
    }
    .into()
}

pub fn menu_separator<'a, Message: 'a + Clone>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(1)
        .style(menu_separator_style)
        .into()
}

pub fn styled_menu<'a, Message: 'a>(
    items: Column<'a, Message>,
    width: impl Into<Length>,
) -> Element<'a, Message> {
    container(items.spacing(2))
        .width(width)
        .padding(6)
        .style(menu_container_style)
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SubMenuSide {
    #[default]
    Right,
    Left,
}

#[derive(Default)]
struct SubMenuState {
    is_hovered: bool,
}

pub struct SubMenuItem<'a, Message> {
    label: &'a str,
    menu: Element<'a, Message, Theme, Renderer>,
    side: SubMenuSide,
}

impl<'a, Message: Clone + 'a> SubMenuItem<'a, Message> {
    pub fn side(mut self, side: SubMenuSide) -> Self {
        self.side = side;
        self
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for SubMenuItem<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SubMenuState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SubMenuState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.menu]);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(ITEM_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, ITEM_HEIGHT)
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
        let state = tree.state.downcast_mut::<SubMenuState>();
        let is_over = cursor.is_over(layout.bounds());
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            if is_over && !state.is_hovered {
                state.is_hovered = true;
                shell.request_redraw();
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;
        use iced::advanced::text::{self, Renderer as _};

        let state = tree.state.downcast_ref::<SubMenuState>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();

        if state.is_hovered {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: border::rounded(radius()),
                    ..Default::default()
                },
                Background::Color(menu_item_hover_color(theme)),
            );
        }

        let (label_x, label_align, arrow_x, arrow_align) = match self.side {
            SubMenuSide::Right => (
                bounds.x + ITEM_PADDING_H,
                text::Alignment::Left,
                bounds.x + bounds.width - ITEM_PADDING_H,
                text::Alignment::Right,
            ),
            SubMenuSide::Left => (
                bounds.x + bounds.width - ITEM_PADDING_H,
                text::Alignment::Right,
                bounds.x + ITEM_PADDING_H,
                text::Alignment::Left,
            ),
        };

        renderer.fill_text(
            text::Text {
                content: self.label.to_owned(),
                bounds: Size::new(bounds.width - 2.0 * ITEM_PADDING_H, bounds.height),
                size: renderer.default_size(),
                line_height: text::LineHeight::default(),
                font: renderer.default_font(),
                align_x: label_align,
                align_y: Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(label_x, bounds.y + bounds.height / 2.0),
            palette.background.base.text,
            bounds,
        );

        renderer.fill_text(
            text::Text {
                content: match self.side {
                    SubMenuSide::Right => "›",
                    SubMenuSide::Left => "‹",
                }
                .to_owned(),
                bounds: Size::new(ITEM_PADDING_H * 2.0, bounds.height),
                size: renderer.default_size(),
                line_height: text::LineHeight::default(),
                font: renderer.default_font(),
                align_x: arrow_align,
                align_y: Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(arrow_x, bounds.y + bounds.height / 2.0),
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
        if !tree.state.downcast_ref::<SubMenuState>().is_hovered {
            return None;
        }

        let position = layout.position() + translation;
        let bounds = layout.bounds();

        Some(overlay::Element::new(Box::new(SubMenuOverlay {
            menu_tree: &mut tree.children[0],
            widget_state: &mut tree.state,
            menu: &mut self.menu,
            trigger_bounds: Rectangle {
                x: position.x,
                y: position.y,
                width: bounds.width,
                height: bounds.height,
            },
            side: self.side,
        })))
    }
}

impl<'a, Message: Clone + 'a> From<SubMenuItem<'a, Message>> for Element<'a, Message> {
    fn from(item: SubMenuItem<'a, Message>) -> Self {
        Element::new(item)
    }
}

pub fn sub_menu<'a, Message: Clone + 'a>(
    label: &'a str,
    menu: Element<'a, Message>,
) -> SubMenuItem<'a, Message> {
    SubMenuItem {
        label,
        menu,
        side: SubMenuSide::default(),
    }
}

struct SubMenuOverlay<'a, 'b, Message> {
    menu_tree: &'b mut Tree,
    widget_state: &'b mut tree::State,
    menu: &'b mut Element<'a, Message, Theme, Renderer>,
    trigger_bounds: Rectangle,
    side: SubMenuSide,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for SubMenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(
            self.menu_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let menu_size = node.bounds().size();

        let right_x = self.trigger_bounds.x + self.trigger_bounds.width + CONTAINER_PADDING;
        let left_x = self.trigger_bounds.x - menu_size.width - CONTAINER_PADDING;
        let x = match self.side {
            SubMenuSide::Right => {
                if right_x + menu_size.width <= bounds.width {
                    right_x
                } else {
                    left_x
                }
            }
            SubMenuSide::Left => {
                if left_x >= 0.0 {
                    left_x
                } else {
                    right_x
                }
            }
        };

        let y = self
            .trigger_bounds
            .y
            .clamp(0.0, (bounds.height - menu_size.height).max(0.0));

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
        let viewport = layout.bounds();
        self.menu.as_widget().draw(
            self.menu_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &viewport,
        );
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
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            let extended_trigger = match self.side {
                SubMenuSide::Right => Rectangle {
                    width: self.trigger_bounds.width + CONTAINER_PADDING,
                    ..self.trigger_bounds
                },
                SubMenuSide::Left => Rectangle {
                    x: self.trigger_bounds.x - CONTAINER_PADDING,
                    width: self.trigger_bounds.width + CONTAINER_PADDING,
                    ..self.trigger_bounds
                },
            };
            if !cursor.is_over(layout.bounds()) && !cursor.is_over(extended_trigger) {
                self.widget_state.downcast_mut::<SubMenuState>().is_hovered = false;
                shell.request_redraw();
                return;
            }
        }

        let viewport = layout.bounds();
        let had_messages = !shell.is_empty();
        self.menu.as_widget_mut().update(
            self.menu_tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &viewport,
        );
        if !had_messages && !shell.is_empty() {
            self.widget_state.downcast_mut::<SubMenuState>().is_hovered = false;
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        self.menu
            .as_widget()
            .mouse_interaction(self.menu_tree, layout, cursor, &viewport, renderer)
    }
}
