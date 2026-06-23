use std::sync::Arc;

use crate::modifiers::kinds::Text;
use crate::modifiers::text_render::{self, GlyphSdf, SDF_EDGE};

struct CpuGlyph {
    sdf: Arc<GlyphSdf>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

pub struct TextRaster {
    glyphs: Vec<CpuGlyph>,
    block_min_x: f32,
    block_min_y: f32,
    anchor_x: f32,
    anchor_y: f32,
    block_w: f32,
    block_h: f32,
    cos: f32,
    sin: f32,
    aa: f32,
    color: [f32; 3],
    opacity: f32,
}

impl TextRaster {
    pub fn build(text: &Text, full_w: u32, full_h: u32) -> Option<Self> {
        if text.content.is_empty() || text.opacity <= 0.0 {
            return None;
        }

        let shaped = text_render::shape_glyphs(text);
        if shaped.is_empty() {
            return None;
        }

        let (block_w, block_h) = shaped.bbox();
        let glyphs = shaped
            .glyphs
            .iter()
            .map(|g| CpuGlyph {
                sdf: g.sdf.clone(),
                x0: g.x,
                y0: g.y,
                x1: g.x + g.w,
                y1: g.y + g.h,
            })
            .collect();

        let (sin, cos) = text.rotation.to_radians().sin_cos();
        let px_per_unit = block_w.max(block_h).max(1.0);
        let aa = (1.5 / px_per_unit).clamp(1e-4, 0.5);

        Some(Self {
            glyphs,
            block_min_x: shaped.min_x,
            block_min_y: shaped.min_y,
            anchor_x: text.x * full_w as f32,
            anchor_y: text.y * full_h as f32,
            block_w,
            block_h,
            cos,
            sin,
            aa,
            color: [text.r, text.g, text.b],
            opacity: text.opacity,
        })
    }

    pub fn sample(&self, fx: f32, fy: f32) -> Option<[f32; 4]> {
        let dx = fx - self.anchor_x;
        let dy = fy - self.anchor_y;
        let lx = dx * self.cos + dy * self.sin;
        let ly = -dx * self.sin + dy * self.cos;

        let u = lx / self.block_w + 0.5;
        let v = ly / self.block_h + 0.5;
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }

        let bx = self.block_min_x + u * self.block_w;
        let by = self.block_min_y + v * self.block_h;

        let mut coverage = 0.0f32;
        for g in &self.glyphs {
            if bx < g.x0 || bx >= g.x1 || by < g.y0 || by >= g.y1 {
                continue;
            }
            let gu = (bx - g.x0) / (g.x1 - g.x0);
            let gv = (by - g.y0) / (g.y1 - g.y0);
            let d = sample_sdf(&g.sdf, gu, gv);
            let c = smoothstep(SDF_EDGE - self.aa, SDF_EDGE + self.aa, d);
            coverage = coverage.max(c);
        }

        let a = coverage * self.opacity;
        if a <= 0.0 {
            return None;
        }
        Some([self.color[0], self.color[1], self.color[2], a])
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn median3(a: f32, b: f32, c: f32) -> f32 {
    a.max(b).min(a.min(b).max(c))
}

fn sample_sdf(sdf: &GlyphSdf, u: f32, v: f32) -> f32 {
    let gw = sdf.width as f32;
    let gh = sdf.height as f32;
    let fx = (u * gw - 0.5).clamp(0.0, gw - 1.0);
    let fy = (v * gh - 0.5).clamp(0.0, gh - 1.0);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(sdf.width as usize - 1);
    let y1 = (y0 + 1).min(sdf.height as usize - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let w = sdf.width as usize;
    let at = |x: usize, y: usize| -> f32 {
        let i = (y * w + x) * 3;
        median3(
            sdf.data[i] as f32 / 255.0,
            sdf.data[i + 1] as f32 / 255.0,
            sdf.data[i + 2] as f32 / 255.0,
        )
    };
    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
    let bot = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
    top * (1.0 - ty) + bot * ty
}

pub fn build_layers(
    modifiers: &[crate::modifiers::Modifier],
    full_w: u32,
    full_h: u32,
) -> Vec<Option<TextRaster>> {
    use crate::modifiers::ModifierKind;

    let needs_text = modifiers
        .iter()
        .any(|m| m.has_visible_effect() && matches!(m.kind, ModifierKind::Text(_)));
    if !needs_text {
        return Vec::new();
    }

    modifiers
        .iter()
        .map(|m| {
            if m.has_visible_effect()
                && let ModifierKind::Text(t) = &m.kind
            {
                TextRaster::build(t, full_w, full_h)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::kinds::Text;

    fn raster(text: &Text, w: u32, h: u32) -> Option<TextRaster> {
        TextRaster::build(text, w, h)
    }

    #[test]
    fn empty_text_has_no_raster() {
        let text = Text::default();
        assert!(raster(&text, 100, 100).is_none());
    }

    #[test]
    fn covers_center_and_misses_corner() {
        let text = Text {
            content: "Hello".to_string(),
            size: 64.0,
            x: 0.5,
            y: 0.5,
            ..Text::default()
        };
        let Some(r) = raster(&text, 400, 400) else {
            return;
        };
        let covered_near_center = (170..230)
            .any(|y| (140..260).any(|x| r.sample(x as f32, y as f32).is_some_and(|c| c[3] > 0.0)));
        assert!(
            covered_near_center,
            "expected text coverage near block center"
        );
        assert!(
            r.sample(0.0, 0.0).is_none(),
            "expected no coverage at far corner"
        );
    }

    #[test]
    fn opacity_scales_alpha() {
        let mut text = Text {
            content: "X".to_string(),
            size: 80.0,
            ..Text::default()
        };
        text.opacity = 1.0;
        let full = raster(&text, 200, 200);
        text.opacity = 0.5;
        let half = raster(&text, 200, 200);
        if let (Some(f), Some(h)) = (full, half) {
            let pf = (0..200)
                .flat_map(|y| (0..200).map(move |x| (x as f32, y as f32)))
                .filter_map(|(x, y)| f.sample(x, y).map(|c| c[3]))
                .fold(0.0f32, f32::max);
            let ph = (0..200)
                .flat_map(|y| (0..200).map(move |x| (x as f32, y as f32)))
                .filter_map(|(x, y)| h.sample(x, y).map(|c| c[3]))
                .fold(0.0f32, f32::max);
            assert!(ph <= pf + 1e-4 && ph > 0.0);
        }
    }
}
