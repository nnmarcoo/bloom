use std::collections::HashSet;
use std::path::Path;

use iced::{
    Center, Element, Length, Theme,
    widget::{column, container, row, shader, stack, text},
};
use iced_aw::ContextMenu;

use crate::{
    app::Message,
    components::{edit_column, info_column, tool_bar},
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
) -> Element<'a, Message> {
    let base = shader(program.clone())
        .height(Length::Fill)
        .width(Length::Fill);

    let viewer: Element<'a, Message> = if let Some(filename) = loading {
        let overlay = container(
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

        stack![base, overlay]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    } else {
        base.into()
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

    let center: Element<'a, Message> = if show_edit {
        column![tool_bar::view(), viewer_with_menu]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    } else {
        viewer_with_menu
    };

    match (show_info, show_edit) {
        (true, true) => row![
            info_column::view(path, gallery, &program, theme, info_collapsed),
            center,
            edit_column::view(),
        ]
        .height(Length::Fill)
        .into(),
        (true, false) => row![
            info_column::view(path, gallery, &program, theme, info_collapsed),
            center,
        ]
        .height(Length::Fill)
        .into(),
        (false, true) => row![center, edit_column::view()]
            .height(Length::Fill)
            .into(),
        (false, false) => center,
    }
}
