use iced::{Element, Size, Subscription, widget::column, window};
use rfd::FileDialog;

use crate::comps::{bottom_row::bottom_row, main_panel::data_panel};

#[derive(Debug, Default)]
pub struct Img {}

#[derive(Debug, Clone)]
pub enum Message {
    FileSelect,
}

impl Img {
    pub fn update(&mut self, message: Message) {}

    pub fn view(&self) -> Element<Message> {
        column![data_panel(), bottom_row()].into()
    }
}
