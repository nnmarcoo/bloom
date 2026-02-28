#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod components;
mod gallery;
mod styles;
mod wgpu;
mod widgets;
use std::{env, path::PathBuf};

use app::App;
use iced::window;

fn app_icon() -> Option<window::Icon> {
    #[cfg(target_os = "linux")]
    return window::icon::from_file_data(include_bytes!("../assets/logo/bloom64.png"), None).ok();
    #[cfg(windows)]
    return window::icon::from_file_data(include_bytes!("../assets/logo/bloom32.png"), None).ok();
    #[allow(unreachable_code)]
    None
}

fn main() -> iced::Result {
    let media = env::args().nth(1).map(PathBuf::from);

    iced::application(move || App::new(media.clone()), App::update, App::view)
        .title("bloom")
        .window(window::Settings {
            icon: app_icon(),
            ..Default::default()
        })
        .centered()
        .subscription(App::subscription)
        .run()
}
