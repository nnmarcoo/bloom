use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::svg::Svg;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Overlay, Shell, Widget};
use iced::mouse;
use iced::overlay;
use iced::widget::button::Status;
use iced::widget::svg::{self, Handle};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Vector};

use crate::styles::{BUTTON_SIZE, PAD, icon_button_active_style, icon_button_style, svg_style};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuAlign {
    TopEnd,
    TopStart,
    BottomEnd,
    BottomStart,
}

#[derive(Default)]
struct State {
    expanded: bool,
}

pub struct MenuButton<'a, Message> {
    icon: &'static [u8],
    menu: Element<'a, Message, iced::Theme, Renderer>,
    active: bool,
    align: MenuAlign,
    gap: f32,
}

impl<'a, Message: Clone + 'a> MenuButton<'a, Message> {
    pub fn new(
        icon: &'static [u8],
        menu: impl Into<Element<'a, Message, iced::Theme, Renderer>>,
    ) -> Self {
        Self {
            icon,
            menu: menu.into(),
            active: false,
            align: MenuAlign::TopEnd,
            gap: 4.0,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn align(mut self, align: MenuAlign) -> Self {
        self.align = align;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, iced::Theme, Renderer> for MenuButton<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        let side = BUTTON_SIZE + PAD * 2.0;
        Size {
            width: Length::Fixed(side),
            height: Length::Fixed(side),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, BUTTON_SIZE + PAD * 2.0, BUTTON_SIZE + PAD * 2.0)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.menu]);
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if cursor.is_over(layout.bounds()) {
                let state = tree.state.downcast_mut::<State>();
                state.expanded = !state.expanded;
                shell.capture_event();
                shell.request_redraw();
            }
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
        viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;
        use advanced::svg::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let is_hovered = cursor.is_over(bounds) || state.expanded;
        let status = if is_hovered {
            Status::Hovered
        } else {
            Status::Active
        };

        let btn_style = if self.active {
            icon_button_active_style(theme, status)
        } else {
            icon_button_style(theme, status)
        };

        if let Some(bg) = btn_style.background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: btn_style.border,
                    ..renderer::Quad::default()
                },
                bg,
            );
        }

        let icon_bounds = Rectangle {
            x: bounds.x + (bounds.width - BUTTON_SIZE) / 2.0,
            y: bounds.y + (bounds.height - BUTTON_SIZE) / 2.0,
            width: BUTTON_SIZE,
            height: BUTTON_SIZE,
        };

        let svg_appearance = svg_style(
            theme,
            if is_hovered {
                svg::Status::Hovered
            } else {
                svg::Status::Idle
            },
        );

        let mut svg_image = Svg::new(Handle::from_memory(self.icon));
        if let Some(color) = svg_appearance.color {
            svg_image = svg_image.color(color);
        }

        renderer.draw_svg(svg_image, icon_bounds, *viewport);
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
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        if !tree.state.downcast_ref::<State>().expanded {
            return None;
        }

        let position = layout.position() + translation;
        let bounds = layout.bounds();

        Some(overlay::Element::new(Box::new(MenuOverlay {
            menu_tree: &mut tree.children[0],
            widget_state: &mut tree.state,
            menu: &mut self.menu,
            button_bounds: Rectangle {
                x: position.x,
                y: position.y,
                width: bounds.width,
                height: bounds.height,
            },
            align: self.align,
            gap: self.gap,
        })))
    }
}

impl<'a, Message: Clone + 'a> From<MenuButton<'a, Message>>
    for Element<'a, Message, iced::Theme, Renderer>
{
    fn from(w: MenuButton<'a, Message>) -> Self {
        Self::new(w)
    }
}

struct MenuOverlay<'a, 'b, Message> {
    menu_tree: &'b mut Tree,
    widget_state: &'b mut tree::State,
    menu: &'b mut Element<'a, Message, iced::Theme, Renderer>,
    button_bounds: Rectangle,
    align: MenuAlign,
    gap: f32,
}

impl<Message: Clone> Overlay<Message, iced::Theme, Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(
            self.menu_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let menu_size = node.bounds().size();

        let x = match self.align {
            MenuAlign::TopEnd | MenuAlign::BottomEnd => {
                self.button_bounds.x + self.button_bounds.width - menu_size.width
            }
            MenuAlign::TopStart | MenuAlign::BottomStart => self.button_bounds.x,
        };

        let y = match self.align {
            MenuAlign::TopEnd | MenuAlign::TopStart => {
                self.button_bounds.y - menu_size.height - self.gap
            }
            MenuAlign::BottomEnd | MenuAlign::BottomStart => {
                self.button_bounds.y + self.button_bounds.height + self.gap
            }
        };

        node.move_to(Point::new(
            x.clamp(0.0, (bounds.width - menu_size.width).max(0.0)),
            y.clamp(0.0, (bounds.height - menu_size.height).max(0.0)),
        ))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &iced::Theme,
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if !cursor.is_over(layout.bounds()) && !cursor.is_over(self.button_bounds) {
                self.widget_state.downcast_mut::<State>().expanded = false;
                shell.request_redraw();
                return;
            }
        }

        let viewport = layout.bounds();
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
