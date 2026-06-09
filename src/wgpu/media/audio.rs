use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use image::ImageError;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};

fn err(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::IoError(std::io::Error::other(e))
}

pub struct AudioClock {
    sample_rate: u32,
    playing: AtomicBool,
    samples_played: AtomicU64,
    base_us: AtomicU64,
    clear: AtomicBool,
    volume: AtomicU32,
}

impl AudioClock {
    fn position(&self) -> Duration {
        let frames = self.samples_played.load(Ordering::Relaxed);
        let base_us = self.base_us.load(Ordering::Relaxed);
        Duration::from_micros(base_us)
            + Duration::from_secs_f64(frames as f64 / self.sample_rate as f64)
    }

    fn play(&self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    fn seek_reset(&self, target: Duration) {
        self.base_us
            .store(target.as_micros() as u64, Ordering::Relaxed);
        self.samples_played.store(0, Ordering::Relaxed);
        self.clear.store(true, Ordering::Release);
    }

    pub fn request_clear(&self) {
        self.clear.store(true, Ordering::Release);
    }

    pub fn clear_pending(&self) -> bool {
        self.clear.load(Ordering::Acquire)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn set_base(&self, target: Duration) {
        self.base_us
            .store(target.as_micros() as u64, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }
}

pub struct AudioParams {
    pub producer: ringbuf::HeapProd<f32>,
    pub clock: Arc<AudioClock>,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    clock: Arc<AudioClock>,
}

impl AudioOutput {
    pub fn new() -> Result<(Self, AudioParams), ImageError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| err("no audio output device"))?;
        let default_cfg = device.default_output_config().map_err(err)?;

        let sample_rate = default_cfg.sample_rate().0;
        let channels: u16 = 2;
        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let capacity = sample_rate as usize * channels as usize * 2;
        let rb = HeapRb::<f32>::new(capacity);
        let (producer, mut consumer) = rb.split();

        let clock = Arc::new(AudioClock {
            sample_rate,
            playing: AtomicBool::new(false),
            samples_played: AtomicU64::new(0),
            base_us: AtomicU64::new(0),
            clear: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
        });
        let cb_clock = Arc::clone(&clock);

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if cb_clock.clear.swap(false, Ordering::AcqRel) {
                        consumer.clear();
                        cb_clock.samples_played.store(0, Ordering::Relaxed);
                    }
                    if !cb_clock.playing.load(Ordering::Relaxed) {
                        data.iter_mut().for_each(|s| *s = 0.0);
                        return;
                    }
                    let popped = consumer.pop_slice(data);
                    let volume = f32::from_bits(cb_clock.volume.load(Ordering::Relaxed));
                    if volume != 1.0 {
                        for s in data[..popped].iter_mut() {
                            *s = (*s * volume).clamp(-1.0, 1.0);
                        }
                    }
                    data[popped..].iter_mut().for_each(|s| *s = 0.0);
                    cb_clock
                        .samples_played
                        .fetch_add((popped / channels as usize) as u64, Ordering::Relaxed);
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(err)?;

        stream.play().map_err(err)?;

        Ok((
            Self {
                _stream: stream,
                clock: Arc::clone(&clock),
            },
            AudioParams {
                producer,
                clock,
                sample_rate,
                channels,
            },
        ))
    }

    pub fn position(&self) -> Duration {
        self.clock.position()
    }

    pub fn play(&self) {
        self.clock.play();
    }

    pub fn pause(&self) {
        self.clock.pause();
    }

    pub fn seek_reset(&self, target: Duration) {
        self.clock.seek_reset(target);
    }

    pub fn set_base(&self, target: Duration) {
        self.clock.set_base(target);
    }

    pub fn set_volume(&self, volume: f32) {
        self.clock.set_volume(volume);
    }
}
