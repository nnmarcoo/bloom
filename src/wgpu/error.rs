use std::fmt::{self, Display};

#[derive(Debug)]
pub enum ViewError {
    ImageDataMismatch { expected: usize, actual: usize },
}

impl Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageDataMismatch { expected, actual } => write!(
                f,
                "image data mismatch: expected {expected} bytes, got {actual}"
            ),
        }
    }
}

impl std::error::Error for ViewError {}
