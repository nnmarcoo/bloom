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
    rotation: u8,
    focus_scale: bool,
    show_info: bool,
    show_edit: bool,
    show_checkerboard: bool,
    has_image: bool,
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

    let rotation_icon: &'static [u8] = if !has_image {
        include_bytes!("../../assets/icons/rotate.svg")
    } else if rotation == 0 {
        include_bytes!("../../assets/icons/rotate0.svg")
    } else if rotation == 1 {
        include_bytes!("../../assets/icons/rotate90.svg")
    } else if rotation == 2 {
        include_bytes!("../../assets/icons/rotate180.svg")
    } else {
        include_bytes!("../../assets/icons/rotate270.svg")
    };

    let rotation = rotation as usize % 4;

    let left_buttons = row![
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/left.svg"),
                Some(Message::Previous)
            ),
            "Previous media",
            Position::Top,
        ),
        with_tooltip(
            svg_button_maybe(
                include_bytes!("../../assets/icons/right.svg"),
                Some(Message::Next)
            ),
            "Next media",
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
            svg_button_maybe(rotation_icon, Some(Message::Rotate)),
            [
                "Rotate view (0°)",
                "Rotate view (90°)",
                "Rotate view (180°)",
                "Rotate view (270°)"
            ][rotation],
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
            if show_edit {
                svg_button_active(
                    include_bytes!("../../assets/icons/edit.svg"),
                    Message::ToggleEditPanel,
                )
            } else {
                svg_button(
                    include_bytes!("../../assets/icons/edit.svg"),
                    Message::ToggleEditPanel,
                )
            },
            "Edit",
            Position::Top,
        ),
        with_tooltip(
            if show_checkerboard {
                svg_button_active(
                    include_bytes!("../../assets/icons/checkerboard.svg"),
                    Message::ToggleCheckerboard,
                )
            } else {
                svg_button(
                    include_bytes!("../../assets/icons/checkerboard.svg"),
                    Message::ToggleCheckerboard,
                )
            },
            "Checkerboard background",
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
                    menu_item("Copy file path", Message::CopyPath),
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
