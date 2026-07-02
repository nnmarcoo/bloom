use iced::advanced::widget::{Tree, tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget, layout, overlay};
use iced::border::Radius;
use iced::{
    Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Vector, mouse,
};

use crate::styles::RULE_HEIGHT;

pub struct HoverRow<'a, Message> {
    children: Vec<Element<'a, Message>>,
    has_trailing: bool,
    hover_slot: f32,
    has_hover: bool,
}

impl<'a, Message> HoverRow<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            children: vec![content.into()],
            has_trailing: false,
            hover_slot: 0.0,
            has_hover: false,
        }
    }

    pub fn trailing(mut self, trailing: impl Into<Element<'a, Message>>) -> Self {
        self.children.insert(1, trailing.into());
        self.has_trailing = true;
        self
    }

    pub fn hover_slot(mut self, slot_width: f32, element: Option<Element<'a, Message>>) -> Self {
        self.hover_slot = slot_width;
        self.has_hover = element.is_some();
        if let Some(element) = element {
            self.children.push(element);
        }
        self
    }

    fn hover_index(&self) -> Option<usize> {
        self.has_hover.then_some(1 + usize::from(self.has_trailing))
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
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        self.children[0].as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let inset = BAR_WIDTH + BAR_GAP;
        let slot_extra = if self.hover_slot > 0.0 {
            self.hover_slot + BAR_GAP
        } else {
            0.0
        };

        let trailing_node = self.has_trailing.then(|| {
            let loose = layout::Limits::new(Size::ZERO, limits.max());
            self.children[1]
                .as_widget_mut()
                .layout(&mut tree.children[1], renderer, &loose)
        });
        let trailing_extra = trailing_node
            .as_ref()
            .map_or(0.0, |n| n.size().width + BAR_GAP);

        let inner_limits = limits.shrink(Size::new(inset * 2.0 + slot_extra + trailing_extra, 0.0));
        let inner = self.children[0]
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &inner_limits)
            .move_to((inset, 0.0));
        let inner_size = inner.size();

        let hover_node = self.hover_index().map(|i| {
            let slot_limits =
                layout::Limits::new(Size::ZERO, Size::new(self.hover_slot, limits.max().height));
            self.children[i]
                .as_widget_mut()
                .layout(&mut tree.children[i], renderer, &slot_limits)
        });

        let mut height = inner_size.height;
        for node in trailing_node.iter().chain(hover_node.iter()) {
            height = height.max(node.size().height);
        }

        let center = |node: layout::Node, x: f32| {
            let y = ((height - node.size().height) / 2.0).round();
            node.move_to((x, y))
        };

        let mut nodes = vec![inner];
        if let Some(node) = trailing_node {
            let x = inset + inner_size.width + slot_extra + BAR_GAP;
            nodes.push(center(node, x));
        }
        if let Some(node) = hover_node {
            let x = inset
                + inner_size.width
                + BAR_GAP
                + ((self.hover_slot - node.size().width) / 2.0).round();
            nodes.push(center(node, x));
        }

        layout::Node::with_children(
            Size::new(
                inner_size.width + inset * 2.0 + slot_extra + trailing_extra,
                height,
            ),
            nodes,
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
        let hovered = tree.state.downcast_ref::<State>().hovered;
        if hovered {
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

        let hover_index = self.hover_index();
        for (i, ((child, state), child_layout)) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if Some(i) == hover_index && !hovered {
                continue;
            }
            child.as_widget().draw(
                state,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }
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

        for ((child, child_tree), child_layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), child_layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, child_layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn advanced::widget::Operation,
    ) {
        for ((child, child_tree), child_layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(child_tree, child_layout, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
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
