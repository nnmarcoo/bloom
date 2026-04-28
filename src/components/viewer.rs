use std::collections::HashSet;
use std::path::Path;

use iced::{
    Center, Element, Length, Theme,
    widget::{column, container, shader, stack, text},
};
use iced_aw::ContextMenu;

use crate::{
    app::{Message, Tool},
    components::notifications::NotificationEntry,
    components::{edit_panel, info_panel, notifications},
    gallery::Gallery,
    modifiers::{Modifier, ModifierKind},
    styles::{PAD, spinner_bg_style},
    wgpu::view_program::ViewProgram,
    widgets::{
        crop_overlay::CropOverlay,
        loading_spinner::Circular,
        menu::{menu_item, menu_separator, styled_menu},
    },
};

#[allow(clippy::too_many_arguments)]
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
    modifiers: &'a [Modifier],
    active_modifier: Option<usize>,
    dragging_modifier: Option<usize>,
    drag_hover_target: Option<usize>,
) -> Element<'a, Message> {
    let base: Element<'a, Message> = shader(program.clone())
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

    let notif_overlay = notifications::view(notifs);

    let image_size = program.image_size();
    let (img_w, img_h) = image_size
        .map(|(w, h)| (w as f32, h as f32))
        .unwrap_or((1.0, 1.0));

    let mut layers: Vec<Element<'a, Message>> = vec![base];

    if selected_tool == &Tool::Crop
        && loading.is_none()
        && let Some((crop_idx, crop_m)) = modifiers
            .iter()
            .enumerate()
            .find(|(_, m)| m.enabled && matches!(m.kind, ModifierKind::Crop { .. }))
        && let ModifierKind::Crop {
            x,
            y,
            width,
            height,
        } = crop_m.kind
    {
        layers.push(
            CropOverlay::new(program.clone(), crop_idx, x, y, width, height, img_w, img_h).into(),
        );
    }

    if let Some(filename) = loading {
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
        layers.push(spinner_overlay.into());
    }

    layers.push(notif_overlay);

    let viewer: Element<'a, Message> = stack(layers)
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

    let viewer_with_menu: Element<'a, Message> = ContextMenu::new(viewer, || {
        styled_menu(
            column![
                menu_item("Copy Color", Message::CopyColor),
                menu_item("Copy File Path", Message::CopyPath),
                menu_separator(),
                menu_item("Fit", Message::Fit),
            ],
            180,
        )
    })
    .into();

    if !show_info && !show_edit {
        return viewer_with_menu;
    }

    let mut content = iced::widget::Row::new().height(Length::Fill);
    if show_info {
        content = content.push(info_panel::view(
            path,
            gallery,
            &program,
            theme,
            info_collapsed,
            pixel_preview_size,
        ));
    }
    content = content.push(viewer_with_menu);
    if show_edit {
        content = content.push(edit_panel::view(
            selected_tool,
            modifiers,
            active_modifier,
            dragging_modifier,
            drag_hover_target,
            image_size,
            program.rotation(),
        ));
    }
    content.into()
}
