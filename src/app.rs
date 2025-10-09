use glam::{Vec2, vec2};
use iced::{
    Element,
    Length::Fill,
    Rectangle,
    widget::{column, shader},
};
use rfd::FileDialog;

use crate::{comps::bottom_row::bottom_row, wgpu::program::FragmentShaderProgram};

#[derive(Debug, Default)]
pub struct Img {
    program: FragmentShaderProgram,
}

#[derive(Debug, Clone)]
pub enum Message {
    SetImage,
    PanDelta(Vec2),
    ZoomDelta(Vec2, Rectangle, f32),
}

impl Img {
    pub fn new() -> Self {
        Self {
            program: FragmentShaderProgram::new(),
        }
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::PanDelta(delta) => {
                self.program.controls.pos += 2. * delta / self.program.controls.scale();
            }

            Message::ZoomDelta(cursor, bounds, delta) => {
                let prev_scale = self.program.controls.scale();

                if delta > 0.0 {
                    self.program.controls.scale_up();
                } else if delta < 0.0 {
                    self.program.controls.scale_down();
                }

                let new_scale = self.program.controls.scale();
                if (new_scale - prev_scale).abs() < f32::EPSILON {
                    return;
                }

                let cursor = vec2(cursor.x as f32, cursor.y as f32);
                let res = vec2(bounds.width as f32, bounds.height as f32);

                let ndc: Vec2 = vec2(
                    (cursor.x / res.x) * 2.0 - 1.0,
                    1.0 - (cursor.y / res.y) * 2.0,
                );

                let factor = (1.0 / new_scale) - (1.0 / prev_scale);
                let delta_pos = res * ndc * factor;

                self.program.controls.pos += delta_pos;
            }

            Message::SetImage => {
                if let Some(path) = FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    todo!("set texture");
                }
            }
        }
    }

    pub fn view(&self) -> Element<Message> {
        let bottom_row = bottom_row(self.program.controls.pos);
        let shader = shader(&self.program).width(Fill).height(Fill);

        column![shader, bottom_row].into()
    }
}
