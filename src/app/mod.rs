mod edit;
mod transport;

pub use edit::{EditMsg, EditState, Tool};
pub use transport::{TransportMsg, TransportState};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use iced::time::every;
use iced::{
    Color, Element, Event, Rectangle, Subscription, Task, Theme, event,
    keyboard::{self, key::Physical},
    widget::column,
    window::{self, Mode},
};

use crate::{
    components::{
        bottom_bar,
        notifications::{Notification, NotificationEntry},
        preferences,
        preferences::{PreferenceMessage, PreferenceOutcome},
        timeline_bar, viewer,
    },
    config::{Config, UI_SCALE_DEFAULT, UI_SCALE_MAX, UI_SCALE_MIN, UI_SCALE_STEP},
    gallery::Gallery,
    keybinds::Action,
    styles, tasks,
    wgpu::{
        media::image_data::{ImageId, MediaData},
        passes::checkerboard::CheckerboardUniforms,
        view_program::{Histogram, ViewProgram, hash_modifiers},
    },
};

pub struct App {
    program: ViewProgram,
    gallery: Gallery,
    mode: Mode,
    loading: Option<String>,
    load_generation: u64,
    pending_media: Option<PathBuf>,
    focus_scale: bool,
    config: Config,
    editing_config: Option<Config>,
    preference_state: preferences::PreferenceState,
    cursor_window: Vec2,
    picked_color: Option<[u8; 4]>,
    transport: TransportState,
    notifications: Vec<NotificationEntry>,
    export_progress: Option<f32>,
    edit: EditState,
    histogram: Option<HistogramResult>,
    histogram_inflight: Option<(ImageId, u64)>,
}

impl App {
    fn from_config(config: Config) -> Self {
        let mut program = ViewProgram::default();
        program.show_checkerboard = config.show_checkerboard;
        if config.show_checkerboard {
            program.checker_uniforms = checker_uniforms_from_theme(&config.theme);
        }
        program.show_pixel_grid = config.show_pixel_grid;
        program.mipmap_zoom_out = config.mipmap_zoom_out;
        program.smooth_zoom_in = config.smooth_zoom_in;
        program.loop_animations = config.loop_animations;
        styles::set_radius(config.rounded);
        let transport = TransportState::from_config(&config);
        Self {
            program,
            gallery: Gallery::default(),
            mode: Mode::Windowed,
            loading: None,
            load_generation: 0,
            pending_media: None,
            focus_scale: false,
            config,
            editing_config: None,
            preference_state: preferences::PreferenceState::default(),
            cursor_window: Vec2::ZERO,
            picked_color: None,
            transport,
            notifications: Vec::new(),
            export_progress: None,
            edit: EditState::default(),
            histogram: None,
            histogram_inflight: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistogramResult {
    pub image_id: ImageId,
    pub modifier_hash: u64,
    pub data: Histogram,
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
    ToggleFullscreen,
    ToggleInfoColumn,
    ToggleInfoSection(&'static str),
    TogglePreferences,
    Preference(PreferenceMessage),
    ClipboardLoaded(MediaData),
    CursorMoved(Vec2),
    CursorWindow(Vec2),
    PickColor,
    PanStarted,
    PanEnded,
    CopyColor,
    CopyImage,
    CopyImageDone(Result<(), String>),
    CopyPath,
    OpenFileLocation,
    ToggleBottomBar,
    RotateCw,
    RotateCcw,
    Exit,
    ToggleEditPanel,
    ToggleCheckerboard,
    Transport(TransportMsg),
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    Notify(Notification),
    DismissNotification(usize),
    NotificationTick(Instant),
    Edit(EditMsg),
    ExportImage,
    ExportFrame,
    ExportProgress(f32),
    ExportDone(Result<String, String>),
    HistogramReady(Box<HistogramResult>),
    Noop,
}

impl From<EditMsg> for Message {
    fn from(msg: EditMsg) -> Self {
        Message::Edit(msg)
    }
}

impl From<TransportMsg> for Message {
    fn from(msg: TransportMsg) -> Self {
        Message::Transport(msg)
    }
}

impl App {
    pub fn new(path: Option<PathBuf>, config: Config) -> (Self, Task<Message>) {
        let effective_path = path.or_else(|| {
            if config.remember_last {
                config.last_media.as_ref().filter(|p| p.exists()).cloned()
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
            Message::Pan(delta) => {
                self.program.pan(delta);
            }
            Message::ScaleUp(cursor) => {
                self.program.scale_up(cursor);
            }
            Message::ScaleDown(cursor) => {
                self.program.scale_down(cursor);
            }
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
                if self.program.fit_active() {
                    self.program.set_fit_active(false);
                } else {
                    self.program.fit();
                }
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
            Message::SelectMedia => return tasks::select_media(),
            Message::MediaSelected(path) => {
                if let Some(p) = self.gallery.set(path) {
                    let inflight = self.loading.is_some();
                    self.loading = Some(Gallery::filename(p));
                    self.program.release_image_pixels();
                    if inflight {
                        self.pending_media = Some(p.clone());
                        return Task::none();
                    }
                    self.load_generation = self.load_generation.wrapping_add(1);
                    return tasks::load_media(p.clone(), self.load_generation);
                }
            }
            Message::MediaLoaded(generation, media) => {
                if generation == self.load_generation {
                    if let Some(p) = self.pending_media.take() {
                        self.load_generation = self.load_generation.wrapping_add(1);
                        return tasks::load_media(p, self.load_generation);
                    }
                    self.loading = None;
                    self.apply_media(media);
                    if self.config.remember_last {
                        self.config.last_media = self.gallery.current().cloned();
                        self.config.save();
                    }
                    return self.maybe_request_histogram();
                }
            }
            Message::ClipboardLoaded(media) => {
                self.loading = None;
                self.apply_media(media);
                return self.maybe_request_histogram();
            }
            Message::Transport(msg) => {
                let task = transport::update(
                    &mut self.transport,
                    &mut self.program,
                    &mut self.config,
                    msg,
                );
                return Task::batch([task, self.maybe_request_histogram()]);
            }
            Message::MediaFailed(generation, err) => {
                let notify = Task::done(Message::Notify(Notification::error(err)));
                if generation == self.load_generation {
                    if let Some(p) = self.pending_media.take() {
                        self.load_generation = self.load_generation.wrapping_add(1);
                        return Task::batch([notify, tasks::load_media(p, self.load_generation)]);
                    }
                    self.loading = None;
                }
                return notify;
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
                return self.maybe_request_histogram();
            }
            Message::ToggleBottomBar => {
                self.config.show_bottom_bar = !self.config.show_bottom_bar;
                self.config.save();
            }
            Message::ToggleInfoSection(label) => {
                if !self.config.info_collapsed.remove(label) {
                    self.config.info_collapsed.insert(label.to_string());
                }
                self.config.save();
                return self.maybe_request_histogram();
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
                        self.program
                            .set_loop_animations(self.config.loop_animations);
                        if self.program.show_checkerboard {
                            self.program.checker_uniforms =
                                checker_uniforms_from_theme(&self.config.theme);
                        }
                        self.program.show_pixel_grid = self.config.show_pixel_grid;
                        if mipmap_changed {
                            self.notifications
                                .push(NotificationEntry::new(Notification::warning(
                                    "Mipmapping change applies on next image load.",
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
                if self.editing_config.is_none() {
                    self.program.set_cursor_pos(Some(pos));
                }
            }
            Message::CursorWindow(pos) => self.cursor_window = pos,
            Message::PickColor => {
                self.picked_color = self.program.color_at_window(self.cursor_window);
            }
            Message::PanStarted => {
                self.program.set_panning(true);
            }
            Message::PanEnded => self.program.set_panning(false),
            Message::CopyColor => {
                if let Some([r, g, b, _]) = self.picked_color {
                    return tasks::copy_text(format!("#{r:02X}{g:02X}{b:02X}"));
                }
            }
            Message::CopyImage => {
                if let Some(data) = self.program.export_data() {
                    return tasks::copy_image(data);
                }
            }
            Message::CopyImageDone(result) => {
                if let Err(e) = result {
                    self.notifications
                        .push(NotificationEntry::new(Notification::error(format!(
                            "Copy failed: {e}"
                        ))));
                }
            }
            Message::CopyPath => {
                if let Some(path) = self.gallery.current() {
                    return tasks::copy_text(path.to_string_lossy().into_owned());
                }
            }
            Message::OpenFileLocation => {
                if let Some(path) = self.gallery.current() {
                    return tasks::open_file_location(path.clone());
                }
            }
            Message::Exit => {
                self.config.save();
                return tasks::close_window();
            }
            Message::UiScaleUp => {
                self.config.ui_scale = (self.config.ui_scale + UI_SCALE_STEP).min(UI_SCALE_MAX);
            }
            Message::UiScaleDown => {
                self.config.ui_scale = (self.config.ui_scale - UI_SCALE_STEP).max(UI_SCALE_MIN);
            }
            Message::UiScaleReset => {
                self.config.ui_scale = UI_SCALE_DEFAULT;
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
            Message::Edit(msg) => {
                let task = edit::update(&mut self.edit, &mut self.program, msg);
                return Task::batch([task, self.maybe_request_histogram()]);
            }
            Message::ExportImage => {
                if let Some(data) = self.program.export_data() {
                    let ext = if data.frames.len() > 1 { "gif" } else { "png" };
                    let suggested = self.suggested_export_name(ext);
                    return tasks::export_image(data, suggested);
                }
            }
            Message::ExportFrame => {
                if let Some(data) = self.program.export_frame_data() {
                    let suggested = self.suggested_export_name("png");
                    return tasks::export_image(data, suggested);
                }
            }
            Message::ExportProgress(p) => {
                self.export_progress = Some(p);
            }
            Message::ExportDone(result) => {
                self.export_progress = None;
                let n = match result {
                    Ok(name) => Notification::info(format!("Exported \"{name}\"")),
                    Err(e) => Notification::error(format!("Export failed: {e}")),
                };
                self.notifications.push(NotificationEntry::new(n));
            }
            Message::HistogramReady(result) => {
                if self.histogram_inflight == Some((result.image_id, result.modifier_hash)) {
                    self.histogram_inflight = None;
                    self.histogram = Some(*result);
                }
                return self.maybe_request_histogram();
            }
            Message::Noop => {}
            Message::Event(event) => return self.handle_event(event),
        }
        Task::none()
    }

    fn maybe_request_histogram(&mut self) -> Task<Message> {
        if !self.config.show_info || self.config.info_collapsed.contains("HISTOGRAM") {
            return Task::none();
        }
        let Some(image) = self.program.current_image() else {
            return Task::none();
        };
        if self.histogram_inflight.is_some() {
            return Task::none();
        }
        let key = (image.id, hash_modifiers(&self.program.modifiers));
        if self
            .histogram
            .as_ref()
            .is_some_and(|h| (h.image_id, h.modifier_hash) == key)
        {
            return Task::none();
        }
        let pixels = image.pixels_snapshot();
        if pixels.len() < image.size_bytes() {
            return Task::none();
        }
        self.histogram_inflight = Some(key);
        tasks::compute_histogram(
            pixels,
            image.width,
            image.height,
            Arc::clone(&self.program.modifiers),
            key.0,
            key.1,
        )
    }

    fn suggested_export_name(&self, ext: &str) -> String {
        self.gallery
            .current()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}.{ext}", s.to_string_lossy()))
            .unwrap_or_else(|| format!("export.{ext}"))
    }

    fn apply_media(&mut self, media: MediaData) {
        self.histogram = None;
        self.histogram_inflight = None;
        self.transport.clear_video();
        match media {
            MediaData::Image(data) => self.program.set_image(*data),
            MediaData::Animation(anim) => self.program.set_animation(anim),
            #[cfg(feature = "video")]
            MediaData::Video(info) => self.transport.attach_video(*info, &mut self.program),
        }
        self.transport.on_media_applied(self.config.autoplay);
        self.program.fit();
    }

    fn handle_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if self.edit.dragging.is_some() =>
            {
                Task::done(EditMsg::DragEnd.into())
            }
            Event::Window(window::Event::CloseRequested) => {
                self.config.save();
                tasks::close_window()
            }
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
            return match preferences::capture_key(&self.preference_state, physical_key, modifiers) {
                Some(msg) => Task::done(Message::Preference(msg)),
                None => Task::none(),
            };
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
            Some(Action::ToolSelect) => Task::done(EditMsg::SelectTool(Tool::Select).into()),
            Some(Action::ToolCrop) => Task::done(EditMsg::SelectTool(Tool::Crop).into()),
            Some(Action::ToolDraw) => Task::done(EditMsg::SelectTool(Tool::Draw).into()),
            Some(Action::ToolText) => Task::done(EditMsg::SelectTool(Tool::Text).into()),
            Some(Action::TogglePlayback) => {
                if self.transport.playback_active(&self.program) {
                    Task::done(TransportMsg::TogglePlayback.into())
                } else {
                    Task::none()
                }
            }
            Some(Action::ToggleInfoPanel) => Task::done(Message::ToggleInfoColumn),
            Some(Action::ToggleEditPanel) => Task::done(Message::ToggleEditPanel),
            None => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        if let Some(pending) = &self.editing_config {
            return preferences::view(pending, &self.config.theme, &self.preference_state);
        }
        #[cfg(feature = "video")]
        let video_panel = self.transport.video_panel();

        let histogram = self.histogram.as_ref().map(|h| &h.data);

        let mut col = column![];
        col = col.push(viewer::view(viewer::ViewerCtx {
            program: self.program.clone(),
            loading: self.loading.as_deref(),
            show_info: self.config.show_info,
            show_edit: self.config.show_edit,
            show_bottom_bar: self.config.show_bottom_bar,
            path: self.gallery.current().map(|p| p.as_path()),
            gallery: &self.gallery,
            theme: &self.config.theme,
            info_collapsed: &self.config.info_collapsed,
            notifs: &self.notifications,
            pixel_preview_size: self.config.pixel_preview_size,
            selected_tool: &self.edit.selected_tool,
            modifiers: &self.program.modifiers,
            active_modifier: self.edit.active,
            dragging_modifier: self.edit.dragging,
            drag_hover_target: self.edit.drag_hover,
            histogram,
            #[cfg(feature = "video")]
            video_panel,
        }));

        if self.config.show_bottom_bar
            && let Some((total, position, timestamp)) = self.transport.transport_view(&self.program)
        {
            let (volume, muted) = self.transport.volume_indicator();
            col = col.push(timeline_bar::view(
                total,
                position,
                !self.transport.paused,
                timestamp,
                volume,
                muted,
            ));
        }

        if self.config.show_bottom_bar {
            col = col.push(bottom_bar::view(
                self.mode,
                self.program.scale(),
                self.program.rotation(),
                self.focus_scale,
                self.config.show_info,
                self.config.show_edit,
                self.program.show_checkerboard,
                self.gallery.current().is_some(),
                self.transport.playback_active(&self.program),
                self.transport.is_video(),
                self.program.fit_active(),
                self.export_progress,
            ));
        }

        col.into()
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
        let events = event::listen_with(|event, status, _window| match (&event, status) {
            (Event::Mouse(iced::mouse::Event::CursorMoved { position }), _) => {
                Some(Message::CursorWindow(Vec2::new(position.x, position.y)))
            }
            (Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)), _) => {
                Some(Message::PickColor)
            }
            (_, event::Status::Ignored) => Some(Message::Event(event)),
            _ => None,
        });
        let mut subs = vec![events];

        if let Some(delay) = self.transport.tick_interval(&self.program) {
            let delay = delay.max(Duration::from_millis(1));
            subs.push(every(delay).map(|t| TransportMsg::Tick(t).into()));
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
