use iced::alignment::Vertical;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Renderer, Theme};

use crate::config::ALL_THEMES;
use crate::styles::radius;
use crate::widgets::menu_button::{MenuAlign, MenuButton};

const ROW_HEIGHT: f32 = 28.0;
const ITEM_PADDING_H: f32 = 8.0;
const SWATCH_SIZE: f32 = 12.0;
const SWATCH_GAP: f32 = 3.0;
const TEXT_SIZE: f32 = 13.0;
const DROPDOWN_WIDTH: f32 = 220.0;
const MAX_VISIBLE_ITEMS: usize = 12;
const PADDING: f32 = 6.0;
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_GUTTER: f32 = 4.0;

fn max_dropdown_height() -> f32 {
    ROW_HEIGHT * MAX_VISIBLE_ITEMS as f32 + PADDING * 2.0
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

impl<'a, Message: Clone + 'a> From<ThemePicker<Message>> for Element<'a, Message, Theme, Renderer> {
    fn from(picker: ThemePicker<Message>) -> Self {
        MenuButton::new(
            theme_trigger(&picker.selected),
            theme_list(picker.selected, picker.on_select),
        )
        .width(picker.width)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(button_style)
        .align(MenuAlign::BottomStart)
        .into()
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

fn swatches<'a, Message: 'a>(theme: &Theme) -> Element<'a, Message> {
    let p = theme.extended_palette();
    row![
        swatch(p.background.base.color),
        swatch(p.primary.base.color),
        swatch(p.background.strong.color),
    ]
    .align_y(Vertical::Center)
    .spacing(SWATCH_GAP)
    .into()
}

fn theme_trigger<'a, Message: 'a>(selected: &Theme) -> Element<'a, Message> {
    row![
        text(selected.to_string()).size(TEXT_SIZE),
        Space::new().width(Length::Fill),
        swatches(selected),
    ]
    .align_y(Vertical::Center)
    .spacing(SWATCH_GAP)
    .padding([0.0, ITEM_PADDING_H])
    .into()
}

fn theme_list<'a, Message: Clone + 'a>(
    selected: Theme,
    on_select: Box<dyn Fn(Theme) -> Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let mut col = column![].width(Length::Fill);

    for t in ALL_THEMES.iter() {
        let is_selected = *t == selected;
        let theme_for_row = t.clone();
        let label = t.to_string();

        let item = button(
            row![
                text(label).size(TEXT_SIZE),
                Space::new().width(Length::Fill),
                swatches(t),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .spacing(SWATCH_GAP)
            .padding([0.0, ITEM_PADDING_H]),
        )
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .padding(0.0)
        .style(move |theme, status| item_style(theme, status, is_selected))
        .on_press((on_select)(theme_for_row));

        col = col.push(item);
    }

    let visible_h = max_dropdown_height() - PADDING * 2.0;
    let gutter = SCROLLBAR_WIDTH + SCROLLBAR_GUTTER;

    container(
        scrollable(col.padding(iced::Padding::ZERO.right(gutter)))
            .width(Length::Fill)
            .height(Length::Shrink)
            .direction(Direction::Vertical(
                Scrollbar::new()
                    .width(SCROLLBAR_WIDTH)
                    .scroller_width(SCROLLBAR_WIDTH)
                    .margin(0.0),
            )),
    )
    .width(Length::Fixed(DROPDOWN_WIDTH))
    .max_height(visible_h)
    .padding(iced::Padding::new(PADDING).right(0.0))
    .style(list_container_style)
    .into()
}

fn list_container_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: radius().into(),
        },
        ..container::Style::default()
    }
}

fn item_style(theme: &Theme, status: button::Status, is_selected: bool) -> button::Style {
    let palette = theme.extended_palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    let background = if is_selected {
        Some(Background::Color(palette.primary.weak.color))
    } else if hovered {
        Some(Background::Color(palette.background.strong.color))
    } else {
        None
    };

    let text_color = if is_selected {
        palette.primary.base.color
    } else {
        palette.background.base.text
    };

    button::Style {
        background,
        text_color,
        border: Border {
            radius: radius().into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
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
