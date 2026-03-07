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
    deadline: Instant,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Self {
        let total_duration = frames.iter().map(|f| f.delay).sum();
        let first_delay = frames[0].delay;
        Self {
            frames: Arc::new(frames),
            total_duration,
            current: 0,
            deadline: Instant::now() + first_delay,
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

    pub fn time_until_next_frame(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn total_duration(&self) -> Duration {
        self.total_duration
    }

    pub fn seek(&mut self, index: usize) -> Arc<ImageData> {
        let index = index.min(self.frames.len() - 1);
        self.current = index;
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
            self.current = (self.current + 1) % self.frames.len();
            self.deadline += self.frames[self.current].delay;
            if self.deadline > now {
                break;
            }
        }

        Some(Arc::clone(&self.frames[self.current].data))
    }
}
