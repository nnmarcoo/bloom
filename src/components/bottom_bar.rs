use iced::alignment::Vertical;
use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{button, container, row, svg, text, tooltip};
use iced::window::Mode;
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{
    BAR_HEIGHT, BUTTON_SIZE, PAD, TOOLTIP_DELAY, bar_style, icon_button_active_style,
    icon_button_style, svg_style,
};
use crate::widgets::loading_spinner::Circular;

pub fn view<'a>(
    mode: Mode,
    loading: Option<&'a str>,
    lanczos_enabled: bool,
) -> Element<'a, Message> {
    let is_fullscreen = matches!(mode, Mode::Fullscreen);
    let (fullscreen_icon, fullscreen_tooltip) = if is_fullscreen {
        (
            include_bytes!("../../assets/icons/restore.svg").as_slice(),
            "Restore",
        )
    } else {
        (
            include_bytes!("../../assets/icons/fullscreen.svg").as_slice(),
            "Fullscreen",
        )
    };

    let loading_indicator: Element<'a, Message> = if let Some(filename) = loading {
        row![
            Circular::new().size(BUTTON_SIZE).bar_height(3.0),
            text(filename).size(12)
        ]
        .spacing(PAD)
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .into()
    } else {
        row![].width(Length::Fill).into()
    };

    let buttons = row![
        tooltip(
            button(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/left.svg"
                )))
                .style(svg_style)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style)
            .on_press(Message::Previous),
            container(text("Previous").size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/right.svg"
                )))
                .style(svg_style)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style)
            .on_press(Message::Next),
            container(text("Next").size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/fit.svg"
                )))
                .style(svg_style)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style)
            .on_press(Message::Fit),
            container(text("Fit to viewport").size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(
                svg(Handle::from_memory(fullscreen_icon))
                    .style(svg_style)
                    .width(Length::Fixed(BUTTON_SIZE))
                    .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style)
            .on_press(Message::ToggleFullscreen),
            container(text(fullscreen_tooltip).size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/folder.svg"
                )))
                .style(svg_style)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style)
            .on_press(Message::SelectMedia),
            container(text("Select media").size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(text("L").size(12))
                .padding(PAD)
                .style(if lanczos_enabled {
                    icon_button_active_style
                } else {
                    icon_button_style
                })
                .on_press(Message::ToggleLanczos),
            container(
                text(if lanczos_enabled {
                    "Lanczos quality: on"
                } else {
                    "Lanczos quality: off"
                })
                .size(12)
            )
            .padding(PAD)
            .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
        tooltip(
            button(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/kebab.svg"
                )))
                .style(svg_style)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(Length::Fixed(BUTTON_SIZE))
            )
            .padding(PAD)
            .style(icon_button_style),
            container(text("More actions").size(12))
                .padding(PAD)
                .style(container::rounded_box),
            Position::Top
        )
        .delay(TOOLTIP_DELAY),
    ]
    .align_y(Vertical::Center);

    container(
        row![loading_indicator, buttons]
            .height(Length::Fixed(BAR_HEIGHT))
            .width(Length::Fill)
            .align_y(Vertical::Center),
    )
    .style(bar_style)
    .into()
}
