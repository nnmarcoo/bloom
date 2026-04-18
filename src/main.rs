#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animation;
mod app;
mod clipboard;
mod components;
mod config;
mod gallery;
mod keybinds;
mod modifiers;
mod styles;
mod tasks;
mod ui;
mod wgpu;
mod widgets;
use std::{env, path::PathBuf};

use app::App;
use iced::{Size, window};

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
    let config = config::Config::load();
    let level = if config.always_on_top {
        window::Level::AlwaysOnTop
    } else {
        window::Level::Normal
    };

    iced::application(move || App::new(media.clone()), App::update, App::view)
        .title(App::title)
        .window(window::Settings {
            min_size: Some(Size::new(460.0, 220.0)),
            decorations: config.decorations,
            level,
            icon: app_icon(),
            ..Default::default()
        })
        .centered()
        .theme(App::theme)
        .subscription(App::subscription)
        .scale_factor(App::scale_factor)
        .run()
}
