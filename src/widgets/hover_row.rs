use iced::advanced::widget::{Tree, tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget, layout, overlay};
use iced::border::Radius;
use iced::{
    Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Vector, mouse,
};

use crate::styles::RULE_HEIGHT;

pub struct HoverRow<'a, Message> {
    content: Element<'a, Message>,
}

impl<'a, Message> HoverRow<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

#[derive(Default)]
struct State {
    hovered: bool,
}

const BAR_WIDTH: f32 = RULE_HEIGHT / 2.0;
const BAR_GAP: f32 = 8.0;

impl<'a, Message> Widget<Message, iced::Theme, Renderer> for HoverRow<'a, Message>
where
    Message: Clone + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

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
        let inset = BAR_WIDTH + BAR_GAP;
        let inner_limits = limits.shrink(Size::new(inset * 2.0, 0.0));
        let inner = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &inner_limits)
            .move_to((inset, 0.0));
        let inner_size = inner.size();
        layout::Node::with_children(
            Size::new(inner_size.width + inset * 2.0, inner_size.height),
            vec![inner],
        )
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
        let bounds = layout.bounds();
        if tree.state.downcast_ref::<State>().hovered {
            let color = theme.extended_palette().primary.base.color;
            let bar = |x: f32| advanced::renderer::Quad {
                bounds: Rectangle {
                    x,
                    y: bounds.y,
                    width: BAR_WIDTH,
                    height: bounds.height,
                },
                border: Border {
                    radius: Radius::from(0.0),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..Default::default()
            };
            advanced::Renderer::fill_quad(renderer, bar(bounds.x), Background::Color(color));
            advanced::Renderer::fill_quad(
                renderer,
                bar(bounds.x + bounds.width - BAR_WIDTH),
                Background::Color(color),
            );
        }

        let child = layout.children().next().unwrap();
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child,
            cursor,
            viewport,
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
        let hovered = cursor.is_over(layout.bounds());
        let state = tree.state.downcast_mut::<State>();
        if state.hovered != hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }

        let child = layout.children().next().unwrap();
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            cursor,
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
        let child = layout.children().next().unwrap();
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            cursor,
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
        let child = layout.children().next().unwrap();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        let child = layout.children().next().unwrap();
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message> From<HoverRow<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(widget: HoverRow<'a, Message>) -> Self {
        Element::new(widget)
    }
}
