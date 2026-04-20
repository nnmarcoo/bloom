use iced::alignment::Vertical;
use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{Column, Space, column, container, row, svg};
use iced::window::Mode;
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{
    BAR_HEIGHT, BUTTON_SIZE, PAD, bar_style, icon_button_style, panel_divider_style, svg_style,
};
use crate::ui::{svg_button, svg_button_toggle, with_tooltip};
use crate::widgets::menu::{menu_item, menu_separator, styled_menu};
use crate::widgets::menu_button::{MenuAlign, MenuButton};
use crate::widgets::scale_entry::ScaleEntry;

#[allow(clippy::too_many_arguments)]
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
            svg_button(
                include_bytes!("../../assets/icons/left.svg"),
                Message::Previous
            ),
            "Previous media",
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/right.svg"),
                Message::Next
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
            svg_button(include_bytes!("../../assets/icons/fit.svg"), Message::Fit),
            "Fit to viewport",
            Position::Top,
        ),
        with_tooltip(
            svg_button(rotation_icon, Message::RotateCw),
            [
                "Rotate view (0°)",
                "Rotate view (90°)",
                "Rotate view (180°)",
                "Rotate view (270°)"
            ][rotation],
            Position::Top,
        ),
    ]
    .spacing(2)
    .align_y(Vertical::Center);

    let right_buttons = row![
        with_tooltip(
            svg_button_toggle(
                include_bytes!("../../assets/icons/info.svg"),
                Message::ToggleInfoColumn,
                show_info,
            ),
            "Information",
            Position::Top,
        ),
        with_tooltip(
            svg_button_toggle(
                include_bytes!("../../assets/icons/pencil.svg"),
                Message::ToggleEditPanel,
                show_edit,
            ),
            "Edit",
            Position::Top,
        ),
        with_tooltip(
            svg_button_toggle(
                include_bytes!("../../assets/icons/checkerboard.svg"),
                Message::ToggleCheckerboard,
                show_checkerboard,
            ),
            "Checkerboard background",
            Position::Top,
        ),
        with_tooltip(
            svg_button(fullscreen_icon, Message::ToggleFullscreen),
            fullscreen_tooltip,
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/folder.svg"),
                Message::SelectMedia
            ),
            "Select media",
            Position::Top,
        ),
        with_tooltip(
            MenuButton::new(
                svg(Handle::from_memory(include_bytes!(
                    "../../assets/icons/kebab.svg"
                )))
                .style(svg_style)
                .width(BUTTON_SIZE)
                .height(BUTTON_SIZE),
                styled_menu(
                    column![
                        menu_item("Preferences", Message::TogglePreferences),
                        menu_separator(),
                        menu_item("Copy file path", Message::CopyPath),
                        menu_item("Export", Message::Noop),
                        menu_separator(),
                        menu_item("About", Message::Noop),
                        menu_item("Exit", Message::Exit),
                    ],
                    180
                ),
            )
            .padding(PAD)
            .style(icon_button_style)
            .align(MenuAlign::TopEnd),
            "More actions",
            Position::Top,
        ),
    ]
    .spacing(2)
    .align_y(Vertical::Center);

    let divider = container(Space::new().height(Length::Fixed(2.0)))
        .width(Length::Fill)
        .style(panel_divider_style);

    let bar_content = container(
        row![
            left_buttons,
            Space::new().width(Length::Fill),
            right_buttons
        ]
        .height(Length::Fixed(BAR_HEIGHT))
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .spacing(PAD),
    )
    .padding([0.0, PAD]);

    container(Column::new().push(divider).push(bar_content))
        .style(bar_style)
        .into()
}
