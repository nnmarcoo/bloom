use std::path::PathBuf;

use iced::window::{self, Level, Mode};

use crate::app::Message;
use crate::{
    clipboard::{self, ClipboardImage},
    gallery::SUPPORTED,
    wgpu::media::{
        animation::Frame,
        image_data::{ImageData, MediaData},
    },
};

fn is_streamed_animation(path: &PathBuf) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "gif" | "apng"
    )
}

pub fn load_media(path: PathBuf, generation: u64) -> iced::Task<Message> {
    // WebP needs a quick peek to decide if it's animated before we can choose a path.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "webp" {
        // Open once to check; if animated, stream; otherwise fall through to static load.
        return iced::Task::future(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
            let path2 = path.clone();
            std::thread::spawn(move || {
                let animated = std::fs::File::open(&path2)
                    .ok()
                    .and_then(|f| {
                        image_webp::WebPDecoder::new(std::io::BufReader::new(f)).ok()
                    })
                    .map(|d| d.is_animated())
                    .unwrap_or(false);
                let _ = tx.send(animated);
            });
            match rx.await {
                Ok(true) => return Message::StreamAnimation(path, generation),
                _ => {}
            }
            // Static WebP: fall back to normal load
            let (tx2, rx2) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx2.send(ImageData::load_media(&path));
            });
            match rx2.await {
                Ok(Ok(media)) => Message::MediaLoaded(generation, media),
                Ok(Err(e)) => Message::MediaFailed(generation, e.to_string()),
                Err(_) => Message::MediaFailed(generation, "load thread panicked".to_string()),
            }
        });
    }

    if is_streamed_animation(&path) {
        return stream_animation(path, generation);
    }

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

pub fn stream_animation(path: PathBuf, generation: u64) -> iced::Task<Message> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    iced::Task::stream(iced::stream::channel(8, async move |mut output| {
        use iced::futures::SinkExt;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Frame, String>>(8);

        // Helper: drains any concrete iterator into the channel.
        // Called inside the spawn closure so no Send bound on the iterator itself.
        fn send_frames(
            iter: impl Iterator<Item = Result<Frame, image::ImageError>>,
            tx: &tokio::sync::mpsc::Sender<Result<Frame, String>>,
        ) {
            for result in iter {
                if tx.blocking_send(result.map_err(|e| e.to_string())).is_err() {
                    return;
                }
            }
        }

        std::thread::spawn(move || match ext.as_str() {
            "gif" => match ImageData::iter_gif_frames(&path) {
                Ok(it) => send_frames(it, &tx),
                Err(e) => { let _ = tx.blocking_send(Err(e.to_string())); }
            },
            "apng" => match ImageData::iter_apng_frames(&path) {
                Ok(it) => send_frames(it, &tx),
                Err(e) => { let _ = tx.blocking_send(Err(e.to_string())); }
            },
            "webp" => match ImageData::iter_webp_frames(&path) {
                Ok(it) => send_frames(it, &tx),
                Err(e) => { let _ = tx.blocking_send(Err(e.to_string())); }
            },
            _ => {}
        });

        while let Some(result) = rx.recv().await {
            match result {
                Ok(frame) => {
                    let _ = output
                        .send(Message::AnimationFrameLoaded(generation, frame))
                        .await;
                }
                Err(e) => {
                    let _ = output
                        .send(Message::MediaFailed(generation, e))
                        .await;
                    return;
                }
            }
        }

        let _ = output.send(Message::AnimationDone(generation)).await;
    }))
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
