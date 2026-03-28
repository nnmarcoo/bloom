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

use crate::config::ALL_THEMES;
use crate::styles::radius;

const ITEM_HEIGHT: f32 = 28.0;
const ITEM_PADDING_H: f32 = 8.0;
const SWATCH_SIZE: f32 = 12.0;
const SWATCH_GAP: f32 = 3.0;
const SWATCHES_WIDTH: f32 = SWATCH_SIZE * 3.0 + SWATCH_GAP * 2.0;
const BUTTON_HEIGHT: f32 = 28.0;
const TEXT_SIZE: f32 = 13.0;
const DROPDOWN_WIDTH: f32 = 220.0;
const MAX_VISIBLE_ITEMS: usize = 12;
const PADDING: f32 = 6.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_MARGIN: f32 = 3.0;

fn max_dropdown_height() -> f32 {
    ITEM_HEIGHT * MAX_VISIBLE_ITEMS as f32 + PADDING * 2.0
}

fn full_list_height() -> f32 {
    ITEM_HEIGHT * ALL_THEMES.len() as f32
}

fn max_scroll_offset_for(visible_h: f32) -> f32 {
    (full_list_height() - visible_h).max(0.0)
}

#[derive(Default)]
struct State {
    expanded: bool,
    scroll_offset: f32,
}

pub struct ThemePicker<Message> {
    selected: Theme,
    on_select: Box<dyn Fn(Theme) -> Message>,
    width: Length,
}

impl<Message> ThemePicker<Message> {
    pub fn new(selected: Theme, on_select: impl Fn(Theme) -> Message + 'static) -> Self {
        Self {
            selected,
            on_select: Box::new(on_select),
            width: Length::Fixed(DROPDOWN_WIDTH),
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

fn draw_swatches(renderer: &mut Renderer, center_y: f32, right_x: f32, theme: &Theme) {
    use advanced::Renderer as _;

    let p = theme.extended_palette();
    let colors = [
        p.background.base.color,
        p.primary.base.color,
        p.background.strong.color,
    ];
    let start_x = right_x - SWATCHES_WIDTH;

    for (i, color) in colors.iter().enumerate() {
        let x = start_x + i as f32 * (SWATCH_SIZE + SWATCH_GAP);
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x,
                    y: center_y - SWATCH_SIZE / 2.0,
                    width: SWATCH_SIZE,
                    height: SWATCH_SIZE,
                },
                border: Border {
                    radius: (SWATCH_SIZE / 4.0).into(),
                    color: Color::BLACK.scale_alpha(0.2),
                    width: 1.0,
                },
                ..Quad::default()
            },
            Background::Color(*color),
        );
    }
}

fn draw_label(renderer: &mut Renderer, theme_to_draw: &Theme, bounds: Rectangle, color: Color) {
    use advanced::text::Renderer as _;

    renderer.fill_text(
        Text {
            content: theme_to_draw.to_string(),
            bounds: Size::new(
                bounds.width - ITEM_PADDING_H * 2.0 - SWATCHES_WIDTH - SWATCH_GAP,
                bounds.height,
            ),
            size: Pixels(TEXT_SIZE),
            line_height: text::LineHeight::default(),
            font: Font::DEFAULT,
            align_x: text::Alignment::Left,
            align_y: Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        },
        Point::new(bounds.x + ITEM_PADDING_H, bounds.y + bounds.height / 2.0),
        color,
        bounds,
    );
}

fn draw_scrollbar(renderer: &mut Renderer, bounds: Rectangle, scroll_offset: f32, theme: &Theme) {
    use advanced::Renderer as _;

    let list_h = full_list_height();
    let visible_h = bounds.height - PADDING * 2.0;

    if list_h <= visible_h {
        return;
    }

    let palette = theme.extended_palette();
    let track_x = bounds.x + bounds.width - PADDING - SCROLLBAR_WIDTH;
    let track_y = bounds.y + PADDING;

    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: track_x,
                y: track_y,
                width: SCROLLBAR_WIDTH,
                height: visible_h,
            },
            border: Border {
                radius: (SCROLLBAR_WIDTH / 2.0).into(),
                ..Border::default()
            },
            ..Quad::default()
        },
        Background::Color(palette.background.strong.color),
    );

    let thumb_h = (visible_h / list_h * visible_h).max(16.0);
    let max_offset = list_h - visible_h;
    let thumb_y = track_y + (scroll_offset / max_offset) * (visible_h - thumb_h);

    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: track_x,
                y: thumb_y,
                width: SCROLLBAR_WIDTH,
                height: thumb_h,
            },
            border: Border {
                radius: (SCROLLBAR_WIDTH / 2.0).into(),
                ..Border::default()
            },
            ..Quad::default()
        },
        Background::Color(palette.background.base.text.scale_alpha(0.4)),
    );
}

impl<Message: Clone + 'static> Widget<Message, Theme, Renderer> for ThemePicker<Message> {
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
                if state.expanded {
                    if let Some(idx) = ALL_THEMES.iter().position(|t| t == &self.selected) {
                        let visible_h = max_dropdown_height() - PADDING * 2.0;
                        let item_top = idx as f32 * ITEM_HEIGHT;
                        let item_bot = item_top + ITEM_HEIGHT;
                        if item_top < state.scroll_offset {
                            state.scroll_offset = item_top;
                        } else if item_bot > state.scroll_offset + visible_h {
                            state.scroll_offset = item_bot - visible_h;
                        }
                        state.scroll_offset = state
                            .scroll_offset
                            .clamp(0.0, max_scroll_offset_for(visible_h));
                    }
                }
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
        let is_active = cursor.is_over(bounds) || state.expanded;

        let bg_color = if is_active {
            palette.background.weak.color
        } else {
            palette.background.base.color
        };

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
            Background::Color(bg_color),
        );

        draw_label(
            renderer,
            &self.selected,
            bounds,
            palette.background.base.text,
        );
        draw_swatches(
            renderer,
            bounds.center_y(),
            bounds.x + bounds.width - ITEM_PADDING_H,
            &self.selected,
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
            selected: &self.selected,
            on_select: &self.on_select,
            button_bounds,
        })))
    }
}

impl<'a, Message: Clone + 'static> From<ThemePicker<Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(w: ThemePicker<Message>) -> Self {
        Self::new(w)
    }
}

struct DropdownOverlay<'b, Message> {
    widget_state: &'b mut tree::State,
    selected: &'b Theme,
    on_select: &'b dyn Fn(Theme) -> Message,
    button_bounds: Rectangle,
}

impl<Message: Clone> DropdownOverlay<'_, Message> {
    fn scroll_offset(&self) -> f32 {
        self.widget_state.downcast_ref::<State>().scroll_offset
    }

    fn item_bounds(dropdown_origin: Point, index: usize, scroll_offset: f32) -> Rectangle {
        Rectangle {
            x: dropdown_origin.x + PADDING,
            y: dropdown_origin.y + PADDING + index as f32 * ITEM_HEIGHT - scroll_offset,
            width: DROPDOWN_WIDTH - PADDING * 2.0 - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
            height: ITEM_HEIGHT,
        }
    }

    fn scroll_area(dropdown_bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: dropdown_bounds.x,
            y: dropdown_bounds.y + PADDING,
            width: dropdown_bounds.width - PADDING - SCROLLBAR_WIDTH,
            height: dropdown_bounds.height - PADDING * 2.0,
        }
    }
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for DropdownOverlay<'_, Message> {
    fn layout(&mut self, _renderer: &Renderer, viewport: Size) -> layout::Node {
        let ideal_h = max_dropdown_height();
        let gap = 2.0;

        let space_below =
            (viewport.height - (self.button_bounds.y + self.button_bounds.height + gap)).max(0.0);
        let space_above = (self.button_bounds.y - gap).max(0.0);

        let (y, available_h) = if space_below >= ideal_h || space_below >= space_above {
            (
                self.button_bounds.y + self.button_bounds.height + gap,
                space_below,
            )
        } else {
            let h = ideal_h.min(space_above);
            (self.button_bounds.y - gap - h, space_above)
        };

        let dropdown_h = ideal_h.min(available_h).max(ITEM_HEIGHT + PADDING * 2.0);

        let x = self
            .button_bounds
            .x
            .min(viewport.width - DROPDOWN_WIDTH)
            .max(0.0);

        layout::Node::new(Size::new(DROPDOWN_WIDTH, dropdown_h)).move_to(Point::new(x, y))
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
        let scroll_offset = self.scroll_offset();

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

        draw_scrollbar(renderer, bounds, scroll_offset, theme);

        let origin = Point::new(bounds.x, bounds.y);
        let clip_area = Self::scroll_area(bounds);

        renderer.with_layer(clip_area, |renderer| {
            for (i, t) in ALL_THEMES.iter().enumerate() {
                let item_bounds = Self::item_bounds(origin, i, scroll_offset);

                if item_bounds.y + item_bounds.height < clip_area.y
                    || item_bounds.y > clip_area.y + clip_area.height
                {
                    continue;
                }

                let is_selected = t == self.selected;
                let is_hovered = cursor.is_over(item_bounds);

                if is_selected {
                    renderer.fill_quad(
                        Quad {
                            bounds: item_bounds,
                            border: Border {
                                radius: radius().into(),
                                ..Border::default()
                            },
                            ..Quad::default()
                        },
                        Background::Color(palette.primary.weak.color),
                    );
                } else if is_hovered {
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

                let text_color = if is_selected {
                    palette.primary.base.color
                } else {
                    palette.background.base.text
                };

                draw_label(renderer, t, item_bounds, text_color);
                draw_swatches(
                    renderer,
                    item_bounds.center_y(),
                    item_bounds.x + item_bounds.width - ITEM_PADDING_H,
                    t,
                );
            }
        });
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
        let visible_h = bounds.height - PADDING * 2.0;
        let scroll_offset = self.scroll_offset();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if !cursor.is_over(bounds) && !cursor.is_over(self.button_bounds) {
                    self.widget_state.downcast_mut::<State>().expanded = false;
                    shell.request_redraw();
                    return;
                }

                let clip_area = Self::scroll_area(bounds);
                for (i, t) in ALL_THEMES.iter().enumerate() {
                    let item_bounds = Self::item_bounds(origin, i, scroll_offset);
                    if cursor.is_over(item_bounds) && cursor.is_over(clip_area) {
                        shell.publish((self.on_select)(t.clone()));
                        self.widget_state.downcast_mut::<State>().expanded = false;
                        shell.capture_event();
                        return;
                    }
                }
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / ITEM_HEIGHT,
                };
                let new_offset = (scroll_offset - lines * ITEM_HEIGHT)
                    .clamp(0.0, max_scroll_offset_for(visible_h));
                self.widget_state.downcast_mut::<State>().scroll_offset = new_offset;
                shell.capture_event();
                shell.request_redraw();
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
        let clip_area = Self::scroll_area(bounds);
        let origin = Point::new(bounds.x, bounds.y);
        let scroll_offset = self.scroll_offset();

        for (i, _) in ALL_THEMES.iter().enumerate() {
            let item_bounds = Self::item_bounds(origin, i, scroll_offset);
            if cursor.is_over(item_bounds) && cursor.is_over(clip_area) {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}
