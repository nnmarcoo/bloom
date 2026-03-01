use std::path::Path;

use iced::{
    Center, Element, Length,
    widget::{column, container, row, shader, stack, text},
};
use iced_aw::ContextMenu;

use crate::{
    app::Message,
    components::info_column,
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
    path: Option<&'a Path>,
    gallery: &'a Gallery,
) -> Element<'a, Message> {
    let base = shader(program.clone())
        .height(Length::Fill)
        .width(Length::Fill);

    let viewer = if let Some(filename) = loading {
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

    let content: Element<'a, Message> = if show_info {
        row![info_column::view(path, gallery, &program), viewer,]
            .height(Length::Fill)
            .into()
    } else {
        viewer
    };

    ContextMenu::new(content, || {
        styled_menu(column![
            menu_item("Copy Color", Message::CopyColor),
            menu_separator(),
            menu_item("Fit", Message::Fit),
        ])
    })
    .into()
}
