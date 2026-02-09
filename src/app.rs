use std::fs;

use glam::{Vec2, vec2};
use iced::{
    Element, Subscription,
    Length::Fill,
    Rectangle,
    advanced::graphics::image::image_rs,
    widget::{column, shader},
};
use rfd::FileDialog;

use crate::{
    comps::bottom_row::bottom_row,
    wgpu::{primitive::ViewState, program::ImageProgram},
};

#[derive(Debug, Default)]
pub struct Img {
    program: ImageProgram,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    SetImage,
    PanDelta(Vec2),
    ZoomDelta(Vec2, Rectangle, f32),
}

impl Img {
    pub fn update(&mut self, message: Message) {
        self.program.resolve_scale();

        match message {
            Message::Tick => {}

            Message::PanDelta(delta) => {
                self.program.view.pan += 2.0 * delta / self.program.view.scale;
                self.program.view.clamp_pan();
            }

            Message::ZoomDelta(cursor, bounds, delta) => {
                let viewport = vec2(bounds.width, bounds.height);
                let prev_scale = self.program.view.scale;

                if delta > 0.0 {
                    self.program.view.scale_up();
                } else if delta < 0.0 {
                    self.program.view.scale_down();
                }

                let new_scale = self.program.view.scale;
                let ndc = vec2(
                    (cursor.x / viewport.x) * 2.0 - 1.0,
                    1.0 - (cursor.y / viewport.y) * 2.0,
                );
                let factor = (1.0 / new_scale) - (1.0 / prev_scale);
                self.program.view.pan += viewport * ndc * factor;
                self.program.view.clamp_pan();
            }

            Message::SetImage => {
                if let Some(path) = FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "webp", "gif"])
                    .pick_file()
                {
                    if let Ok(bytes) = fs::read(&path) {
                        let mut view = ViewState::default();
                        if let Ok(reader) =
                            image_rs::io::Reader::new(std::io::Cursor::new(&bytes))
                                .with_guessed_format()
                        {
                            if let Ok((w, h)) = reader.into_dimensions() {
                                view.image_size = vec2(w as f32, h as f32);
                            }
                        }
                        self.program.set_pending_image(bytes);
                        self.program.view = view;
                    }
                }
            }
        }
    }

    pub fn view(&self) -> Element<Message> {
        let bottom_row = bottom_row(self.program.view.pan, self.program.view.scale);
        let shader = shader(&self.program).width(Fill).height(Fill);
        column![shader, bottom_row].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.program.view.scale == 0.0 {
            iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }
}
