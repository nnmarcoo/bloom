use cosmic_text::{FontSystem, SwashCache};

use crate::modifiers::kinds::Text;
use crate::modifiers::text_render;

const REFERENCE_SIZE: f32 = 1024.0;

pub struct TextRaster {
    alpha: Vec<u8>,
    bw: usize,
    bh: usize,
    anchor_x: f32,
    anchor_y: f32,
    block_w: f32,
    block_h: f32,
    cos: f32,
    sin: f32,
    color: [f32; 3],
    opacity: f32,
}

impl TextRaster {
    pub fn build(
        text: &Text,
        full_w: u32,
        full_h: u32,
        font_system: &mut FontSystem,
        swash: &mut SwashCache,
    ) -> Option<Self> {
        if text.content.is_empty() || text.opacity <= 0.0 {
            return None;
        }

        let raster_size = text.size.clamp(1.0, REFERENCE_SIZE);
        let mut raster_text = text.clone();
        raster_text.size = raster_size;

        let bmp = text_render::rasterize_text(&raster_text, font_system, swash);
        let packed = bmp.pack_alpha()?;
        let bbox_w = packed.bbox_w;
        let bbox_h = packed.bbox_h;
        let bw = packed.width as usize;
        let bh = packed.height as usize;
        let alpha = packed.alpha;

        let scale = text.size / raster_size;
        let (sin, cos) = text.rotation.to_radians().sin_cos();

        Some(Self {
            alpha,
            bw,
            bh,
            anchor_x: text.x * full_w as f32,
            anchor_y: text.y * full_h as f32,
            block_w: bbox_w * scale,
            block_h: bbox_h * scale,
            cos,
            sin,
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

        let a = self.sample_alpha(u, v) * self.opacity;
        if a <= 0.0 {
            return None;
        }
        Some([self.color[0], self.color[1], self.color[2], a])
    }

    fn sample_alpha(&self, u: f32, v: f32) -> f32 {
        let fx = (u * self.bw as f32 - 0.5).clamp(0.0, self.bw as f32 - 1.0);
        let fy = (v * self.bh as f32 - 0.5).clamp(0.0, self.bh as f32 - 1.0);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(self.bw - 1);
        let y1 = (y0 + 1).min(self.bh - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;

        let at = |x: usize, y: usize| self.alpha[y * self.bw + x] as f32 / 255.0;
        let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
        let bot = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
        top * (1.0 - ty) + bot * ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::kinds::Text;

    fn raster(text: &Text, w: u32, h: u32) -> Option<TextRaster> {
        let mut fs = FontSystem::new();
        let mut swash = SwashCache::new();
        TextRaster::build(text, w, h, &mut fs, &mut swash)
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
        let center = r.sample(200.0, 200.0);
        assert!(
            center.is_some_and(|c| c[3] > 0.0),
            "expected coverage at block center"
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

fn raster_resources()
-> &'static std::sync::Mutex<(FontSystem, SwashCache)> {
    use std::sync::{Mutex, OnceLock};
    static RES: OnceLock<Mutex<(FontSystem, SwashCache)>> = OnceLock::new();
    RES.get_or_init(|| Mutex::new((FontSystem::new(), SwashCache::new())))
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

    let mut guard = raster_resources().lock().unwrap_or_else(|e| e.into_inner());
    let (font_system, swash) = &mut *guard;
    modifiers
        .iter()
        .map(|m| {
            if m.has_visible_effect()
                && let ModifierKind::Text(t) = &m.kind
            {
                TextRaster::build(t, full_w, full_h, font_system, swash)
            } else {
                None
            }
        })
        .collect()
}
