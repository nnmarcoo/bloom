use std::time::Duration;

use iced::alignment::Vertical;
use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{button, container, row, svg, text, tooltip};
use iced::{Element, Length};

use crate::app::Message;
use crate::keybinds::{Action, Keymap};
use crate::styles::{
    BUTTON_SIZE, PAD, TOOLTIP_DELAY, icon_button_active_style, icon_button_style,
    key_chip_container_style, plain_icon_button_style, svg_style, tooltip_style,
};

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    let secs = ms / 1000;
    let mins = secs / 60;
    let secs = secs % 60;
    let rem = ms % 1000;
    format!("{mins}:{secs:02}.{rem:03}")
}

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
    label: impl ToString,
    position: Position,
) -> Element<'a, Message> {
    with_tooltip_delay(content, label, position, TOOLTIP_DELAY)
}

pub fn with_tooltip_delay<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl ToString,
    position: Position,
    delay: Duration,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label.to_string()).size(12))
            .padding(PAD)
            .style(tooltip_style),
        position,
    )
    .delay(delay)
    .into()
}

pub fn with_tooltip_key<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl ToString,
    position: Position,
    keymap: &Keymap,
    action: Action,
) -> Element<'a, Message> {
    let chip = keymap.binding_for(&action).map(|kb| {
        container(text(kb.display_pretty()).size(11))
            .padding([2.0, 5.0])
            .style(key_chip_container_style)
    });

    let body: Element<'a, Message> = match chip {
        Some(chip) => row![text(label.to_string()).size(12), chip]
            .spacing(PAD * 1.5)
            .align_y(Vertical::Center)
            .into(),
        None => text(label.to_string()).size(12).into(),
    };

    tooltip(
        content,
        container(body).padding(PAD).style(tooltip_style),
        position,
    )
    .delay(TOOLTIP_DELAY)
    .into()
}

pub fn svg_button_toggle<'a>(
    icon: &'static [u8],
    msg: Message,
    active: bool,
) -> Element<'a, Message> {
    if active {
        svg_button_active(icon, msg)
    } else {
        svg_button(icon, msg)
    }
}
