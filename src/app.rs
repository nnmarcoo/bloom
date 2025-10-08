use glam::Vec2;
use iced::{Element, Rectangle, widget::column};
use rfd::FileDialog;

use crate::{
    comps::{bottom_row::bottom_row, main_panel::data_panel},
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
    pub fn update(&mut self, message: Message) {}

    pub fn view(&self) -> Element<Message> {
        column![data_panel(), bottom_row()].into()
    }
}
