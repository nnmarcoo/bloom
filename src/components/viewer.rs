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
    modifiers::Modifier,
    styles::{PAD, spinner_bg_style},
    wgpu::view_program::{Histogram, ViewProgram},
    widgets::{
        crop_overlay::CropOverlay,
        loading_spinner::Circular,
        menu::{menu_item, menu_item_enabled, menu_separator, styled_menu},
    },
};

pub struct ViewerCtx<'a> {
    pub program: ViewProgram,
    pub loading: Option<&'a str>,
    pub show_info: bool,
    pub show_edit: bool,
    pub show_bottom_bar: bool,
    pub path: Option<&'a Path>,
    pub gallery: &'a Gallery,
    pub theme: &'a Theme,
    pub info_collapsed: &'a HashSet<String>,
    pub notifs: &'a [NotificationEntry],
    pub pixel_preview_size: u32,
    pub selected_tool: &'a Tool,
    pub modifiers: &'a [Modifier],
    pub active_modifier: Option<usize>,
    pub dragging_modifier: Option<usize>,
    pub drag_hover_target: Option<usize>,
    pub histogram: Option<&'a Histogram>,
    #[cfg(feature = "av")]
    pub video_panel: Option<info_panel::VideoPanel<'a>>,
}

pub fn view(ctx: ViewerCtx<'_>) -> Element<'_, Message> {
    let base: Element<'_, Message> = shader(ctx.program.clone())
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

    let notif_overlay = notifications::view(ctx.notifs);

    let image_size = ctx.program.image_size();
    let (img_w, img_h) = image_size
        .map(|(w, h)| (w as f32, h as f32))
        .unwrap_or((1.0, 1.0));

    let mut layers: Vec<Element<'_, Message>> = vec![base];

    if ctx.selected_tool == &Tool::Crop
        && ctx.loading.is_none()
        && let Some((crop_idx, crop_m)) = ctx
            .modifiers
            .iter()
            .enumerate()
            .find(|(_, m)| m.enabled && m.kind.as_crop().is_some())
        && let Some(crop) = crop_m.kind.as_crop()
    {
        layers.push(
            CropOverlay::new(
                ctx.program.clone(),
                crop_idx,
                crop.x,
                crop.y,
                crop.width,
                crop.height,
                img_w,
                img_h,
            )
            .into(),
        );
    }

    if let Some(filename) = ctx.loading {
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

    let viewer: Element<'_, Message> = stack(layers)
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

    let bottom_bar_label = if ctx.show_bottom_bar {
        "Hide Bottom Bar"
    } else {
        "Show Bottom Bar"
    };
    let has_media = image_size.is_some();
    let viewer_with_menu: Element<'_, Message> = ContextMenu::new(viewer, move || {
        styled_menu(
            column![
                menu_item_enabled("Open File Location", Message::OpenFileLocation, has_media),
                menu_separator(),
                menu_item_enabled("Copy Color", Message::CopyColor, has_media),
                menu_item_enabled("Copy Image", Message::CopyImage, has_media),
                menu_item_enabled("Copy File Path", Message::CopyPath, has_media),
                menu_separator(),
                menu_item_enabled("Rotate Left", Message::RotateCcw, has_media),
                menu_item_enabled("Rotate Right", Message::RotateCw, has_media),
                menu_item_enabled("Export Image", Message::ExportImage, has_media),
                menu_separator(),
                menu_item(bottom_bar_label, Message::ToggleBottomBar),
            ],
            180,
        )
    })
    .into();

    if !ctx.show_info && !ctx.show_edit {
        return viewer_with_menu;
    }

    let mut content = iced::widget::Row::new().height(Length::Fill);
    if ctx.show_info && image_size.is_some() {
        content = content.push(info_panel::view(
            ctx.path,
            ctx.gallery,
            &ctx.program,
            ctx.theme,
            ctx.info_collapsed,
            ctx.pixel_preview_size,
            ctx.histogram,
            #[cfg(feature = "av")]
            ctx.video_panel,
        ));
    }
    content = content.push(viewer_with_menu);
    if ctx.show_edit {
        content = content.push(edit_panel::view(
            ctx.selected_tool,
            ctx.modifiers,
            ctx.active_modifier,
            ctx.dragging_modifier,
            ctx.drag_hover_target,
            image_size,
            ctx.program.rotation(),
        ));
    }
    content.into()
}
