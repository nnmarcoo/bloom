use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::{Column, container, text};
use iced::window;
use iced::{Background, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::styles::{menu_container_style, menu_item_hover_color, menu_separator_style, radius};

const ITEM_HEIGHT: f32 = 28.0;
const ITEM_PADDING_H: f32 = 8.0;

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
            Event::Window(window::Event::RedrawRequested(_)) => {
                state.is_hovered = is_over;
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
                    border: iced::border::rounded(radius()),
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

pub fn styled_menu<'a, Message: 'a>(items: Column<'a, Message>) -> Element<'a, Message> {
    container(items.spacing(2))
        .width(180)
        .padding(6)
        .style(menu_container_style)
        .into()
}
