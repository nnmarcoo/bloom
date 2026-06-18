use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::{column, text_input};

use crate::app::{EditMsg, Message};
use crate::modifiers::{InputClass, ModifierImpl, ModifierParam};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
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
            ModifierParam::TextR(v) => self.r = v,
            ModifierParam::TextG(v) => self.g = v,
            ModifierParam::TextB(v) => self.b = v,
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
        finish(column![
            text_input("Type something...", &self.content)
                .on_input(move |v| EditMsg::Update(index, ModifierParam::TextContent(v)).into())
                .size(11)
                .padding([4, 6]),
            value_row("X", self.x, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::TextX(v)).into()
            }),
            value_row("Y", self.y, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::TextY(v)).into()
            }),
            value_row("Size", self.size, 4.0..=200.0, 0.5, Fmt::num(0), move |v| {
                EditMsg::Update(index, ModifierParam::TextSize(v)).into()
            },),
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
            value_row("R", self.r, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::TextR(v)).into()
            }),
            value_row("G", self.g, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::TextG(v)).into()
            }),
            value_row("B", self.b, 0.0..=1.0, 0.01, Fmt::num(2), move |v| {
                EditMsg::Update(index, ModifierParam::TextB(v)).into()
            }),
        ])
    }
}
