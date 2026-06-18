use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};

use crate::modifiers::kinds::{Text, TextAlign};

fn enumerate_families(font_system: &FontSystem) -> Vec<String> {
    let mut families: Vec<String> = font_system
        .db()
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .collect();
    families.sort_unstable();
    families.dedup();
    families
}

/// Sorted, deduplicated list of system font family names, enumerated once and cached.
/// Building a FontSystem scans the OS font dirs, so this is gated behind a OnceLock.
pub fn font_families() -> &'static [String] {
    use std::sync::OnceLock;
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let fs = FontSystem::new();
        enumerate_families(&fs)
    })
}

pub struct GlyphQuad {
    pub dst_x: f32,
    pub dst_y: f32,
    pub width: f32,
    pub height: f32,
    pub alpha: Vec<u8>,
}

pub struct TextBitmap {
    pub glyphs: Vec<GlyphQuad>,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl TextBitmap {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

pub fn rasterize_text(
    text: &Text,
    font_system: &mut FontSystem,
    swash: &mut SwashCache,
) -> TextBitmap {
    let mut bitmap = TextBitmap {
        glyphs: Vec::new(),
        min_x: 0.0,
        min_y: 0.0,
        max_x: 0.0,
        max_y: 0.0,
    };

    if text.content.is_empty() {
        return bitmap;
    }

    let metrics = Metrics::new(text.size, text.size * 1.2);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(font_system, None, None);

    let attrs = if text.font.is_empty() {
        Attrs::new()
    } else {
        Attrs::new().family(Family::Name(&text.font))
    };
    let align = match text.align {
        TextAlign::Left => cosmic_text::Align::Left,
        TextAlign::Center => cosmic_text::Align::Center,
        TextAlign::Right => cosmic_text::Align::Right,
    };
    buffer.set_text(
        font_system,
        &text.content,
        &attrs,
        Shaping::Advanced,
        Some(align),
    );
    buffer.shape_until_scroll(font_system, false);

    let mut first = true;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let Some(image) = swash.get_image(font_system, physical.cache_key) else {
                continue;
            };
            if image.placement.width == 0 || image.placement.height == 0 {
                continue;
            }

            let gx = physical.x as f32 + image.placement.left as f32;
            let gy = run.line_y + physical.y as f32 - image.placement.top as f32;
            let gw = image.placement.width as f32;
            let gh = image.placement.height as f32;

            let alpha = match image.content {
                SwashContent::Mask => image.data.clone(),
                SwashContent::SubpixelMask => image.data.clone(),
                SwashContent::Color => image
                    .data
                    .chunks_exact(4)
                    .map(|px| px[3])
                    .collect::<Vec<u8>>(),
            };

            if first {
                bitmap.min_x = gx;
                bitmap.min_y = gy;
                bitmap.max_x = gx + gw;
                bitmap.max_y = gy + gh;
                first = false;
            } else {
                bitmap.min_x = bitmap.min_x.min(gx);
                bitmap.min_y = bitmap.min_y.min(gy);
                bitmap.max_x = bitmap.max_x.max(gx + gw);
                bitmap.max_y = bitmap.max_y.max(gy + gh);
            }

            bitmap.glyphs.push(GlyphQuad {
                dst_x: gx,
                dst_y: gy,
                width: gw,
                height: gh,
                alpha,
            });
        }
    }

    bitmap
}
