use glam::Vec2;
use iced::{
    Alignment::Center,
    Element,
    Length::Fill,
    Theme,
    widget::{button, container, row, text},
};

use crate::app::Message;

const FONT_SIZE: f32 = 14.;
const PADDING: f32 = 3.;
const SPACING: f32 = 5.;

pub fn bottom_row<'a>(pos: Vec2) -> Element<'a, Message> {
    container(
        row![
            text("hi").width(Fill).size(FONT_SIZE),
            button(text(format!("x: {:.2}, y: {:.2}", pos.x, pos.y))).on_press(Message::SetImage)
        ]
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
