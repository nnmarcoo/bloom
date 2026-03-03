use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec2;
use iced::time::every;
use iced::{
    Element, Event, Rectangle, Subscription, Task, Theme, event,
    keyboard::{
        self,
        key::{self, Physical},
    },
    widget::column,
    window::Mode,
};

use crate::{
    clipboard,
    components::{bottom_bar, preferences, preferences::PreferenceMessage, viewer},
    config::Config,
    gallery::Gallery,
    keybinds::{Action, KeyBinding},
    styles, tasks,
    wgpu::{media::image_data::MediaData, view_program::ViewProgram},
};

pub struct App {
    program: ViewProgram,
    gallery: Gallery,
    mode: Mode,
    loading: Option<String>,
    load_generation: u64,
    focus_scale: bool,
    show_preferences: bool,
    config: Config,
    pending_config: Config,
    prefs_state: preferences::PrefsState,
    context_menu_pos: Option<Vec2>,
}

impl Default for App {
    fn default() -> Self {
        let config = Config::load();
        let mut program = ViewProgram::default();
        program.lanczos_enabled = config.lanczos;
        styles::set_radius(config.rounded);
        Self {
            program,
            gallery: Gallery::default(),
            mode: Mode::Windowed,
            loading: None,
            load_generation: 0,
            focus_scale: false,
            show_preferences: false,
            pending_config: config.clone(),
            config,
            prefs_state: preferences::PrefsState::default(),
            context_menu_pos: None,
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
    ToggleInfoColumn,
    TogglePreferences,
    Preference(PreferenceMessage),
    ClipboardLoaded(MediaData),
    CursorMoved(Vec2),
    CursorLeft,
    ContextMenuOpened(Vec2),
    CopyColor,
    CopyPath,
    Rotate,
    Exit,
    Noop,
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
            return (app, tasks::load_media(p, 1));
        }
        (Self::default(), Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.focus_scale = false;

        match message {
            Message::Pan(delta) => self.program.pan(delta),
            Message::ScaleUp(cursor) => self.program.scale_up(cursor),
            Message::ScaleDown(cursor) => self.program.scale_down(cursor),
            Message::Rotate => self.program.rotate(),
            Message::Fit => self.program.fit(),
            Message::BoundsChanged(bounds) => self.program.set_bounds(bounds),
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
            Message::SelectMedia => return tasks::select_media(),
            Message::MediaSelected(path) => {
                if let Some(p) = self.gallery.set(path) {
                    self.loading = Some(Gallery::filename(p));
                    self.load_generation = self.load_generation.wrapping_add(1);
                    return tasks::load_media(p.clone(), self.load_generation);
                }
            }
            Message::MediaLoaded(generation, media) => {
                if generation == self.load_generation {
                    self.loading = None;
                    self.apply_media(media);
                }
            }
            Message::ClipboardLoaded(media) => {
                self.loading = None;
                self.apply_media(media);
            }
            Message::AnimationTick(now) => self.program.tick_animation(now),
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
                return tasks::set_window_mode(self.mode);
            }
            Message::ToggleInfoColumn => {
                self.config.show_info = !self.config.show_info;
                self.config.save();
            }
            Message::TogglePreferences => {
                self.pending_config = self.config.clone();
                self.prefs_state = preferences::PrefsState::default();
                self.show_preferences = true;
            }
            Message::Preference(msg) => {
                let saving = matches!(msg, PreferenceMessage::Save);
                let decorations_before = saving.then_some(self.config.decorations);
                let always_on_top_before = saving.then_some(self.config.always_on_top);
                self.show_preferences = preferences::update(
                    msg,
                    &mut self.config,
                    &mut self.pending_config,
                    &mut self.program,
                    &mut self.prefs_state,
                );
                if saving {
                    self.config.save();
                    let dec = decorations_before != Some(self.config.decorations);
                    let aot = always_on_top_before != Some(self.config.always_on_top);
                    if dec || aot {
                        return Task::batch([
                            if dec { tasks::toggle_decorations() } else { Task::none() },
                            if aot { tasks::set_always_on_top(self.config.always_on_top) } else { Task::none() },
                        ]);
                    }
                }
            }
            Message::CursorMoved(pos) => {
                if !self.show_preferences {
                    self.program.set_cursor_pos(Some(pos));
                }
            }
            Message::CursorLeft => self.program.set_cursor_pos(None),
            Message::ContextMenuOpened(pos) => self.context_menu_pos = Some(pos),
            Message::CopyColor => {
                if let Some(pos) = self.context_menu_pos {
                    if let Some((_, _, [r, g, b, _])) = self.program.color_at(pos) {
                        clipboard::write_text(&format!("#{r:02X}{g:02X}{b:02X}"));
                    }
                }
            }
            Message::CopyPath => {
                if let Some(path) = self.gallery.current() {
                    clipboard::write_text(&path.to_string_lossy());
                }
            }
            Message::Exit => return tasks::close_window(),
            Message::Noop => {}
            Message::Event(event) => return self.handle_event(event),
        }
        Task::none()
    }

    fn apply_media(&mut self, media: MediaData) {
        match media {
            MediaData::Image(data) => self.program.set_image(data),
            MediaData::Animation(anim) => self.program.set_animation(anim),
        }
        self.program.fit();
    }

    fn handle_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Window(iced::window::Event::FileDropped(path)) => {
                Task::done(Message::MediaSelected(path))
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                physical_key,
                modifiers,
                ..
            }) => self.handle_key(physical_key, modifiers),
            _ => Task::none(),
        }
    }

    fn handle_key(
        &mut self,
        physical_key: Physical,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        if self.show_preferences {
            if let Some(action) = self.prefs_state.capturing {
                if let Physical::Code(code) = physical_key {
                    let is_modifier = matches!(
                        code,
                        key::Code::ControlLeft
                            | key::Code::ControlRight
                            | key::Code::ShiftLeft
                            | key::Code::ShiftRight
                            | key::Code::AltLeft
                            | key::Code::AltRight
                            | key::Code::SuperLeft
                            | key::Code::SuperRight
                    );
                    if !is_modifier {
                        if code == key::Code::Escape {
                            return Task::done(Message::Preference(
                                PreferenceMessage::CancelCapture,
                            ));
                        }
                        let kb = KeyBinding {
                            ctrl: modifiers.control(),
                            shift: modifiers.shift(),
                            alt: modifiers.alt(),
                            code,
                        };
                        return Task::done(Message::Preference(PreferenceMessage::SetKeybinding(
                            action, kb,
                        )));
                    }
                }
            }
            return Task::none();
        }

        match self.config.keymap.resolve(&physical_key, &modifiers) {
            Some(Action::Next) => Task::done(Message::Next),
            Some(Action::Previous) => Task::done(Message::Previous),
            Some(Action::ToggleFullscreen) => Task::done(Message::ToggleFullscreen),
            Some(Action::FocusScale) => {
                self.focus_scale = true;
                Task::none()
            }
            Some(Action::PasteFromClipboard) => tasks::load_from_clipboard(),
            Some(Action::ZoomIn) => Task::done(Message::ScaleUp(self.program.viewport_center())),
            Some(Action::ZoomOut) => Task::done(Message::ScaleDown(self.program.viewport_center())),
            Some(Action::ZoomFit) => Task::done(Message::Fit),
            Some(Action::ZoomPreset(n)) => Task::done(Message::Scale(n as f32)),
            None => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.show_preferences {
            return preferences::view(&self.pending_config, &self.config.theme, &self.prefs_state);
        }
        column![
            viewer::view(
                self.program.clone(),
                self.loading.as_deref(),
                self.config.show_info,
                self.gallery.current().map(|p| p.as_path()),
                &self.gallery,
                &self.config.theme,
            ),
            bottom_bar::view(
                self.mode,
                self.program.scale(),
                self.program.rotation(),
                self.focus_scale,
                self.config.show_info,
            ),
        ]
        .into()
    }

    pub fn title(&self) -> String {
        self.gallery
            .current()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "bloom".into())
    }

    pub fn theme(&self) -> Theme {
        self.config.theme.clone()
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
