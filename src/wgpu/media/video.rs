use std::collections::VecDeque;
use std::io::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use ffmpeg::format::Pixel;
use ffmpeg::format::Sample;
use ffmpeg::format::sample::Type as SampleType;
use ffmpeg::software::resampling::Context as Resampler;
use ffmpeg::software::scaling::{Context as Scaler, Flags};
use ffmpeg_next as ffmpeg;
use image::ImageError;
use ringbuf::traits::Producer;

use super::audio::{AudioOutput, AudioParams};
use super::image_data::ImageData;

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "mpg", "mpeg", "ts", "m2ts", "wmv", "flv",
];

pub const VIDEO_SCRUB_STEPS: usize = 1000;

const FRAME_BUFFER: usize = 8;
const MAX_FRAMES: usize = 16;

fn err(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::IoError(Error::other(e))
}

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
    pub avg_fps: f64,
    pub has_audio: bool,
    pub rotation: u8,
    pub meta: VideoMeta,
}

#[derive(Debug, Clone, Default)]
pub struct VideoMeta {
    pub codec: Option<String>,
    pub bitrate: Option<String>,
    pub pixel_format: Option<String>,
    pub bit_depth: Option<String>,
    pub color_space: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_sample_rate: Option<String>,
    pub audio_channels: Option<String>,
    pub audio_bitrate: Option<String>,
}

struct VideoFrame {
    data: Arc<ImageData>,
    pts: Duration,
    epoch: u64,
}

enum VideoCommand {
    Stop,
    Seek { target: Duration, precise: bool },
}

struct AvClock {
    anchor_pts: Duration,
    anchor_instant: Instant,
    paused: bool,
}

impl AvClock {
    fn new() -> Self {
        Self {
            anchor_pts: Duration::ZERO,
            anchor_instant: Instant::now(),
            paused: true,
        }
    }

    fn now(&self) -> Duration {
        if self.paused {
            self.anchor_pts
        } else {
            self.anchor_pts + self.anchor_instant.elapsed()
        }
    }

    fn pause(&mut self) {
        self.anchor_pts = self.now();
        self.paused = true;
    }

    fn resume(&mut self) {
        self.anchor_instant = Instant::now();
        self.paused = false;
    }

    fn reset(&mut self, to: Duration) {
        self.anchor_pts = to;
        self.anchor_instant = Instant::now();
    }
}

pub struct VideoState {
    info: VideoInfo,
    pub current: Arc<ImageData>,
    frame_rx: Receiver<VideoFrame>,
    cmd_tx: Sender<VideoCommand>,
    decode_thread: Option<JoinHandle<()>>,
    audio: Option<AudioOutput>,
    clock: AvClock,
    eos: Arc<AtomicBool>,
    frames: VecDeque<(Arc<ImageData>, Duration)>,
    cursor: usize,
    position: Duration,
    expected_epoch: u64,
    awaiting_seek: bool,
    stepped: bool,
    ended: bool,
    stall_clock: Option<Duration>,
}

impl VideoState {
    pub fn new(info: VideoInfo) -> Result<Self, ImageError> {
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(FRAME_BUFFER);
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        let (audio, audio_params) = match info.has_audio.then(AudioOutput::new) {
            Some(Ok((output, params))) => (Some(output), Some(params)),
            Some(Err(e)) => {
                eprintln!("audio init failed, playing silent: {e}");
                (None, None)
            }
            None => (None, None),
        };

        let eos = Arc::new(AtomicBool::new(false));
        let thread_info = info.clone();
        let thread_eos = Arc::clone(&eos);
        let decode_thread = std::thread::spawn(move || {
            decode_loop(thread_info, frame_tx, cmd_rx, audio_params, thread_eos)
        });

        let first = frame_rx
            .recv()
            .map_err(|_| err("no decodable video frame"))?;
        let position = first.pts;
        let current = Arc::clone(&first.data);

        if let Some(audio) = &audio {
            audio.set_base(position);
        }

        Ok(Self {
            info,
            current,
            frame_rx,
            cmd_tx,
            decode_thread: Some(decode_thread),
            audio,
            clock: AvClock::new(),
            eos,
            frames: VecDeque::from([(Arc::clone(&first.data), first.pts)]),
            cursor: 0,
            position,
            expected_epoch: 0,
            awaiting_seek: false,
            stepped: false,
            ended: false,
            stall_clock: None,
        })
    }

    pub fn duration(&self) -> Duration {
        self.info.duration
    }

    pub fn rotation(&self) -> u8 {
        self.info.rotation
    }

    pub fn meta(&self) -> &VideoMeta {
        &self.info.meta
    }

    pub fn avg_fps(&self) -> f64 {
        self.info.avg_fps
    }

    pub fn position(&self) -> Duration {
        self.position
    }

    fn master_now(&self) -> Duration {
        match &self.audio {
            Some(audio) => audio.position(),
            None => self.clock.now(),
        }
    }

    pub fn play(&mut self) {
        if self.audio.is_some() {
            if self.stepped {
                self.stepped = false;
                self.seek(self.position, true);
            }
            if let Some(audio) = &self.audio {
                audio.play();
            }
        } else {
            self.clock.reset(self.position);
            self.clock.resume();
        }
    }

    pub fn pause(&mut self) {
        match &self.audio {
            Some(audio) => audio.pause(),
            None => self.clock.pause(),
        }
    }

    fn recv_next(&mut self) -> Option<VideoFrame> {
        loop {
            match self.frame_rx.try_recv() {
                Ok(f) if f.epoch != self.expected_epoch => continue,
                Ok(f) => return Some(f),
                Err(_) => return None,
            }
        }
    }

    fn recv_until(&mut self, dur: Duration) -> Option<VideoFrame> {
        let deadline = Instant::now() + dur;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.frame_rx.recv_timeout(remaining) {
                Ok(f) if f.epoch != self.expected_epoch => continue,
                Ok(f) => return Some(f),
                Err(_) => return None,
            }
        }
    }

    fn push_frame(&mut self, f: VideoFrame) {
        self.frames.push_back((f.data, f.pts));
        if self.frames.len() > MAX_FRAMES {
            self.frames.pop_front();
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    fn show_cursor(&mut self) -> Option<Arc<ImageData>> {
        let (img, pts) = self.frames.get(self.cursor)?;
        self.position = *pts;
        self.current = Arc::clone(img);
        Some(Arc::clone(img))
    }

    fn land_into_window(&mut self, dur: Duration) -> bool {
        match self.recv_until(dur) {
            Some(f) => {
                self.realign(f.pts);
                self.push_frame(f);
                self.cursor = self.frames.len() - 1;
                self.awaiting_seek = false;
                true
            }
            None => false,
        }
    }

    pub fn step(&mut self, forward: bool) -> Option<Arc<ImageData>> {
        self.pause();

        if self.awaiting_seek && !self.land_into_window(Duration::from_millis(200)) {
            return None;
        }

        if forward {
            if self.cursor + 1 < self.frames.len() {
                self.cursor += 1;
            } else if let Some(f) = self.recv_until(Duration::from_millis(150)) {
                self.push_frame(f);
                self.cursor = self.frames.len() - 1;
            } else {
                return None;
            }
        } else if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            let back = self.frame_interval().mul_f64(1.5);
            let target = self.position.saturating_sub(back);
            self.seek(target, true);
            if !self.land_into_window(Duration::from_millis(250)) {
                return None;
            }
        }

        if self.audio.is_some() {
            self.stepped = true;
        }
        self.show_cursor()
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn set_volume(&self, volume: f32) {
        if let Some(audio) = &self.audio {
            audio.set_volume(volume);
        }
    }

    pub fn is_seeking(&self) -> bool {
        self.awaiting_seek
    }

    pub fn frame_interval(&self) -> Duration {
        let fps = if self.info.avg_fps > 1.0 {
            self.info.avg_fps.clamp(24.0, 120.0)
        } else {
            60.0
        };
        Duration::from_secs_f64(1.0 / fps)
    }

    pub fn seek_target_from_step(&self, step: usize) -> Duration {
        let steps = VIDEO_SCRUB_STEPS.max(2);
        let frac = (step as f64 / (steps - 1) as f64).clamp(0.0, 1.0);
        self.info.duration.mul_f64(frac)
    }

    pub fn seek(&mut self, target: Duration, precise: bool) {
        let target = target.min(self.info.duration);
        self.expected_epoch += 1;
        self.ended = false;
        self.stall_clock = None;
        self.awaiting_seek = true;
        self.stepped = false;
        self.position = target;
        self.frames.clear();
        self.cursor = 0;
        self.eos.store(false, Ordering::Relaxed);
        match &self.audio {
            Some(audio) => audio.seek_reset(target),
            None => self.clock.reset(target),
        }
        let _ = self.cmd_tx.send(VideoCommand::Seek { target, precise });
    }

    fn realign(&mut self, pts: Duration) {
        match &self.audio {
            Some(audio) => audio.set_base(pts),
            None => self.clock.reset(pts),
        }
    }

    fn poll_ended(&mut self) {
        if !self.eos.load(Ordering::Relaxed) {
            return;
        }
        let now = self.master_now();
        if now + self.frame_interval() >= self.info.duration {
            self.ended = true;
            return;
        }
        if self.stall_clock == Some(now) {
            self.ended = true;
        } else {
            self.stall_clock = Some(now);
        }
    }

    pub fn present(&mut self) -> Option<Arc<ImageData>> {
        if self.awaiting_seek {
            if self.frames.is_empty() {
                match self.recv_next() {
                    Some(f) => self.push_frame(f),
                    None => {
                        if self.eos.load(Ordering::Relaxed) {
                            self.awaiting_seek = false;
                            self.ended = true;
                        }
                        return None;
                    }
                }
            }
            self.awaiting_seek = false;
            self.cursor = self.frames.len() - 1;
            self.realign(self.frames[self.cursor].1);
            return self.show_cursor();
        }

        let now = self.master_now();
        let mut advanced = false;
        loop {
            if self.cursor + 1 < self.frames.len() {
                if self.frames[self.cursor + 1].1 <= now {
                    self.cursor += 1;
                    advanced = true;
                    continue;
                }
                break;
            }
            match self.recv_next() {
                Some(f) => {
                    let future = f.pts > now;
                    self.push_frame(f);
                    if future {
                        break;
                    }
                    self.cursor = self.frames.len() - 1;
                    advanced = true;
                }
                None => {
                    self.poll_ended();
                    break;
                }
            }
        }

        if advanced { self.show_cursor() } else { None }
    }
}

impl Drop for VideoState {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(VideoCommand::Stop);
        if let Some(handle) = self.decode_thread.take() {
            let _ = handle.join();
        }
    }
}

fn init_ffmpeg() -> Result<(), ImageError> {
    ffmpeg::init().map_err(err)?;
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Fatal);
    Ok(())
}

pub fn probe_video(path: &Path) -> Result<VideoInfo, ImageError> {
    init_ffmpeg()?;
    let ictx = ffmpeg::format::input(path).map_err(err)?;

    let stream = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| err("no video stream"))?;

    let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;

    let avg = stream.avg_frame_rate();
    let avg_fps = if avg.denominator() != 0 {
        avg.numerator() as f64 / avg.denominator() as f64
    } else {
        0.0
    };

    let duration = Duration::from_micros(ictx.duration().max(0) as u64);
    let audio_stream = ictx.streams().best(ffmpeg::media::Type::Audio);
    let has_audio = audio_stream.is_some();
    let rotation = stream_rotation(&stream);

    let video_params = stream.parameters();
    let mut meta = VideoMeta {
        codec: codec_label(&video_params),
        bitrate: param_bitrate(&video_params),
        pixel_format: decoder.format().descriptor().map(|d| d.name().to_string()),
        bit_depth: component_depth(decoder.format()).map(|b| format!("{b}-bit")),
        color_space: decoder.color_space().name().map(str::to_string),
        ..VideoMeta::default()
    };
    if let Some(astream) = audio_stream {
        let params = astream.parameters();
        let codec = codec_label(&params);
        let bitrate = param_bitrate(&params);
        if let Ok(adec) = ffmpeg::codec::context::Context::from_parameters(params)
            .and_then(|c| c.decoder().audio())
        {
            meta.audio_codec = codec;
            meta.audio_sample_rate = Some(format!("{:.1} kHz", adec.rate() as f64 / 1000.0));
            meta.audio_channels = Some(channel_label(adec.channels()));
            meta.audio_bitrate = bitrate;
        }
    }

    Ok(VideoInfo {
        path: path.to_path_buf(),
        width: decoder.width(),
        height: decoder.height(),
        duration,
        avg_fps,
        has_audio,
        rotation,
        meta,
    })
}

fn codec_label(params: &ffmpeg::codec::Parameters) -> Option<String> {
    let name = params.id().name();
    if name.is_empty() {
        return None;
    }
    let upper = name.to_uppercase();
    match profile_name(params) {
        Some(p) => Some(format!("{upper} ({p})")),
        None => Some(upper),
    }
}

fn param_bitrate(params: &ffmpeg::codec::Parameters) -> Option<String> {
    let bits = unsafe { (*params.as_ptr()).bit_rate };
    if bits <= 0 {
        return None;
    }
    let mbps = bits as f64 / 1_000_000.0;
    if mbps >= 1.0 {
        Some(format!("{mbps:.1} Mbps"))
    } else {
        Some(format!("{:.0} kbps", bits as f64 / 1000.0))
    }
}

fn component_depth(format: Pixel) -> Option<u32> {
    let descriptor = format.descriptor()?;
    let depth = unsafe { (*descriptor.as_ptr()).comp[0].depth };
    (depth > 0).then_some(depth as u32)
}

fn channel_label(channels: u16) -> String {
    match channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        n => format!("{n} ch"),
    }
}

fn profile_name(params: &ffmpeg::codec::Parameters) -> Option<&'static str> {
    use ffmpeg::codec::id::Id;
    let profile = unsafe { (*params.as_ptr()).profile };
    if profile < 0 {
        return None;
    }
    Some(match (params.id(), profile) {
        (Id::HEVC, 1) => "Main",
        (Id::HEVC, 2) => "Main 10",
        (Id::HEVC, 3) => "Main Still",
        (Id::HEVC, 4) => "Range Ext",
        (Id::H264, 66) => "Baseline",
        (Id::H264, 77) => "Main",
        (Id::H264, 88) => "Extended",
        (Id::H264, 100) => "High",
        (Id::H264, 110) => "High 10",
        (Id::H264, 122) => "High 4:2:2",
        (Id::H264, 244) => "High 4:4:4",
        (Id::AV1, 0) => "Main",
        (Id::AV1, 1) => "High",
        (Id::AV1, 2) => "Professional",
        (Id::VP9, 0) => "Profile 0",
        (Id::VP9, 2) => "Profile 2",
        _ => return None,
    })
}

fn stream_rotation(stream: &ffmpeg::format::stream::Stream) -> u8 {
    use ffmpeg::codec::packet::side_data::Type;
    for sd in stream.side_data() {
        if sd.kind() == Type::DisplayMatrix {
            let bytes = sd.data();
            if bytes.len() >= 36 {
                let m = |i: usize| {
                    i32::from_ne_bytes([
                        bytes[i * 4],
                        bytes[i * 4 + 1],
                        bytes[i * 4 + 2],
                        bytes[i * 4 + 3],
                    ]) as f64
                        / 65536.0
                };
                let quarter = (m(1).atan2(m(0)).to_degrees() / 90.0).round() as i32;
                return quarter.rem_euclid(4) as u8;
            }
        }
    }
    0
}

enum Flow {
    Continue,
    Stop,
    Seek(Duration, bool),
}

const MAX_PENDING_FRAMES: usize = 24;

fn drain_pending(frame_tx: &Sender<VideoFrame>, pending: &mut VecDeque<VideoFrame>) -> Flow {
    while let Some(frame) = pending.pop_front() {
        match frame_tx.try_send(frame) {
            Ok(()) => {}
            Err(crossbeam_channel::TrySendError::Full(frame)) => {
                pending.push_front(frame);
                return Flow::Continue;
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return Flow::Stop,
        }
    }
    Flow::Continue
}

fn send_frame(
    frame_tx: &Sender<VideoFrame>,
    cmd_rx: &Receiver<VideoCommand>,
    pending: &mut VecDeque<VideoFrame>,
    frame: VideoFrame,
) -> Flow {
    match drain_pending(frame_tx, pending) {
        Flow::Continue => {}
        other => return other,
    }
    if !pending.is_empty() {
        pending.push_back(frame);
        while pending.len() >= MAX_PENDING_FRAMES {
            match blocking_send_front(frame_tx, cmd_rx, pending) {
                Flow::Continue => {}
                other => return other,
            }
        }
        return Flow::Continue;
    }

    match frame_tx.try_send(frame) {
        Ok(()) => Flow::Continue,
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => Flow::Stop,
        Err(crossbeam_channel::TrySendError::Full(frame)) => match cmd_rx.try_recv() {
            Ok(VideoCommand::Seek { target, precise }) => Flow::Seek(target, precise),
            Ok(VideoCommand::Stop) | Err(TryRecvError::Disconnected) => Flow::Stop,
            Err(TryRecvError::Empty) => {
                pending.push_back(frame);
                Flow::Continue
            }
        },
    }
}

fn blocking_send_front(
    frame_tx: &Sender<VideoFrame>,
    cmd_rx: &Receiver<VideoCommand>,
    pending: &mut VecDeque<VideoFrame>,
) -> Flow {
    let Some(frame) = pending.pop_front() else {
        return Flow::Continue;
    };
    crossbeam_channel::select! {
        send(frame_tx, frame) -> res => if res.is_err() { Flow::Stop } else { Flow::Continue },
        recv(cmd_rx) -> msg => match msg {
            Ok(VideoCommand::Seek { target, precise }) => Flow::Seek(target, precise),
            Ok(VideoCommand::Stop) | Err(_) => Flow::Stop,
        },
    }
}

fn push_audio(
    params: &mut AudioParams,
    cmd_rx: &Receiver<VideoCommand>,
    mut samples: &[f32],
) -> Flow {
    while !samples.is_empty() {
        match cmd_rx.try_recv() {
            Ok(VideoCommand::Seek { target, precise }) => return Flow::Seek(target, precise),
            Ok(VideoCommand::Stop) | Err(TryRecvError::Disconnected) => return Flow::Stop,
            Err(TryRecvError::Empty) => {}
        }
        let pushed = params.producer.push_slice(samples);
        samples = &samples[pushed..];
        if !samples.is_empty() {
            if !params.clock.is_playing() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    Flow::Continue
}

fn decode_loop(
    info: VideoInfo,
    frame_tx: Sender<VideoFrame>,
    cmd_rx: Receiver<VideoCommand>,
    audio: Option<AudioParams>,
    eos: Arc<AtomicBool>,
) {
    if let Err(e) = run_decode(&info, &frame_tx, &cmd_rx, audio, &eos) {
        eprintln!("video decode ended: {e}");
    }
}

fn run_decode(
    info: &VideoInfo,
    frame_tx: &Sender<VideoFrame>,
    cmd_rx: &Receiver<VideoCommand>,
    mut audio: Option<AudioParams>,
    eos: &Arc<AtomicBool>,
) -> Result<(), ImageError> {
    init_ffmpeg()?;
    let mut ictx = ffmpeg::format::input(&info.path).map_err(err)?;

    let stream = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| err("no video stream"))?;
    let video_index = stream.index();
    let time_base = stream.time_base();
    let tb = if time_base.denominator() != 0 {
        time_base.numerator() as f64 / time_base.denominator() as f64
    } else {
        0.0
    };

    let mut decoder_ctx =
        ffmpeg::codec::context::Context::from_parameters(stream.parameters()).map_err(err)?;
    decoder_ctx.set_threading(ffmpeg::codec::threading::Config::kind(
        ffmpeg::codec::threading::Type::Frame,
    ));
    let mut decoder = decoder_ctx.decoder().video().map_err(err)?;

    let mut scaler = Scaler::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGBA,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )
    .map_err(err)?;

    let mut audio_index = None;
    let mut audio_decoder = None;
    let mut resampler = None;
    let mut audio_tb = 0.0;
    if let Some(params) = &audio
        && let Some(astream) = ictx.streams().best(ffmpeg::media::Type::Audio)
    {
        let adec = ffmpeg::codec::context::Context::from_parameters(astream.parameters())
            .map_err(err)?
            .decoder()
            .audio()
            .map_err(err)?;
        let resamp = Resampler::get(
            adec.format(),
            adec.channel_layout(),
            adec.rate(),
            Sample::F32(SampleType::Packed),
            target_layout(params.channels),
            params.sample_rate,
        )
        .map_err(err)?;
        let atb = astream.time_base();
        audio_tb = if atb.denominator() != 0 {
            atb.numerator() as f64 / atb.denominator() as f64
        } else {
            0.0
        };
        audio_index = Some(astream.index());
        audio_decoder = Some(adec);
        resampler = Some(resamp);
    }

    let mut decoded = ffmpeg::frame::Video::empty();
    let mut decoded_audio = ffmpeg::frame::Audio::empty();
    let mut epoch: u64 = 0;
    let mut seek_target = Duration::ZERO;
    let mut pending_seek: Option<(Duration, bool)> = None;
    let mut pending_frames: VecDeque<VideoFrame> = VecDeque::new();

    'outer: loop {
        if let Some((target, precise)) = pending_seek.take() {
            let ts = target.as_micros() as i64;
            ictx.seek(ts, ..ts).map_err(err)?;
            pending_frames.clear();
            decoder.flush();
            if let Some(adec) = audio_decoder.as_mut() {
                adec.flush();
            }
            if let Some(params) = &audio {
                params.clock.request_clear();
                let deadline = Instant::now() + Duration::from_millis(50);
                while params.clock.clear_pending() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            eos.store(false, Ordering::Relaxed);
            seek_target = if precise { target } else { Duration::ZERO };
            epoch += 1;
        }

        let mut next_seek: Option<(Duration, bool)> = None;
        'packets: for (stream, packet) in ictx.packets() {
            match cmd_rx.try_recv() {
                Ok(VideoCommand::Stop) => return Ok(()),
                Ok(VideoCommand::Seek { target, precise }) => {
                    next_seek = Some((target, precise));
                    break 'packets;
                }
                Err(_) => {}
            }

            if let Flow::Stop = drain_pending(frame_tx, &mut pending_frames) {
                return Ok(());
            }

            let index = stream.index();
            if index == video_index {
                decoder.send_packet(&packet).map_err(err)?;
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let pts = ts_to_duration(decoded.pts().or_else(|| decoded.timestamp()), tb);
                    if pts < seek_target {
                        continue;
                    }
                    let frame = build_frame(&mut scaler, &decoded, pts, epoch)?;
                    match send_frame(frame_tx, cmd_rx, &mut pending_frames, frame) {
                        Flow::Continue => {}
                        Flow::Stop => return Ok(()),
                        Flow::Seek(target, precise) => {
                            next_seek = Some((target, precise));
                            break 'packets;
                        }
                    }
                }
            } else if Some(index) == audio_index
                && let (Some(adec), Some(resamp), Some(params)) =
                    (audio_decoder.as_mut(), resampler.as_mut(), audio.as_mut())
            {
                adec.send_packet(&packet).map_err(err)?;
                while adec.receive_frame(&mut decoded_audio).is_ok() {
                    let apts = ts_to_duration(
                        decoded_audio.pts().or_else(|| decoded_audio.timestamp()),
                        audio_tb,
                    );
                    if apts < seek_target {
                        continue;
                    }
                    let mut out = ffmpeg::frame::Audio::empty();
                    resamp.run(&decoded_audio, &mut out).map_err(err)?;
                    let count = out.samples() * params.channels as usize;
                    let samples: &[f32] = bytemuck::cast_slice(&out.data(0)[..count * 4]);
                    match push_audio(params, cmd_rx, samples) {
                        Flow::Continue => {}
                        Flow::Stop => return Ok(()),
                        Flow::Seek(target, precise) => {
                            next_seek = Some((target, precise));
                            break 'packets;
                        }
                    }
                }
            }
        }

        if let Some(seek) = next_seek {
            pending_seek = Some(seek);
            continue 'outer;
        }

        let _ = decoder.send_eof();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let pts = ts_to_duration(decoded.pts().or_else(|| decoded.timestamp()), tb);
            if pts < seek_target {
                continue;
            }
            let frame = build_frame(&mut scaler, &decoded, pts, epoch)?;
            match send_frame(frame_tx, cmd_rx, &mut pending_frames, frame) {
                Flow::Continue => {}
                Flow::Stop => return Ok(()),
                Flow::Seek(target, precise) => {
                    pending_seek = Some((target, precise));
                    continue 'outer;
                }
            }
        }
        loop {
            match drain_pending(frame_tx, &mut pending_frames) {
                Flow::Stop => return Ok(()),
                _ if pending_frames.is_empty() => break,
                _ => match cmd_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(VideoCommand::Seek { target, precise }) => {
                        pending_seek = Some((target, precise));
                        continue 'outer;
                    }
                    Ok(VideoCommand::Stop)
                    | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        return Ok(());
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                },
            }
        }
        eos.store(true, Ordering::Relaxed);

        match cmd_rx.recv() {
            Ok(VideoCommand::Seek { target, precise }) => pending_seek = Some((target, precise)),
            Ok(VideoCommand::Stop) | Err(_) => return Ok(()),
        }
    }
}

fn target_layout(channels: u16) -> ffmpeg::ChannelLayout {
    match channels {
        1 => ffmpeg::ChannelLayout::MONO,
        _ => ffmpeg::ChannelLayout::STEREO,
    }
}

fn ts_to_duration(ts: Option<i64>, tb: f64) -> Duration {
    Duration::from_secs_f64(ts.unwrap_or(0).max(0) as f64 * tb)
}

fn build_frame(
    scaler: &mut Scaler,
    decoded: &ffmpeg::frame::Video,
    pts: Duration,
    epoch: u64,
) -> Result<VideoFrame, ImageError> {
    let data = frame_to_image(scaler, decoded)?;
    Ok(VideoFrame { data, pts, epoch })
}

fn frame_to_image(
    scaler: &mut Scaler,
    frame: &ffmpeg::frame::Video,
) -> Result<Arc<ImageData>, ImageError> {
    let mut rgba = ffmpeg::frame::Video::empty();
    scaler.run(frame, &mut rgba).map_err(err)?;

    let width = rgba.width();
    let height = rgba.height();
    let stride = rgba.stride(0);
    let pixels_in = rgba.data(0);
    let row_bytes = (width * 4) as usize;

    let pixels = if stride == row_bytes {
        pixels_in[..row_bytes * height as usize].to_vec()
    } else {
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            let start = y * stride;
            pixels.extend_from_slice(&pixels_in[start..start + row_bytes]);
        }
        pixels
    };

    Ok(Arc::new(ImageData::new(pixels, width, height)))
}
