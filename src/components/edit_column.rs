use iced::alignment::{Horizontal, Vertical};
use iced::widget::{column, container, text};
use iced::{Element, Font, Length};

use crate::app::Message;
use crate::styles::{PAD, bar_style};

pub fn view<'a>() -> Element<'a, Message> {
    let edit_stack = container(
        column![
            text("Edit Stack")
                .size(11)
                .font(Font::MONOSPACE)
                .color(iced::Color::from_rgba(0.5, 0.5, 0.5, 1.0)),
        ]
        .spacing(PAD),
    )
    .padding(PAD * 2.0)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center);

    container(edit_stack)
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(220.0))
        .into()
}
