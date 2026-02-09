use std::fs;

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
    wgpu::program::ImageProgram,
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
                let scale = self.program.view.scale();
                self.program.view.pan += 2.0 * delta / scale;
            }

            Message::ZoomDelta(cursor, bounds, delta) => {
                let prev_scale = self.program.view.scale();

                if delta > 0.0 {
                    self.program.view.scale_up();
                } else if delta < 0.0 {
                    self.program.view.scale_down();
                }

                let new_scale = self.program.view.scale();
                let viewport = vec2(bounds.width, bounds.height);

                // Convert cursor from screen-space to NDC (-1..1)
                let ndc = vec2(
                    (cursor.x / viewport.x) * 2.0 - 1.0,
                    1.0 - (cursor.y / viewport.y) * 2.0,
                );

                // Adjust pan so the point under the cursor stays fixed
                let factor = (1.0 / new_scale) - (1.0 / prev_scale);
                self.program.view.pan += viewport * ndc * factor;
            }

            Message::SetImage => {
                if let Some(path) = FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "webp", "gif"])
                    .pick_file()
                {
                    if let Ok(bytes) = fs::read(&path) {
                        self.program.set_pending_image(bytes);
                        self.program.view.reset();
                    }
                }
            }
        }
    }

    pub fn view(&self) -> Element<Message> {
        let view = &self.program.view;
        let bottom_row = bottom_row(view.pan, view.scale());
        let shader = shader(&self.program).width(Fill).height(Fill);

        column![shader, bottom_row].into()
    }
}
