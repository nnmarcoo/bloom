//! A dropdown over a small fixed set of options.
//!
//! `ThemePicker` and `FontPicker` are dropdowns bound to one specific list.
//! This is the same shape made generic, so a modifier with an enum parameter
//! gets a real dropdown instead of a slider pretending to be one.

use iced::alignment::Vertical;
use iced::widget::svg::Handle;
use iced::widget::{Space, button, column, container, row, svg, text};
use iced::{Background, Border, Element, Length, Renderer, Theme};

use crate::styles::radius;
use crate::widgets::menu_button::{MenuAlign, MenuButton};

const TRIGGER_H: f32 = 22.0;
const ROW_HEIGHT: f32 = 24.0;
const ITEM_PADDING_H: f32 = 8.0;
const TEXT_SIZE: f32 = 11.0;
const PADDING: f32 = 4.0;
const CARET_SIZE: f32 = 12.0;

pub struct OptionPicker<'a, T, Message> {
    options: &'a [(T, &'a str)],
    selected: T,
    on_select: Box<dyn Fn(T) -> Message + 'a>,
    width: Length,
}

impl<'a, T: Copy + PartialEq + 'a, Message> OptionPicker<'a, T, Message> {
    pub fn new(
        options: &'a [(T, &'a str)],
        selected: T,
        on_select: impl Fn(T) -> Message + 'a,
    ) -> Self {
        Self {
            options,
            selected,
            on_select: Box::new(on_select),
            width: Length::Fill,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    fn label_of(&self) -> &'a str {
        self.options
            .iter()
            .find(|(v, _)| *v == self.selected)
            .map(|(_, name)| *name)
            .unwrap_or("")
    }
}

impl<'a, T: Copy + PartialEq + 'a, Message: Clone + 'a> From<OptionPicker<'a, T, Message>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(picker: OptionPicker<'a, T, Message>) -> Self {
        let trigger: Element<'a, Message> = row![
            text(picker.label_of()).size(TEXT_SIZE),
            Space::new().width(Length::Fill),
            svg(Handle::from_memory(include_bytes!(
                "../../assets/icons/down.svg"
            )))
            .style(crate::styles::svg_style)
            .width(CARET_SIZE)
            .height(CARET_SIZE),
        ]
        .align_y(Vertical::Center)
        .spacing(4)
        .padding([0.0, ITEM_PADDING_H])
        .into();

        let mut col: iced::widget::Column<'a, Message, Theme, Renderer> =
            column![].width(Length::Fill);
        for (value, name) in picker.options {
            let (value, name) = (*value, *name);
            let is_selected = value == picker.selected;
            let msg = (picker.on_select)(value);
            col = col.push(
                button(
                    row![text(name).size(TEXT_SIZE)]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_y(Vertical::Center)
                        .padding([0.0, ITEM_PADDING_H]),
                )
                .width(Length::Fill)
                .height(Length::Fixed(ROW_HEIGHT))
                .padding(0.0)
                .style(move |theme, status| item_style(theme, status, is_selected))
                .on_press(msg),
            );
        }

        let list: Element<'a, Message, Theme, Renderer> = container(col)
            .width(Length::Fixed(120.0))
            .padding(PADDING)
            .style(list_container_style)
            .into();

        MenuButton::new(trigger, list)
            .width(picker.width)
            .height(Length::Fixed(TRIGGER_H))
            .style(trigger_style)
            .align(MenuAlign::BottomStart)
            .into()
    }
}

fn trigger_style(theme: &Theme, status: button::Status) -> button::Style {
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

fn item_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let palette = theme.extended_palette();
    let background = if selected {
        Some(Background::Color(palette.primary.base.color))
    } else {
        match status {
            button::Status::Hovered => Some(Background::Color(palette.background.strong.color)),
            _ => None,
        }
    };
    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: if selected {
            palette.primary.base.text
        } else {
            palette.background.base.text
        },
        ..Default::default()
    }
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
