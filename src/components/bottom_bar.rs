use iced::alignment::Vertical;
use iced::widget::progress_bar;
use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{Column, Space, column, container, row, svg};
use iced::window::Mode;
use iced::{Border, Element, Length};

use crate::app::Message;
use crate::keybinds::{Action, Keymap};
use crate::styles::{
    BAR_HEIGHT, BUTTON_SIZE, PAD, bar_style, icon_button_style, panel_divider_style, svg_style,
};
use crate::ui::{svg_button, svg_button_toggle, with_tooltip, with_tooltip_key};
use crate::widgets::menu::{menu_item, menu_item_enabled, menu_separator, styled_menu};
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
    is_animation: bool,
    fit_active: bool,
    export_progress: Option<f32>,
    keymap: &Keymap,
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
        with_tooltip_key(
            svg_button(
                include_bytes!("../../assets/icons/left.svg"),
                Message::Previous
            ),
            "Previous media",
            Position::Top,
            keymap,
            Action::Previous,
        ),
        with_tooltip_key(
            svg_button(
                include_bytes!("../../assets/icons/right.svg"),
                Message::Next
            ),
            "Next media",
            Position::Top,
            keymap,
            Action::Next,
        ),
        with_tooltip_key(
            ScaleEntry::new(scale, Message::Scale).focused(focus_scale),
            "Scale",
            Position::Top,
            keymap,
            Action::FocusScale,
        ),
        with_tooltip_key(
            svg_button_toggle(
                include_bytes!("../../assets/icons/fit.svg"),
                Message::Fit,
                fit_active,
            ),
            "Fit to viewport",
            Position::Top,
            keymap,
            Action::ZoomFit,
        ),
        with_tooltip_key(
            svg_button(rotation_icon, Message::RotateCw),
            [
                "Rotate view (0°)",
                "Rotate view (90°)",
                "Rotate view (180°)",
                "Rotate view (270°)"
            ][rotation],
            Position::Top,
            keymap,
            Action::RotateCw,
        ),
    ]
    .spacing(2)
    .align_y(Vertical::Center);

    let right_buttons = row![
        with_tooltip_key(
            svg_button_toggle(
                include_bytes!("../../assets/icons/info.svg"),
                Message::ToggleInfoColumn,
                show_info,
            ),
            "Information",
            Position::Top,
            keymap,
            Action::ToggleInfoPanel,
        ),
        with_tooltip_key(
            svg_button_toggle(
                include_bytes!("../../assets/icons/pencil.svg"),
                Message::ToggleEditPanel,
                show_edit,
            ),
            "Edit",
            Position::Top,
            keymap,
            Action::ToggleEditPanel,
        ),
        with_tooltip_key(
            svg_button_toggle(
                include_bytes!("../../assets/icons/checkerboard.svg"),
                Message::ToggleCheckerboard,
                show_checkerboard,
            ),
            "Checkerboard background",
            Position::Top,
            keymap,
            Action::ToggleCheckerboard,
        ),
        with_tooltip_key(
            svg_button(fullscreen_icon, Message::ToggleFullscreen),
            fullscreen_tooltip,
            Position::Top,
            keymap,
            Action::ToggleFullscreen,
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
                        menu_item_enabled("Export", Message::ExportImage, has_image),
                        menu_item_enabled("Export frame", Message::ExportFrame, is_animation),
                    ]
                    .push(menu_separator())
                    .push(menu_item("About", Message::OpenAbout))
                    .push(menu_item("Exit", Message::Exit)),
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

    let top: Element<'a, Message> = if let Some(p) = export_progress {
        container(
            progress_bar(0.0..=1.0, p).style(|theme: &iced::Theme| progress_bar::Style {
                background: theme.extended_palette().background.strong.color.into(),
                bar: theme.extended_palette().primary.base.color.into(),
                border: Border::default(),
            }),
        )
        .height(Length::Fixed(2.0))
        .width(Length::Fill)
        .clip(true)
        .into()
    } else {
        container(Space::new().height(Length::Fixed(2.0)))
            .width(Length::Fill)
            .style(panel_divider_style)
            .into()
    };

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

    container(Column::new().push(top).push(bar_content))
        .style(bar_style)
        .into()
}
