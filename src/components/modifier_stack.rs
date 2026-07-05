use iced::alignment::Vertical;
use iced::widget::rule;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::svg::Handle;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, svg, text};
use iced::{Element, Length, Padding, mouse};

use crate::app::{EditMsg, Message};
use crate::modifiers::{Modifier, ModifierType};
use crate::styles::{
    PAD, modifier_active_card_style, modifier_add_button_style, modifier_card_style,
    modifier_drop_indicator_style, plain_icon_button_style, svg_style,
};
use crate::widgets::menu::{SubMenuSide, menu_item, styled_menu, sub_menu};
use crate::widgets::menu_button::{MenuAlign, MenuButton};

pub fn view<'a>(
    modifiers: &'a [Modifier],
    active: Option<usize>,
    dragging: Option<usize>,
    drag_target: Option<usize>,
    image_size: Option<(u32, u32)>,
    rotation: u8,
) -> Element<'a, Message> {
    let n = modifiers.len();
    let mut stack_col = column![];
    for (i, modifier) in modifiers.iter().enumerate() {
        let show_indicator = matches!((dragging, drag_target),
            (Some(src), Some(tgt)) if tgt == i && src != i);
        stack_col = stack_col.push(gap(show_indicator));
        stack_col = stack_col.push(card(
            i,
            modifier,
            active == Some(i),
            dragging.is_some(),
            image_size,
            rotation,
        ));
    }
    let show_trailing = matches!((dragging, drag_target),
        (Some(_), Some(tgt)) if tgt == n);
    stack_col = stack_col.push(gap(show_trailing));
    stack_col = stack_col.push(
        mouse_area(Space::new().height(20).width(Length::Fill))
            .on_enter(EditMsg::DragHover(n).into()),
    );

    mouse_area(
        column![
            scrollable(stack_col.padding(PAD))
                .height(Length::Fill)
                .direction(Direction::Vertical(
                    Scrollbar::new().width(4).scroller_width(4),
                )),
            container(add_row()).padding(PAD).width(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .on_press(EditMsg::ClearActive.into())
    .into()
}

fn gap<'a>(show_indicator: bool) -> Element<'a, Message> {
    if show_indicator {
        column![
            Space::new().height(1),
            container(Space::new().height(2))
                .width(Length::Fill)
                .style(modifier_drop_indicator_style),
            Space::new().height(1),
        ]
        .into()
    } else {
        Space::new().height(4).into()
    }
}

fn icon_btn<'a>(icon: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        svg(Handle::from_memory(icon))
            .style(svg_style)
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0)),
    )
    .padding(Padding::from([1, 1]))
    .style(plain_icon_button_style)
    .on_press(msg)
    .into()
}

fn card<'a>(
    index: usize,
    modifier: &'a Modifier,
    is_active: bool,
    dragging: bool,
    image_size: Option<(u32, u32)>,
    rotation: u8,
) -> Element<'a, Message> {
    let arrow_icon: &'static [u8] = if modifier.expanded {
        include_bytes!("../../assets/icons/down.svg")
    } else {
        include_bytes!("../../assets/icons/right.svg")
    };
    let circle_icon: &'static [u8] = if modifier.enabled {
        include_bytes!("../../assets/icons/circle-filled.svg")
    } else {
        include_bytes!("../../assets/icons/circle.svg")
    };

    let grip = mouse_area(
        svg(Handle::from_memory(include_bytes!(
            "../../assets/icons/grip.svg"
        )))
        .style(svg_style)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0)),
    )
    .on_press(EditMsg::DragStart(index).into())
    .interaction(if dragging {
        mouse::Interaction::Grabbing
    } else {
        mouse::Interaction::Grab
    });

    let header = row![
        grip,
        icon_btn(arrow_icon, EditMsg::ToggleExpanded(index).into()),
        container(
            text(modifier.kind.name())
                .size(10)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true),
        icon_btn(circle_icon, EditMsg::ToggleEnabled(index).into()),
        icon_btn(
            include_bytes!("../../assets/icons/close.svg"),
            EditMsg::Remove(index).into(),
        ),
    ]
    .align_y(Vertical::Center)
    .spacing(2);

    let mut card_col = column![header].spacing(5);

    if modifier.expanded {
        card_col = card_col.push(rule::horizontal(1));
        card_col = card_col.push(modifier.kind.view(index, image_size, rotation));
    }

    let card_area = mouse_area(
        container(card_col)
            .style(if is_active {
                modifier_active_card_style
            } else {
                modifier_card_style
            })
            .padding([6.0, PAD])
            .width(Length::Fill),
    )
    .on_release(EditMsg::SetActive(index).into())
    .on_enter(EditMsg::DragHover(index).into());

    if dragging {
        card_area.interaction(mouse::Interaction::Grabbing).into()
    } else {
        card_area.into()
    }
}

fn add_row<'a>() -> Element<'a, Message> {
    MenuButton::new(
        text("+ Add Modifier").size(11),
        styled_menu(
            column![
                sub_menu(
                    "Adjustments",
                    styled_menu(
                        column![
                            menu_item("Levels", EditMsg::Add(ModifierType::Levels).into()),
                            menu_item(
                                "Brightness / Contrast",
                                EditMsg::Add(ModifierType::BrightnessContrast).into()
                            ),
                            menu_item(
                                "Hue / Saturation",
                                EditMsg::Add(ModifierType::HueSaturation).into()
                            ),
                            menu_item("Exposure", EditMsg::Add(ModifierType::Exposure).into()),
                            menu_item("Vibrance", EditMsg::Add(ModifierType::Vibrance).into()),
                            menu_item(
                                "Color Balance",
                                EditMsg::Add(ModifierType::ColorBalance).into()
                            ),
                            menu_item(
                                "Temperature",
                                EditMsg::Add(ModifierType::Temperature).into()
                            ),
                            menu_item("Grayscale", EditMsg::Add(ModifierType::Grayscale).into()),
                            menu_item("Invert", EditMsg::Add(ModifierType::Invert).into()),
                        ],
                        210
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Pixelate",
                    styled_menu(
                        column![menu_item(
                            "Halftone",
                            EditMsg::Add(ModifierType::Halftone).into()
                        ),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Blur",
                    styled_menu(
                        column![menu_item(
                            "Gaussian Blur",
                            EditMsg::Add(ModifierType::GaussianBlur).into()
                        ),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Stylize",
                    styled_menu(
                        column![
                            menu_item("Vignette", EditMsg::Add(ModifierType::Vignette).into()),
                            menu_item(
                                "Chromatic Aberration",
                                EditMsg::Add(ModifierType::ChromaticAberration).into()
                            ),
                            menu_item("Posterize", EditMsg::Add(ModifierType::Posterize).into()),
                            menu_item("Threshold", EditMsg::Add(ModifierType::Threshold).into()),
                            menu_item("Sepia", EditMsg::Add(ModifierType::Sepia).into()),
                            menu_item("Solarize", EditMsg::Add(ModifierType::Solarize).into()),
                            menu_item("Duotone", EditMsg::Add(ModifierType::Duotone).into()),
                        ],
                        200
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Glitch",
                    styled_menu(
                        column![menu_item(
                            "Pixel Sort",
                            EditMsg::Add(ModifierType::PixelSort).into()
                        ),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Noise",
                    styled_menu(
                        column![menu_item("Grain", EditMsg::Add(ModifierType::Grain).into()),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Transform",
                    styled_menu(
                        column![menu_item("Crop", EditMsg::Add(ModifierType::Crop).into()),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Create",
                    styled_menu(
                        column![
                            menu_item("Text", EditMsg::Add(ModifierType::Text).into()),
                            menu_item("Drawing", EditMsg::Add(ModifierType::Drawing).into()),
                        ],
                        160
                    )
                )
                .side(SubMenuSide::Left),
            ],
            180,
        ),
    )
    .width(Length::Fill)
    .height(Length::Fixed(28.0))
    .style(modifier_add_button_style)
    .align(MenuAlign::TopStart)
    .into()
}
