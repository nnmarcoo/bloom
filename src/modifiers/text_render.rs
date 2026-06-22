use std::sync::{Mutex, MutexGuard, OnceLock};

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent};

use crate::modifiers::kinds::Text;

pub struct FontResources {
    pub font_system: FontSystem,
    pub swash: SwashCache,
}

fn font_resources() -> &'static Mutex<FontResources> {
    static RES: OnceLock<Mutex<FontResources>> = OnceLock::new();
    RES.get_or_init(|| {
        Mutex::new(FontResources {
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
        })
    })
}

pub fn lock_font_resources() -> MutexGuard<'static, FontResources> {
    font_resources().lock().unwrap_or_else(|e| e.into_inner())
}

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

pub fn font_families() -> &'static [String] {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let guard = lock_font_resources();
        enumerate_families(&guard.font_system)
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

pub struct PackedAlpha {
    pub alpha: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bbox_w: f32,
    pub bbox_h: f32,
}

impl TextBitmap {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub fn pack_alpha(&self) -> Option<PackedAlpha> {
        if self.is_empty() {
            return None;
        }
        let bbox_w = (self.max_x - self.min_x).ceil().max(1.0);
        let bbox_h = (self.max_y - self.min_y).ceil().max(1.0);
        let w = bbox_w as u32;
        let h = bbox_h as u32;
        let mut buf = vec![0u8; (w as usize) * (h as usize)];

        for g in &self.glyphs {
            let ox = (g.dst_x - self.min_x).round() as i32;
            let oy = (g.dst_y - self.min_y).round() as i32;
            let gw = g.width.round() as i32;
            let gh = g.height.round() as i32;
            for row in 0..gh {
                let py = oy + row;
                if py < 0 || py >= h as i32 {
                    continue;
                }
                for col in 0..gw {
                    let px = ox + col;
                    if px < 0 || px >= w as i32 {
                        continue;
                    }
                    let src = (row as usize) * (gw as usize) + col as usize;
                    let Some(&a) = g.alpha.get(src) else { continue };
                    if a == 0 {
                        continue;
                    }
                    let dst = (py as usize) * (w as usize) + px as usize;
                    let prev = buf[dst];
                    buf[dst] = src_over_u8(a, prev);
                }
            }
        }

        Some(PackedAlpha {
            alpha: buf,
            width: w,
            height: h,
            bbox_w,
            bbox_h,
        })
    }
}

fn src_over_u8(src: u8, dst: u8) -> u8 {
    let s = src as u32;
    let d = dst as u32;
    (s + d * (255 - s) / 255).min(255) as u8
}

fn shape_buffer(text: &Text, font_system: &mut FontSystem) -> Buffer {
    let metrics = Metrics::new(text.size, text.size * 1.2);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(font_system, None, None);

    let attrs = if text.font.is_empty() {
        Attrs::new()
    } else {
        Attrs::new().family(Family::Name(&text.font))
    };
    buffer.set_text(font_system, &text.content, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

pub fn measure_text(text: &Text, font_system: &mut FontSystem) -> (f32, f32) {
    if text.content.is_empty() {
        return (0.0, 0.0);
    }
    let buffer = shape_buffer(text, font_system);
    measure_buffer(&buffer)
}

fn measure_buffer(buffer: &Buffer) -> (f32, f32) {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    (w, h)
}

pub struct ShapedText {
    buffer: Option<Buffer>,
    ox: f32,
    oy: f32,
    bases: Vec<usize>,
    content_len: usize,
    default_h: f32,
}

impl ShapedText {
    pub fn shape(text: &Text) -> Self {
        let default_h = text.size * 1.2;
        if text.content.is_empty() {
            return Self {
                buffer: None,
                ox: 0.0,
                oy: 0.0,
                bases: vec![0],
                content_len: 0,
                default_h,
            };
        }
        let buffer = {
            let mut guard = lock_font_resources();
            shape_buffer(text, &mut guard.font_system)
        };
        let (ox, oy) = block_origin(&buffer);
        Self {
            bases: line_byte_bases(&text.content),
            content_len: text.content.len(),
            ox,
            oy,
            default_h,
            buffer: Some(buffer),
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        let Some(buffer) = &self.buffer else {
            return (0.0, 0.0);
        };
        measure_buffer(buffer)
    }

    pub fn caret_offset(&self, caret: usize) -> (f32, f32, f32) {
        let Some(buffer) = &self.buffer else {
            return (0.0, 0.0, self.default_h);
        };
        let mut fallback: Option<(f32, f32, f32)> = None;
        for run in buffer.layout_runs() {
            let (base, line_end) = line_byte_span(&self.bases, run.line_i, self.content_len);
            fallback = Some((0.0, run.line_top, run.line_height));

            if caret < base || caret > line_end {
                continue;
            }

            let mut x = 0.0;
            for glyph in run.glyphs.iter() {
                if caret < base + glyph.end {
                    x = glyph.x;
                    break;
                }
                x = glyph.x + glyph.w;
            }
            return (x - self.ox, run.line_top - self.oy, run.line_height);
        }

        let (cx, cy, ch) = fallback.unwrap_or((0.0, 0.0, self.default_h));
        (cx - self.ox, cy - self.oy, ch)
    }

    pub fn selection_rects(&self, lo: usize, hi: usize) -> Vec<(f32, f32, f32, f32)> {
        let Some(buffer) = &self.buffer else {
            return Vec::new();
        };
        if lo >= hi {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for run in buffer.layout_runs() {
            let (base, line_end) = line_byte_span(&self.bases, run.line_i, self.content_len);
            let s = lo.max(base);
            let e = hi.min(line_end);
            if s > e {
                continue;
            }

            let mut x0: Option<f32> = None;
            let mut x1: Option<f32> = None;
            for glyph in run.glyphs.iter() {
                let g_start = base + glyph.start;
                let g_end = base + glyph.end;
                if g_end <= s {
                    continue;
                }
                if g_start >= e {
                    break;
                }
                x0 = Some(x0.map_or(glyph.x, |v: f32| v.min(glyph.x)));
                x1 = Some(x1.map_or(glyph.x + glyph.w, |v: f32| v.max(glyph.x + glyph.w)));
            }
            if let (Some(a), Some(b)) = (x0, x1)
                && b > a
            {
                rects.push((a - self.ox, run.line_top - self.oy, b - a, run.line_height));
            }
        }
        rects
    }

    pub fn caret_at_point(&self, local_x: f32, local_y: f32) -> usize {
        let Some(buffer) = &self.buffer else {
            return 0;
        };
        let px = local_x + self.ox;
        let py = local_y + self.oy;

        let mut chosen: Option<usize> = None;
        let mut best_dy = f32::INFINITY;

        for run in buffer.layout_runs() {
            let top = run.line_top;
            let bot = run.line_top + run.line_height;
            let dy = if py < top {
                top - py
            } else if py > bot {
                py - bot
            } else {
                0.0
            };
            if dy < best_dy {
                best_dy = dy;
                let (base, line_end) = line_byte_span(&self.bases, run.line_i, self.content_len);
                let mut idx = line_end;
                for glyph in run.glyphs.iter() {
                    let mid = glyph.x + glyph.w * 0.5;
                    if px < mid {
                        idx = base + glyph.start;
                        break;
                    }
                }
                chosen = Some(idx);
            }
        }
        chosen.unwrap_or(0)
    }
}

fn measure_key(text: &Text) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.content.hash(&mut h);
    text.font.hash(&mut h);
    text.size.to_bits().hash(&mut h);
    h.finish()
}

pub fn measure_block(text: &Text) -> (f32, f32) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u64, (f32, f32)>>> = OnceLock::new();
    const CACHE_CAP: usize = 64;

    if text.content.is_empty() {
        return (0.0, 0.0);
    }

    let key = measure_key(text);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock()
        && let Some(&v) = guard.get(&key)
    {
        return v;
    }

    let measured = {
        let mut guard = lock_font_resources();
        measure_text(text, &mut guard.font_system)
    };
    if let Ok(mut guard) = cache.lock() {
        if guard.len() >= CACHE_CAP {
            guard.clear();
        }
        guard.insert(key, measured);
    }
    measured
}

fn block_origin(buffer: &Buffer) -> (f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    for run in buffer.layout_runs() {
        min_y = min_y.min(run.line_top);
        for glyph in run.glyphs.iter() {
            min_x = min_x.min(glyph.x);
        }
    }
    let min_x = if min_x.is_finite() { min_x } else { 0.0 };
    let min_y = if min_y.is_finite() { min_y } else { 0.0 };
    (min_x, min_y)
}

fn line_byte_bases(content: &str) -> Vec<usize> {
    let mut bases = vec![0usize];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            bases.push(i + 1);
        }
    }
    bases
}

fn line_byte_span(bases: &[usize], line_i: usize, content_len: usize) -> (usize, usize) {
    let base = bases.get(line_i).copied().unwrap_or(0);
    let end = bases.get(line_i + 1).map(|b| b - 1).unwrap_or(content_len);
    (base, end)
}

pub fn caret_at_point(text: &Text, local_x: f32, local_y: f32) -> usize {
    ShapedText::shape(text).caret_at_point(local_x, local_y)
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

    let buffer = shape_buffer(text, font_system);

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
                SwashContent::SubpixelMask => image
                    .data
                    .chunks_exact(3)
                    .map(|px| (((px[0] as u16) + (px[1] as u16) + (px[2] as u16)) / 3) as u8)
                    .collect::<Vec<u8>>(),
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
