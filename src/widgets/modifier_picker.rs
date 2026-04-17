use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Overlay, Shell, Widget};
use iced::alignment::Vertical;
use iced::mouse;
use iced::overlay;
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size, Theme, Vector,
};

use crate::modifiers::{MODIFIER_TYPES, ModifierType};
use crate::styles::radius;

const ITEM_HEIGHT: f32 = 26.0;
const ITEM_PADDING_H: f32 = 8.0;
const BUTTON_HEIGHT: f32 = 28.0;
const TEXT_SIZE: f32 = 11.0;
const PADDING: f32 = 4.0;

#[derive(Default)]
struct State {
    expanded: bool,
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
}

fn fill_text(
    renderer: &mut Renderer,
    content: &str,
    x: f32,
    bounds: Rectangle,
    color: Color,
    align_x: text::Alignment,
) {
    use advanced::text::Renderer as _;

    let (origin_x, width) = match align_x {
        text::Alignment::Center => (bounds.center_x(), bounds.width),
        _ => (x, bounds.x + bounds.width - x),
    };

    renderer.fill_text(
        Text {
            content: content.to_string(),
            bounds: Size::new(width, bounds.height),
            size: Pixels(TEXT_SIZE),
            line_height: text::LineHeight::default(),
            font: Font::DEFAULT,
            align_x,
            align_y: Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        },
        Point::new(origin_x, bounds.center_y()),
        color,
        bounds,
    );
}

impl<Message: Clone + 'static> Widget<Message, Theme, Renderer> for ModifierPicker<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Fixed(BUTTON_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let w = match self.width {
            Length::Fixed(w) => w,
            _ => limits.max().width,
        };
        layout::atomic(limits, w, BUTTON_HEIGHT)
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
        let state = tree.state.downcast_mut::<State>();
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if cursor.is_over(layout.bounds()) {
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
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
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
            Background::Color(if cursor.is_over(bounds) || state.expanded {
                palette.background.weak.color
            } else {
                palette.background.base.color
            }),
        );

        fill_text(
            renderer,
            "+ Add Modifier",
            0.0,
            bounds,
            palette.background.base.text,
            text::Alignment::Center,
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

        let bounds = layout.bounds();
        let button_bounds = Rectangle {
            x: bounds.x + translation.x,
            y: bounds.y + translation.y,
            width: bounds.width,
            height: bounds.height,
        };

        Some(overlay::Element::new(Box::new(DropdownOverlay {
            widget_state: &mut tree.state,
            on_select: &self.on_select,
            button_bounds,
        })))
    }
}

impl<'a, Message: Clone + 'static> From<ModifierPicker<Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(w: ModifierPicker<Message>) -> Self {
        Self::new(w)
    }
}

struct DropdownOverlay<'b, Message> {
    widget_state: &'b mut tree::State,
    on_select: &'b dyn Fn(ModifierType) -> Message,
    button_bounds: Rectangle,
}

impl<Message: Clone> DropdownOverlay<'_, Message> {
    fn item_bounds(&self, origin: Point, index: usize) -> Rectangle {
        Rectangle {
            x: origin.x + PADDING,
            y: origin.y + PADDING + index as f32 * ITEM_HEIGHT,
            width: self.button_bounds.width - PADDING * 2.0,
            height: ITEM_HEIGHT,
        }
    }

    fn dropdown_height(&self) -> f32 {
        MODIFIER_TYPES.len() as f32 * ITEM_HEIGHT + PADDING * 2.0
    }
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for DropdownOverlay<'_, Message> {
    fn layout(&mut self, _renderer: &Renderer, viewport: Size) -> layout::Node {
        let h = self.dropdown_height();
        let w = self.button_bounds.width;
        let gap = 2.0;

        let space_below =
            (viewport.height - (self.button_bounds.y + self.button_bounds.height + gap)).max(0.0);
        let space_above = (self.button_bounds.y - gap).max(0.0);

        let y = if space_below >= h || space_below >= space_above {
            self.button_bounds.y + self.button_bounds.height + gap
        } else {
            self.button_bounds.y - gap - h.min(space_above)
        };

        let x = self.button_bounds.x.min(viewport.width - w).max(0.0);
        layout::Node::new(Size::new(w, h)).move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use advanced::Renderer as _;

        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let origin = Point::new(bounds.x, bounds.y);

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

        for (i, t) in MODIFIER_TYPES.iter().enumerate() {
            let item_bounds = self.item_bounds(origin, i);

            if cursor.is_over(item_bounds) {
                renderer.fill_quad(
                    Quad {
                        bounds: item_bounds,
                        border: Border {
                            radius: radius().into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(palette.background.strong.color),
                );
            }

            fill_text(
                renderer,
                &t.to_string(),
                item_bounds.x + ITEM_PADDING_H,
                item_bounds,
                palette.background.base.text,
                text::Alignment::Left,
            );
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
    ) {
        let bounds = layout.bounds();
        let origin = Point::new(bounds.x, bounds.y);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if !cursor.is_over(bounds) && !cursor.is_over(self.button_bounds) {
                    self.widget_state.downcast_mut::<State>().expanded = false;
                    shell.request_redraw();
                    return;
                }

                for (i, t) in MODIFIER_TYPES.iter().enumerate() {
                    if cursor.is_over(self.item_bounds(origin, i)) {
                        shell.publish((self.on_select)(t.clone()));
                        self.widget_state.downcast_mut::<State>().expanded = false;
                        shell.capture_event();
                        return;
                    }
                }
            }

            Event::Mouse(mouse::Event::CursorMoved { .. }) if cursor.is_over(bounds) => {
                shell.request_redraw();
            }

            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::default();
        }

        let bounds = layout.bounds();
        let origin = Point::new(bounds.x, bounds.y);

        for (i, _) in MODIFIER_TYPES.iter().enumerate() {
            if cursor.is_over(self.item_bounds(origin, i)) {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}
