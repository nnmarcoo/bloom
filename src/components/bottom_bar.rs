use iced::alignment::Vertical;
use iced::widget::svg::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{button, column, container, row, svg, text, tooltip};
use iced::window::Mode;
use iced::{Element, Length};
use iced_aw::ContextMenu;

use crate::app::Message;
use crate::styles::{
    BAR_HEIGHT, BUTTON_SIZE, PAD, TOOLTIP_DELAY, bar_style, icon_button_active_style,
    icon_button_style, svg_style,
};
use crate::widgets::menu::{menu_item, menu_separator, styled_menu};
use crate::widgets::scale_entry::ScaleEntry;

fn bottom_bar_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    tooltip_text: &'a str,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tooltip_text).size(12))
            .padding(PAD)
            .style(container::rounded_box),
        Position::Top,
    )
    .delay(TOOLTIP_DELAY)
    .into()
}

fn icon_button<'a>(
    icon: &'static [u8],
    tooltip_text: &'a str,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let button = button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(BUTTON_SIZE))
            .height(Length::Fixed(BUTTON_SIZE)),
    )
    .padding(PAD)
    .style(icon_button_style)
    .on_press_maybe(msg);

    bottom_bar_tooltip(button, tooltip_text)
}

// This is temporary
fn lanczos_button(enabled: bool) -> Element<'static, Message> {
    let style = if enabled {
        icon_button_active_style
    } else {
        icon_button_style
    };

    let label = if enabled {
        "Lanczos quality: on"
    } else {
        "Lanczos quality: off"
    };

    bottom_bar_tooltip(
        button(text("L").size(12))
            .padding(PAD)
            .style(style)
            .on_press(Message::ToggleLanczos),
        label,
    )
}

pub fn view<'a>(
    mode: Mode,
    lanczos_enabled: bool,
    scale: f32,
    focus_scale: bool,
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
        icon_button(
            include_bytes!("../../assets/icons/left.svg"),
            "Previous",
            Some(Message::Previous)
        ),
        icon_button(
            include_bytes!("../../assets/icons/right.svg"),
            "Next",
            Some(Message::Next)
        ),
        bottom_bar_tooltip(
            ScaleEntry::new(scale, Message::Scale).focused(focus_scale),
            "Scale"
        ),
    ]
    .align_y(Vertical::Center);

    let right_buttons = row![
        icon_button(
            include_bytes!("../../assets/icons/fit.svg"),
            "Fit to viewport",
            Some(Message::Fit)
        ),
        icon_button(
            fullscreen_icon,
            fullscreen_tooltip,
            Some(Message::ToggleFullscreen)
        ),
        icon_button(
            include_bytes!("../../assets/icons/folder.svg"),
            "Select media",
            Some(Message::SelectMedia)
        ),
        lanczos_button(lanczos_enabled),
        icon_button(
            include_bytes!("../../assets/icons/kebab.svg"),
            "More actions",
            None
        ),
    ]
    .align_y(Vertical::Center);

    let bar = container(
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
    .style(bar_style);

    ContextMenu::new(bar, || {
        styled_menu(column![
            menu_item("Hide Bar", Message::Noop),
            menu_separator(),
            menu_item("Copy Path", Message::Noop),
        ])
    })
    .into()
}
