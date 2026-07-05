#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod components;
mod config;
mod easing;
mod export;
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

fn install_panic_logger() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = dirs::data_local_dir() {
            let dir = dir.join("bloom");
            let bt = std::backtrace::Backtrace::force_capture();
            let msg = format!("panic: {info}\nbacktrace:\n{bt}\n");
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::write(dir.join("crash.log"), msg);
            }
        }
        default(info);
    }));
}

fn main() -> iced::Result {
    install_panic_logger();

    let _ = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("bloom-rayon-{i}"))
        .stack_size(8 * 1024 * 1024)
        .build_global();

    let media = env::args().nth(1).map(PathBuf::from);
    let config = config::Config::load();
    let level = if config.always_on_top {
        window::Level::AlwaysOnTop
    } else {
        window::Level::Normal
    };
    let decorations = config.decorations;

    iced::application(
        move || App::new(media.clone(), config.clone()),
        App::update,
        App::view,
    )
    .title(App::title)
    .window(window::Settings {
        min_size: Some(Size::new(460.0, 220.0)),
        decorations,
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
