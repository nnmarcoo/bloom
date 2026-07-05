use rayon::prelude::*;

use crate::modifiers::drawing_raster::LayerView;
use crate::modifiers::text_raster::TextRaster;
use crate::modifiers::{Modifier, ModifierKind};

pub(crate) fn render_full(
    modifiers: &[Modifier],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
) -> Vec<u8> {
    let n = img_w as usize * img_h as usize * 4;
    let mut cur = vec![0u8; n];
    let copy = n.min(pixels.len());
    cur[..copy].copy_from_slice(&pixels[..copy]);

    let w = img_w as usize;
    let h = img_h as usize;

    let mut segment: Vec<&Modifier> = Vec::new();
    let flush_segment = |cur: &mut [u8], segment: &mut Vec<&Modifier>| {
        if segment.is_empty() {
            return;
        }
        apply_pointwise_segment(cur, img_w, img_h, segment);
        segment.clear();
    };

    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        match &m.kind {
            ModifierKind::GaussianBlur(gb) => {
                flush_segment(&mut cur, &mut segment);
                blur_full(&mut cur, w, h, gb.radius);
            }
            ModifierKind::ChromaticAberration(ca) => {
                flush_segment(&mut cur, &mut segment);
                cur = chromatic_aberration_full(&cur, img_w, img_h, ca.amount);
            }
            ModifierKind::Text(_) => {
                flush_segment(&mut cur, &mut segment);
                if let Some(Some(raster)) = text_layers.get(i) {
                    text_full(&mut cur, img_w, img_h, raster);
                }
            }
            ModifierKind::Drawing(_) => {
                flush_segment(&mut cur, &mut segment);
                if let Some(Some(raster)) = drawing_layers.get(i) {
                    drawing_full(&mut cur, img_w, raster);
                }
            }
            ModifierKind::PixelSort(ps) => {
                flush_segment(&mut cur, &mut segment);
                cur = crate::modifiers::pixel_sort::pixel_sort_cpu(
                    &cur,
                    w,
                    h,
                    ps.threshold,
                    ps.angle,
                );
            }
            _ => segment.push(m),
        }
    }
    flush_segment(&mut cur, &mut segment);
    cur
}

fn apply_pointwise_segment(buf: &mut [u8], img_w: u32, img_h: u32, segment: &[&Modifier]) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let v = (y as f32 + 0.5) / img_h as f32;
        for x in 0..w {
            let o = x * 4;
            let u = (x as f32 + 0.5) / img_w as f32;
            let mut c = pixel_to_f32(&row[o..o + 4]);
            for m in segment {
                c = m.kind.apply_cpu(img_w, img_h, [u, v], c);
            }
            row[o..o + 4].copy_from_slice(&f32_to_pixel(c.map(|v| v.clamp(0.0, 1.0))));
        }
    });
}

fn blur_full(buf: &mut [u8], w: usize, h: usize, radius: f32) {
    let r = radius.ceil() as i32;
    if r <= 0 || w == 0 || h == 0 {
        return;
    }
    let sigma = (radius / 3.0).max(0.5);
    let inv = 1.0 / (2.0 * sigma * sigma);
    let kernel: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 * inv).exp()).collect();
    let wsum: f32 = kernel.iter().sum();
    let norm: Vec<f32> = kernel.iter().map(|k| k / wsum).collect();

    let mut scratch = vec![0u8; buf.len()];
    scratch
        .par_chunks_mut(w * 4)
        .zip(buf.par_chunks(w * 4))
        .for_each(|(out_row, in_row)| {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for (ki, &k) in norm.iter().enumerate() {
                    let sx = (x as i32 - r + ki as i32).clamp(0, w as i32 - 1) as usize;
                    let o = sx * 4;
                    acc[0] += in_row[o] as f32 * k;
                    acc[1] += in_row[o + 1] as f32 * k;
                    acc[2] += in_row[o + 2] as f32 * k;
                    acc[3] += in_row[o + 3] as f32 * k;
                }
                let o = x * 4;
                for c in 0..4 {
                    out_row[o + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        });
    buf.par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, out_row)| {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for (ki, &k) in norm.iter().enumerate() {
                    let sy = (y as i32 - r + ki as i32).clamp(0, h as i32 - 1) as usize;
                    let o = (sy * w + x) * 4;
                    acc[0] += scratch[o] as f32 * k;
                    acc[1] += scratch[o + 1] as f32 * k;
                    acc[2] += scratch[o + 2] as f32 * k;
                    acc[3] += scratch[o + 3] as f32 * k;
                }
                let o = x * 4;
                for c in 0..4 {
                    out_row[o + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        });
}

fn chromatic_aberration_full(src: &[u8], img_w: u32, img_h: u32, amount: f32) -> Vec<u8> {
    let w = img_w as usize;
    let scale = amount / img_w as f32;
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let v = (y as f32 + 0.5) / img_h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / img_w as f32;
            let r_uv = [
                (u + (u - 0.5) * scale).clamp(0.0, 1.0),
                (v + (v - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let b_uv = [
                (u - (u - 0.5) * scale).clamp(0.0, 1.0),
                (v - (v - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let cr = sample_pixel(src, img_w, img_h, r_uv[0], r_uv[1]);
            let cg = sample_pixel(src, img_w, img_h, u, v);
            let cb = sample_pixel(src, img_w, img_h, b_uv[0], b_uv[1]);
            let o = x * 4;
            row[o..o + 4].copy_from_slice(&f32_to_pixel([cr[0], cg[1], cb[2], cg[3]]));
        }
    });
    out
}

fn drawing_full(buf: &mut [u8], img_w: u32, raster: &LayerView<'_>) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = y as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

fn text_full(buf: &mut [u8], img_w: u32, img_h: u32, raster: &TextRaster) {
    let w = img_w as usize;
    let _ = img_h;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = y as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

pub(crate) fn apply_modifiers(
    modifiers: &[Modifier],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    c: [f32; 4],
) -> [f32; 4] {
    apply_modifiers_with_layers(modifiers, &[], &[], pixels, img_w, img_h, 0.0, 0.0, uv, c)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_modifiers_with_layers(
    modifiers: &[Modifier],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    fx: f32,
    fy: f32,
    uv: [f32; 2],
    mut c: [f32; 4],
) -> [f32; 4] {
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        match &m.kind {
            ModifierKind::ChromaticAberration(ca) => {
                let scale = ca.amount / img_w as f32;
                let r_uv = [
                    (uv[0] + (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                    (uv[1] + (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
                ];
                let b_uv = [
                    (uv[0] - (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                    (uv[1] - (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
                ];
                let prior = &modifiers[..i];
                let cr = apply_prior(
                    prior,
                    &text_layers[..i.min(text_layers.len())],
                    &drawing_layers[..i.min(drawing_layers.len())],
                    img_w,
                    img_h,
                    r_uv,
                    sample_pixel(pixels, img_w, img_h, r_uv[0], r_uv[1]),
                );
                let cb = apply_prior(
                    prior,
                    &text_layers[..i.min(text_layers.len())],
                    &drawing_layers[..i.min(drawing_layers.len())],
                    img_w,
                    img_h,
                    b_uv,
                    sample_pixel(pixels, img_w, img_h, b_uv[0], b_uv[1]),
                );
                c[0] = cr[0];
                c[2] = cb[2];
            }
            ModifierKind::GaussianBlur(gb) => {
                let prior = &modifiers[..i];
                let prior_text = &text_layers[..i.min(text_layers.len())];
                let prior_drawing = &drawing_layers[..i.min(drawing_layers.len())];
                c = gaussian_blur_cpu(
                    prior,
                    prior_text,
                    prior_drawing,
                    pixels,
                    img_w,
                    img_h,
                    uv,
                    gb.radius,
                );
            }
            ModifierKind::Text(_) => {
                if let Some(Some(raster)) = text_layers.get(i)
                    && let Some(src) = raster.sample(fx, fy)
                {
                    c = blend_over(c, src);
                }
            }
            ModifierKind::Drawing(_) => {
                if let Some(Some(raster)) = drawing_layers.get(i)
                    && let Some(src) = raster.sample(fx, fy)
                {
                    c = blend_over(c, src);
                }
            }
            _ => {
                c = m.kind.apply_cpu(img_w, img_h, uv, c);
            }
        }
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

#[allow(clippy::too_many_arguments)]
fn gaussian_blur_cpu(
    prior: &[Modifier],
    prior_text: &[Option<TextRaster>],
    prior_drawing: &[Option<LayerView<'_>>],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    radius: f32,
) -> [f32; 4] {
    let r = radius.ceil() as i32;
    if r <= 0 {
        return apply_prior(
            prior,
            prior_text,
            prior_drawing,
            img_w,
            img_h,
            uv,
            sample_pixel(pixels, img_w, img_h, uv[0], uv[1]),
        );
    }
    let sigma = (radius / 3.0).max(0.5);
    let inv_two_sigma_sq = 1.0 / (2.0 * sigma * sigma);
    let cx = uv[0] * img_w as f32;
    let cy = uv[1] * img_h as f32;
    let mut sum = [0.0f32; 4];
    let mut weight_sum = 0.0f32;
    for dy in -r..=r {
        let wy = (-(dy * dy) as f32 * inv_two_sigma_sq).exp();
        for dx in -r..=r {
            let w = wy * (-(dx * dx) as f32 * inv_two_sigma_sq).exp();
            let su = ((cx + dx as f32) / img_w as f32).clamp(0.0, 1.0);
            let sv = ((cy + dy as f32) / img_h as f32).clamp(0.0, 1.0);
            let tap = apply_prior(
                prior,
                prior_text,
                prior_drawing,
                img_w,
                img_h,
                [su, sv],
                sample_pixel(pixels, img_w, img_h, su, sv),
            );
            for k in 0..4 {
                sum[k] += tap[k] * w;
            }
            weight_sum += w;
        }
    }
    if weight_sum > 0.0 {
        sum.map(|v| v / weight_sum)
    } else {
        sum
    }
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn blend_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let sa = src[3];
    let da = dst[3];
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let blend = |s: f32, d: f32| (s * sa + d * da * (1.0 - sa)) / out_a;
    [
        blend(src[0], dst[0]),
        blend(src[1], dst[1]),
        blend(src[2], dst[2]),
        out_a,
    ]
}

fn apply_prior(
    modifiers: &[Modifier],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    mut c: [f32; 4],
) -> [f32; 4] {
    let fx = uv[0] * img_w as f32;
    let fy = uv[1] * img_h as f32;
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        match &m.kind {
            ModifierKind::Text(_) => {
                if let Some(Some(raster)) = text_layers.get(i)
                    && let Some(src) = raster.sample(fx, fy)
                {
                    c = blend_over(c, src);
                }
            }
            ModifierKind::Drawing(_) => {
                if let Some(Some(raster)) = drawing_layers.get(i)
                    && let Some(src) = raster.sample(fx, fy)
                {
                    c = blend_over(c, src);
                }
            }
            kind if !kind.effect_class().is_pointwise() => {}
            kind => {
                c = kind.apply_cpu(img_w, img_h, uv, c);
            }
        }
    }
    c
}

pub(crate) fn pixel_to_f32(p: &[u8]) -> [f32; 4] {
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}

pub(crate) fn f32_to_pixel(c: [f32; 4]) -> [u8; 4] {
    [
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        (c[3] * 255.0).round() as u8,
    ]
}

pub(crate) fn sample_pixel(pixels: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
    let x = (u * w as f32).clamp(0.0, w as f32 - 1.0) as usize;
    let y = (v * h as f32).clamp(0.0, h as f32 - 1.0) as usize;
    let base = (y * w as usize + x) * 4;
    match pixels.get(base..base + 4) {
        Some(p) => [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            p[3] as f32 / 255.0,
        ],
        None => [0.0; 4],
    }
}

fn hash_u(v: u32) -> u32 {
    let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let s = ((s >> ((s >> 28).wrapping_add(4))) ^ s).wrapping_mul(277803737);
    (s >> 22) ^ s
}

pub(crate) fn hash21(ix: i32, iy: i32, seed: i32) -> f32 {
    let h = hash_u(
        (ix as u32) ^ (iy as u32).wrapping_mul(1664525) ^ (seed as u32).wrapping_mul(22695477),
    );
    h as f32 / 4294967295.0
}

pub(crate) fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let max_c = rgb[0].max(rgb[1]).max(rgb[2]);
    let min_c = rgb[0].min(rgb[1]).min(rgb[2]);
    let l = (max_c + min_c) * 0.5;
    if max_c == min_c {
        return [0.0, 0.0, l];
    }
    let d = max_c - min_c;
    let s = if l < 0.5 {
        d / (max_c + min_c)
    } else {
        d / (2.0 - max_c - min_c)
    };
    let h = if max_c == rgb[0] {
        (rgb[1] - rgb[2]) / d + if rgb[1] >= rgb[2] { 0.0 } else { 6.0 }
    } else if max_c == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.0
    } else {
        (rgb[0] - rgb[1]) / d + 4.0
    };
    [h / 6.0, s, l]
}

pub(crate) fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    if hsl[1] == 0.0 {
        return [hsl[2]; 3];
    }
    let q = if hsl[2] < 0.5 {
        hsl[2] * (1.0 + hsl[1])
    } else {
        hsl[2] + hsl[1] - hsl[2] * hsl[1]
    };
    let p = 2.0 * hsl[2] - q;
    [
        hue_to_rgb(p, q, hsl[0] + 1.0 / 3.0),
        hue_to_rgb(p, q, hsl[0]),
        hue_to_rgb(p, q, hsl[0] - 1.0 / 3.0),
    ]
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    let mut t = t_in;
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

#[cfg(test)]
mod pointwise_tests {
    use crate::modifiers::kinds::{Grayscale, Invert, Sepia, Temperature};
    use crate::modifiers::{Modifier, ModifierKind};

    fn apply(kind: ModifierKind, c: [f32; 4]) -> [f32; 4] {
        Modifier::new(kind).kind.apply_cpu(1, 1, [0.5, 0.5], c)
    }

    #[test]
    fn invert_flips_channels_and_scales_by_amount() {
        let full = apply(ModifierKind::Invert(Invert { amount: 1.0 }), [0.2, 0.6, 0.9, 1.0]);
        assert!((full[0] - 0.8).abs() < 1e-5);
        assert!((full[1] - 0.4).abs() < 1e-5);
        assert!((full[2] - 0.1).abs() < 1e-5);
        assert_eq!(full[3], 1.0, "alpha untouched");

        let half = apply(ModifierKind::Invert(Invert { amount: 0.5 }), [0.2, 0.6, 0.9, 1.0]);
        assert!((half[0] - 0.5).abs() < 1e-5, "amount 0.5 is halfway to inverse");

        let none = apply(ModifierKind::Invert(Invert { amount: 0.0 }), [0.2, 0.6, 0.9, 1.0]);
        assert!((none[0] - 0.2).abs() < 1e-5, "amount 0 is identity");
    }

    #[test]
    fn grayscale_collapses_to_luma() {
        let g = apply(
            ModifierKind::Grayscale(Grayscale { amount: 1.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        let luma = 0.2 * 0.2126 + 0.6 * 0.7152 + 0.9 * 0.0722;
        assert!((g[0] - luma).abs() < 1e-5);
        assert!((g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6, "all channels equal");
        assert_eq!(g[3], 1.0);
    }

    #[test]
    fn temperature_warms_red_cools_blue() {
        let t = apply(
            ModifierKind::Temperature(Temperature {
                temperature: 0.1,
                tint: 0.05,
            }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!((t[0] - 0.6).abs() < 1e-5, "temp raises red");
        assert!((t[1] - 0.55).abs() < 1e-5, "tint raises green");
        assert!((t[2] - 0.4).abs() < 1e-5, "temp lowers blue");
    }

    #[test]
    fn sepia_tints_toward_warm_and_desaturates() {
        let s = apply(
            ModifierKind::Sepia(Sepia { intensity: 1.0 }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!(s[0] > s[1] && s[1] > s[2], "sepia is R>G>B warm tint");
        assert_eq!(s[3], 1.0);

        let none = apply(ModifierKind::Sepia(Sepia { intensity: 0.0 }), [0.5, 0.5, 0.5, 1.0]);
        assert!((none[0] - 0.5).abs() < 1e-5, "intensity 0 is identity");
    }
}
