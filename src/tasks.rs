use std::path::PathBuf;

use iced::window::{self, Level, Mode};

use crate::app::Message;
use crate::{
    clipboard::{self, ClipboardImage},
    gallery::SUPPORTED,
    wgpu::media::image_data::{ImageData, MediaData},
};

pub fn load_media(path: PathBuf, generation: u64) -> iced::Task<Message> {
    iced::Task::future(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ImageData::load_media(&path));
        });
        match rx.await {
            Ok(Ok(media)) => Message::MediaLoaded(generation, media),
            Ok(Err(e)) => Message::MediaFailed(generation, e.to_string()),
            Err(_) => Message::MediaFailed(generation, "load thread panicked".to_string()),
        }
    })
}

pub fn load_from_clipboard() -> iced::Task<Message> {
    iced::Task::future(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(clipboard::read());
        });
        match rx.await {
            Ok(Some(ClipboardImage::Pixels(data))) => {
                Message::ClipboardLoaded(MediaData::Image(data))
            }
            Ok(Some(ClipboardImage::Path(path))) => Message::MediaSelected(path),
            _ => Message::Noop,
        }
    })
}

pub fn select_media() -> iced::Task<Message> {
    iced::Task::future(async {
        let handle = rfd::AsyncFileDialog::new()
            .add_filter("Media", SUPPORTED)
            .pick_file()
            .await;
        match handle {
            Some(h) => Message::MediaSelected(h.path().to_path_buf()),
            None => Message::Noop,
        }
    })
}

pub fn set_window_mode(mode: Mode) -> iced::Task<Message> {
    window::oldest().then(move |id| match id {
        Some(id) => window::set_mode(id, mode),
        None => iced::Task::none(),
    })
}

pub fn toggle_decorations() -> iced::Task<Message> {
    window::oldest().then(|id| match id {
        Some(id) => window::toggle_decorations(id),
        None => iced::Task::none(),
    })
}

pub fn set_always_on_top(v: bool) -> iced::Task<Message> {
    let level = if v { Level::AlwaysOnTop } else { Level::Normal };
    window::oldest().then(move |id| match id {
        Some(id) => window::set_level(id, level),
        None => iced::Task::none(),
    })
}

pub fn close_window() -> iced::Task<Message> {
    window::oldest().then(|id| match id {
        Some(id) => window::close(id),
        None => iced::Task::none(),
    })
}
