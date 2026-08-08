//! Playback transport: play, pause, seek, and frame stepping, shared by
//! animations and video.
//!
//! The two sources keep time differently. An animation advances by frame
//! delays, a video by its own clock, so this layer holds the intent (playing,
//! target position) and lets each source resolve it.

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

    pub fn on_media_applied(&mut self, autoplay: bool, program: &mut ViewProgram) {
        self.paused = !autoplay;
        self.scrubbing = false;
        #[cfg(feature = "av")]
        let span = self.span(program);
        #[cfg(feature = "av")]
        if let Some(video) = self.video.as_mut() {
            if let Some((start, _)) = span
                && !start.is_zero()
            {
                video.seek(start, true);
            }
            if !self.paused {
                video.play();
            }
            return;
        }
        if let Some((first, _)) = self.frame_span(program)
            && first > 0
        {
            program.seek_animation(first);
        }
    }

    pub fn media_timing(&self, program: &ViewProgram) -> Option<crate::modifiers::MediaTiming> {
        #[cfg(feature = "av")]
        if let Some(video) = &self.video {
            let duration = video.duration();
            let frame_count = match video.frame_count() {
                0 => (duration.as_secs_f64() * video.avg_fps()).round().max(0.0) as u64,
                n => n,
            };
            return Some(crate::modifiers::MediaTiming {
                duration,
                frame_count,
            });
        }

        let (_, total) = program.animation_info()?;
        Some(crate::modifiers::MediaTiming {
            duration: program.animation_duration().unwrap_or_default(),
            frame_count: total as u64,
        })
    }

    #[cfg(any(feature = "av", test))]
    fn span(&self, program: &ViewProgram) -> Option<(Duration, Duration)> {
        let timing = self.media_timing(program)?;
        Some(
            program
                .active_trim(timing.duration)
                .unwrap_or((Duration::ZERO, timing.duration)),
        )
    }

    fn frame_span(&self, program: &ViewProgram) -> Option<(usize, usize)> {
        let (_, total) = program.animation_info()?;
        let last = total.saturating_sub(1);
        let Some((start, end)) = program.active_trim(program.animation_duration()?) else {
            return Some((0, last));
        };
        let (mut first, mut final_idx) = (last, 0usize);
        let mut clock = Duration::ZERO;
        let mut found = false;
        for (i, delay) in program.animation_delays().enumerate() {
            let frame_end = clock + delay;
            if frame_end > start && clock < end {
                if !found {
                    first = i;
                    found = true;
                }
                final_idx = i;
            }
            clock = frame_end;
        }
        Some(if found { (first, final_idx) } else { (0, 0) })
    }

    pub fn playback_active(&self, program: &ViewProgram) -> bool {
        #[cfg(feature = "av")]
        if self.video.is_some() {
            return true;
        }
        program.animation_info().is_some()
    }

    #[cfg(feature = "av")]
    pub fn video_export_data(&self, program: &ViewProgram) -> Option<crate::export::ExportData> {
        self.video
            .as_ref()
            .map(|v| program.build_video_export(v.info()))
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
            let span = state.span(program);
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
                let (span_start, hit_end) = match span {
                    Some((start, end)) => (start, !state.scrubbing && video.position() >= end),
                    None => (Duration::ZERO, false),
                };
                if video.is_ended() || hit_end {
                    if _config.loop_video {
                        video.seek(span_start, true);
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
            let past_end = state
                .frame_span(program)
                .zip(program.animation_info())
                .is_some_and(|((first, last), (frame, _))| frame > last && first <= last);
            if past_end {
                if program.loop_animations {
                    let first = state.frame_span(program).map_or(0, |(f, _)| f);
                    program.seek_animation(first);
                    program.resume_animation();
                } else {
                    state.paused = true;
                }
            } else if program.animation_ended() {
                state.paused = true;
            }
        }
        TransportMsg::TogglePlayback => {
            state.paused = !state.paused;
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if state.paused {
                    video.pause();
                } else {
                    let (start, end) = span.unwrap_or((Duration::ZERO, video.duration()));
                    if video.is_ended() || video.position() >= end || video.position() < start {
                        video.seek(start, true);
                    }
                    video.play();
                }
                return Task::none();
            }
            if !state.paused {
                let (first, last) = state.frame_span(program).unwrap_or((0, usize::MAX));
                let frame = program.animation_info().map_or(0, |(f, _)| f);
                if program.animation_ended() || frame > last || frame < first {
                    program.seek_animation(first);
                }
                program.resume_animation();
            }
        }
        TransportMsg::FrameFirst => {
            state.paused = true;
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                video.pause();
                video.seek(span.map_or(Duration::ZERO, |(s, _)| s), true);
                return Task::none();
            }
            let first = state.frame_span(program).map_or(0, |(f, _)| f);
            program.seek_animation(first);
        }
        TransportMsg::FrameLast => {
            state.paused = true;
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                video.pause();
                let end = span.map_or_else(|| video.duration(), |(_, e)| e);
                let target = end.saturating_sub(video.frame_interval());
                video.seek(target, true);
                return Task::none();
            }
            if let Some((_, last)) = state.frame_span(program) {
                program.seek_animation(last);
            }
        }
        TransportMsg::FrameNext => {
            state.paused = true;
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if let Some(frame) = video.step(true) {
                    program.set_video_frame(frame, false);
                }
                if let Some((_, end)) = span
                    && video.position() >= end
                {
                    video.seek(end.saturating_sub(video.frame_interval()), true);
                }
                return Task::none();
            }
            if let Some((frame, _)) = program.animation_info() {
                let last = state
                    .frame_span(program)
                    .map_or(usize::MAX, |(_, last)| last);
                program.seek_animation((frame + 1).min(last));
            }
        }
        TransportMsg::FramePrev => {
            state.paused = true;
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                if let Some(frame) = video.step(false) {
                    program.set_video_frame(frame, false);
                }
                if let Some((start, _)) = span
                    && video.position() < start
                {
                    video.seek(start, true);
                }
                return Task::none();
            }
            if let Some((frame, _)) = program.animation_info() {
                let first = state.frame_span(program).map_or(0, |(first, _)| first);
                program.seek_animation(frame.saturating_sub(1).max(first));
            }
        }
        TransportMsg::FrameSeek(index) => {
            #[cfg(feature = "av")]
            let span = state.span(program);
            #[cfg(feature = "av")]
            if let Some(video) = state.video.as_mut() {
                let mut target = video.seek_target_from_step(index);
                if let Some((start, end)) = span {
                    target =
                        target.clamp(start, end.saturating_sub(video.frame_interval()).max(start));
                }
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
            let (first, last) = state.frame_span(program).unwrap_or((0, usize::MAX));
            program.seek_animation(index.clamp(first, last));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::modifiers::kinds::Trim;
    use crate::modifiers::{Modifier, ModifierKind};
    use crate::wgpu::media::animation::{Animation, Frame};
    use crate::wgpu::media::image_data::ImageData;

    fn program_with_animation() -> ViewProgram {
        let frames = (0..10)
            .map(|_| Frame {
                data: Arc::new(ImageData::new(vec![0u8; 4], 1, 1)),
                delay: Duration::from_millis(100),
            })
            .collect();
        let mut program = ViewProgram::default();
        program.set_animation(Animation::new(frames).expect("animation"));
        program
    }

    fn with_trim(start_ms: u64, end_ms: u64) -> ViewProgram {
        let mut program = program_with_animation();
        program
            .modifiers_mut()
            .push(Modifier::new(ModifierKind::Trim(Trim {
                start: Duration::from_millis(start_ms),
                end: Some(Duration::from_millis(end_ms)),
            })));
        program
    }

    fn state() -> TransportState {
        TransportState::from_config(&Config::default())
    }

    #[test]
    fn frame_span_covers_everything_without_a_trim() {
        let program = program_with_animation();
        assert_eq!(state().frame_span(&program), Some((0, 9)));
    }

    #[test]
    fn frame_span_follows_the_trim() {
        let program = with_trim(250, 650);
        assert_eq!(state().frame_span(&program), Some((2, 6)));
    }

    #[test]
    fn frame_span_is_exact_on_frame_boundaries() {
        let program = with_trim(300, 500);
        assert_eq!(state().frame_span(&program), Some((3, 4)));
    }

    #[test]
    fn frame_span_ignores_a_disabled_trim() {
        let mut program = with_trim(300, 500);
        program.modifiers_mut()[0].enabled = false;
        assert_eq!(state().frame_span(&program), Some((0, 9)));
    }

    #[test]
    fn frame_span_is_none_for_stills() {
        assert_eq!(state().frame_span(&ViewProgram::default()), None);
    }

    #[test]
    fn span_matches_the_trim_for_animations() {
        let program = with_trim(250, 650);
        assert_eq!(
            state().span(&program),
            Some((Duration::from_millis(250), Duration::from_millis(650)))
        );
    }

    #[test]
    fn span_is_the_whole_media_without_a_trim() {
        let program = program_with_animation();
        assert_eq!(
            state().span(&program),
            Some((Duration::ZERO, Duration::from_millis(1000)))
        );
    }
}
