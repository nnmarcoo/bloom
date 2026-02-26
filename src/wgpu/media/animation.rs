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
    elapsed: Duration,
    last_tick: Option<Instant>,
}

impl Animation {
    pub fn new(frames: Vec<Frame>) -> Self {
        Self {
            frames: Arc::new(frames),
            current: 0,
            elapsed: Duration::ZERO,
            last_tick: None,
        }
    }

    pub fn current_image(&self) -> &Arc<ImageData> {
        &self.frames[self.current].data
    }

    pub fn time_until_next_frame(&self) -> Duration {
        self.frames[self.current].delay.saturating_sub(self.elapsed)
    }

    pub fn tick(&mut self, now: Instant) -> Option<Arc<ImageData>> {
        let delta = self.last_tick.map(|t| now - t).unwrap_or(Duration::ZERO);
        self.last_tick = Some(now);
        self.elapsed += delta;

        let delay = self.frames[self.current].delay;
        if self.elapsed >= delay {
            self.elapsed -= delay;
            self.current = (self.current + 1) % self.frames.len();
            Some(Arc::clone(&self.frames[self.current].data))
        } else {
            None
        }
    }
}
