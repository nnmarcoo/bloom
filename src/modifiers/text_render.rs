use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cosmic_text::fontdb;
use cosmic_text::{Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping};

use crate::modifiers::kinds::Text;

pub const SDF_TILE: u32 = 64;
pub const SDF_RANGE: f64 = 6.0;
pub const SDF_REFERENCE_PX: f32 = 52.0;
pub const SDF_EDGE: f32 = 0.5;
pub const SDF_PX_RANGE: f32 = SDF_RANGE as f32;

pub struct FontResources {
    pub font_system: FontSystem,
}

fn font_resources() -> &'static Mutex<FontResources> {
    static RES: OnceLock<Mutex<FontResources>> = OnceLock::new();
    RES.get_or_init(|| {
        Mutex::new(FontResources {
            font_system: FontSystem::new(),
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SdfKey {
    font_id: fontdb::ID,
    glyph_id: u16,
    weight: fontdb::Weight,
    flags: CacheKeyFlags,
}

pub struct GlyphSdf {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub quad_w: f32,
    pub quad_h: f32,
    pub left: f32,
    pub top: f32,
}

fn sdf_cache() -> &'static Mutex<HashMap<SdfKey, Arc<GlyphSdf>>> {
    static CACHE: OnceLock<Mutex<HashMap<SdfKey, Arc<GlyphSdf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_sdf_cache() -> MutexGuard<'static, HashMap<SdfKey, Arc<GlyphSdf>>> {
    sdf_cache().lock().unwrap_or_else(|e| e.into_inner())
}

fn build_msdf(face: &ttf_parser::Face, glyph_id: u16) -> Option<GlyphSdf> {
    use fdsm::generate::generate_msdf;
    use fdsm::render::correct_sign_msdf;
    use fdsm::shape::Shape;
    use fdsm::transform::Transform;
    use nalgebra::{Affine2, Similarity2, Vector2};
    use ttf_parser::GlyphId;

    let upem = face.units_per_em() as f64;
    let range = SDF_RANGE;
    let max_glyph_px = (SDF_TILE as f64 - 2.0 * range - 2.0).max(1.0);

    let shape = fdsm_ttf_parser::load_shape_from_face(face, GlyphId(glyph_id))?;
    let bbox = face.glyph_bounding_box(GlyphId(glyph_id))?;

    let glyph_w_units = (bbox.x_max as f64 - bbox.x_min as f64).max(1.0);
    let glyph_h_units = (bbox.y_max as f64 - bbox.y_min as f64).max(1.0);

    let natural_per_unit = SDF_REFERENCE_PX as f64 / upem;
    let fit = (max_glyph_px / (glyph_w_units * natural_per_unit))
        .min(max_glyph_px / (glyph_h_units * natural_per_unit))
        .min(1.0);
    let px_per_unit = natural_per_unit * fit;

    let width = (glyph_w_units * px_per_unit + 2.0 * range).ceil() as u32;
    let height = (glyph_h_units * px_per_unit + 2.0 * range).ceil() as u32;
    if width > SDF_TILE || height > SDF_TILE {
        return None;
    }

    let transformation = nalgebra::convert::<_, Affine2<f64>>(Similarity2::new(
        Vector2::new(
            range - bbox.x_min as f64 * px_per_unit,
            range - bbox.y_min as f64 * px_per_unit,
        ),
        0.0,
        px_per_unit,
    ));

    let mut shape = shape;
    Transform::transform(&mut shape, &transformation);
    let colored = Shape::edge_coloring_simple(shape, 0.03, 6_948_572_109_135_u64);
    let prepared = colored.prepare();

    let mut msdf = image::RgbImage::new(width, height);
    generate_msdf(&prepared, range, &mut msdf);
    correct_sign_msdf(
        &mut msdf,
        &prepared,
        fdsm::bezier::scanline::FillRule::Nonzero,
    );

    let mut data = vec![0u8; (width * height * 3) as usize];
    for y in 0..height {
        let src_y = height - 1 - y;
        let dst_row = (y * width * 3) as usize;
        let src_row = (src_y * width) as usize;
        for x in 0..width as usize {
            let p = &msdf.as_raw()[(src_row + x) * 3..];
            let di = dst_row + x * 3;
            data[di] = p[0];
            data[di + 1] = p[1];
            data[di + 2] = p[2];
        }
    }

    let inv_fit = (1.0 / fit) as f32;
    let quad_w = width as f32 * inv_fit;
    let quad_h = height as f32 * inv_fit;
    let range_natural = range as f32 * inv_fit;
    let left = bbox.x_min as f32 * natural_per_unit as f32 - range_natural;
    let top = -(bbox.y_max as f32 * natural_per_unit as f32) - range_natural;

    Some(GlyphSdf {
        data,
        width,
        height,
        quad_w,
        quad_h,
        left,
        top,
    })
}

fn ensure_glyph_sdf(key: SdfKey, font_system: &mut FontSystem) -> Option<Arc<GlyphSdf>> {
    if let Some(existing) = lock_sdf_cache().get(&key) {
        return Some(existing.clone());
    }

    let built = font_system
        .db_mut()
        .with_face_data(key.font_id, |data, index| {
            let face = ttf_parser::Face::parse(data, index).ok()?;
            build_msdf(&face, key.glyph_id)
        })??;

    let sdf = Arc::new(built);
    lock_sdf_cache().insert(key, sdf.clone());
    Some(sdf)
}

pub struct PlacedGlyph {
    pub key: SdfKey,
    pub sdf: Arc<GlyphSdf>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct ShapedGlyphs {
    pub glyphs: Vec<PlacedGlyph>,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl ShapedGlyphs {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub fn bbox(&self) -> (f32, f32) {
        if self.glyphs.is_empty() {
            return (0.0, 0.0);
        }
        (
            (self.max_x - self.min_x).max(1.0),
            (self.max_y - self.min_y).max(1.0),
        )
    }
}

pub fn shape_glyphs(text: &Text) -> ShapedGlyphs {
    let mut shaped = ShapedGlyphs {
        glyphs: Vec::new(),
        min_x: 0.0,
        min_y: 0.0,
        max_x: 0.0,
        max_y: 0.0,
    };
    if text.content.is_empty() {
        return shaped;
    }

    let mut guard = lock_font_resources();
    let FontResources { font_system } = &mut *guard;
    let buffer = shape_buffer(text, font_system);

    let ref_scale = text.size / SDF_REFERENCE_PX;
    let mut first = true;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let key = SdfKey {
                font_id: physical.cache_key.font_id,
                glyph_id: physical.cache_key.glyph_id,
                weight: glyph.font_weight,
                flags: glyph.cache_key_flags,
            };
            let Some(sdf) = ensure_glyph_sdf(key, font_system) else {
                continue;
            };

            let x = physical.x as f32 + sdf.left * ref_scale;
            let y = run.line_y + physical.y as f32 + sdf.top * ref_scale;
            let w = sdf.quad_w * ref_scale;
            let h = sdf.quad_h * ref_scale;

            if first {
                shaped.min_x = x;
                shaped.min_y = y;
                shaped.max_x = x + w;
                shaped.max_y = y + h;
                first = false;
            } else {
                shaped.min_x = shaped.min_x.min(x);
                shaped.min_y = shaped.min_y.min(y);
                shaped.max_x = shaped.max_x.max(x + w);
                shaped.max_y = shaped.max_y.max(y + h);
            }

            shaped.glyphs.push(PlacedGlyph {
                key,
                sdf,
                x,
                y,
                w,
                h,
            });
        }
    }

    shaped
}

#[cfg(test)]
mod sdf_tests {
    use super::*;

    #[test]
    fn wide_glyphs_not_dropped() {
        for s in ["W", "m", "M", "@", "%", "—", "WWW", "iiiii"] {
            let text = Text {
                content: s.to_string(),
                size: 52.0,
                ..Text::default()
            };
            let shaped = shape_glyphs(&text);
            let want = s.chars().count();
            let got = shaped.glyphs.len();
            assert_eq!(
                got, want,
                "glyph(s) dropped for {s:?}: got {got} want {want}"
            );
            for g in &shaped.glyphs {
                assert!(g.sdf.width <= SDF_TILE && g.sdf.height <= SDF_TILE);
            }
        }
    }

    #[test]
    fn msdf_glyph_has_field() {
        let text = Text {
            content: "R".to_string(),
            size: 52.0,
            ..Text::default()
        };
        let shaped = shape_glyphs(&text);
        if shaped.is_empty() {
            return;
        }
        let g = &shaped.glyphs[0];
        assert_eq!(g.sdf.data.len(), (g.sdf.width * g.sdf.height * 3) as usize);
        let has_inside = g
            .sdf
            .data
            .chunks_exact(3)
            .any(|p| median3(p[0], p[1], p[2]) > 128);
        let has_outside = g
            .sdf
            .data
            .chunks_exact(3)
            .any(|p| median3(p[0], p[1], p[2]) < 128);
        assert!(has_inside && has_outside, "msdf must span the edge");
    }

    fn median3(a: u8, b: u8, c: u8) -> u8 {
        a.max(b).min(a.min(b).max(c))
    }

    #[test]
    fn shape_glyphs_caches_glyphs() {
        let text = Text {
            content: "AA".to_string(),
            size: 48.0,
            ..Text::default()
        };
        let first = shape_glyphs(&text);
        if first.is_empty() {
            return;
        }
        let second = shape_glyphs(&text);
        assert_eq!(
            first.glyphs.len(),
            second.glyphs.len(),
            "re-shaping identical text yields the same glyphs"
        );
        let cache = lock_sdf_cache();
        for g in &second.glyphs {
            assert!(
                cache.contains_key(&g.key),
                "shaped glyph should be present in the SDF cache"
            );
        }
    }
}
