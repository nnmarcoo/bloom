use iced::alignment::Vertical;
use iced::widget::{container, row};
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{BAR_HEIGHT, PAD, bar_style};

pub fn view<'a>() -> Element<'a, Message> {
    container(
        row![]
            .spacing(PAD)
            .align_y(Vertical::Center)
            .height(Length::Fixed(BAR_HEIGHT))
            .width(Length::Fill),
    )
    .padding([0.0, PAD])
    .style(bar_style)
    .into()
}
