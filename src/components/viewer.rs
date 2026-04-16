use std::collections::HashSet;
use std::path::Path;

use iced::{
    Center, Element, Length, Theme,
    widget::{column, container, row, shader, stack, text},
};
use iced_aw::ContextMenu;

use crate::{
    app::{Message, Tool},
    components::notifications::NotificationEntry,
    components::{edit_panel, info_panel, notifications},
    gallery::Gallery,
    styles::{PAD, spinner_bg_style},
    wgpu::view_program::ViewProgram,
    widgets::{
        loading_spinner::Circular,
        menu::{menu_item, menu_separator, styled_menu},
    },
};

pub fn view<'a>(
    program: ViewProgram,
    loading: Option<&'a str>,
    show_info: bool,
    show_edit: bool,
    path: Option<&'a Path>,
    gallery: &'a Gallery,
    theme: &'a Theme,
    info_collapsed: &'a HashSet<String>,
    notifs: &'a [NotificationEntry],
    pixel_preview_size: u32,
    selected_tool: &'a Tool,
) -> Element<'a, Message> {
    let base = shader(program.clone())
        .height(Length::Fill)
        .width(Length::Fill);

    let notif_overlay = notifications::view(notifs);

    let viewer: Element<'a, Message> = if let Some(filename) = loading {
        let spinner_overlay = container(
            container(
                column![
                    Circular::<iced::Theme>::new().size(36.0).bar_height(4.0),
                    text(filename).size(12),
                ]
                .spacing(PAD * 2.0)
                .align_x(Center),
            )
            .padding(PAD * 3.0)
            .style(spinner_bg_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Center)
        .align_y(Center);

        stack![base, spinner_overlay, notif_overlay]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    } else {
        stack![base, notif_overlay]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    let viewer_with_menu: Element<'a, Message> = ContextMenu::new(viewer, || {
        styled_menu(column![
            menu_item("Copy Color", Message::CopyColor),
            menu_item("Copy File Path", Message::CopyPath),
            menu_separator(),
            menu_item("Fit", Message::Fit),
        ])
    })
    .into();

    match (show_info, show_edit) {
        (true, true) => row![
            info_panel::view(
                path,
                gallery,
                &program,
                theme,
                info_collapsed,
                pixel_preview_size
            ),
            viewer_with_menu,
            edit_panel::view(selected_tool),
        ]
        .height(Length::Fill)
        .into(),
        (true, false) => row![
            info_panel::view(
                path,
                gallery,
                &program,
                theme,
                info_collapsed,
                pixel_preview_size
            ),
            viewer_with_menu,
        ]
        .height(Length::Fill)
        .into(),
        (false, true) => row![viewer_with_menu, edit_panel::view(selected_tool)]
            .height(Length::Fill)
            .into(),
        (false, false) => viewer_with_menu,
    }
}
