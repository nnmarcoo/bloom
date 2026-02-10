use iced::{Font, Theme, application};

use crate::app::Img;

mod app;
mod comps;
mod constants;
mod wgpu;

fn main() -> iced::Result {
    application(Img::default, Img::update, Img::view)
        .title("img")
        .subscription(Img::subscription)
        .theme(|_: &Img| Theme::Nord)
        .default_font(Font::MONOSPACE)
        .centered()
        .run()
}
