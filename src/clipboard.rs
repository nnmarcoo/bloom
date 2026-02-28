use std::path::PathBuf;

use crate::{gallery::SUPPORTED, wgpu::media::image_data::ImageData};

pub enum ClipboardImage {
    Pixels(ImageData),
    Path(PathBuf),
}

pub fn read() -> Option<ClipboardImage> {
    let mut ctx = arboard::Clipboard::new().ok()?;

    if let Ok(img) = ctx.get_image() {
        let pixels = img
            .bytes
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        return Some(ClipboardImage::Pixels(ImageData::new(
            pixels,
            img.width as u32,
            img.height as u32,
        )));
    }

    if let Ok(text) = ctx.get_text() {
        let path = PathBuf::from(text.trim());
        if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if SUPPORTED.contains(&ext.as_str()) {
                return Some(ClipboardImage::Path(path));
            }
        }
    }

    None
}
