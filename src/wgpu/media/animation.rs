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
    current: usize,
    deadline: Instant,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Self {
        let deadline = Instant::now() + frames[0].delay;
        Self {
            frames: Arc::new(frames),
            current: 0,
            deadline,
        }
    }

    pub fn current_image(&self) -> &Arc<ImageData> {
        &self.frames[self.current].data
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

    pub fn tick(&mut self, now: Instant) -> Option<Arc<ImageData>> {
        if now < self.deadline {
            return None;
        }

        self.current = (self.current + 1) % self.frames.len();
        self.deadline += self.frames[self.current].delay;
        while self.deadline <= now {
            self.current = (self.current + 1) % self.frames.len();
            self.deadline += self.frames[self.current].delay;
        }

        Some(Arc::clone(&self.frames[self.current].data))
    }
}
