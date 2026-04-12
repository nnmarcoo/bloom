use iced::advanced::layout;
use iced::advanced::widget::Tree;
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Vector};

pub struct SlideIn<'a, Message> {
    content: Element<'a, Message>,
    offset_x: f32,
}

impl<'a, Message> SlideIn<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message>>, offset_x: f32) -> Self {
        Self {
            content: content.into(),
            offset_x,
        }
    }
}

fn shift_cursor(cursor: mouse::Cursor, dx: f32) -> mouse::Cursor {
    match cursor {
        mouse::Cursor::Available(p) => mouse::Cursor::Available(Point::new(p.x - dx, p.y)),
        mouse::Cursor::Levitating(p) => mouse::Cursor::Levitating(Point::new(p.x - dx, p.y)),
        mouse::Cursor::Unavailable => mouse::Cursor::Unavailable,
    }
}

impl<'a, Message> Widget<Message, iced::Theme, Renderer> for SlideIn<'a, Message>
where
    Message: Clone + 'a,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        advanced::Renderer::with_translation(
            renderer,
            Vector::new(self.offset_x, 0.0),
            |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    shift_cursor(cursor, self.offset_x),
                    viewport,
                );
            },
        );
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            shift_cursor(cursor, self.offset_x),
            renderer,
            clipboard,
            shell,
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            shift_cursor(cursor, self.offset_x),
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, Message> From<SlideIn<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(widget: SlideIn<'a, Message>) -> Self {
        Element::new(widget)
    }
}
