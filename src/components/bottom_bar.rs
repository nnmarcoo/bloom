use iced::alignment::Vertical;
use iced::widget::container;
use iced::widget::tooltip::Position;
use iced::widget::{column, row};
use iced::window::Mode;
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{BAR_HEIGHT, PAD, bar_style};
use crate::ui::{svg_button, svg_button_active, svg_button_maybe, with_tooltip};
use crate::widgets::menu::{menu_item, menu_separator, styled_menu};
use crate::widgets::menu_button::{MenuAlign, MenuButton};
use crate::widgets::scale_entry::ScaleEntry;

pub fn view<'a>(
    mode: Mode,
    scale: f32,
    focus_scale: bool,
    show_info: bool,
) -> Element<'a, Message> {
    let is_fullscreen = matches!(mode, Mode::Fullscreen);
    let (fullscreen_icon, fullscreen_tooltip): (&'static [u8], &str) = if is_fullscreen {
        (include_bytes!("../../assets/icons/restore.svg"), "Restore")
    } else {
        (
            include_bytes!("../../assets/icons/fullscreen.svg"),
            "Fullscreen",
        )
    };

    let left_buttons = row![
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/left.svg"),
                Some(Message::Previous)
            ),
            "Previous",
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/right.svg"),
                Some(Message::Next)
            ),
            "Next",
            Position::Top,
        ),
        with_tooltip(
            ScaleEntry::new(scale, Message::Scale).focused(focus_scale),
            "Scale",
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/fit.svg"),
                Some(Message::Fit)
            ),
            "Fit to viewport",
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/rotate.svg"),
                Some(Message::Noop)
            ),
            "Rotate view",
            Position::Top,
        ),
    ]
    .align_y(Vertical::Center);

    let info_btn = if show_info {
        svg_button_active(
            include_bytes!("../../assets/icons/info.svg"),
            Message::ToggleInfoColumn,
        )
    } else {
        svg_button(
            include_bytes!("../../assets/icons/info.svg"),
            Message::ToggleInfoColumn,
        )
    };

    let right_buttons = row![
        with_tooltip(info_btn, "Information", Position::Top),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/edit.svg"),
                Some(Message::Noop)
            ),
            "Edit",
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(fullscreen_icon, Some(Message::ToggleFullscreen)),
            fullscreen_tooltip,
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/folder.svg"),
                Some(Message::SelectMedia)
            ),
            "Select media",
            Position::Top,
        ),
        with_tooltip(
            MenuButton::new(
                include_bytes!("../../assets/icons/kebab.svg"),
                styled_menu(column![
                    menu_item("Preferences", Message::TogglePreferences),
                    menu_separator(),
                    menu_item("Export", Message::Noop),
                    menu_separator(),
                    menu_item("About", Message::Noop),
                    menu_item("Exit", Message::Exit),
                ]),
            )
            .align(MenuAlign::TopEnd),
            "More actions",
            Position::Top,
        ),
    ]
    .align_y(Vertical::Center);

    container(
        row![
            left_buttons,
            iced::widget::Space::new().width(Length::Fill),
            right_buttons
        ]
        .height(Length::Fixed(BAR_HEIGHT))
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .spacing(PAD),
    )
    .padding([0.0, PAD])
    .style(bar_style)
    .into()
}
