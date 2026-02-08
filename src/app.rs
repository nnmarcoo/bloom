use glam::{Vec2, vec2};
use iced::{
    Element,
    Length::Fill,
    Rectangle,
    widget::{column, shader},
};
use rfd::FileDialog;

use crate::{
    comps::bottom_row::bottom_row,
    wgpu::{image_data::ScaleDirection, program::ImageProgram},
};

#[derive(Debug, Default)]
pub struct Img {
    program: ImageProgram,
}

#[derive(Debug, Clone)]
pub enum Message {
    SetImage,
    PanDelta(Vec2),
    ZoomDelta(Vec2, Rectangle, f32),
}

impl Img {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::PanDelta(delta) => {
                self.program.view.pan += 2. * delta / self.program.view.scale();
                self.program.view.image.pan(delta);
            }

            Message::ZoomDelta(cursor, bounds, delta) => {
                let prev_scale = self.program.view.scale();

                if delta > 0.0 {
                    self.program.view.scale_up();
                    self.program.view.image.scale(ScaleDirection::UP, cursor);
                } else if delta < 0.0 {
                    self.program.view.scale_down();
                    self.program.view.image.scale(ScaleDirection::DOWN, cursor);
                }

                let new_scale = self.program.view.scale();
                let cursor = vec2(cursor.x, cursor.y);
                let viewport = vec2(bounds.width, bounds.height);

                let ndc = vec2(
                    (cursor.x / viewport.x) * 2.0 - 1.0,
                    1.0 - (cursor.y / viewport.y) * 2.0,
                );

                let factor = (1.0 / new_scale) - (1.0 / prev_scale);
                self.program.view.pan += viewport * ndc * factor;
            }

            Message::SetImage => {
                if let Some(_path) = FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    todo!("set texture");
                }
            }
        }
    }

    pub fn view(&self) -> Element<Message> {
        let bottom_row = bottom_row(self.program.view.pan);
        let shader = shader(&self.program).width(Fill).height(Fill);

        column![shader, bottom_row].into()
    }
}
