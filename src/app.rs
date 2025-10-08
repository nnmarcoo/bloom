use glam::Vec2;
use iced::{
    Element,
    Length::Fill,
    Rectangle,
    widget::{column, shader},
};
use rfd::FileDialog;

use crate::{
    comps::{bottom_row::bottom_row, main_panel::main_panel},
    wgpu::program::FragmentShaderProgram,
};

#[derive(Debug, Default)]
pub struct Img {
    program: FragmentShaderProgram,
}

#[derive(Debug, Clone)]
pub enum Message {
    FileSelect,
    UpdateZoom(f32),
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
            Message::UpdateZoom(zoom) => {
                self.program.controls.zoom = zoom;
            }

            Message::PanDelta(delta) => {
                self.program.controls.center -= 2.0 * delta * self.program.controls.scale();
            }

            // put actual zoom logic
            Message::ZoomDelta(pos, bounds, delta) => {
                let delta = delta * 0.2;
                let prev_scale = self.program.controls.scale();
                let prev_zoom = self.program.controls.zoom;
                self.program.controls.zoom = (prev_zoom + delta).max(1.).min(17.);

                let vec = pos - Vec2::new(bounds.width, bounds.height) * 0.5;
                let new_scale = self.program.controls.scale();
                self.program.controls.center += vec * (prev_scale - new_scale) * 2.0;
            }

            Message::FileSelect => {}
        }
    }

    pub fn view(&self) -> Element<Message> {
        let shader = shader(&self.program).width(Fill).height(Fill);

        column![shader, bottom_row()].into()
    }
}
