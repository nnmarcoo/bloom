use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{button, container, svg, text, tooltip};
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{
    BUTTON_SIZE, PAD, icon_button_active_style, icon_button_style, plain_icon_button_style,
    svg_style, tooltip_style,
};

pub fn svg_button<'a>(icon: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(BUTTON_SIZE))
            .height(Length::Fixed(BUTTON_SIZE)),
    )
    .padding(PAD)
    .style(icon_button_style)
    .on_press(msg)
    .into()
}

pub fn svg_button_active<'a>(icon: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(BUTTON_SIZE))
            .height(Length::Fixed(BUTTON_SIZE)),
    )
    .padding(PAD)
    .style(icon_button_active_style)
    .on_press(msg)
    .into()
}

pub fn svg_button_plain<'a>(icon: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(BUTTON_SIZE))
            .height(Length::Fixed(BUTTON_SIZE)),
    )
    .padding(PAD)
    .style(plain_icon_button_style)
    .on_press(msg)
    .into()
}

pub fn with_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    position: Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label).size(12))
            .padding(PAD)
            .style(tooltip_style),
        position,
    )
    .delay(crate::styles::TOOLTIP_DELAY)
    .into()
}

pub fn svg_button_maybe<'a>(icon: &'static [u8], msg: Option<Message>) -> Element<'a, Message> {
    button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(BUTTON_SIZE))
            .height(Length::Fixed(BUTTON_SIZE)),
    )
    .padding(PAD)
    .style(icon_button_style)
    .on_press_maybe(msg)
    .into()
}
