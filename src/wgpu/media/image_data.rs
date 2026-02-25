use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use image::{AnimationDecoder, ImageError, ImageReader, codecs::gif::GifDecoder};

use super::animation::{Animation, Frame};

#[derive(Debug, Clone)]
pub enum MediaData {
    Image(ImageData),
    Animation(Animation),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageId(u64);

#[derive(Debug, Clone)]
pub struct ImageData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub id: ImageId,
}

impl ImageData {
    pub fn size_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    pub fn load(path: &Path) -> Result<Self, ImageError> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        reader.no_limits();
        let img = reader.decode()?.into_rgba8();
        let (width, height) = img.dimensions();
        let pixels = img.into_raw();
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));

        Ok(Self {
            pixels,
            width,
            height,
            id,
        })
    }

    pub fn load_gif(path: &Path) -> Result<Animation, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let decoder = GifDecoder::new(BufReader::new(file))?;

        let mut frames = Vec::new();
        for frame_result in decoder.into_frames() {
            let frame = frame_result?;
            let (numer, denom) = frame.delay().numer_denom_ms();
            let delay_ms = (numer as u64) / (denom as u64).max(1);
            // GIF spec minimum is 10ms; many tools emit 0 meaning "as fast as possible".
            // 100ms (10 fps) is a safe fallback that matches browser behavior.
            let delay = Duration::from_millis(if delay_ms < 20 { 100 } else { delay_ms });

            let rgba = frame.into_buffer();
            let (width, height) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));

            frames.push(Frame {
                data: Arc::new(ImageData {
                    pixels,
                    width,
                    height,
                    id,
                }),
                delay,
            });
        }

        Ok(Animation::new(frames))
    }

    pub fn load_media(path: &Path) -> Result<MediaData, ImageError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("gif") {
            Ok(MediaData::Animation(Self::load_gif(path)?))
        } else {
            Ok(MediaData::Image(Self::load(path)?))
        }
    }
}
