use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget};
use iced::mouse;
use iced::overlay;
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

pub struct ContextMenu<'a, Message> {
    underlay: Element<'a, Message, Theme, Renderer>,
    menu: Element<'a, Message, Theme, Renderer>,
    open: Option<Point>,
    on_open: Box<dyn Fn(Point) -> Message + 'a>,
    on_close: Message,
}

impl<'a, Message: Clone + 'a> ContextMenu<'a, Message> {
    pub fn new(
        underlay: impl Into<Element<'a, Message, Theme, Renderer>>,
        menu: impl Into<Element<'a, Message, Theme, Renderer>>,
        open: Option<Point>,
        on_open: impl Fn(Point) -> Message + 'a,
        on_close: Message,
    ) -> Self {
        Self {
            underlay: underlay.into(),
            menu: menu.into(),
            open,
            on_open: Box::new(on_open),
            on_close,
        }
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for ContextMenu<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.underlay), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.underlay, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.underlay.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.underlay
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(position) = cursor.position_over(layout.bounds())
        {
            shell.publish((self.on_open)(position));
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        self.underlay.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.underlay.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.underlay.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let position = self.open? + translation;

        Some(overlay::Element::new(Box::new(MenuOverlay {
            menu_tree: &mut tree.children[1],
            menu: &mut self.menu,
            position,
            on_close: self.on_close.clone(),
        })))
    }
}

impl<'a, Message: Clone + 'a> From<ContextMenu<'a, Message>> for Element<'a, Message> {
    fn from(w: ContextMenu<'a, Message>) -> Self {
        Self::new(w)
    }
}

struct MenuOverlay<'a, 'b, Message> {
    menu_tree: &'b mut Tree,
    menu: &'b mut Element<'a, Message, Theme, Renderer>,
    position: Point,
    on_close: Message,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(
            self.menu_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let menu_size = node.bounds().size();

        let x = self
            .position
            .x
            .clamp(0.0, (bounds.width - menu_size.width).max(0.0));
        let y = self
            .position
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
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && !cursor.is_over(layout.bounds())
        {
            shell.publish(self.on_close.clone());
            shell.capture_event();
            shell.request_redraw();
            return;
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
            shell.publish(self.on_close.clone());
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
