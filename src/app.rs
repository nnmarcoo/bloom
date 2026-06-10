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
    modifiers::{Modifier, ModifierKind, ModifierParam, ModifierType, kinds::Crop},
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
    cursor_window: Vec2,
    picked_color: Option<[u8; 4]>,
    paused: bool,
    scrubbing: bool,
    #[cfg(feature = "video")]
    video: Option<crate::wgpu::media::video::VideoState>,
    #[cfg(feature = "video")]
    scrub_pending: Option<Duration>,
    #[cfg(feature = "video")]
    scrub_sent: Option<Duration>,
    #[cfg(feature = "video")]
    volume: f32,
    #[cfg(feature = "video")]
    muted: bool,
    notifications: Vec<NotificationEntry>,
    export_progress: Option<f32>,
    pub selected_tool: Tool,
    pub active_modifier: Option<usize>,
    pub dragging_modifier: Option<usize>,
    pub drag_hover_target: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Draw,
    Text,
}

type TransportView = (usize, f32, Option<(Duration, Duration)>);

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
        program.show_pixel_grid = config.show_pixel_grid;
        program.mipmap_zoom_out = config.mipmap_zoom_out;
        program.smooth_zoom_in = config.smooth_zoom_in;
        program.loop_animations = config.loop_animations;
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
            cursor_window: Vec2::ZERO,
            picked_color: None,
            paused: false,
            scrubbing: false,
            #[cfg(feature = "video")]
            video: None,
            #[cfg(feature = "video")]
            scrub_pending: None,
            #[cfg(feature = "video")]
            scrub_sent: None,
            #[cfg(feature = "video")]
            volume: 1.0,
            #[cfg(feature = "video")]
            muted: false,
            notifications: Vec::new(),
            export_progress: None,
            selected_tool: Tool::Select,
            active_modifier: None,
            dragging_modifier: None,
            drag_hover_target: None,
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
    CursorWindow(Vec2),
    PickColor,
    PanStarted,
    PanEnded,
    CopyColor,
    CopyPath,
    ToggleBottomBar,
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
    SetVolume(f32),
    ToggleMute,
    TimelineScrubStart,
    TimelineScrubEnd,
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    Notify(Notification),
    DismissNotification(usize),
    NotificationTick(Instant),
    AddModifier(ModifierType),
    RemoveModifier(usize),
    ToggleModifierExpanded(usize),
    ToggleModifierEnabled(usize),
    UpdateModifier(usize, ModifierParam),
    SetActiveModifier(usize),
    ClearActiveModifier,
    StartModifierDrag(usize),
    ModifierDragHover(usize),
    ModifierDragEnd,
    SetCropRect(usize, f32, f32, f32, f32),
    ExportImage,
    ExportFrame,
    ExportProgress(f32),
    ExportDone(Result<String, String>),
    Noop,
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
                    self.loading = Some(Gallery::filename(p));
                    self.load_generation = self.load_generation.wrapping_add(1);
                    self.program.release_image_pixels();
                    return tasks::load_media(p.clone(), self.load_generation);
                }
            }
            Message::MediaLoaded(generation, media) => {
                if generation == self.load_generation {
                    self.loading = None;
                    self.apply_media(media);
                    if self.config.remember_last {
                        self.config.last_media = self.gallery.current().cloned();
                        self.config.save();
                    }
                }
            }
            Message::ClipboardLoaded(media) => {
                self.loading = None;
                self.apply_media(media);
            }
            Message::AnimationTick(now) => {
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    if let Some(frame) = video.present() {
                        self.program.set_video_frame(frame, false);
                    }
                    if self.scrubbing
                        && !video.is_seeking()
                        && self.scrub_pending != self.scrub_sent
                        && let Some(target) = self.scrub_pending
                    {
                        video.seek(target, false);
                        self.scrub_sent = Some(target);
                    }
                    if video.is_ended() {
                        if self.config.loop_video {
                            video.seek(Duration::ZERO, true);
                            if !self.paused {
                                video.play();
                            }
                        } else {
                            self.paused = true;
                            video.pause();
                        }
                    }
                    return Task::none();
                }
                self.program.tick_animation(now);
                if self.program.animation_ended() {
                    self.paused = true;
                }
            }
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
            Message::ToggleBottomBar => {
                self.config.show_bottom_bar = !self.config.show_bottom_bar;
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
                    clipboard::write_text(&format!("#{r:02X}{g:02X}{b:02X}"));
                }
            }
            Message::CopyPath => {
                if let Some(path) = self.gallery.current() {
                    clipboard::write_text(&path.to_string_lossy());
                }
            }
            Message::Exit => {
                self.config.save();
                return tasks::close_window();
            }
            Message::TogglePlayback => {
                self.paused = !self.paused;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    if self.paused {
                        video.pause();
                    } else {
                        if video.is_ended() {
                            video.seek(Duration::ZERO, true);
                        }
                        video.play();
                    }
                    return Task::none();
                }
                if !self.paused {
                    if self.program.animation_ended() {
                        self.program.seek_animation(0);
                    }
                    self.program.resume_animation();
                }
            }
            Message::FrameFirst => {
                self.paused = true;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    video.pause();
                    video.seek(Duration::ZERO, true);
                    return Task::none();
                }
                self.program.seek_animation(0);
            }
            Message::FrameLast => {
                self.paused = true;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    video.pause();
                    let target = video.duration().saturating_sub(video.frame_interval());
                    video.seek(target, true);
                    return Task::none();
                }
                if let Some((_, total)) = self.program.animation_info() {
                    self.program.seek_animation(total.saturating_sub(1));
                }
            }
            Message::FrameNext => {
                self.paused = true;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    if let Some(frame) = video.step(true) {
                        self.program.set_video_frame(frame, false);
                    }
                    return Task::none();
                }
                if let Some((frame, total)) = self.program.animation_info() {
                    self.program
                        .seek_animation((frame + 1).min(total.saturating_sub(1)));
                }
            }
            Message::FramePrev => {
                self.paused = true;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    if let Some(frame) = video.step(false) {
                        self.program.set_video_frame(frame, false);
                    }
                    return Task::none();
                }
                if let Some((frame, _)) = self.program.animation_info() {
                    self.program.seek_animation(frame.saturating_sub(1));
                }
            }
            Message::FrameSeek(index) => {
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    let target = video.seek_target_from_step(index);
                    if self.scrubbing {
                        self.scrub_pending = Some(target);
                        if !video.is_seeking() && self.scrub_sent != Some(target) {
                            video.seek(target, false);
                            self.scrub_sent = Some(target);
                        }
                    } else {
                        video.seek(target, true);
                    }
                    return Task::none();
                }
                self.program.seek_animation(index);
                if !self.paused && !self.scrubbing {
                    self.program.resume_animation();
                }
            }
            Message::SetVolume(_v) => {
                #[cfg(feature = "video")]
                {
                    self.volume = _v.clamp(0.0, 2.0);
                    self.muted = self.volume <= 0.0;
                    if let Some(video) = &self.video {
                        video.set_volume(self.volume);
                    }
                }
            }
            Message::ToggleMute => {
                #[cfg(feature = "video")]
                {
                    self.muted = !self.muted;
                    let effective = if self.muted { 0.0 } else { self.volume };
                    if let Some(video) = &self.video {
                        video.set_volume(effective);
                    }
                }
            }
            Message::TimelineScrubStart => {
                self.scrubbing = true;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    video.pause();
                    self.scrub_pending = None;
                    self.scrub_sent = None;
                }
            }
            Message::TimelineScrubEnd => {
                self.scrubbing = false;
                #[cfg(feature = "video")]
                if let Some(video) = self.video.as_mut() {
                    self.scrub_sent = None;
                    if let Some(target) = self.scrub_pending.take() {
                        video.seek(target, true);
                    }
                    if !self.paused {
                        video.play();
                    }
                    return Task::none();
                }
                if !self.paused {
                    self.program.resume_animation();
                }
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
            Message::SelectTool(tool) => {
                let was_crop = self.selected_tool == Tool::Crop;
                let is_crop = tool == Tool::Crop;
                self.selected_tool = tool;
                self.program.crop_tool_active = is_crop;
                if is_crop {
                    if let Some(idx) = self
                        .program
                        .modifiers
                        .iter()
                        .position(|m| m.kind.as_crop().is_some())
                    {
                        self.active_modifier = Some(idx);
                    } else {
                        let (iw, ih) = self
                            .program
                            .image_size()
                            .map(|(w, h)| (w as f32, h as f32))
                            .unwrap_or((1.0, 1.0));
                        let idx = self.program.modifiers.len();
                        self.program
                            .modifiers_mut()
                            .push(Modifier::new(ModifierKind::Crop(Crop {
                                x: 0.0,
                                y: 0.0,
                                width: iw,
                                height: ih,
                            })));
                        self.active_modifier = Some(idx);
                        self.program.mark_dirty();
                    }
                    self.program.fit();
                } else if was_crop {
                    self.program.fit();
                }
            }
            Message::SetActiveModifier(i) => {
                if i < self.program.modifiers.len() {
                    self.active_modifier = Some(i);
                }
            }
            Message::ClearActiveModifier => {
                self.active_modifier = None;
            }
            Message::StartModifierDrag(i) => {
                self.dragging_modifier = Some(i);
                self.drag_hover_target = Some(i);
            }
            Message::ModifierDragHover(i) => {
                if self.dragging_modifier.is_some() {
                    self.drag_hover_target = Some(i);
                }
            }
            Message::ModifierDragEnd => {
                let source = self.dragging_modifier.take();
                let target = self.drag_hover_target.take();
                if let (Some(src), Some(tgt)) = (source, target)
                    && src != tgt
                {
                    let m = self.program.modifiers_mut().remove(src);
                    let insert_at = if tgt > src { tgt - 1 } else { tgt };
                    self.program.modifiers_mut().insert(insert_at, m);
                    self.program.mark_dirty();
                    if let Some(active) = self.active_modifier {
                        self.active_modifier = Some(if active == src {
                            insert_at
                        } else {
                            let after_remove = if active > src { active - 1 } else { active };
                            if after_remove >= insert_at {
                                after_remove + 1
                            } else {
                                after_remove
                            }
                        });
                    }
                }
            }
            Message::AddModifier(t) => {
                let is_crop = matches!(t, ModifierType::Crop);
                let already_has_crop = is_crop
                    && self
                        .program
                        .modifiers
                        .iter()
                        .any(|m| m.kind.as_crop().is_some());
                if already_has_crop {
                    self.notifications
                        .push(NotificationEntry::new(Notification::warning(
                            "Only one Crop modifier is allowed.",
                        )));
                } else {
                    let kind = if is_crop {
                        let (iw, ih) = self
                            .program
                            .image_size()
                            .map(|(w, h)| (w as f32, h as f32))
                            .unwrap_or((1.0, 1.0));
                        ModifierKind::Crop(Crop {
                            x: 0.0,
                            y: 0.0,
                            width: iw,
                            height: ih,
                        })
                    } else {
                        ModifierKind::from(t)
                    };
                    self.program.modifiers_mut().push(Modifier::new(kind));
                    let idx = self.program.modifiers.len() - 1;
                    self.active_modifier = Some(idx);
                    self.program.mark_dirty();
                }
            }
            Message::RemoveModifier(i) => {
                if i < self.program.modifiers.len() {
                    self.program.mark_dirty();
                    self.program.modifiers_mut().remove(i);
                    self.active_modifier = match self.active_modifier {
                        Some(a) if a == i => None,
                        Some(a) if a > i => Some(a - 1),
                        other => other,
                    };
                }
            }
            Message::ToggleModifierExpanded(i) => {
                if let Some(m) = self.program.modifiers_mut().get_mut(i) {
                    m.expanded = !m.expanded;
                }
            }
            Message::ToggleModifierEnabled(i) => {
                if let Some(m) = self.program.modifiers_mut().get_mut(i) {
                    m.enabled = !m.enabled;
                }
                self.program.mark_dirty();
            }
            Message::UpdateModifier(i, param) => {
                let img_size = self.program.image_size();
                if let Some(m) = self.program.modifiers_mut().get_mut(i) {
                    m.apply_param(param, img_size);
                }
                self.program.mark_dirty();
            }
            Message::SetCropRect(i, x, y, w, h) => {
                if let Some(m) = self.program.modifiers_mut().get_mut(i)
                    && let Some(crop) = m.kind.as_crop_mut()
                {
                    crop.x = x;
                    crop.y = y;
                    crop.width = w;
                    crop.height = h;
                }
                self.program.mark_dirty();
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
            Message::Noop => {}
            Message::Event(event) => return self.handle_event(event),
        }
        Task::none()
    }

    fn suggested_export_name(&self, ext: &str) -> String {
        self.gallery
            .current()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}.{ext}", s.to_string_lossy()))
            .unwrap_or_else(|| format!("export.{ext}"))
    }

    #[cfg(feature = "video")]
    fn playback_active(&self) -> bool {
        self.program.animation_info().is_some() || self.video.is_some()
    }

    #[cfg(not(feature = "video"))]
    fn playback_active(&self) -> bool {
        self.program.animation_info().is_some()
    }

    fn transport_view(&self) -> Option<TransportView> {
        #[cfg(feature = "video")]
        if let Some(video) = &self.video {
            let dur = video.duration();
            let pos = video.position();
            let frac = if dur > Duration::ZERO {
                (pos.as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let total = crate::wgpu::media::video::VIDEO_SCRUB_STEPS;
            return Some((total, frac, Some((pos, dur))));
        }

        self.program.animation_info().map(|(frame, total)| {
            let frac = if total > 1 {
                frame as f32 / (total - 1) as f32
            } else {
                0.0
            };
            let timestamp = self
                .program
                .animation_timestamp()
                .zip(self.program.animation_duration());
            (total, frac, timestamp)
        })
    }

    fn apply_media(&mut self, media: MediaData) {
        #[cfg(feature = "video")]
        {
            self.video = None;
        }
        match media {
            MediaData::Image(data) => self.program.set_image(*data),
            MediaData::Animation(anim) => self.program.set_animation(anim),
            #[cfg(feature = "video")]
            MediaData::Video(info) => match crate::wgpu::media::video::VideoState::new(*info) {
                Ok(state) => {
                    state.set_volume(if self.muted { 0.0 } else { self.volume });
                    self.program
                        .set_video_frame(std::sync::Arc::clone(&state.current), true);
                    self.program.set_base_rotation(state.rotation());
                    self.video = Some(state);
                }
                Err(e) => eprintln!("video load failed: {e}"),
            },
        }
        self.paused = !self.config.autoplay;
        self.scrubbing = false;
        #[cfg(feature = "video")]
        if !self.paused
            && let Some(video) = self.video.as_mut()
        {
            video.play();
        }
        self.program.fit();
    }

    fn handle_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if self.dragging_modifier.is_some() =>
            {
                Task::done(Message::ModifierDragEnd)
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
            if let Some(action) = self.preference_state.capturing
                && let Physical::Code(code) = physical_key
            {
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
                        return Task::done(Message::Preference(PreferenceMessage::CancelCapture));
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
            Some(Action::TogglePlayback) => {
                if self.playback_active() {
                    Task::done(Message::TogglePlayback)
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
        let video_panel = self.video.as_ref().map(|v| {
            let position = v.position();
            let duration = v.duration();
            let fps = v.avg_fps();
            let frame = (position.as_secs_f64() * fps).round() as u64;
            let frame_count = (duration.as_secs_f64() * fps).round() as u64;
            crate::components::info_panel::VideoPanel {
                meta: v.meta(),
                fps,
                rotation: v.rotation(),
                position,
                duration,
                frame,
                frame_count,
            }
        });

        let mut col = column![];
        col = col.push(viewer::view(
            self.program.clone(),
            self.loading.as_deref(),
            self.config.show_info,
            self.config.show_edit,
            self.config.show_bottom_bar,
            self.gallery.current().map(|p| p.as_path()),
            &self.gallery,
            &self.config.theme,
            &self.config.info_collapsed,
            &self.notifications,
            self.config.pixel_preview_size,
            &self.selected_tool,
            &self.program.modifiers,
            self.active_modifier,
            self.dragging_modifier,
            self.drag_hover_target,
            #[cfg(feature = "video")]
            video_panel,
        ));

        if let Some((total, position, timestamp)) = self.transport_view() {
            #[cfg(feature = "video")]
            let (volume, muted) = match &self.video {
                Some(v) if v.has_audio() => (Some(self.volume), self.muted),
                _ => (None, false),
            };
            #[cfg(not(feature = "video"))]
            let (volume, muted): (Option<f32>, bool) = (None, false);
            col = col.push(timeline_bar::view(
                total,
                position,
                !self.paused,
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
                self.playback_active(),
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
        let events = event::listen().map(Message::Event);
        let picker = event::listen_with(|event, _status, _window| match event {
            Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorWindow(Vec2::new(position.x, position.y)))
            }
            Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) => {
                Some(Message::PickColor)
            }
            _ => None,
        });
        let mut subs = vec![events, picker];

        #[cfg(feature = "video")]
        let tick = match &self.video {
            Some(video) => (!self.paused || self.scrubbing || video.is_seeking())
                .then(|| video.frame_interval()),
            None => (!self.paused && !self.scrubbing)
                .then(|| self.program.time_until_next_frame())
                .flatten(),
        };
        #[cfg(not(feature = "video"))]
        let tick = (!self.paused && !self.scrubbing)
            .then(|| self.program.time_until_next_frame())
            .flatten();

        if let Some(delay) = tick {
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
