use std::time::{Duration, Instant};

use iced::Task;

use crate::{app::Message, config::Config, wgpu::view_program::ViewProgram};

#[cfg(feature = "av")]
use crate::wgpu::media::video::{VideoInfo, VideoState};

pub type TransportView = (usize, f32, Option<(Duration, Duration)>);

#[derive(Debug, Clone)]
pub enum TransportMsg {
    TogglePlayback,
    FrameFirst,
    FrameLast,
    FrameNext,
    FramePrev,
    FrameSeek(usize),
    SetVolume(f32),
    CommitVolume,
    ToggleMute,
    ScrubStart,
    ScrubEnd,
    Tick(Instant),
}

pub struct TransportState {
    pub paused: bool,
    pub scrubbing: bool,
    #[cfg(feature = "av")]
    video: Option<VideoState>,
    #[cfg(feature = "av")]
    scrub_pending: Option<Duration>,
    #[cfg(feature = "av")]
    scrub_sent: Option<Duration>,
    #[cfg(feature = "av")]
    volume: f32,
    #[cfg(feature = "av")]
    muted: bool,
}

impl TransportState {
    pub fn from_config(_config: &Config) -> Self {
        Self {
            paused: false,
            scrubbing: false,
            #[cfg(feature = "av")]
            video: None,
            #[cfg(feature = "av")]
            scrub_pending: None,
            #[cfg(feature = "av")]
            scrub_sent: None,
            #[cfg(feature = "av")]
            volume: _config.volume,
            #[cfg(feature = "av")]
            muted: _config.muted,
        }
    }

    pub fn clear_video(&mut self) {
        #[cfg(feature = "av")]
        {
            self.video = None;
        }
    }

    #[cfg(feature = "av")]
    pub fn attach_video(&mut self, info: VideoInfo, program: &mut ViewProgram) {
        match VideoState::new(info) {
            Ok(state) => {
                state.set_volume(if self.muted { 0.0 } else { self.volume });
                program.set_video_frame(std::sync::Arc::clone(&state.current), true);
                program.set_base_rotation(state.rotation());
                self.video = Some(state);
            }
            Err(e) => eprintln!("video load failed: {e}"),
        }
    }

    pub fn on_media_applied(&mut self, autoplay: bool) {
        self.paused = !autoplay;
        self.scrubbing = false;
        #[cfg(feature = "av")]
        if !self.paused
            && let Some(video) = self.video.as_mut()
        {
            video.play();
        }
    }

    pub fn playback_active(&self, program: &ViewProgram) -> bool {
        #[cfg(feature = "av")]
        if self.video.is_some() {
            return true;
        }
        program.animation_info().is_some()
    }

    pub fn is_video(&self) -> bool {
        #[cfg(feature = "av")]
        {
            self.video.is_some()
        }
        #[cfg(not(feature = "av"))]
        {
            false
        }
    }

    pub fn volume_indicator(&self) -> (Option<f32>, bool) {
        #[cfg(feature = "av")]
        {
            match &self.video {
                Some(v) if v.has_audio() => (Some(self.volume), self.muted),
                _ => (None, false),
            }
        }
        #[cfg(not(feature = "av"))]
        {
            (None, false)
        }
    }

    pub fn transport_view(&self, program: &ViewProgram) -> Option<TransportView> {
        #[cfg(feature = "av")]
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

        program.animation_info().map(|(frame, total)| {
            let frac = if total > 1 {
                frame as f32 / (total - 1) as f32
            } else {
                0.0
            };
            let timestamp = program
                .animation_timestamp()
                .zip(program.animation_duration());
            (total, frac, timestamp)
        })
    }

    pub fn tick_interval(&self, program: &ViewProgram) -> Option<Duration> {
        #[cfg(feature = "av")]
        {
            match &self.video {
                Some(video) => (!self.paused || self.scrubbing || video.is_seeking())
                    .then(|| video.frame_interval()),
                None => (!self.paused && !self.scrubbing)
                    .then(|| program.time_until_next_frame())
                    .flatten(),
            }
        }
        #[cfg(not(feature = "av"))]
        {
            (!self.paused && !self.scrubbing)
                .then(|| program.time_until_next_frame())
                .flatten()
        }
    }

    #[cfg(feature = "av")]
    pub fn video_panel(&self) -> Option<crate::components::info_panel::VideoPanel<'_>> {
        self.video.as_ref().map(|v| {
            let position = v.position();
            let duration = v.duration();
            let fps = v.avg_fps();
            let dur_secs = duration.as_secs_f64();
            let frame_count = match v.frame_count() {
                0 => (dur_secs * fps).round() as u64,
                n => n,
            };
            let frac = if dur_secs > 0.0 {
                (position.as_secs_f64() / dur_secs).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let frame = ((frac * frame_count as f64).round() as u64 + 1).min(frame_count.max(1));
            crate::components::info_panel::VideoPanel {
                meta: v.meta(),
                fps,
                rotation: v.rotation(),
                position,
                duration,
                frame,
                frame_count,
            }
        })
    }
}

pub fn update(
    state: &mut TransportState,
    program: &mut ViewProgram,
    _config: &mut Config,
    msg: TransportMsg,
) -> Task<Message> {
    match msg {
        TransportMsg::Tick(now) => {
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if let Some(frame) = video.present() {
                    program.set_video_frame(frame, false);
                }
                if state.scrubbing
                    && !video.is_seeking()
                    && state.scrub_pending != state.scrub_sent
                    && let Some(target) = state.scrub_pending
                {
                    video.seek(target, false);
                    state.scrub_sent = Some(target);
                }
                if video.is_ended() {
                    if _config.loop_video {
                        video.seek(Duration::ZERO, true);
                        if !state.paused {
                            video.play();
                        }
                    } else {
                        state.paused = true;
                        video.pause();
                    }
                }
                return Task::none();
            }
            program.tick_animation(now);
            if program.animation_ended() {
                state.paused = true;
            }
        }
        TransportMsg::TogglePlayback => {
            state.paused = !state.paused;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if state.paused {
                    video.pause();
                } else {
                    if video.is_ended() {
                        video.seek(Duration::ZERO, true);
                    }
                    video.play();
                }
                return Task::none();
            }
            if !state.paused {
                if program.animation_ended() {
                    program.seek_animation(0);
                }
                program.resume_animation();
            }
        }
        TransportMsg::FrameFirst => {
            state.paused = true;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                video.pause();
                video.seek(Duration::ZERO, true);
                return Task::none();
            }
            program.seek_animation(0);
        }
        TransportMsg::FrameLast => {
            state.paused = true;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                video.pause();
                let target = video.duration().saturating_sub(video.frame_interval());
                video.seek(target, true);
                return Task::none();
            }
            if let Some((_, total)) = program.animation_info() {
                program.seek_animation(total.saturating_sub(1));
            }
        }
        TransportMsg::FrameNext => {
            state.paused = true;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if let Some(frame) = video.step(true) {
                    program.set_video_frame(frame, false);
                }
                return Task::none();
            }
            if let Some((frame, total)) = program.animation_info() {
                program.seek_animation((frame + 1).min(total.saturating_sub(1)));
            }
        }
        TransportMsg::FramePrev => {
            state.paused = true;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if let Some(frame) = video.step(false) {
                    program.set_video_frame(frame, false);
                }
                return Task::none();
            }
            if let Some((frame, _)) = program.animation_info() {
                program.seek_animation(frame.saturating_sub(1));
            }
        }
        TransportMsg::FrameSeek(index) => {
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                let target = video.seek_target_from_step(index);
                if state.scrubbing {
                    state.scrub_pending = Some(target);
                    if !video.is_seeking() && state.scrub_sent != Some(target) {
                        video.seek(target, false);
                        state.scrub_sent = Some(target);
                    }
                } else {
                    video.seek(target, true);
                }
                return Task::none();
            }
            program.seek_animation(index);
            if !state.paused && !state.scrubbing {
                program.resume_animation();
            }
        }
        TransportMsg::SetVolume(_v) => {
            #[cfg(feature = "av")]
            {
                state.volume = _v.clamp(0.0, crate::config::VOLUME_MAX);
                state.muted = state.volume <= 0.0;
                if let Some(video) = &state.video {
                    video.set_volume(state.volume);
                }
                _config.volume = state.volume;
                _config.muted = state.muted;
            }
        }
        TransportMsg::CommitVolume => {}
        TransportMsg::ToggleMute => {
            #[cfg(feature = "av")]
            {
                state.muted = !state.muted;
                let effective = if state.muted { 0.0 } else { state.volume };
                if let Some(video) = &state.video {
                    video.set_volume(effective);
                }
                _config.muted = state.muted;
            }
        }
        TransportMsg::ScrubStart => {
            state.scrubbing = true;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                video.pause();
                state.scrub_pending = None;
                state.scrub_sent = None;
            }
        }
        TransportMsg::ScrubEnd => {
            state.scrubbing = false;
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                state.scrub_sent = None;
                if let Some(target) = state.scrub_pending.take() {
                    video.seek(target, true);
                }
                if !state.paused {
                    video.play();
                }
                return Task::none();
            }
            if !state.paused {
                program.resume_animation();
            }
        }
    }
    Task::none()
}
