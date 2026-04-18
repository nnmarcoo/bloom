use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget};
use iced::mouse;
use iced::overlay;
use iced::widget::button;
use iced::{Element, Event, Length, Padding, Point, Rectangle, Renderer, Size, Theme, Vector};

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
    content: Element<'a, Message, Theme, Renderer>,
    menu: Element<'a, Message, Theme, Renderer>,
    style: Box<dyn Fn(&Theme, button::Status) -> button::Style>,
    width: Length,
    height: Length,
    padding: Padding,
    align: MenuAlign,
    gap: f32,
}

impl<'a, Message: Clone + 'a> MenuButton<'a, Message> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        menu: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            content: content.into(),
            menu: menu.into(),
            style: Box::new(|_, _| button::Style::default()),
            width: Length::Shrink,
            height: Length::Shrink,
            padding: Padding::ZERO,
            align: MenuAlign::TopEnd,
            gap: 4.0,
        }
    }

    pub fn style(
        mut self,
        style: impl Fn(&Theme, button::Status) -> button::Style + 'static,
    ) -> Self {
        self.style = Box::new(style);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
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

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for MenuButton<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        let max = limits.max();
        let pad = self.padding;
        let pad_w = pad.left + pad.right;
        let pad_h = pad.top + pad.bottom;

        let inner_limits = layout::Limits::new(
            Size::ZERO,
            Size::new((max.width - pad_w).max(0.0), (max.height - pad_h).max(0.0)),
        );

        let content_node =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &inner_limits);
        let content_size = content_node.bounds().size();

        let button_w = match self.width {
            Length::Shrink => content_size.width + pad_w,
            _ => max.width,
        };
        let button_h = match self.height {
            Length::Shrink => content_size.height + pad_h,
            _ => max.height,
        };

        let offset_x = pad.left + ((button_w - pad_w - content_size.width) / 2.0).max(0.0);
        let offset_y = pad.top + ((button_h - pad_h - content_size.height) / 2.0).max(0.0);

        layout::Node::with_children(
            Size::new(button_w, button_h),
            vec![content_node.move_to(Point::new(offset_x, offset_y))],
        )
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
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let status = if cursor.is_over(bounds) || state.expanded {
            button::Status::Hovered
        } else {
            button::Status::Active
        };
        let btn_style = (self.style)(theme, status);

        if let Some(bg) = btn_style.background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: btn_style.border,
                    shadow: btn_style.shadow,
                    snap: true,
                },
                bg,
            );
        }

        let content_layout = layout.children().next().unwrap();
        let content_style = renderer::Style {
            text_color: btn_style.text_color,
        };
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &content_style,
            content_layout,
            cursor,
            viewport,
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
        if !tree.state.downcast_ref::<State>().expanded {
            return None;
        }

        let position = layout.position() + translation;
        let bounds = layout.bounds();

        Some(overlay::Element::new(Box::new(MenuOverlay {
            menu_tree: &mut tree.children[1],
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
    for Element<'a, Message, Theme, Renderer>
{
    fn from(w: MenuButton<'a, Message>) -> Self {
        Self::new(w)
    }
}

struct MenuOverlay<'a, 'b, Message> {
    menu_tree: &'b mut Tree,
    widget_state: &'b mut tree::State,
    menu: &'b mut Element<'a, Message, Theme, Renderer>,
    button_bounds: Rectangle,
    align: MenuAlign,
    gap: f32,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for MenuOverlay<'_, '_, Message> {
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if !cursor.is_over(layout.bounds()) && !cursor.is_over(self.button_bounds) {
                self.widget_state.downcast_mut::<State>().expanded = false;
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
            self.widget_state.downcast_mut::<State>().expanded = false;
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

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'c, Message, Theme, Renderer>> {
        let viewport = layout.bounds();
        self.menu
            .as_widget_mut()
            .overlay(self.menu_tree, layout, renderer, &viewport, Vector::ZERO)
    }
}
