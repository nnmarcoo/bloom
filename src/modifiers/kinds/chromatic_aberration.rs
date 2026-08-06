use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use iced::Element;
use iced::widget::column;

use crate::app::{EditMsg, Message};
use crate::modifiers::{InputRequest, ModifierImpl, ModifierParam, ViewCtx};
use crate::widgets::value_slider::Fmt;

use super::{finish, hash_f32, value_row};

#[derive(Debug, Clone)]
pub struct ChromaticAberration {
    pub amount: f32,
}

impl Default for ChromaticAberration {
    fn default() -> Self {
        Self { amount: 5.0 }
    }
}

impl ModifierImpl for ChromaticAberration {
    fn name(&self) -> &'static str {
        "Chromatic Aberration"
    }

    fn has_effect(&self) -> bool {
        self.amount != 0.0
    }

    /// Genuinely full-frame, despite `amount` looking like a radius.
    ///
    /// The fringe offset scales the sample coordinate about the image centre
    /// (`0.5 + offset * (1 ± amount)`), so displacement grows with distance
    /// from the centre and peaks at the corners — for `amount` 5.0 on a 4000px
    /// image that is thousands of pixels, not five. There is no bounded apron
    /// to declare.
    fn input_request(&self) -> InputRequest {
        InputRequest::FullFrame
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        if let ModifierParam::ChromaticAberrationAmount(v) = param {
            self.amount = v;
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        13u8.hash(hasher);
        hash_f32(self.amount, hasher);
    }

    fn view(&self, index: usize, _ctx: ViewCtx) -> Element<'_, Message> {
        finish(column![value_row(
            "Amount",
            self.amount,
            0.0..=50.0,
            0.1,
            Fmt::num(1),
            move |v| EditMsg::Update(index, ModifierParam::ChromaticAberrationAmount(v)).into(),
        )])
    }
}
