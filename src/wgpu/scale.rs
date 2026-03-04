use iced::Rectangle;

const STEPS: &[f32] = &[
    0.01, 0.02, 0.03, 0.05, 0.10, 0.15, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 1.00, 1.25,
    1.50, 1.75, 2.00, 2.50, 3.00, 3.50, 4.00, 5.00, 6.00, 7.00, 8.00, 10.0, 12.0, 15.0, 18.0, 21.0,
    25.0, 30.0, 35.0,
];

const DEFAULT_INDEX: usize = 14; // 1.00
const EPS: f32 = 1e-6;

enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub enum Scale {
    Stepped(usize),
    Custom(f32),
}

impl Default for Scale {
    fn default() -> Self {
        Self::Stepped(DEFAULT_INDEX)
    }
}

impl Scale {
    #[must_use]
    pub fn up(&mut self) -> f32 {
        let prev = self.value();
        *self = match *self {
            Scale::Custom(v) => Scale::Stepped(Self::snap_index(v, Direction::Up)),
            Scale::Stepped(i) => Scale::Stepped((i + 1).min(STEPS.len() - 1)),
        };
        prev
    }

    #[must_use]
    pub fn down(&mut self) -> f32 {
        let prev = self.value();
        *self = match *self {
            Scale::Custom(v) => Scale::Stepped(Self::snap_index(v, Direction::Down)),
            Scale::Stepped(i) => Scale::Stepped(i.saturating_sub(1)),
        };
        prev
    }

    pub fn fit_dims(&mut self, iw: f32, ih: f32, bounds: Rectangle) {
        *self = Scale::Custom((bounds.width / iw).min(bounds.height / ih));
    }

    pub fn custom(&mut self, scale: f32) {
        *self = if let Some(index) = STEPS.iter().position(|&s| (s - scale).abs() < EPS) {
            Scale::Stepped(index)
        } else {
            Scale::Custom(scale)
        };
    }

    pub fn value(&self) -> f32 {
        match *self {
            Scale::Stepped(i) => STEPS[i],
            Scale::Custom(v) => v,
        }
    }

    fn snap_index(scale: f32, dir: Direction) -> usize {
        match STEPS.binary_search_by(|s| s.partial_cmp(&scale).unwrap()) {
            Ok(index) => index,
            Err(index) => match dir {
                Direction::Up => index.min(STEPS.len() - 1),
                Direction::Down => index.saturating_sub(1),
            },
        }
    }
}
