use std::path::PathBuf;
use std::sync::Arc;

use futures::SinkExt;
use iced::window::{self, Level, Mode};
use image::ImageError;

use crate::app::{HistogramResult, Message};
use crate::export::{ExportData, do_export, render_still_rgba};
use crate::{
    clipboard::{self, ClipboardImage},
    gallery::SUPPORTED,
    modifiers::Modifier,
    wgpu::media::image_data::{ImageData, ImageId, MediaData},
    wgpu::view_program::compute_subsampled_histogram,
};

pub fn load_media(path: PathBuf, generation: u64) -> iced::Task<Message> {
    iced::Task::future(async move {
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match tokio::task::spawn_blocking(move || ImageData::load_media(&path)).await {
            Ok(Ok(media)) => Message::MediaLoaded(generation, media),
            Ok(Err(e)) => Message::MediaFailed(generation, friendly_error(&e, &filename)),
            Err(_) => Message::MediaFailed(generation, "load thread panicked".to_string()),
        }
    })
}

pub fn load_from_clipboard() -> iced::Task<Message> {
    iced::Task::future(async move {
        match tokio::task::spawn_blocking(clipboard::read).await {
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

pub fn export_image(data: ExportData, suggested_name: String) -> iced::Task<Message> {
    let (mut tx, rx) = futures::channel::mpsc::channel(64);

    let animated = data.frames.len() > 1;

    tokio::spawn(async move {
        let mut dialog = rfd::AsyncFileDialog::new();
        if animated {
            dialog = dialog
                .add_filter("GIF Animation", &["gif"])
                .add_filter("Animated PNG", &["apng"]);
        }
        dialog = dialog
            .add_filter("PNG Image", &["png"])
            .add_filter("JPEG Image", &["jpg", "jpeg"])
            .add_filter("WebP Image", &["webp"]);
        let handle = dialog.set_file_name(&suggested_name).save_file().await;

        let Some(handle) = handle else { return };
        let path = handle.path().to_path_buf();

        let _ = tx.try_send(Message::ExportProgress(0.0));

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<f32>(256);
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<Result<String, String>>();

        std::thread::spawn(move || {
            let result = do_export(data, &path, |p| {
                let _ = progress_tx.blocking_send(p);
            });
            let _ = done_tx.send(result);
        });

        loop {
            tokio::select! {
                Some(p) = progress_rx.recv() => {
                    let _ = tx.try_send(Message::ExportProgress(p));
                }
                result = &mut done_rx => {
                    while let Ok(p) = progress_rx.try_recv() {
                        let _ = tx.try_send(Message::ExportProgress(p));
                    }
                    let msg = match result {
                        Ok(Ok(name)) => Message::ExportDone(Ok(name)),
                        Ok(Err(e)) => Message::ExportDone(Err(e)),
                        Err(_) => Message::ExportDone(Err("thread panicked".to_string())),
                    };
                    let _ = tx.send(msg).await;
                    break;
                }
            }
        }
    });

    iced::Task::stream(rx)
}

pub fn copy_image(data: ExportData) -> iced::Task<Message> {
    iced::Task::future(async move {
        let result = tokio::task::spawn_blocking(move || {
            let (w, h, rgba) = render_still_rgba(&data)?;
            clipboard::write_image(w, h, rgba)
        })
        .await
        .unwrap_or_else(|_| Err("render thread panicked".to_string()));
        Message::CopyImageDone(result)
    })
}

pub fn compute_histogram(
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    modifiers: Arc<Vec<Modifier>>,
    image_id: ImageId,
    modifier_hash: u64,
) -> iced::Task<Message> {
    iced::Task::future(async move {
        let data = tokio::task::spawn_blocking(move || {
            compute_subsampled_histogram(&pixels, width, height, &modifiers)
        })
        .await
        .unwrap_or(([0; 256], [0; 256], [0; 256]));
        Message::HistogramReady(Box::new(HistogramResult {
            image_id,
            modifier_hash,
            data,
        }))
    })
}

pub fn copy_text(text: String) -> iced::Task<Message> {
    iced::Task::future(async move {
        clipboard::write_text(&text);
        Message::Noop
    })
}

pub fn open_file_location(path: PathBuf) -> iced::Task<Message> {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();

    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg("-R").arg(&path).spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = {
        if let Some(parent) = path.parent() {
            Command::new("xdg-open").arg(parent).spawn()
        } else {
            Command::new("xdg-open").arg(&path).spawn()
        }
    };

    iced::Task::none()
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

fn friendly_error(e: &ImageError, filename: &str) -> String {
    let raw = e.to_string();
    if raw.contains("not recognized as an image format") {
        format!("'{filename}' is not a supported format.")
    } else {
        format!("Failed to load '{filename}': {raw}")
    }
}
