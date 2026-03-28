use std::collections::HashSet;

use iced::alignment::Vertical;
use iced::widget::tooltip::Position;
use iced::widget::{button, column, container, row, scrollable, slider, text};
use iced::{Color, Element, Font, Length, Theme};

use crate::app::Message;
use crate::edit::nodes::{
    BrightnessContrastParams, CropParams, EditNode, EditOp, HueSaturationParams,
};
use crate::styles::{PAD, bar_style, plain_icon_button_style, radius};
use crate::ui::{svg_button_plain, with_tooltip};
use crate::widgets::menu::{menu_item, menu_separator, styled_menu};
use crate::widgets::menu_button::{MenuAlign, MenuButton};

const COL_WIDTH: f32 = 220.0;
const NODE_FONT_SIZE: f32 = 12.0;

fn muted(theme: &Theme) -> Color {
    theme.extended_palette().background.base.text.scale_alpha(0.5)
}

fn node_row<'a>(node: &'a EditNode, expanded: bool, theme: &Theme) -> Element<'a, Message> {
    let id = node.id;
    let label_color = if node.enabled {
        theme.extended_palette().background.base.text
    } else {
        muted(theme)
    };

    let chevron = svg_button_plain(
        if expanded {
            include_bytes!("../../assets/icons/chevron_down.svg")
        } else {
            include_bytes!("../../assets/icons/chevron_right.svg")
        },
        Message::ToggleExpandNode(id),
    );

    let label = button(
        text(node.label())
            .size(NODE_FONT_SIZE)
            .color(label_color)
            .font(Font::MONOSPACE),
    )
    .on_press(Message::ToggleExpandNode(id))
    .padding([2, 4])
    .style(plain_icon_button_style)
    .width(Length::Fill);

    let toggle_icon: &'static [u8] = if node.enabled {
        include_bytes!("../../assets/icons/check.svg")
    } else {
        include_bytes!("../../assets/icons/close.svg")
    };
    let toggle = with_tooltip(
        svg_button_plain(toggle_icon, Message::ToggleEditNode(id)),
        if node.enabled { "Disable" } else { "Enable" },
        Position::Left,
    );

    let delete = with_tooltip(
        svg_button_plain(
            include_bytes!("../../assets/icons/trash.svg"),
            Message::RemoveEditNode(id),
        ),
        "Remove",
        Position::Left,
    );

    let header = row![chevron, label, toggle, delete]
        .spacing(2)
        .align_y(Vertical::Center)
        .width(Length::Fill);

    if expanded {
        let params = node_params(node, theme);
        column![header, params]
            .spacing(4)
            .width(Length::Fill)
            .into()
    } else {
        header.into()
    }
}

fn node_params<'a>(node: &'a EditNode, theme: &Theme) -> Element<'a, Message> {
    let id = node.id;
    let muted_color = muted(theme);

    match &node.op {
        EditOp::BrightnessContrast(p) => {
            let p = p.clone();
            column![
                param_slider(
                    "Brightness",
                    p.brightness,
                    -1.0,
                    1.0,
                    muted_color,
                    {
                        let p = p.clone();
                        move |v| {
                            Message::UpdateEditNode(
                                id,
                                EditOp::BrightnessContrast(BrightnessContrastParams {
                                    brightness: v,
                                    ..p.clone()
                                }),
                            )
                        }
                    },
                ),
                param_slider(
                    "Contrast",
                    p.contrast,
                    -1.0,
                    1.0,
                    muted_color,
                    move |v| {
                        Message::UpdateEditNode(
                            id,
                            EditOp::BrightnessContrast(BrightnessContrastParams {
                                contrast: v,
                                ..p.clone()
                            }),
                        )
                    },
                ),
            ]
            .spacing(4)
            .padding(iced::Padding { top: 0.0, right: 4.0, bottom: 4.0, left: 16.0 })
            .into()
        }
        EditOp::HueSaturation(p) => {
            let p = p.clone();
            column![
                param_slider(
                    "Hue",
                    p.hue / 180.0,
                    -1.0,
                    1.0,
                    muted_color,
                    {
                        let p = p.clone();
                        move |v| {
                            Message::UpdateEditNode(
                                id,
                                EditOp::HueSaturation(HueSaturationParams {
                                    hue: v * 180.0,
                                    ..p.clone()
                                }),
                            )
                        }
                    },
                ),
                param_slider(
                    "Saturation",
                    p.saturation,
                    -1.0,
                    1.0,
                    muted_color,
                    {
                        let p = p.clone();
                        move |v| {
                            Message::UpdateEditNode(
                                id,
                                EditOp::HueSaturation(HueSaturationParams {
                                    saturation: v,
                                    ..p.clone()
                                }),
                            )
                        }
                    },
                ),
                param_slider(
                    "Lightness",
                    p.lightness,
                    -1.0,
                    1.0,
                    muted_color,
                    move |v| {
                        Message::UpdateEditNode(
                            id,
                            EditOp::HueSaturation(HueSaturationParams {
                                lightness: v,
                                ..p.clone()
                            }),
                        )
                    },
                ),
            ]
            .spacing(4)
            .padding(iced::Padding { top: 0.0, right: 4.0, bottom: 4.0, left: 16.0 })
            .into()
        }
        EditOp::Crop(p) => {
            let p = p.clone();
            column![
                param_slider("X", p.x, 0.0, 1.0, muted_color, {
                    let p = p.clone();
                    move |v| {
                        Message::UpdateEditNode(
                            id,
                            EditOp::Crop(CropParams { x: v, ..p.clone() }),
                        )
                    }
                }),
                param_slider("Y", p.y, 0.0, 1.0, muted_color, {
                    let p = p.clone();
                    move |v| {
                        Message::UpdateEditNode(
                            id,
                            EditOp::Crop(CropParams { y: v, ..p.clone() }),
                        )
                    }
                }),
                param_slider("Width", p.width, 0.0, 1.0, muted_color, {
                    let p = p.clone();
                    move |v| {
                        Message::UpdateEditNode(
                            id,
                            EditOp::Crop(CropParams {
                                width: v,
                                ..p.clone()
                            }),
                        )
                    }
                }),
                param_slider("Height", p.height, 0.0, 1.0, muted_color, move |v| {
                    Message::UpdateEditNode(
                        id,
                        EditOp::Crop(CropParams {
                            height: v,
                            ..p.clone()
                        }),
                    )
                }),
            ]
            .spacing(4)
            .padding(iced::Padding { top: 0.0, right: 4.0, bottom: 4.0, left: 16.0 })
            .into()
        }
        EditOp::Curves(_) => container(
            text("Curves editor coming soon")
                .size(11)
                .color(muted_color)
                .font(Font::MONOSPACE),
        )
        .padding(iced::Padding { top: 0.0, right: 4.0, bottom: 4.0, left: 16.0 })
        .into(),
        EditOp::Paint(_) => container(
            text("Paint editor coming soon")
                .size(11)
                .color(muted_color)
                .font(Font::MONOSPACE),
        )
        .padding(iced::Padding { top: 0.0, right: 4.0, bottom: 4.0, left: 16.0 })
        .into(),
    }
}

fn param_slider<'a, F>(
    label: &'a str,
    value: f32,
    min: f32,
    max: f32,
    label_color: Color,
    on_change: F,
) -> Element<'a, Message>
where
    F: Fn(f32) -> Message + 'a,
{
    column![
        text(label)
            .size(10)
            .color(label_color)
            .font(Font::MONOSPACE),
        slider(min..=max, value, on_change).step(0.01),
    ]
    .spacing(2)
    .into()
}

pub fn view<'a>(
    nodes: &'a [EditNode],
    expanded: &'a HashSet<u64>,
    theme: &'a Theme,
) -> Element<'a, Message> {
    let muted_color = muted(theme);

    let add_btn = MenuButton::new(
        include_bytes!("../../assets/icons/add.svg"),
        styled_menu(column![
            menu_item(
                "Brightness / Contrast",
                Message::AddEditNode(EditOp::BrightnessContrast(
                    BrightnessContrastParams::default()
                ))
            ),
            menu_item(
                "Hue / Saturation",
                Message::AddEditNode(EditOp::HueSaturation(HueSaturationParams::default()))
            ),
            menu_separator(),
            menu_item(
                "Crop",
                Message::AddEditNode(EditOp::Crop(CropParams::default()))
            ),
        ]),
    )
    .align(MenuAlign::TopStart);

    let header = row![
        text("Edit Stack")
            .size(11)
            .color(muted_color)
            .font(Font::MONOSPACE)
            .width(Length::Fill),
        with_tooltip(add_btn, "Add adjustment", Position::Left),
    ]
    .align_y(Vertical::Center)
    .padding(iced::Padding { top: 0.0, right: 0.0, bottom: PAD, left: 0.0 });

    let content: Element<'_, Message> = if nodes.is_empty() {
        container(
            text("No adjustments")
                .size(11)
                .color(muted_color)
                .font(Font::MONOSPACE),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
    } else {
        let node_list = nodes
            .iter()
            .fold(column![].spacing(2).width(Length::Fill), |col, node| {
                col.push(
                    container(node_row(node, expanded.contains(&node.id), theme))
                        .style(|t: &Theme| {
                            let palette = t.extended_palette();
                            container::Style {
                                background: Some(iced::Background::Color(
                                    palette.background.weak.color,
                                )),
                                border: iced::Border {
                                    radius: radius().into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        })
                        .padding([4, 4])
                        .width(Length::Fill),
                )
            });

        scrollable(node_list)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    container(
        column![header, content]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(PAD)
    .style(bar_style)
    .height(Length::Fill)
    .width(Length::Fixed(COL_WIDTH))
    .into()
}
