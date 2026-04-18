use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{Alignment, LineHeight, Shaping, Text, Wrapping};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::button;
use iced::widget::{Space, container, row, text};
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size, Theme,
};

use crate::config::ALL_THEMES;
use crate::styles::radius;
use crate::widgets::menu_button::{MenuAlign, MenuButton};

const ROW_HEIGHT: f32 = 28.0;
const ITEM_PADDING_H: f32 = 8.0;
const SWATCH_SIZE: f32 = 12.0;
const SWATCH_GAP: f32 = 3.0;
const SWATCHES_WIDTH: f32 = SWATCH_SIZE * 3.0 + SWATCH_GAP * 2.0;
const TEXT_SIZE: f32 = 13.0;
const DROPDOWN_WIDTH: f32 = 220.0;
const MAX_VISIBLE_ITEMS: usize = 12;
const PADDING: f32 = 6.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_MARGIN: f32 = 3.0;

fn max_dropdown_height() -> f32 {
    ROW_HEIGHT * MAX_VISIBLE_ITEMS as f32 + PADDING * 2.0
}

fn full_list_height() -> f32 {
    ROW_HEIGHT * ALL_THEMES.len() as f32
}

fn max_scroll_offset_for(visible_h: f32) -> f32 {
    (full_list_height() - visible_h).max(0.0)
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

impl<'a, Message: Clone + 'static> From<ThemePicker<Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(picker: ThemePicker<Message>) -> Self {
        let initial_scroll = initial_scroll_for(&picker.selected);
        MenuButton::new(
            theme_trigger(&picker.selected),
            Element::new(ThemeList::new(
                picker.selected,
                picker.on_select,
                initial_scroll,
            )),
        )
        .width(picker.width)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(button_style)
        .align(MenuAlign::BottomStart)
        .into()
    }
}

fn initial_scroll_for(selected: &Theme) -> f32 {
    let Some(idx) = ALL_THEMES.iter().position(|t| t == selected) else {
        return 0.0;
    };
    let visible_h = max_dropdown_height() - PADDING * 2.0;
    let item_bot = (idx + 1) as f32 * ROW_HEIGHT;
    if item_bot > visible_h {
        (item_bot - visible_h).clamp(0.0, max_scroll_offset_for(visible_h))
    } else {
        0.0
    }
}

fn swatch<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    container(Space::new().width(SWATCH_SIZE).height(SWATCH_SIZE))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: (SWATCH_SIZE / 4.0).into(),
                color: Color::BLACK.scale_alpha(0.2),
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

fn theme_trigger<'a, Message: 'a>(selected: &Theme) -> Element<'a, Message> {
    let p = selected.extended_palette();
    row![
        text(selected.to_string()).size(TEXT_SIZE),
        Space::new().width(Length::Fill),
        swatch(p.background.base.color),
        swatch(p.primary.base.color),
        swatch(p.background.strong.color),
    ]
    .align_y(Vertical::Center)
    .spacing(SWATCH_GAP)
    .padding([0.0, ITEM_PADDING_H])
    .into()
}

fn button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Background::Color(palette.background.weak.color))
        }
        _ => Some(Background::Color(palette.background.base.color)),
    };
    button::Style {
        background,
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: radius().into(),
        },
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

#[derive(Default)]
struct ThemeListState {
    scroll_offset: f32,
}

struct ThemeList<Message> {
    selected: Theme,
    on_select: Box<dyn Fn(Theme) -> Message>,
    initial_scroll: f32,
}

impl<Message> ThemeList<Message> {
    fn new(
        selected: Theme,
        on_select: impl Fn(Theme) -> Message + 'static,
        initial_scroll: f32,
    ) -> Self {
        Self {
            selected,
            on_select: Box::new(on_select),
            initial_scroll,
        }
    }

    fn item_bounds(origin: Point, index: usize, scroll_offset: f32) -> Rectangle {
        Rectangle {
            x: origin.x + PADDING,
            y: origin.y + PADDING + index as f32 * ROW_HEIGHT - scroll_offset,
            width: DROPDOWN_WIDTH - PADDING * 2.0 - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
            height: ROW_HEIGHT,
        }
    }

    fn scroll_area(bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x,
            y: bounds.y + PADDING,
            width: bounds.width - PADDING - SCROLLBAR_WIDTH,
            height: bounds.height - PADDING * 2.0,
        }
    }
}

impl<Message: Clone + 'static> Widget<Message, Theme, Renderer> for ThemeList<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ThemeListState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ThemeListState {
            scroll_offset: self.initial_scroll,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(DROPDOWN_WIDTH),
            height: Length::Fixed(max_dropdown_height()),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, DROPDOWN_WIDTH, max_dropdown_height())
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
        let state = tree.state.downcast_mut::<ThemeListState>();
        let bounds = layout.bounds();
        let origin = Point::new(bounds.x, bounds.y);
        let visible_h = bounds.height - PADDING * 2.0;

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let clip_area = Self::scroll_area(bounds);
                for (i, t) in ALL_THEMES.iter().enumerate() {
                    let item_bounds = Self::item_bounds(origin, i, state.scroll_offset);
                    if cursor.is_over(item_bounds) && cursor.is_over(clip_area) {
                        shell.publish((self.on_select)(t.clone()));
                        shell.capture_event();
                        return;
                    }
                }
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / ROW_HEIGHT,
                };
                state.scroll_offset = (state.scroll_offset - lines * ROW_HEIGHT)
                    .clamp(0.0, max_scroll_offset_for(visible_h));
                shell.capture_event();
                shell.request_redraw();
            }

            Event::Mouse(mouse::Event::CursorMoved { .. }) if cursor.is_over(bounds) => {
                shell.request_redraw();
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
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<ThemeListState>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        let scroll_offset = state.scroll_offset;

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

                let is_selected = t == &self.selected;
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

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::default();
        }

        let state = tree.state.downcast_ref::<ThemeListState>();
        let bounds = layout.bounds();
        let clip_area = Self::scroll_area(bounds);
        let origin = Point::new(bounds.x, bounds.y);

        for (i, _) in ALL_THEMES.iter().enumerate() {
            if cursor.is_over(Self::item_bounds(origin, i, state.scroll_offset))
                && cursor.is_over(clip_area)
            {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
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
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: start_x + i as f32 * (SWATCH_SIZE + SWATCH_GAP),
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
            line_height: LineHeight::default(),
            font: Font::DEFAULT,
            align_x: Alignment::Left,
            align_y: Vertical::Center,
            shaping: Shaping::Basic,
            wrapping: Wrapping::None,
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
