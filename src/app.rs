use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec2;
use iced::time::every;
use iced::{
    Element, Event, Rectangle, Subscription, Task, event,
    keyboard::{
        self,
        key::{self, Physical},
    },
    widget::column,
    window::{self, Mode},
};
use rfd::AsyncFileDialog;

use crate::{
    clipboard::{self, ClipboardImage},
    components::{bottom_bar, viewer},
    gallery::{Gallery, SUPPORTED},
    wgpu::{
        media::image_data::{ImageData, MediaData},
        view_program::ViewProgram,
    },
};

pub struct App {
    program: ViewProgram,
    gallery: Gallery,
    mode: Mode,
    loading: Option<String>,
    load_generation: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            program: ViewProgram::default(),
            gallery: Gallery::default(),
            mode: Mode::Windowed,
            loading: None,
            load_generation: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Pan(Vec2),
    ScaleUp(Vec2),
    ScaleDown(Vec2),
    Fit,
    BoundsChanged(Rectangle),
    Scale(f32),
    Event(Event),
    Next,
    Previous,
    SelectMedia,
    MediaSelected(PathBuf),
    MediaLoaded(u64, MediaData),
    MediaFailed(u64, String),
    AnimationTick(Instant),
    ToggleFullscreen,
    ToggleLanczos,
    ClipboardLoaded(MediaData),
    Noop,
}

fn load_media(path: PathBuf, generation: u64) -> Task<Message> {
    Task::future(async move {
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

fn load_from_clipboard() -> Task<Message> {
    Task::future(async move {
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

fn set_window_mode(mode: Mode) -> Task<Message> {
    window::oldest().then(move |id| {
        if let Some(id) = id {
            window::set_mode(id, mode)
        } else {
            Task::none()
        }
    })
}

impl App {
    pub fn new(path: Option<PathBuf>) -> (Self, Task<Message>) {
        if let Some(p) = path {
            let app = Self {
                gallery: Gallery::new(&p),
                loading: Some(Gallery::filename(&p)),
                load_generation: 1,
                ..Self::default()
            };
            return (app, load_media(p, 1));
        }
        (Self::default(), Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Pan(delta) => {
                self.program.pan(delta);
            }
            Message::ScaleUp(cursor) => {
                self.program.scale_up(cursor);
            }
            Message::ScaleDown(cursor) => {
                self.program.scale_down(cursor);
            }
            Message::Fit => {
                self.program.fit();
            }
            Message::BoundsChanged(bounds) => {
                self.program.set_bounds(bounds);
            }
            Message::Scale(scale) => {
                let center = self.program.viewport_center();
                self.program.set_scale(scale, center);
            }
            Message::Next => {
                if let Some(p) = self.gallery.next() {
                    return Task::done(Message::MediaSelected(p.clone()));
                }
            }
            Message::Previous => {
                if let Some(p) = self.gallery.previous() {
                    return Task::done(Message::MediaSelected(p.clone()));
                }
            }
            Message::SelectMedia => {
                return Task::future(async {
                    let handle = AsyncFileDialog::new()
                        .add_filter("Media", SUPPORTED)
                        .pick_file()
                        .await;
                    if let Some(h) = handle {
                        Message::MediaSelected(h.path().to_path_buf())
                    } else {
                        Message::Noop
                    }
                });
            }
            Message::MediaSelected(path) => {
                if let Some(p) = self.gallery.set(path) {
                    self.loading = Some(Gallery::filename(p));
                    self.load_generation = self.load_generation.wrapping_add(1);
                    return load_media(p.clone(), self.load_generation);
                }
            }
            Message::MediaLoaded(generation, media) => {
                if generation == self.load_generation {
                    self.loading = None;
                    match media {
                        MediaData::Image(data) => self.program.set_image(data),
                        MediaData::Animation(anim) => self.program.set_animation(anim),
                    }
                    self.program.fit();
                }
            }
            Message::AnimationTick(now) => {
                self.program.tick_animation(now);
            }
            Message::MediaFailed(generation, err) => {
                if generation == self.load_generation {
                    self.loading = None;
                }
                eprintln!("Failed to load media: {err}");
            }
            Message::ToggleFullscreen => {
                self.mode = match self.mode {
                    Mode::Fullscreen => Mode::Windowed,
                    _ => Mode::Fullscreen,
                };
                return set_window_mode(self.mode);
            }
            Message::ToggleLanczos => {
                self.program.lanczos_enabled = !self.program.lanczos_enabled;
            }
            Message::ClipboardLoaded(media) => {
                self.loading = None;
                match media {
                    MediaData::Image(data) => self.program.set_image(data),
                    MediaData::Animation(anim) => self.program.set_animation(anim),
                }
                self.program.fit();
            }
            Message::Noop => {}
            Message::Event(event) => match event {
                Event::Window(window::Event::FileDropped(path)) => {
                    return Task::done(Message::MediaSelected(path));
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    physical_key,
                    modifiers,
                    ..
                }) => match physical_key {
                    Physical::Code(key::Code::ArrowRight) => {
                        return Task::done(Message::Next);
                    }
                    Physical::Code(key::Code::ArrowLeft) => {
                        return Task::done(Message::Previous);
                    }
                    Physical::Code(key::Code::KeyF) => {
                        return Task::done(Message::ToggleFullscreen);
                    }
                    Physical::Code(key::Code::KeyV) if modifiers.control() => {
                        return load_from_clipboard();
                    }
                    _ => {}
                },
                _ => {}
            },
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        column![
            viewer::view(self.program.clone(), self.loading.as_deref()),
            bottom_bar::view(
                self.mode,
                self.program.lanczos_enabled,
                self.program.scale()
            ),
        ]
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let events = event::listen().map(Message::Event);

        if let Some(delay) = self.program.time_until_next_frame() {
            let delay = delay.max(Duration::from_millis(1));
            Subscription::batch([events, every(delay).map(Message::AnimationTick)])
        } else {
            events
        }
    }
}
