use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context as ScaleCtx, Flags as ScaleFlags};
use ffmpeg_next::util::format::pixel::Pixel;
use ffmpeg_next::packet::Mut as PacketMut;

use super::image_data::ImageData;

const FRAME_BUFFER: usize = 8;
const AUDIO_BUFFER_MS: usize = 300;

// Tagged video frame — dropped if generation doesn't match.
pub struct VideoFrame {
    pub image: Arc<ImageData>,
    pub pts: Duration,
    pub generation: u64,
}

enum DecoderCmd {
    Seek(Duration, bool), // (target, accurate)
    Stop,
}

enum AudioCmd {
    Seek(Duration),
    Pause,
    Resume,
    Stop,
}

struct AudioHandle {
    _stream: cpal::Stream,
    cmd_tx: std::sync::mpsc::SyncSender<AudioCmd>,
}
unsafe impl Send for AudioHandle {}

struct AudioQueue {
    samples: VecDeque<f32>,
    paused: bool,
}

pub struct VideoData {
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
    pub fps: f64,

    current: Arc<ImageData>,
    current_pts: Duration,
    clock_origin: Option<Instant>,

    frame_rx: std::sync::mpsc::Receiver<VideoFrame>,
    next: Option<VideoFrame>,
    cmd_tx: std::sync::mpsc::SyncSender<DecoderCmd>,
    stop: Arc<AtomicBool>,

    // Incremented on every seek; frames with older generation are discarded.
    seek_gen: Arc<AtomicU64>,
    current_gen: u64,

    _audio: Option<AudioHandle>,
}

impl std::fmt::Debug for VideoData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoData")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

impl Clone for VideoData {
    fn clone(&self) -> Self {
        let (_tx, rx) = std::sync::mpsc::sync_channel(0);
        let (tx, _rx) = std::sync::mpsc::sync_channel(1);
        Self {
            width: self.width,
            height: self.height,
            duration: self.duration,
            fps: self.fps,
            current: Arc::clone(&self.current),
            current_pts: self.current_pts,
            clock_origin: self.clock_origin,
            frame_rx: rx,
            next: None,
            cmd_tx: tx,
            stop: Arc::new(AtomicBool::new(true)),
            seek_gen: Arc::new(AtomicU64::new(0)),
            current_gen: self.current_gen,
            _audio: None,
        }
    }
}

impl Drop for VideoData {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.cmd_tx.try_send(DecoderCmd::Stop);
    }
}

impl VideoData {
    pub fn open(path: &std::path::Path) -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;

        let path_buf = path.to_path_buf();
        let ictx = ffmpeg::format::input(path)?;

        let video_idx = ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?
            .index();

        let (width, height, fps, video_tb) = {
            let vs = ictx.stream(video_idx).unwrap();
            let ctx = ffmpeg::codec::context::Context::from_parameters(vs.parameters())?;
            let dec = ctx.decoder().video()?;
            let r = vs.rate();
            let fps = if r.denominator() == 0 {
                30.0
            } else {
                r.numerator() as f64 / r.denominator() as f64
            };
            (dec.width(), dec.height(), fps, vs.time_base())
        };

        let duration = {
            let d = ictx.duration();
            Duration::from_micros(
                (d.max(0) as u64)
                    .saturating_mul(1_000_000)
                    .saturating_div(ffmpeg::ffi::AV_TIME_BASE as u64),
            )
        };

        let blank = Arc::new(ImageData::new(
            vec![0u8; width as usize * height as usize * 4],
            width,
            height,
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let seek_gen = Arc::new(AtomicU64::new(0));
        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<VideoFrame>(FRAME_BUFFER);
        let (cmd_tx, cmd_rx) = std::sync::mpsc::sync_channel::<DecoderCmd>(4);

        let audio = start_audio(&path_buf, &stop);

        let stop_dec = Arc::clone(&stop);
        let seek_gen_dec = Arc::clone(&seek_gen);
        std::thread::Builder::new()
            .name("bloom-video-decoder".into())
            .spawn(move || {
                video_decode_loop(
                    ictx, video_idx, video_tb, width, height,
                    frame_tx, cmd_rx, stop_dec, seek_gen_dec,
                );
            })
            .expect("failed to spawn video decoder thread");

        // Wait for the first decoded frame.
        let first_frame = frame_rx.recv_timeout(Duration::from_millis(500)).ok();
        let (current, current_pts) = match first_frame {
            Some(f) => (f.image, f.pts),
            None => (blank, Duration::ZERO),
        };

        Ok(Self {
            width,
            height,
            duration,
            fps,
            current,
            current_pts,
            clock_origin: None,
            frame_rx,
            next: None,
            cmd_tx,
            stop,
            seek_gen,
            current_gen: 0,
            _audio: audio,
        })
    }

    pub fn start_clock(&mut self) {
        self.clock_origin = Some(Instant::now() - self.current_pts);
        if let Some(a) = &self._audio {
            let _ = a.cmd_tx.try_send(AudioCmd::Resume);
        }
    }

    pub fn pause_clock(&mut self) {
        self.clock_origin = None;
        if let Some(a) = &self._audio {
            let _ = a.cmd_tx.try_send(AudioCmd::Pause);
        }
    }

    pub fn current_image(&self) -> &Arc<ImageData> {
        &self.current
    }

    pub fn current_pts(&self) -> Duration {
        self.current_pts
    }

    pub fn time_until_next_frame(&self) -> Option<Duration> {
        let origin = self.clock_origin?;
        let next_pts = self.next.as_ref().map(|f| f.pts)?;
        let wall_pts = Instant::now().duration_since(origin);
        Some(next_pts.saturating_sub(wall_pts))
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(origin) = self.clock_origin else {
            return false;
        };
        let wall_pts = now.duration_since(origin);
        let cur_gen = self.current_gen;

        if self.next.is_none() {
            self.next = self.frame_rx.try_recv().ok();
        }

        // Drain stale frames from a past seek generation.
        while matches!(&self.next, Some(f) if f.generation < cur_gen) {
            self.next = self.frame_rx.try_recv().ok();
        }

        let mut changed = false;
        loop {
            match &self.next {
                Some(f) if f.pts <= wall_pts && f.generation == cur_gen => {
                    let frame = self.next.take().unwrap();
                    self.current = frame.image;
                    // Only update current_pts forward — never let it jump backwards
                    // (which can happen when post-seek keyframes arrive with earlier pts).
                    if frame.pts >= self.current_pts || wall_pts > self.current_pts {
                        self.current_pts = frame.pts;
                    }
                    changed = true;
                    self.next = self.frame_rx.try_recv().ok();
                }
                _ => break,
            }
        }
        changed
    }

    /// Keyframe-only seek — fast, used while scrubbing.
    pub fn seek_coarse(&mut self, target: Duration) {
        self.seek_inner(target, false);
    }

    /// Accurate seek — decodes to exact frame, used on release.
    pub fn seek(&mut self, target: Duration) {
        self.seek_inner(target, true);
    }

    fn seek_inner(&mut self, target: Duration, accurate: bool) {
        let new_gen = self.seek_gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.current_gen = new_gen;
        self.next = None;

        while self.frame_rx.try_recv().is_ok() {}

        let _ = self.cmd_tx.try_send(DecoderCmd::Seek(target, accurate));
        // Only seek audio on accurate seeks (scrub release), not during scrubbing.
        if accurate {
            if let Some(a) = &self._audio {
                let _ = a.cmd_tx.try_send(AudioCmd::Seek(target));
            }
        }

        self.current_pts = target;
        if self.clock_origin.is_some() {
            self.clock_origin = Some(Instant::now() - target);
        }
    }

    /// Block briefly waiting for the first post-seek frame and apply it.
    /// Used when paused so the viewer updates without a tick subscription.
    pub fn recv_seek_frame(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { break; }
            match self.frame_rx.recv_timeout(remaining) {
                Ok(f) if f.generation == self.current_gen => {
                    self.current = f.image;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        let Some(origin) = self.clock_origin else { return false; };
        let wall_pts = Instant::now().duration_since(origin);
        self.next.is_none() && self.frame_rx.try_recv().is_err() && wall_pts >= self.duration
    }
}

// ---------------------------------------------------------------------------
// Video decode loop
// ---------------------------------------------------------------------------

fn video_decode_loop(
    mut ictx: Input,
    video_idx: usize,
    time_base: ffmpeg_next::Rational,
    width: u32,
    height: u32,
    tx: std::sync::mpsc::SyncSender<VideoFrame>,
    cmd_rx: std::sync::mpsc::Receiver<DecoderCmd>,
    stop: Arc<AtomicBool>,
    seek_gen: Arc<AtomicU64>,
) {
    let codec_ctx = {
        let vs = ictx.stream(video_idx).unwrap();
        ffmpeg::codec::context::Context::from_parameters(vs.parameters()).unwrap()
    };
    let mut decoder = codec_ctx.decoder().video().unwrap();
    let mut scaler = ScaleCtx::get(
        decoder.format(), width, height,
        Pixel::RGBA, width, height, ScaleFlags::BILINEAR,
    ).unwrap();

    let tb_num = time_base.numerator() as f64;
    let tb_den = time_base.denominator() as f64;
    let pts_to_dur = |pts: i64| -> Duration {
        if pts < 0 { Duration::ZERO }
        else { Duration::from_secs_f64(pts as f64 * tb_num / tb_den) }
    };
    // ictx.seek() expects timestamps in AV_TIME_BASE (microseconds).
    let dur_to_av_ts = |d: Duration| -> i64 {
        (d.as_micros() as i64)
            .saturating_mul(ffmpeg_next::ffi::AV_TIME_BASE as i64)
            .saturating_div(1_000_000)
    };

    let mut seek_target: Option<Duration> = None;

    'outer: loop {
        // Drain all pending commands.
        loop {
            match cmd_rx.try_recv() {
                Ok(DecoderCmd::Stop) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return;
                }
                Ok(DecoderCmd::Seek(target, accurate)) => {
                    let _ = ictx.seek(dur_to_av_ts(target), ..);
                    decoder.flush();
                    seek_target = if accurate { Some(target) } else { None };
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }

        if stop.load(Ordering::Relaxed) { break; }

        let packet = {
            let mut pkt = ffmpeg_next::codec::packet::Packet::empty();
            let ret = unsafe {
                ffmpeg_next::ffi::av_read_frame(ictx.as_mut_ptr(), pkt.as_mut_ptr())
            };
            if ret < 0 {
                match cmd_rx.recv() {
                    Ok(DecoderCmd::Seek(target, accurate)) => {
                        let _ = ictx.seek(dur_to_av_ts(target), ..);
                        decoder.flush();
                        seek_target = if accurate { Some(target) } else { None };
                        continue;
                    }
                    _ => break,
                }
            }
            pkt
        };

        if packet.stream() != video_idx { continue; }
        if decoder.send_packet(&packet).is_err() { continue; }

        let mut raw = ffmpeg_next::frame::Video::empty();
        while decoder.receive_frame(&mut raw).is_ok() {
            if stop.load(Ordering::Relaxed) { break 'outer; }

            let cur_gen = seek_gen.load(Ordering::SeqCst);
            // raw.pts() is often None for many codecs; use best_effort_timestamp instead.
            let raw_pts = unsafe { (*raw.as_ptr()).best_effort_timestamp };
            let pts = pts_to_dur(if raw_pts != ffmpeg_next::ffi::AV_NOPTS_VALUE as i64 {
                raw_pts
            } else {
                raw.pts().unwrap_or(0)
            });
            // Accurate seek: skip frames before the requested target.
            if let Some(t) = seek_target {
                if pts < t {
                    continue;
                }
                seek_target = None;
            }
            let mut rgba = ffmpeg_next::frame::Video::empty();
            if scaler.run(&raw, &mut rgba).is_err() { continue; }

            let stride = rgba.stride(0);
            let data = rgba.data(0);
            let row_bytes = width as usize * 4;
            let pixels: Vec<u8> = if stride == row_bytes {
                data[..row_bytes * height as usize].to_vec()
            } else {
                (0..height as usize)
                    .flat_map(|r| { let s = r * stride; data[s..s + row_bytes].iter().copied() })
                    .collect()
            };

            let frame = VideoFrame {
                image: Arc::new(ImageData::new(pixels, width, height)),
                pts,
                generation: cur_gen,
            };

            if tx.send(frame).is_err() { break 'outer; }
        }
    }
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn start_audio(path: &PathBuf, stop: &Arc<AtomicBool>) -> Option<AudioHandle> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use ffmpeg_next::software::resampling::Context as ResampleCtx;
    use ffmpeg_next::util::format::sample::{Sample, Type as SampleType};

    let mut ictx = ffmpeg::format::input(path).ok()?;
    let audio_idx = ictx.streams().best(Type::Audio)?.index();

    let (src_rate, src_channels, src_fmt) = {
        let s = ictx.stream(audio_idx)?;
        let ctx = ffmpeg::codec::context::Context::from_parameters(s.parameters()).ok()?;
        let dec = ctx.decoder().audio().ok()?;
        (dec.rate() as i32, dec.channels() as i32, dec.format())
    };

    let audio_tb = ictx.stream(audio_idx)?.time_base();

    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let dst_rate = supported.sample_rate().0 as i32;
    let dst_channels = supported.channels() as i32;

    let queue = Arc::new(Mutex::new(AudioQueue {
        samples: VecDeque::new(),
        paused: true,
    }));
    let queue_write = Arc::clone(&queue);
    let queue_read = Arc::clone(&queue);

    let max_samples = (dst_rate as usize) * (dst_channels as usize) * AUDIO_BUFFER_MS / 1000;

    let stop_audio = Arc::clone(stop);
    let (audio_cmd_tx, audio_cmd_rx) = std::sync::mpsc::sync_channel::<AudioCmd>(8);

    let dur_to_av_ts = |d: Duration| -> i64 {
        (d.as_micros() as i64)
            .saturating_mul(ffmpeg_next::ffi::AV_TIME_BASE as i64)
            .saturating_div(1_000_000)
    };

    std::thread::Builder::new()
        .name("bloom-audio-decoder".into())
        .spawn(move || {
            use ffmpeg_next::channel_layout::ChannelLayout;
            let src_layout = if src_channels == 1 { ChannelLayout::MONO } else { ChannelLayout::STEREO };
            let dst_layout = if dst_channels == 1 { ChannelLayout::MONO } else { ChannelLayout::STEREO };

            let codec_ctx = {
                let s = ictx.stream(audio_idx).unwrap();
                ffmpeg::codec::context::Context::from_parameters(s.parameters()).unwrap()
            };
            let mut decoder = codec_ctx.decoder().audio().unwrap();
            let mut resampler = match ResampleCtx::get(
                src_fmt, src_layout, src_rate as u32,
                Sample::F32(SampleType::Packed), dst_layout, dst_rate as u32,
            ) {
                Ok(r) => r,
                Err(_) => return,
            };

            loop {
                if stop_audio.load(Ordering::Relaxed) { break; }

                // Handle commands.
                loop {
                    match audio_cmd_rx.try_recv() {
                        Ok(AudioCmd::Stop) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                        Ok(AudioCmd::Seek(target)) => {
                            let _ = ictx.seek(dur_to_av_ts(target), ..);
                            decoder.flush();
                            queue_write.lock().unwrap().samples.clear();
                        }
                        Ok(AudioCmd::Pause) => {
                            queue_write.lock().unwrap().paused = true;
                        }
                        Ok(AudioCmd::Resume) => {
                            queue_write.lock().unwrap().paused = false;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    }
                }

                // Back-pressure.
                {
                    let q = queue_write.lock().unwrap();
                    if q.samples.len() >= max_samples {
                        drop(q);
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                }

                let mut packet = ffmpeg_next::codec::packet::Packet::empty();
                let ret = unsafe {
                    ffmpeg_next::ffi::av_read_frame(ictx.as_mut_ptr(), packet.as_mut_ptr())
                };
                if ret < 0 {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }

                if packet.stream() != audio_idx { continue; }
                if decoder.send_packet(&packet).is_err() { continue; }

                let mut frame = ffmpeg_next::frame::Audio::empty();
                while decoder.receive_frame(&mut frame).is_ok() {
                    if stop_audio.load(Ordering::Relaxed) { return; }
                    let mut out = ffmpeg_next::frame::Audio::empty();
                    if resampler.run(&frame, &mut out).is_err() { continue; }
                    let n_samples = out.samples() * dst_channels as usize;
                    if n_samples == 0 { continue; }
                    let data = &out.data(0)[..n_samples * std::mem::size_of::<f32>()];
                    let samples: &[f32] = bytemuck::cast_slice(data);
                    queue_write.lock().unwrap().samples.extend(samples.iter().copied());
                }
            }
        })
        .ok()?;

    let cpal_config: cpal::StreamConfig = supported.into();
    let stream = device
        .build_output_stream(
            &cpal_config,
            move |output: &mut [f32], _| {
                let mut q = queue_read.lock().unwrap();
                if q.paused {
                    for s in output.iter_mut() { *s = 0.0; }
                    return;
                }
                for s in output.iter_mut() {
                    *s = q.samples.pop_front().unwrap_or(0.0);
                }
            },
            |_err| {},
            None,
        )
        .ok()?;

    stream.play().ok()?;
    Some(AudioHandle { _stream: stream, cmd_tx: audio_cmd_tx })
}
