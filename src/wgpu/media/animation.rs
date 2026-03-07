use std::sync::Arc;
use std::time::{Duration, Instant};

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
    frame_start_elapsed: Duration,
    deadline: Instant,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Self {
        let total_duration = frames.iter().map(|f| f.delay).sum();
        let deadline = Instant::now() + frames[0].delay;
        Self {
            frames: Arc::new(frames),
            total_duration,
            current: 0,
            frame_start_elapsed: Duration::ZERO,
            deadline,
        }
    }

    pub fn current_image(&self) -> &Arc<ImageData> {
        &self.frames[self.current].data
    }

    pub fn current_histogram(&self) -> &([u32; 256], [u32; 256], [u32; 256]) {
        &self.frames[self.current].data.histogram
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn timing(&self) -> (Instant, Duration, Duration) {
        let frame_began_at = self.deadline - self.frames[self.current].delay;
        (
            frame_began_at,
            self.frame_start_elapsed,
            self.total_duration,
        )
    }

    pub fn time_until_next_frame(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn total_duration(&self) -> Duration {
        self.total_duration
    }

    pub fn seek(&mut self, index: usize) -> Arc<ImageData> {
        let index = index.min(self.frames.len() - 1);
        self.frame_start_elapsed = self.frames[..index].iter().map(|f| f.delay).sum();
        self.current = index;
        self.deadline = Instant::now() + self.frames[index].delay;
        Arc::clone(&self.frames[index].data)
    }

    pub fn resume(&mut self) {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        let carry = if remaining.is_zero() {
            self.frames[self.current].delay
        } else {
            remaining
        };
        self.deadline = Instant::now() + carry;
    }

    pub fn tick(&mut self, now: Instant) -> Option<Arc<ImageData>> {
        if now < self.deadline {
            return None;
        }

        loop {
            let prev = self.current;
            self.current = (self.current + 1) % self.frames.len();
            self.frame_start_elapsed = if self.current == 0 {
                Duration::ZERO
            } else {
                self.frame_start_elapsed + self.frames[prev].delay
            };
            self.deadline += self.frames[self.current].delay;
            if self.deadline > now {
                break;
            }
        }

        Some(Arc::clone(&self.frames[self.current].data))
    }
}
