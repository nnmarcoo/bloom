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
    frames: Vec<(Arc<ImageData>, Duration)>,
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
            frames: vec![(Arc::clone(&first.data), first.pts)],
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
        self.frames.push((f.data, f.pts));
        if self.frames.len() > MAX_FRAMES {
            self.frames.remove(0);
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
    let has_audio = ictx.streams().best(ffmpeg::media::Type::Audio).is_some();

    Ok(VideoInfo {
        path: path.to_path_buf(),
        width: decoder.width(),
        height: decoder.height(),
        duration,
        avg_fps,
        has_audio,
    })
}

enum Flow {
    Continue,
    Stop,
    Seek(Duration, bool),
}

fn send_frame(
    frame_tx: &Sender<VideoFrame>,
    cmd_rx: &Receiver<VideoCommand>,
    frame: VideoFrame,
) -> Flow {
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

    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;

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

    'outer: loop {
        if let Some((target, precise)) = pending_seek.take() {
            let ts = target.as_micros() as i64;
            ictx.seek(ts, ..ts).map_err(err)?;
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

            let index = stream.index();
            if index == video_index {
                decoder.send_packet(&packet).map_err(err)?;
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let pts = ts_to_duration(decoded.pts().or_else(|| decoded.timestamp()), tb);
                    if pts < seek_target {
                        continue;
                    }
                    let frame = build_frame(&mut scaler, &decoded, pts, epoch)?;
                    match send_frame(frame_tx, cmd_rx, frame) {
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
            match send_frame(frame_tx, cmd_rx, frame) {
                Flow::Continue => {}
                Flow::Stop => return Ok(()),
                Flow::Seek(target, precise) => {
                    pending_seek = Some((target, precise));
                    continue 'outer;
                }
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

    let mut pixels = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        let start = y * stride;
        pixels.extend_from_slice(&pixels_in[start..start + row_bytes]);
    }

    Ok(Arc::new(ImageData::new(pixels, width, height)))
}
