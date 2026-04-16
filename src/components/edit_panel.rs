use iced::alignment::{Horizontal, Vertical};
use iced::widget::tooltip::Position;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{
    EDIT_PANEL_WIDTH, PAD, bar_style, modifier_card_style, panel_divider_style,
    plain_icon_button_style,
};
use crate::ui::{svg_button_plain, with_tooltip};

pub fn view<'a>() -> Element<'a, Message> {
    let tool_strip = container(
        column![
            with_tooltip(
                svg_button_plain(
                    include_bytes!("../../assets/icons/cursor.svg"),
                    Message::Noop
                ),
                "Select",
                Position::Left,
            ),
            with_tooltip(
                svg_button_plain(include_bytes!("../../assets/icons/crop.svg"), Message::Noop),
                "Crop",
                Position::Left,
            ),
            with_tooltip(
                svg_button_plain(
                    include_bytes!("../../assets/icons/pencil.svg"),
                    Message::Noop
                ),
                "Draw",
                Position::Left,
            ),
            with_tooltip(
                svg_button_plain(include_bytes!("../../assets/icons/text.svg"), Message::Noop),
                "Text",
                Position::Left,
            ),
        ]
        .spacing(2),
    )
    .padding(PAD)
    .width(Length::Shrink)
    .height(Length::Fill);

    let divider = container(Space::new().width(Length::Fixed(1.0)))
        .height(Length::Fill)
        .style(panel_divider_style);

    let modifier_stack = container(
        column![
            modifier_entry("Levels"),
            modifier_entry("Mosaic"),
            Space::new().height(Length::Fill),
            add_effect_row(),
        ]
        .spacing(2)
        .padding(PAD),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    container(row![tool_strip, divider, modifier_stack].height(Length::Fill))
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(EDIT_PANEL_WIDTH))
        .into()
}

fn modifier_entry<'a>(name: &'a str) -> Element<'a, Message> {
    container(
        row![
            text("▶").size(10),
            text(name).size(11),
            Space::new().width(Length::Fill),
            svg_button_plain(
                include_bytes!("../../assets/icons/close.svg"),
                Message::Noop,
            ),
        ]
        .align_y(Vertical::Center)
        .spacing(PAD),
    )
    .style(modifier_card_style)
    .padding([2.0, PAD])
    .width(Length::Fill)
    .into()
}

fn add_effect_row<'a>() -> Element<'a, Message> {
    button(
        text("+ Add Effect")
            .size(11)
            .align_x(Horizontal::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(PAD)
    .style(plain_icon_button_style)
    .on_press(Message::Noop)
    .into()
}
