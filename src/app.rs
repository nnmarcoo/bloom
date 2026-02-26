use std::path::PathBuf;
use std::time::Instant;

use glam::Vec2;
use iced::time::every;
use iced::{
    Element, Event, Length, Rectangle, Subscription, Task, event,
    keyboard::{
        self,
        key::{self, Physical},
    },
    widget::{column, shader},
    window::{self, Mode},
};
use rfd::AsyncFileDialog;

use crate::{
    components::bottom_bar,
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
}

impl Default for App {
    fn default() -> Self {
        Self {
            program: ViewProgram::default(),
            gallery: Gallery::default(),
            mode: Mode::Windowed,
            loading: None,
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
    MediaLoaded(MediaData),
    MediaFailed(String),
    AnimationTick(Instant),
    ToggleFullscreen,
    ToggleLanczos,
    Noop,
}

fn load_media(path: PathBuf) -> Task<Message> {
    Task::future(async move {
        match tokio::task::spawn_blocking(move || ImageData::load_media(&path)).await {
            Ok(Ok(media)) => Message::MediaLoaded(media),
            Ok(Err(e)) => Message::MediaFailed(e.to_string()),
            Err(e) => Message::MediaFailed(e.to_string()),
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
            let gallery = Gallery::new(&p);
            let filename = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let app = Self {
                program: ViewProgram::default(),
                gallery,
                mode: Mode::Windowed,
                loading: Some(filename),
            };
            return (app, load_media(p));
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
                    let filename = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.loading = Some(filename);
                    return load_media(p.clone());
                }
            }
            Message::MediaLoaded(media) => {
                self.loading = None;
                match media {
                    MediaData::Image(data) => self.program.set_image(data),
                    MediaData::Animation(anim) => self.program.set_animation(anim),
                }
                self.program.fit();
            }
            Message::AnimationTick(now) => {
                self.program.tick_animation(now);
            }
            Message::MediaFailed(err) => {
                self.loading = None;
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
            Message::Noop => {}
            Message::Event(event) => match event {
                Event::Window(window::Event::FileDropped(path)) => {
                    return Task::done(Message::MediaSelected(path));
                }
                Event::Keyboard(keyboard::Event::KeyPressed { physical_key, .. }) => {
                    match physical_key {
                        Physical::Code(key::Code::ArrowRight) => {
                            return Task::done(Message::Next);
                        }
                        Physical::Code(key::Code::ArrowLeft) => {
                            return Task::done(Message::Previous);
                        }
                        Physical::Code(key::Code::KeyF) => {
                            return Task::done(Message::ToggleFullscreen);
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        column![
            shader(self.program.clone())
                .height(Length::Fill)
                .width(Length::Fill),
            bottom_bar::view(
                self.mode,
                self.loading.as_deref(),
                self.program.lanczos_enabled
            )
        ]
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let events = event::listen().map(Message::Event);

        if let Some(delay) = self.program.time_until_next_frame() {
            Subscription::batch([events, every(delay).map(Message::AnimationTick)])
        } else {
            events
        }
    }
}
