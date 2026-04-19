use std::io::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use image::ImageError;

use super::image_data::ImageData;

#[derive(Debug)]
pub struct Frame {
    pub data: Arc<ImageData>,
    pub delay: Duration,
}

#[derive(Debug, Clone)]
pub struct Animation {
    frames: Arc<Vec<Frame>>,
    total_duration: Duration,
    current: usize,
    current_timestamp: Duration,
    deadline: Instant,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Result<Self, ImageError> {
        let first_delay = frames
            .first()
            .ok_or_else(|| ImageError::IoError(Error::other("animation has no frames")))?
            .delay;
        let total_duration = frames.iter().map(|f| f.delay).sum();
        Ok(Self {
            frames: Arc::new(frames),
            total_duration,
            current: 0,
            current_timestamp: Duration::ZERO,
            deadline: Instant::now() + first_delay,
        })
    }

    pub fn current_image(&self) -> &Arc<ImageData> {
        &self.frames[self.current].data
    }

    pub fn current_histogram(&self) -> &([u32; 256], [u32; 256], [u32; 256]) {
        self.frames[self.current].data.histogram()
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn time_until_next_frame(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn total_duration(&self) -> Duration {
        self.total_duration
    }

    pub fn current_timestamp(&self) -> Duration {
        self.current_timestamp
    }

    pub fn seek(&mut self, index: usize) -> Arc<ImageData> {
        let index = index.min(self.frames.len() - 1);
        self.current = index;
        self.current_timestamp = self.frames[..index].iter().map(|f| f.delay).sum();
        self.deadline = Instant::now() + self.frames[index].delay;
        Arc::clone(&self.frames[index].data)
    }

    pub fn resume(&mut self) {
        let now = Instant::now();
        let remaining = self.deadline.saturating_duration_since(now);
        self.deadline = now + remaining.max(self.frames[self.current].delay);
    }

    pub fn tick(&mut self, now: Instant) -> Option<Arc<ImageData>> {
        if now < self.deadline {
            return None;
        }

        loop {
            self.current_timestamp += self.frames[self.current].delay;
            self.current = (self.current + 1) % self.frames.len();
            if self.current == 0 {
                self.current_timestamp = Duration::ZERO;
            }
            self.deadline += self.frames[self.current].delay;
            if self.deadline > now {
                break;
            }
        }

        Some(Arc::clone(&self.frames[self.current].data))
    }
}
