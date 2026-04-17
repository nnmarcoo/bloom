use iced::alignment::{Horizontal, Vertical};
use iced::widget::rule;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::svg::Handle;
use iced::widget::{
    Space, button, column, container, mouse_area, pick_list, row, scrollable, slider, svg, text,
};
use iced::{Element, Length, Padding, mouse, padding};

use crate::app::Message;
use crate::modifiers::{MODIFIER_TYPES, Modifier, ModifierKind, ModifierParam, ModifierType};
use crate::styles::{
    PAD, modifier_card_style, modifier_drop_indicator_style, plain_icon_button_style, svg_style,
};

pub fn view<'a>(
    modifiers: &'a [Modifier],
    dragging: Option<usize>,
    drag_target: Option<usize>,
) -> Element<'a, Message> {
    let n = modifiers.len();
    let mut stack_col = column![];
    for (i, modifier) in modifiers.iter().enumerate() {
        let show_indicator = matches!((dragging, drag_target),
            (Some(src), Some(tgt)) if tgt == i && src != i);
        stack_col = stack_col.push(gap(show_indicator));
        stack_col = stack_col.push(card(i, modifier, dragging.is_some()));
    }
    let show_trailing = matches!((dragging, drag_target),
        (Some(_), Some(tgt)) if tgt == n);
    stack_col = stack_col.push(gap(show_trailing));
    stack_col = stack_col.push(
        mouse_area(Space::new().height(20).width(Length::Fill))
            .on_enter(Message::ModifierDragHover(n)),
    );

    column![
        scrollable(stack_col.padding(PAD))
            .height(Length::Fill)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4).scroller_width(4),
            )),
        add_row(),
    ]
    .height(Length::Fill)
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
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0)),
    )
    .padding(Padding::from([1, 1]))
    .style(plain_icon_button_style)
    .on_press(msg)
    .into()
}

fn card<'a>(index: usize, modifier: &'a Modifier, dragging: bool) -> Element<'a, Message> {
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
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0)),
    )
    .on_press(Message::StartModifierDrag(index))
    .interaction(if dragging {
        mouse::Interaction::Grabbing
    } else {
        mouse::Interaction::Grab
    });

    let header = row![
        grip,
        icon_btn(arrow_icon, Message::ToggleModifierExpanded(index)),
        text(modifier.kind.name()).size(11),
        Space::new().width(Length::Fill),
        icon_btn(circle_icon, Message::ToggleModifierEnabled(index)),
        icon_btn(
            include_bytes!("../../assets/icons/close.svg"),
            Message::RemoveModifier(index),
        ),
    ]
    .align_y(Vertical::Center)
    .spacing(2);

    let mut card_col = column![header];

    if modifier.expanded {
        card_col = card_col.push(rule::horizontal(1));
        card_col = card_col.push(body(index, &modifier.kind));
    }

    let card_area = mouse_area(
        container(card_col)
            .style(modifier_card_style)
            .padding([3.0, PAD])
            .width(Length::Fill),
    )
    .on_enter(Message::ModifierDragHover(index));

    if dragging {
        card_area.interaction(mouse::Interaction::Grabbing).into()
    } else {
        card_area.into()
    }
}

fn body<'a>(index: usize, kind: &'a ModifierKind) -> Element<'a, Message> {
    let col = match kind {
        ModifierKind::Mosaic { size } => {
            let s = *size;
            column![param_row(
                "Size",
                slider(1u32..=200u32, s, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::MosaicSize(v)
                ))
                .width(Length::Fill)
                .into(),
                s.to_string(),
            )]
        }
        ModifierKind::Levels {
            shadows,
            midtones,
            highlights,
        } => {
            let (sh, mi, hi) = (*shadows, *midtones, *highlights);
            column![
                param_row(
                    "Shadows",
                    slider(0.0f32..=2.0f32, sh, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::LevelsShadows(v)
                    ))
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", sh),
                ),
                param_row(
                    "Midtones",
                    slider(0.0f32..=2.0f32, mi, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::LevelsMidtones(v)
                    ))
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", mi),
                ),
                param_row(
                    "Highlights",
                    slider(0.0f32..=2.0f32, hi, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::LevelsHighlights(v)
                    ))
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", hi),
                ),
            ]
        }
    };

    col.spacing(4).padding(padding::top(4).bottom(2)).into()
}

fn param_row<'a>(
    label: &'a str,
    slider_el: Element<'a, Message>,
    value: String,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(10)
            .width(Length::Fixed(58.0))
            .align_x(Horizontal::Left),
        slider_el,
        text(value)
            .size(10)
            .width(Length::Fixed(30.0))
            .align_x(Horizontal::Right),
    ]
    .align_y(Vertical::Center)
    .spacing(4)
    .into()
}

fn add_row<'a>() -> Element<'a, Message> {
    pick_list(MODIFIER_TYPES, None::<ModifierType>, Message::AddModifier)
        .placeholder("+ Add Modifier")
        .width(Length::Fill)
        .text_size(11)
        .padding([PAD, PAD])
        .into()
}
