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

pub fn bottom_row<'a>(pan: Vec2, scale: f32) -> Element<'a, Message> {
    let zoom_pct = scale * 100.0;

    container(
        row![
            text(format!("{zoom_pct:.0}%")).size(FONT_SIZE),
            text(format!("x: {:.1}  y: {:.1}", pan.x, pan.y))
                .width(Fill)
                .size(FONT_SIZE),
            button(text("Open").size(FONT_SIZE)).on_press(Message::SetImage)
        ]
        .align_y(Center)
        .spacing(SPACING)
        .padding(PADDING),
    )
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();
        container::background(palette.secondary.base.color).color(palette.secondary.base.text)
    })
    .into()
}
