use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec2;
use iced::time::every;
use iced::{
    Color, Element, Event, Rectangle, Subscription, Task, Theme, event,
    keyboard::{
        self,
        key::{self, Physical},
    },
    widget::column,
    window::{self, Mode},
};

use crate::{
    clipboard,
    components::{
        bottom_bar,
        notifications::{Notification, NotificationEntry},
        preferences,
        preferences::{PreferenceMessage, PreferenceOutcome},
        timeline_bar, viewer,
    },
    config::{Config, UI_SCALE_DEFAULT, UI_SCALE_MAX, UI_SCALE_MIN, UI_SCALE_STEP},
    gallery::Gallery,
    keybinds::{Action, KeyBinding},
    styles, tasks,
    wgpu::{
        media::image_data::MediaData, passes::checkerboard::CheckerboardUniforms,
        view_program::ViewProgram,
    },
};

pub struct App {
    program: ViewProgram,
    gallery: Gallery,
    mode: Mode,
    loading: Option<String>,
    load_generation: u64,
    focus_scale: bool,
    config: Config,
    editing_config: Option<Config>,
    preference_state: preferences::PreferenceState,
    context_menu_pos: Option<Vec2>,
    paused: bool,
    scrubbing: bool,
    notifications: Vec<NotificationEntry>,
    pub selected_tool: Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Draw,
    Text,
}

impl Default for App {
    fn default() -> Self {
        Self::from_config(Config::load())
    }
}

impl App {
    fn from_config(config: Config) -> Self {
        let mut program = ViewProgram::default();
        program.show_checkerboard = config.show_checkerboard;
        if config.show_checkerboard {
            program.checker_uniforms = checker_uniforms_from_theme(&config.theme);
        }
        program.mipmap_zoom_out = config.mipmap_zoom_out;
        program.smooth_zoom_in = config.smooth_zoom_in;
        styles::set_radius(config.rounded);
        Self {
            program,
            gallery: Gallery::default(),
            mode: Mode::Windowed,
            loading: None,
            load_generation: 0,
            focus_scale: false,
            config,
            editing_config: None,
            preference_state: preferences::PreferenceState::default(),
            context_menu_pos: None,
            paused: false,
            scrubbing: false,
            notifications: Vec::new(),
            selected_tool: Tool::Select,
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
    ToggleInfoSection(&'static str),
    TogglePreferences,
    Preference(PreferenceMessage),
    ClipboardLoaded(MediaData),
    CursorMoved(Vec2),
    CursorLeft,
    ContextMenuOpened(Vec2),
    PanStarted,
    PanEnded,
    CopyColor,
    CopyPath,
    RotateCw,
    RotateCcw,
    Exit,
    ToggleEditPanel,
    SelectTool(Tool),
    ToggleCheckerboard,
    TogglePlayback,
    FrameFirst,
    FrameLast,
    FrameNext,
    FramePrev,
    FrameSeek(usize),
    TimelineScrubStart,
    TimelineScrubEnd,
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    Notify(Notification),
    DismissNotification(usize),
    NotificationTick(Instant),
    Noop,
}

impl App {
    pub fn new(path: Option<PathBuf>) -> (Self, Task<Message>) {
        let config = Config::load();
        let effective_path = path.or_else(|| {
            if config.remember_last {
                config.last_image.as_ref().filter(|p| p.exists()).cloned()
            } else {
                None
            }
        });
        let mut app = Self::from_config(config);
        if let Some(p) = effective_path {
            app.gallery = Gallery::new(&p);
            app.loading = Some(Gallery::filename(&p));
            app.load_generation = 1;
            return (app, tasks::load_media(p, 1));
        }
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.focus_scale = false;

        match message {
            Message::Pan(delta) => self.program.pan(delta),
            Message::ScaleUp(cursor) => self.program.scale_up(cursor),
            Message::ScaleDown(cursor) => self.program.scale_down(cursor),
            Message::RotateCw => {
                if self.gallery.current().is_some() {
                    self.program.rotate();
                }
            }
            Message::RotateCcw => {
                if self.gallery.current().is_some() {
                    self.program.rotate_ccw();
                }
            }
            Message::Fit => {
                self.context_menu_pos = None;
                self.program.fit();
            }
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
                    if self.config.remember_last {
                        self.config.last_image = self.gallery.current().cloned();
                        self.config.save();
                    }
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
                return Task::done(Message::Notify(Notification::error(err)));
            }
            Message::ToggleEditPanel => {
                self.config.show_edit = !self.config.show_edit;
                self.config.save();
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
            Message::ToggleInfoSection(label) => {
                if !self.config.info_collapsed.remove(label) {
                    self.config.info_collapsed.insert(label.to_string());
                }
                self.config.save();
            }
            Message::TogglePreferences => {
                self.editing_config = Some(self.config.clone());
                self.preference_state = preferences::PreferenceState::default();
            }
            Message::Preference(msg) => {
                let Some(pending) = self.editing_config.as_mut() else {
                    return Task::none();
                };
                let outcome = preferences::update(msg, pending, &mut self.preference_state);
                match outcome {
                    PreferenceOutcome::Open => {}
                    PreferenceOutcome::Save => {
                        let pending = self.editing_config.take().unwrap();
                        let dec = self.config.decorations != pending.decorations;
                        let aot = self.config.always_on_top != pending.always_on_top;
                        let mipmap_changed = self.config.mipmap_zoom_out != pending.mipmap_zoom_out;
                        self.config = pending;
                        self.config.save();
                        self.program.mipmap_zoom_out = self.config.mipmap_zoom_out;
                        self.program.smooth_zoom_in = self.config.smooth_zoom_in;
                        if self.program.show_checkerboard {
                            self.program.checker_uniforms =
                                checker_uniforms_from_theme(&self.config.theme);
                        }
                        if mipmap_changed {
                            self.notifications
                                .push(NotificationEntry::new(Notification::warning(
                                    "Restart required to apply mipmapping change.",
                                )));
                        }
                        if dec || aot {
                            return Task::batch([
                                if dec {
                                    tasks::toggle_decorations()
                                } else {
                                    Task::none()
                                },
                                if aot {
                                    tasks::set_always_on_top(self.config.always_on_top)
                                } else {
                                    Task::none()
                                },
                            ]);
                        }
                    }
                    PreferenceOutcome::Cancel => {
                        self.editing_config = None;
                    }
                }
            }
            Message::CursorMoved(pos) => {
                if self.editing_config.is_none() && self.context_menu_pos.is_none() {
                    self.program.set_cursor_pos(Some(pos));
                }
            }
            Message::CursorLeft => {
                self.context_menu_pos = None;
                self.program.set_cursor_pos(None);
            }
            Message::ContextMenuOpened(pos) => {
                self.context_menu_pos = Some(pos);
                self.program.set_cursor_pos(Some(pos));
            }
            Message::PanStarted => {
                self.context_menu_pos = None;
                self.program.set_panning(true);
            }
            Message::PanEnded => self.program.set_panning(false),
            Message::CopyColor => {
                if let Some(pos) = self.context_menu_pos.take() {
                    if let Some((_, _, [r, g, b, _])) = self.program.color_at(pos) {
                        clipboard::write_text(&format!("#{r:02X}{g:02X}{b:02X}"));
                    }
                }
            }
            Message::CopyPath => {
                self.context_menu_pos = None;
                if let Some(path) = self.gallery.current() {
                    clipboard::write_text(&path.to_string_lossy());
                }
            }
            Message::Exit => return tasks::close_window(),
            Message::TogglePlayback => {
                self.paused = !self.paused;
                if !self.paused {
                    self.program.resume_animation();
                }
            }
            Message::FrameFirst => {
                self.paused = true;
                self.program.seek_animation(0);
            }
            Message::FrameLast => {
                self.paused = true;
                if let Some((_, total)) = self.program.animation_info() {
                    self.program.seek_animation(total.saturating_sub(1));
                }
            }
            Message::FrameNext => {
                self.paused = true;
                if let Some((frame, total)) = self.program.animation_info() {
                    self.program
                        .seek_animation((frame + 1).min(total.saturating_sub(1)));
                }
            }
            Message::FramePrev => {
                self.paused = true;
                if let Some((frame, _)) = self.program.animation_info() {
                    self.program.seek_animation(frame.saturating_sub(1));
                }
            }
            Message::FrameSeek(index) => {
                self.program.seek_animation(index);
                if !self.paused && !self.scrubbing {
                    self.program.resume_animation();
                }
            }
            Message::TimelineScrubStart => self.scrubbing = true,
            Message::TimelineScrubEnd => {
                self.scrubbing = false;
                if !self.paused {
                    self.program.resume_animation();
                }
            }
            Message::UiScaleUp => {
                self.config.ui_scale = (self.config.ui_scale + UI_SCALE_STEP).min(UI_SCALE_MAX);
                self.config.save();
            }
            Message::UiScaleDown => {
                self.config.ui_scale = (self.config.ui_scale - UI_SCALE_STEP).max(UI_SCALE_MIN);
                self.config.save();
            }
            Message::UiScaleReset => {
                self.config.ui_scale = UI_SCALE_DEFAULT;
                self.config.save();
            }
            Message::ToggleCheckerboard => {
                self.program.show_checkerboard = !self.program.show_checkerboard;
                self.config.show_checkerboard = self.program.show_checkerboard;
                if self.program.show_checkerboard {
                    self.program.checker_uniforms = checker_uniforms_from_theme(&self.config.theme);
                }
                self.config.save();
            }
            Message::Notify(n) => {
                self.notifications.push(NotificationEntry::new(n));
            }
            Message::DismissNotification(i) => {
                if let Some(entry) = self.notifications.get_mut(i) {
                    entry.dismissing_at = Some(Instant::now());
                }
            }
            Message::NotificationTick(now) => {
                for entry in &mut self.notifications {
                    entry.expire_if_due(now);
                }
                self.notifications.retain(|entry| !entry.is_gone(now));
            }
            Message::SelectTool(tool) => self.selected_tool = tool,
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
        self.paused = !self.config.autoplay;
        self.scrubbing = false;
        self.program.fit();
    }

    fn handle_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Window(window::Event::FileDropped(path)) => {
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
        if self.editing_config.is_some() {
            if let Some(action) = self.preference_state.capturing {
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
            Some(Action::UiScaleUp) => Task::done(Message::UiScaleUp),
            Some(Action::UiScaleDown) => Task::done(Message::UiScaleDown),
            Some(Action::UiScaleReset) => Task::done(Message::UiScaleReset),
            Some(Action::RotateCw) => Task::done(Message::RotateCw),
            Some(Action::RotateCcw) => Task::done(Message::RotateCcw),
            Some(Action::ToolSelect) => Task::done(Message::SelectTool(Tool::Select)),
            Some(Action::ToolCrop) => Task::done(Message::SelectTool(Tool::Crop)),
            Some(Action::ToolDraw) => Task::done(Message::SelectTool(Tool::Draw)),
            Some(Action::ToolText) => Task::done(Message::SelectTool(Tool::Text)),
            None => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        if let Some(pending) = &self.editing_config {
            return preferences::view(pending, &self.config.theme, &self.preference_state);
        }
        let mut col = column![];
        col = col.push(viewer::view(
            self.program.clone(),
            self.loading.as_deref(),
            self.config.show_info,
            self.config.show_edit,
            self.gallery.current().map(|p| p.as_path()),
            &self.gallery,
            &self.config.theme,
            &self.config.info_collapsed,
            &self.notifications,
            self.config.pixel_preview_size,
            &self.selected_tool,
        ));

        if let Some((frame, total)) = self.program.animation_info() {
            let position = if total > 1 {
                frame as f32 / (total - 1) as f32
            } else {
                0.0
            };
            let timestamp = self
                .program
                .animation_timestamp()
                .zip(self.program.animation_duration());
            col = col.push(timeline_bar::view(total, position, !self.paused, timestamp));
        }

        col.push(bottom_bar::view(
            self.mode,
            self.program.scale(),
            self.program.rotation(),
            self.focus_scale,
            self.config.show_info,
            self.config.show_edit,
            self.program.show_checkerboard,
            self.gallery.current().is_some(),
        ))
        .into()
    }

    pub fn title(&self) -> String {
        self.gallery
            .current()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Bloom".into())
    }

    pub fn theme(&self) -> Theme {
        self.config.theme.clone()
    }

    pub fn scale_factor(&self) -> f32 {
        self.config.ui_scale
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let events = event::listen().map(Message::Event);
        let mut subs = vec![events];

        if let Some(delay) = (!self.paused && !self.scrubbing)
            .then(|| self.program.time_until_next_frame())
            .flatten()
        {
            let delay = delay.max(Duration::from_millis(1));
            subs.push(every(delay).map(Message::AnimationTick));
        }

        if !self.notifications.is_empty() {
            let animating = self
                .notifications
                .iter()
                .any(NotificationEntry::is_animating);
            let tick_ms = if animating { 16 } else { 500 };
            subs.push(every(Duration::from_millis(tick_ms)).map(Message::NotificationTick));
        }

        Subscription::batch(subs)
    }
}

fn checker_uniforms_from_theme(theme: &Theme) -> CheckerboardUniforms {
    let p = theme.extended_palette();
    let to_arr = |c: Color| [c.r, c.g, c.b, c.a];
    CheckerboardUniforms {
        color_a: to_arr(p.background.weak.color),
        color_b: to_arr(p.background.base.color),
        tile_size: 12.0,
        _pad: [0.0; 3],
    }
}
