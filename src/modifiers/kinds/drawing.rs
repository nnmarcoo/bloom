use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use iced::widget::{button, column, row, text};
use iced::{Element, Length};

use crate::app::{EditMsg, Message};
use crate::modifiers::{InputRequest, ModifierImpl, ModifierParam};
use crate::widgets::color_swatch::ColorSwatch;
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub size: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub color: [f32; 3],
}

impl Stroke {
    pub fn brush_sig(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        hash_f32(self.size, &mut hasher);
        hash_f32(self.hardness, &mut hasher);
        hash_f32(self.opacity, &mut hasher);
        for c in self.color {
            hash_f32(c, &mut hasher);
        }
        hasher.finish()
    }

    pub fn points_sig(&self, n: usize) -> u64 {
        let mut hasher = DefaultHasher::new();
        for p in &self.points[..n.min(self.points.len())] {
            hash_f32(p[0], &mut hasher);
            hash_f32(p[1], &mut hasher);
        }
        hasher.finish()
    }

    fn hash_into(&self, hasher: &mut DefaultHasher) {
        (self.points.len() as u64).hash(hasher);
        self.points_sig(self.points.len()).hash(hasher);
        self.brush_sig().hash(hasher);
    }
}

#[derive(Debug, Clone)]
pub struct Drawing {
    pub opacity: f32,
    pub size: f32,
    pub hardness: f32,
    pub color: [f32; 3],
    pub strokes: Vec<Stroke>,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            size: 20.0,
            hardness: 0.8,
            color: [1.0, 0.15, 0.15],
            strokes: Vec::new(),
        }
    }
}

impl Drawing {
    pub fn strokes_sig(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (self.strokes.len() as u64).hash(&mut hasher);
        for s in &self.strokes {
            s.hash_into(&mut hasher);
        }
        hasher.finish()
    }
}

impl ModifierImpl for Drawing {
    fn name(&self) -> &'static str {
        "Drawing"
    }

    fn has_effect(&self) -> bool {
        self.strokes
            .iter()
            .any(|s| !s.points.is_empty() && s.opacity > 0.0)
    }

    fn input_request(&self) -> InputRequest {
        InputRequest::FullFrame
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::DrawingOpacity(v) => self.opacity = v,
            ModifierParam::DrawingSize(v) => self.size = v,
            ModifierParam::DrawingHardness(v) => self.hardness = v,
            ModifierParam::DrawingColor(rgb) => self.color = rgb,
            ModifierParam::DrawingStrokeStart(p) => self.strokes.push(Stroke {
                points: vec![p],
                size: self.size,
                hardness: self.hardness,
                opacity: self.opacity,
                color: self.color,
            }),
            ModifierParam::DrawingStrokeExtend(p) => {
                if let Some(s) = self.strokes.last_mut() {
                    s.points.push(p);
                }
            }
            ModifierParam::DrawingUndoStroke => {
                self.strokes.pop();
            }
            ModifierParam::DrawingClear => self.strokes.clear(),
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        19u8.hash(hasher);
        hash_f32(self.opacity, hasher);
        hash_f32(self.size, hasher);
        hash_f32(self.hardness, hasher);
        for c in self.color {
            hash_f32(c, hasher);
        }
        self.strokes_sig().hash(hasher);
    }

    fn view(
        &self,
        index: usize,
        _image_size: Option<(u32, u32)>,
        _rotation: u8,
    ) -> Element<'_, Message> {
        let small_button = |label: &'static str, msg: Message, enabled: bool| {
            let b = button(text(label).size(10))
                .padding([2, 8])
                .style(crate::styles::modifier_add_button_style);
            if enabled { b.on_press(msg) } else { b }
        };
        let has_strokes = !self.strokes.is_empty();

        finish(column![
            value_row(
                "Opacity",
                self.opacity,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::DrawingOpacity(v)).into(),
            ),
            value_row(
                "Size",
                self.size,
                1.0..=300.0,
                0.5,
                Fmt::num(0).suffix("px"),
                move |v| EditMsg::Update(index, ModifierParam::DrawingSize(v)).into()
            ),
            value_row(
                "Hardness",
                self.hardness,
                0.0..=1.0,
                0.01,
                Fmt::num(2),
                move |v| EditMsg::Update(index, ModifierParam::DrawingHardness(v)).into(),
            ),
            row![
                text("Color").size(10).width(Length::Fixed(58.0)),
                iced::widget::container(ColorSwatch::new(
                    self.color[0],
                    self.color[1],
                    self.color[2],
                    move |rgb| EditMsg::Update(index, ModifierParam::DrawingColor(rgb)).into()
                ))
                .center_x(Length::Fill),
            ]
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .spacing(4),
            row![
                small_button(
                    "Undo Stroke",
                    EditMsg::Update(index, ModifierParam::DrawingUndoStroke).into(),
                    has_strokes,
                ),
                small_button(
                    "Clear",
                    EditMsg::Update(index, ModifierParam::DrawingClear).into(),
                    has_strokes,
                ),
            ]
            .spacing(4),
        ])
    }
}
