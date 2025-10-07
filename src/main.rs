use iced::{Font, Theme, application};

use crate::app::Img;

mod app;
mod comps;
mod wgpu;

fn main() -> iced::Result {
    application("img", Img::update, Img::view)
        .theme(move |_| Theme::Nord)
        .default_font(Font::MONOSPACE)
        .centered()
        .run()
}
