use iced::alignment::{Horizontal, Vertical};
use iced::widget::rule;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::svg::Handle;
use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, slider, svg, text, text_input,
};
use iced::{Element, Length, Padding, mouse, padding};

use crate::app::Message;
use crate::modifiers::{MaskParam, Modifier, ModifierKind, ModifierParam, ModifierType};
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
) -> Element<'a, Message> {
    let n = modifiers.len();
    let mut stack_col = column![];
    for (i, modifier) in modifiers.iter().enumerate() {
        let show_indicator = matches!((dragging, drag_target),
            (Some(src), Some(tgt)) if tgt == i && src != i);
        stack_col = stack_col.push(gap(show_indicator));
        stack_col = stack_col.push(card(i, modifier, active == Some(i), dragging.is_some()));
    }
    let show_trailing = matches!((dragging, drag_target),
        (Some(_), Some(tgt)) if tgt == n);
    stack_col = stack_col.push(gap(show_trailing));
    stack_col = stack_col.push(
        mouse_area(Space::new().height(20).width(Length::Fill))
            .on_enter(Message::ModifierDragHover(n)),
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
    .on_press(Message::ClearActiveModifier)
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

fn card<'a>(
    index: usize,
    modifier: &'a Modifier,
    is_active: bool,
    dragging: bool,
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
        text(modifier.kind.name()).size(11).width(Length::Fill),
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
        if !matches!(modifier.kind, ModifierKind::Crop { .. }) {
            card_col = card_col.push(rule::horizontal(1));
            card_col = card_col.push(mask_section(index, modifier));
        }
    }

    let card_area = mouse_area(
        container(card_col)
            .style(if is_active {
                modifier_active_card_style
            } else {
                modifier_card_style
            })
            .padding([3.0, PAD])
            .width(Length::Fill),
    )
    .on_release(Message::SetActiveModifier(index))
    .on_enter(Message::ModifierDragHover(index));

    if dragging {
        card_area.interaction(mouse::Interaction::Grabbing).into()
    } else {
        card_area.into()
    }
}

fn body<'a>(index: usize, kind: &'a ModifierKind) -> Element<'a, Message> {
    let col = match kind {
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
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", sh)
                ),
                param_row(
                    "Midtones",
                    slider(0.0f32..=2.0f32, mi, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::LevelsMidtones(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", mi)
                ),
                param_row(
                    "Highlights",
                    slider(0.0f32..=2.0f32, hi, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::LevelsHighlights(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", hi)
                ),
            ]
        }
        ModifierKind::BrightnessContrast {
            brightness,
            contrast,
        } => {
            let (br, co) = (*brightness, *contrast);
            column![
                param_row(
                    "Brightness",
                    slider(-1.0f32..=1.0f32, br, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Brightness(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", br)
                ),
                param_row(
                    "Contrast",
                    slider(-1.0f32..=1.0f32, co, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Contrast(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", co)
                ),
            ]
        }
        ModifierKind::HueSaturation {
            hue,
            saturation,
            lightness,
        } => {
            let (hu, sa, li) = (*hue, *saturation, *lightness);
            column![
                param_row(
                    "Hue",
                    slider(-180.0f32..=180.0f32, hu, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Hue(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.0}°", hu)
                ),
                param_row(
                    "Saturation",
                    slider(-1.0f32..=1.0f32, sa, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Saturation(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", sa)
                ),
                param_row(
                    "Lightness",
                    slider(-1.0f32..=1.0f32, li, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Lightness(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", li)
                ),
            ]
        }
        ModifierKind::Exposure { exposure } => {
            let ex = *exposure;
            column![param_row(
                "Exposure",
                slider(-5.0f32..=5.0f32, ex, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::Exposure(v)
                ))
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:+.2}", ex)
            )]
        }
        ModifierKind::Vibrance {
            vibrance,
            saturation,
        } => {
            let (vi, sa) = (*vibrance, *saturation);
            column![
                param_row(
                    "Vibrance",
                    slider(-1.0f32..=1.0f32, vi, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::Vibrance(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", vi)
                ),
                param_row(
                    "Saturation",
                    slider(-1.0f32..=1.0f32, sa, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::VibranceSaturation(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", sa)
                ),
            ]
        }
        ModifierKind::ColorBalance {
            cyan_red,
            magenta_green,
            yellow_blue,
        } => {
            let (cr, mg, yb) = (*cyan_red, *magenta_green, *yellow_blue);
            column![
                param_row(
                    "Cyan / Red",
                    slider(-1.0f32..=1.0f32, cr, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::ColorBalanceCyanRed(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", cr)
                ),
                param_row(
                    "Mag / Green",
                    slider(-1.0f32..=1.0f32, mg, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::ColorBalanceMagentaGreen(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", mg)
                ),
                param_row(
                    "Yel / Blue",
                    slider(-1.0f32..=1.0f32, yb, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::ColorBalanceYellowBlue(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:+.2}", yb)
                ),
            ]
        }
        ModifierKind::GaussianBlur { radius } => {
            let r = *radius;
            column![param_row(
                "Radius",
                slider(0.0f32..=100.0f32, r, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::GaussianBlurRadius(v)
                ))
                .step(0.5f32)
                .width(Length::Fill)
                .into(),
                format!("{:.1}", r)
            )]
        }
        ModifierKind::MotionBlur { angle, distance } => {
            let (an, di) = (*angle, *distance);
            column![
                param_row(
                    "Angle",
                    slider(0.0f32..=360.0f32, an, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::MotionBlurAngle(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}°", an)
                ),
                param_row(
                    "Distance",
                    slider(0.0f32..=200.0f32, di, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::MotionBlurDistance(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}", di)
                ),
            ]
        }
        ModifierKind::RadialBlur { amount } => {
            let am = *amount;
            column![param_row(
                "Amount",
                slider(0.0f32..=100.0f32, am, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::RadialBlurAmount(v)
                ))
                .step(0.5f32)
                .width(Length::Fill)
                .into(),
                format!("{:.0}", am)
            )]
        }
        ModifierKind::Halftone { size, angle } => {
            let (si, an) = (*size, *angle);
            column![
                param_row(
                    "Size",
                    slider(2.0f32..=50.0f32, si, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::HalftoneSize(v)
                    ))
                    .step(0.1f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}", si)
                ),
                param_row(
                    "Angle",
                    slider(0.0f32..=90.0f32, an, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::HalftoneAngle(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}°", an)
                ),
            ]
        }
        ModifierKind::PixelSort { threshold, angle } => {
            let (th, an) = (*threshold, *angle);
            column![
                param_row(
                    "Threshold",
                    slider(0.0f32..=1.0f32, th, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::PixelSortThreshold(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", th)
                ),
                param_row(
                    "Angle",
                    slider(0.0f32..=360.0f32, an, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::PixelSortAngle(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}°", an)
                ),
            ]
        }
        ModifierKind::Vignette {
            strength,
            size,
            softness,
        } => {
            let (st, si, so) = (*strength, *size, *softness);
            column![
                param_row(
                    "Strength",
                    slider(0.0f32..=1.0f32, st, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::VignetteStrength(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", st)
                ),
                param_row(
                    "Size",
                    slider(0.0f32..=1.0f32, si, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::VignetteSize(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", si)
                ),
                param_row(
                    "Softness",
                    slider(0.0f32..=1.0f32, so, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::VignetteSoftness(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", so)
                ),
            ]
        }
        ModifierKind::ChromaticAberration { amount, angle } => {
            let (am, an) = (*amount, *angle);
            column![
                param_row(
                    "Amount",
                    slider(0.0f32..=50.0f32, am, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::ChromaticAberrationAmount(v)
                    ))
                    .step(0.1f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.1}", am)
                ),
                param_row(
                    "Angle",
                    slider(0.0f32..=360.0f32, an, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::ChromaticAberrationAngle(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}°", an)
                ),
            ]
        }
        ModifierKind::Posterize { levels } => {
            let lv = *levels;
            column![param_row(
                "Levels",
                slider(2u32..=32u32, lv, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::PosterizeLevels(v)
                ))
                .width(Length::Fill)
                .into(),
                lv.to_string()
            )]
        }
        ModifierKind::Threshold { cutoff } => {
            let cu = *cutoff;
            column![param_row(
                "Cutoff",
                slider(0.0f32..=1.0f32, cu, move |v| Message::UpdateModifier(
                    index,
                    ModifierParam::ThresholdCutoff(v)
                ))
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:.2}", cu)
            )]
        }
        ModifierKind::Grain {
            amount,
            size,
            roughness,
            seed,
        } => {
            let (am, si, ro, se) = (*amount, *size, *roughness, *seed);
            column![
                param_row(
                    "Amount",
                    slider(0.0f32..=1.0f32, am, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::GrainAmount(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", am)
                ),
                param_row(
                    "Size",
                    slider(0.5f32..=32.0f32, si, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::GrainSize(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.1}px", si)
                ),
                param_row(
                    "Roughness",
                    slider(0.0f32..=1.0f32, ro, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::GrainRoughness(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", ro)
                ),
                param_row(
                    "Seed",
                    slider(0.0f32..=99.0f32, se, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::GrainSeed(v)
                    ))
                    .step(1.0f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{}", se as u32)
                ),
            ]
        }
        ModifierKind::Crop {
            x,
            y,
            width,
            height,
            rotation,
        } => {
            let (cx, cy, cw, ch, ro) = (*x, *y, *width, *height, *rotation);
            column![
                param_row(
                    "X",
                    slider(0.0f32..=1.0f32, cx, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::CropX(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cx)
                ),
                param_row(
                    "Y",
                    slider(0.0f32..=1.0f32, cy, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::CropY(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cy)
                ),
                param_row(
                    "Width",
                    slider(0.0f32..=1.0f32, cw, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::CropWidth(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cw)
                ),
                param_row(
                    "Height",
                    slider(0.0f32..=1.0f32, ch, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::CropHeight(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", ch)
                ),
                param_row(
                    "Rotation",
                    slider(-45.0f32..=45.0f32, ro, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::CropRotation(v)
                    ))
                    .step(0.1f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.1}°", ro)
                ),
            ]
        }
        ModifierKind::Text {
            content,
            x,
            y,
            size,
            rotation,
            opacity,
            r,
            g,
            b,
        } => {
            let (tx, ty, ts, tr, to, cr, cg, cb) = (*x, *y, *size, *rotation, *opacity, *r, *g, *b);
            let content = content.clone();
            column![
                text_input("Type something...", &content)
                    .on_input(move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextContent(v)
                    ))
                    .size(11)
                    .padding([4, 6]),
                param_row(
                    "X",
                    slider(0.0f32..=1.0f32, tx, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextX(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", tx)
                ),
                param_row(
                    "Y",
                    slider(0.0f32..=1.0f32, ty, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextY(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", ty)
                ),
                param_row(
                    "Size",
                    slider(4.0f32..=200.0f32, ts, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextSize(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}", ts)
                ),
                param_row(
                    "Rotation",
                    slider(-180.0f32..=180.0f32, tr, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextRotation(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}°", tr)
                ),
                param_row(
                    "Opacity",
                    slider(0.0f32..=1.0f32, to, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextOpacity(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", to)
                ),
                param_row(
                    "R",
                    slider(0.0f32..=1.0f32, cr, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextR(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cr)
                ),
                param_row(
                    "G",
                    slider(0.0f32..=1.0f32, cg, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextG(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cg)
                ),
                param_row(
                    "B",
                    slider(0.0f32..=1.0f32, cb, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::TextB(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", cb)
                ),
            ]
        }
        ModifierKind::Drawing {
            opacity,
            size,
            hardness,
        } => {
            let (op, si, ha) = (*opacity, *size, *hardness);
            column![
                param_row(
                    "Opacity",
                    slider(0.0f32..=1.0f32, op, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::DrawingOpacity(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", op)
                ),
                param_row(
                    "Size",
                    slider(1.0f32..=100.0f32, si, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::DrawingSize(v)
                    ))
                    .step(0.5f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.0}", si)
                ),
                param_row(
                    "Hardness",
                    slider(0.0f32..=1.0f32, ha, move |v| Message::UpdateModifier(
                        index,
                        ModifierParam::DrawingHardness(v)
                    ))
                    .step(0.01f32)
                    .width(Length::Fill)
                    .into(),
                    format!("{:.2}", ha)
                ),
            ]
        }
    };

    col.spacing(4).padding(padding::top(4).bottom(2)).into()
}

fn mask_section<'a>(index: usize, modifier: &'a Modifier) -> Element<'a, Message> {
    let mask_icon: &'static [u8] = if modifier.mask_enabled {
        include_bytes!("../../assets/icons/circle-filled.svg")
    } else {
        include_bytes!("../../assets/icons/circle.svg")
    };

    let header = row![
        text("Mask").size(11),
        Space::new().width(Length::Fill),
        icon_btn(mask_icon, Message::ToggleModifierMask(index)),
    ]
    .align_y(Vertical::Center)
    .spacing(2);

    let mut col = column![header].spacing(4);

    if modifier.mask_enabled {
        let (mx, my, mw, mh, fe) = (
            modifier.mask_x,
            modifier.mask_y,
            modifier.mask_w,
            modifier.mask_h,
            modifier.feather,
        );
        col = col
            .push(param_row(
                "X",
                slider(0.0f32..=1.0f32, mx, move |v| {
                    Message::UpdateModifierMask(index, MaskParam::X(v))
                })
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:.2}", mx),
            ))
            .push(param_row(
                "Y",
                slider(0.0f32..=1.0f32, my, move |v| {
                    Message::UpdateModifierMask(index, MaskParam::Y(v))
                })
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:.2}", my),
            ))
            .push(param_row(
                "Width",
                slider(0.0f32..=1.0f32, mw, move |v| {
                    Message::UpdateModifierMask(index, MaskParam::Width(v))
                })
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:.2}", mw),
            ))
            .push(param_row(
                "Height",
                slider(0.0f32..=1.0f32, mh, move |v| {
                    Message::UpdateModifierMask(index, MaskParam::Height(v))
                })
                .step(0.01f32)
                .width(Length::Fill)
                .into(),
                format!("{:.2}", mh),
            ))
            .push(param_row(
                "Feather",
                slider(0.0f32..=100.0f32, fe, move |v| {
                    Message::UpdateModifierMask(index, MaskParam::Feather(v))
                })
                .step(0.5f32)
                .width(Length::Fill)
                .into(),
                format!("{:.1}", fe),
            ));
    }

    col.padding(padding::top(4).bottom(2)).into()
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
    MenuButton::new(
        text("+ Add Modifier").size(11),
        styled_menu(
            column![
                sub_menu(
                    "Adjustments",
                    styled_menu(
                        column![
                            menu_item("Levels", Message::AddModifier(ModifierType::Levels)),
                            menu_item(
                                "Brightness / Contrast",
                                Message::AddModifier(ModifierType::BrightnessContrast)
                            ),
                            menu_item(
                                "Hue / Saturation",
                                Message::AddModifier(ModifierType::HueSaturation)
                            ),
                            menu_item("Exposure", Message::AddModifier(ModifierType::Exposure)),
                            menu_item("Vibrance", Message::AddModifier(ModifierType::Vibrance)),
                            menu_item(
                                "Color Balance",
                                Message::AddModifier(ModifierType::ColorBalance)
                            ),
                        ],
                        210
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Blur",
                    styled_menu(
                        column![
                            menu_item(
                                "Gaussian Blur",
                                Message::AddModifier(ModifierType::GaussianBlur)
                            ),
                            menu_item(
                                "Motion Blur",
                                Message::AddModifier(ModifierType::MotionBlur)
                            ),
                            menu_item(
                                "Radial Blur",
                                Message::AddModifier(ModifierType::RadialBlur)
                            ),
                        ],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Pixelate",
                    styled_menu(
                        column![
                            menu_item("Halftone", Message::AddModifier(ModifierType::Halftone)),
                            menu_item("Pixel Sort", Message::AddModifier(ModifierType::PixelSort)),
                        ],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Stylize",
                    styled_menu(
                        column![
                            menu_item("Vignette", Message::AddModifier(ModifierType::Vignette)),
                            menu_item(
                                "Chromatic Aberration",
                                Message::AddModifier(ModifierType::ChromaticAberration)
                            ),
                            menu_item("Posterize", Message::AddModifier(ModifierType::Posterize)),
                            menu_item("Threshold", Message::AddModifier(ModifierType::Threshold)),
                        ],
                        200
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Noise",
                    styled_menu(
                        column![menu_item(
                            "Grain",
                            Message::AddModifier(ModifierType::Grain)
                        ),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Transform",
                    styled_menu(
                        column![menu_item("Crop", Message::AddModifier(ModifierType::Crop)),],
                        160
                    )
                )
                .side(SubMenuSide::Left),
                sub_menu(
                    "Creative",
                    styled_menu(
                        column![
                            menu_item("Text", Message::AddModifier(ModifierType::Text)),
                            menu_item("Drawing", Message::AddModifier(ModifierType::Drawing)),
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
