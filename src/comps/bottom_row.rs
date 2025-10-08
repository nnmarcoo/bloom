use iced::{
    Alignment::Center,
    Color, Element,
    Length::Fill,
    Theme,
    widget::{container, row, text},
};

use crate::app::Message;

const FONT_SIZE: f32 = 14.;
const PADDING: f32 = 3.;
const SPACING: f32 = 5.;

pub fn bottom_row<'a>() -> Element<'a, Message> {
    container(
        row![text("hi").width(Fill).size(FONT_SIZE),]
            .align_y(Center)
            .spacing(SPACING)
            .padding(PADDING),
    )
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();
        //container::background(Color::from_rgb(0., 255., 0.))
        container::background(palette.secondary.base.color).color(palette.secondary.base.text)
    })
    .into()
}
