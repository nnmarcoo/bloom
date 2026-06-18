use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::{column, text_input};

use crate::app::{EditMsg, Message};
use crate::modifiers::{InputClass, ModifierImpl, ModifierParam};
use crate::widgets::number_entry::NumberEntry;
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

// Labeled NumberEntry row, for unbounded values where a slider's track is misleading.
#[allow(clippy::too_many_arguments)]
fn entry_row<'a>(
    label: &'a str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    drag_per_px: f32,
    suffix: &'static str,
    on_change: impl Fn(f32) -> Message + 'static,
) -> Element<'a, Message> {
    let control = NumberEntry::new(value, on_change)
        .range(min, max)
        .step(step)
        .drag_per_px(drag_per_px)
        .suffix(suffix)
        .width(70.0);
    iced::widget::row![
        iced::widget::text(label)
            .size(10)
            .width(iced::Length::Fixed(58.0)),
        iced::widget::container(control).center_x(iced::Length::Fill),
    ]
    .width(iced::Length::Fill)
    .align_y(iced::alignment::Vertical::Center)
    .spacing(4)
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];
}

impl std::fmt::Display for TextAlign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TextAlign::Left => "Left",
            TextAlign::Center => "Center",
            TextAlign::Right => "Right",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Text {
    pub content: String,
    pub font: String,
    pub align: TextAlign,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub rotation: f32,
    pub opacity: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            content: String::new(),
            font: String::new(),
            align: TextAlign::Left,
            x: 0.5,
            y: 0.5,
            size: 48.0,
            rotation: 0.0,
            opacity: 1.0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }
    }
}

impl ModifierImpl for Text {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn has_effect(&self) -> bool {
        !self.content.is_empty() && self.opacity > 0.0
    }

    fn input_class(&self) -> InputClass {
        InputClass::NonPointwise
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::TextContent(v) => self.content = v,
            ModifierParam::TextFont(v) => self.font = v,
            ModifierParam::TextAlign(v) => self.align = v,
            ModifierParam::TextX(v) => self.x = v,
            ModifierParam::TextY(v) => self.y = v,
            ModifierParam::TextSize(v) => self.size = v,
            ModifierParam::TextRotation(v) => self.rotation = v,
            ModifierParam::TextOpacity(v) => self.opacity = v,
            ModifierParam::TextColor([r, g, b]) => {
                self.r = r;
                self.g = g;
                self.b = b;
            }
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        18u8.hash(hasher);
        self.content.hash(hasher);
        self.font.hash(hasher);
        (self.align as u8).hash(hasher);
        hash_f32(self.x, hasher);
        hash_f32(self.y, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.rotation, hasher);
        hash_f32(self.opacity, hasher);
        hash_f32(self.r, hasher);
        hash_f32(self.g, hasher);
        hash_f32(self.b, hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        let fonts = crate::modifiers::text_render::font_families();
        let selected_font = if self.font.is_empty() {
            None
        } else {
            Some(self.font.clone())
        };
        let font_picker = iced::widget::pick_list(fonts, selected_font, move |f| {
            EditMsg::Update(index, ModifierParam::TextFont(f)).into()
        })
        .placeholder("Default font")
        .text_size(11)
        .width(iced::Length::Fill)
        .padding([4, 6]);

        let align_picker = iced::widget::pick_list(
            &TextAlign::ALL[..],
            Some(self.align),
            move |a| EditMsg::Update(index, ModifierParam::TextAlign(a)).into(),
        )
        .text_size(11)
        .width(iced::Length::Fill)
        .padding([4, 6]);

        finish(column![
            text_input("Type something...", &self.content)
                .on_input(move |v| EditMsg::Update(index, ModifierParam::TextContent(v)).into())
                .size(11)
                .padding([4, 6]),
            font_picker,
            align_picker,
            entry_row("X", self.x, -5.0, 5.0, 0.01, 0.005, "", move |v| {
                EditMsg::Update(index, ModifierParam::TextX(v)).into()
            }),
            entry_row("Y", self.y, -5.0, 5.0, 0.01, 0.005, "", move |v| {
                EditMsg::Update(index, ModifierParam::TextY(v)).into()
            }),
            entry_row("Size", self.size, 1.0, 100000.0, 1.0, 1.0, "px", move |v| {
                EditMsg::Update(index, ModifierParam::TextSize(v)).into()
            }),
            value_row(
                "Rotation",
                self.rotation,
                -180.0..=180.0,
                0.5,
                Fmt::num(0).suffix("\u{00b0}"),
                move |v| EditMsg::Update(index, ModifierParam::TextRotation(v)).into(),
            ),
            value_row(
                "Opacity",
                self.opacity,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::TextOpacity(v)).into(),
            ),
            iced::widget::row![
                iced::widget::text("Color")
                    .size(10)
                    .width(iced::Length::Fixed(58.0)),
                iced::widget::container(crate::widgets::color_swatch::ColorSwatch::new(
                    self.r,
                    self.g,
                    self.b,
                    move |rgb| EditMsg::Update(index, ModifierParam::TextColor(rgb)).into()
                ))
                .center_x(iced::Length::Fill),
            ]
            .width(iced::Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .spacing(4),
        ])
    }
}
