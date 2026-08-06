use rayon::prelude::*;

use crate::modifiers::drawing_raster::LayerView;
use crate::modifiers::kinds::ResizeFilter;
use crate::modifiers::plan::{ImageSpec, PlanItem, infer_specs, plan_modifiers};
use crate::modifiers::text_raster::TextRaster;
use crate::modifiers::{Modifier, ModifierKind, motion_blur_samples};

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

    // Walk the shared plan rather than re-deriving the segmentation here: the
    // GPU pipeline consumes the same plan, so the two backends cannot drift
    // apart in how they group modifiers.
    let plan = plan_modifiers(modifiers);
    let specs = infer_specs(ImageSpec::new(img_w, img_h), &plan);

    // Geometry is a running value rather than a loop constant, so a stage that
    // changes dimensions only has to declare it in `infer_specs`. Every stage is
    // passthrough today; the debug assert below pins that.
    for (item, spec) in plan.iter().zip(&specs) {
        let ImageSpec { w: img_w, h: img_h } = spec.input;
        let w = img_w as usize;
        let h = img_h as usize;
        debug_assert_eq!(
            cur.len(),
            w * h * 4,
            "buffer does not match the stage's declared input spec"
        );

        match item {
            PlanItem::Fused(segment) => {
                apply_pointwise_segment(&mut cur, img_w, img_h, segment);
            }
            // `i` indexes the original stack, which is what the positionally
            // stored text and drawing rasters are keyed by.
            PlanItem::Step(i, m) => match &m.kind {
                ModifierKind::GaussianBlur(gb) => blur_full(&mut cur, w, h, gb.radius),
                ModifierKind::ChromaticAberration(ca) => {
                    cur = chromatic_aberration_full(&cur, img_w, img_h, ca.amount);
                }
                ModifierKind::MotionBlur(mb) => {
                    cur = motion_blur_full(&cur, img_w, img_h, mb.angle, mb.distance);
                }
                ModifierKind::Text(_) => {
                    if let Some(Some(raster)) = text_layers.get(*i) {
                        text_full(&mut cur, img_w, img_h, raster);
                    }
                }
                ModifierKind::Drawing(_) => {
                    if let Some(Some(raster)) = drawing_layers.get(*i) {
                        drawing_full(&mut cur, img_w, raster);
                    }
                }
                ModifierKind::PixelSort(ps) => {
                    cur = crate::modifiers::pixel_sort::pixel_sort_cpu(
                        &cur,
                        w,
                        h,
                        ps.threshold,
                        ps.angle,
                    );
                }
                // The only stage whose output geometry differs from its input.
                // `spec.output` is authoritative -- it is what every later
                // stage was sized against by `infer_specs`.
                ModifierKind::Resize(r) => {
                    let out = spec.output;
                    cur = resample(&cur, img_w, img_h, out.w, out.h, r.filter);
                }
                // `plan_modifiers` only emits a `Step` for modifiers the
                // planner classifies as non-pointwise, and
                // `planner_classification_covers_every_modifier_type` pins that
                // set to exactly the arms above. A kind arriving here means a
                // new non-pointwise modifier was added without a CPU
                // implementation, which would otherwise render as a silent
                // no-op.
                other => debug_assert!(
                    false,
                    "{} is planned as a standalone step but has no CPU implementation",
                    other.name()
                ),
            },
        }

        debug_assert_eq!(
            cur.len(),
            spec.output.w as usize * spec.output.h as usize * 4,
            "stage output does not match its declared output spec"
        );
    }
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
            let cr = sample_bilinear(
                src,
                img_w,
                img_h,
                r_uv[0] * img_w as f32,
                r_uv[1] * img_h as f32,
            );
            let cg = sample_pixel(src, img_w, img_h, u, v);
            let cb = sample_bilinear(
                src,
                img_w,
                img_h,
                b_uv[0] * img_w as f32,
                b_uv[1] * img_h as f32,
            );
            let o = x * 4;
            row[o..o + 4].copy_from_slice(&f32_to_pixel([cr[0], cg[1], cb[2], cg[3]]));
        }
    });
    out
}

fn motion_blur_full(src: &[u8], img_w: u32, img_h: u32, angle: f32, distance: f32) -> Vec<u8> {
    let w = img_w as usize;
    let rad = angle.to_radians();
    let du = rad.cos() * distance;
    let dv = rad.sin() * distance;
    let n = motion_blur_samples(distance) as i32;
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let cy = y as f32 + 0.5;
        for x in 0..w {
            let cx = x as f32 + 0.5;
            let mut acc = [0.0f32; 4];
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32 - 0.5;
                let s = sample_bilinear(src, img_w, img_h, cx + du * t, cy + dv * t);
                acc[0] += s[0];
                acc[1] += s[1];
                acc[2] += s[2];
                acc[3] += s[3];
            }
            let inv = 1.0 / n as f32;
            let o = x * 4;
            row[o..o + 4].copy_from_slice(&f32_to_pixel([
                acc[0] * inv,
                acc[1] * inv,
                acc[2] * inv,
                acc[3] * inv,
            ]));
        }
    });
    out
}

fn sample_bilinear(pixels: &[u8], w: u32, h: u32, fx: f32, fy: f32) -> [f32; 4] {
    let px = fx - 0.5;
    let py = fy - 0.5;
    let x0 = px.floor();
    let y0 = py.floor();
    let tx = px - x0;
    let ty = py - y0;
    let load = |x: f32, y: f32| -> [f32; 4] {
        let cx = (x.max(0.0) as u32).min(w - 1);
        let cy = (y.max(0.0) as u32).min(h - 1);
        let base = (cy as usize * w as usize + cx as usize) * 4;
        match pixels.get(base..base + 4) {
            Some(p) => [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
                p[3] as f32 / 255.0,
            ],
            None => [0.0; 4],
        }
    };
    let c00 = load(x0, y0);
    let c10 = load(x0 + 1.0, y0);
    let c01 = load(x0, y0 + 1.0);
    let c11 = load(x0 + 1.0, y0 + 1.0);
    let mut o = [0.0f32; 4];
    for i in 0..4 {
        let top = c00[i] + (c10[i] - c00[i]) * tx;
        let bot = c01[i] + (c11[i] - c01[i]) * tx;
        o[i] = top + (bot - top) * ty;
    }
    o
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

pub(crate) fn pixel_to_f32(p: &[u8]) -> [f32; 4] {
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}

fn lanczos3(x: f32) -> f32 {
    const A: f32 = 3.0;
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= A {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    (px.sin() / px) * ((px / A).sin() / (px / A))
}

/// Resamples `src` to `dst_w` x `dst_h`.
///
/// Separable: one horizontal pass into a scratch buffer, then one vertical
/// pass. The filter radius is scaled by the *reciprocal* of the scale factor
/// when minifying, which is what makes a downscale average its input rather
/// than point-sample it -- a fixed-radius kernel would alias badly at large
/// reductions.
///
/// `Nearest` deliberately ignores that: its blockiness is the reason to pick
/// it, so it stays a true point sample in both directions.
pub(crate) fn resample(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    filter: ResizeFilter,
) -> Vec<u8> {
    if (src_w, src_h) == (dst_w, dst_h) {
        return src.to_vec();
    }

    // Per-axis kernel: `support` is the half-width in *source* pixels, and
    // `weight` is evaluated in kernel space.
    let axis = |dst: u32, src: u32| -> (f32, f32) {
        let scale = dst as f32 / src as f32;
        // Minifying (scale < 1) widens the footprint in source space.
        let inv = if scale < 1.0 { 1.0 / scale } else { 1.0 };
        (scale, inv)
    };
    let radius = |f: ResizeFilter| -> f32 {
        match f {
            ResizeFilter::Nearest => 0.0,
            ResizeFilter::Bilinear => 1.0,
            ResizeFilter::Lanczos => 3.0,
        }
    };
    let weight = |f: ResizeFilter, x: f32| -> f32 {
        match f {
            ResizeFilter::Nearest => 1.0,
            ResizeFilter::Bilinear => (1.0 - x.abs()).max(0.0),
            ResizeFilter::Lanczos => lanczos3(x),
        }
    };

    let one_axis = |input: &[u8], in_w: u32, in_h: u32, out_len: u32, horizontal: bool| -> Vec<u8> {
        let (out_w, out_h) = if horizontal {
            (out_len, in_h)
        } else {
            (in_w, out_len)
        };
        let src_len = if horizontal { in_w } else { in_h };
        let (scale, inv) = axis(out_len, src_len);
        let r = radius(filter) * inv;

        let mut out = vec![0u8; out_w as usize * out_h as usize * 4];
        out.par_chunks_mut(out_w as usize * 4)
            .enumerate()
            .for_each(|(row, out_row)| {
                for col in 0..out_w as usize {
                    let o = if horizontal { col } else { row };
                    // Centre of this output sample, in source coordinates.
                    let center = (o as f32 + 0.5) / scale;

                    if matches!(filter, ResizeFilter::Nearest) {
                        let s = (center.floor().max(0.0) as u32).min(src_len - 1);
                        let (sx, sy) = if horizontal {
                            (s, row as u32)
                        } else {
                            (col as u32, s)
                        };
                        let base = (sy as usize * in_w as usize + sx as usize) * 4;
                        out_row[col * 4..col * 4 + 4].copy_from_slice(&input[base..base + 4]);
                        continue;
                    }

                    let lo = (center - r).floor().max(0.0) as u32;
                    let hi = ((center + r).ceil() as u32).min(src_len);
                    let mut acc = [0.0f32; 4];
                    let mut wsum = 0.0f32;
                    for s in lo..hi {
                        let d = (s as f32 + 0.5 - center) / inv;
                        let wt = weight(filter, d);
                        if wt == 0.0 {
                            continue;
                        }
                        let (sx, sy) = if horizontal {
                            (s, row as u32)
                        } else {
                            (col as u32, s)
                        };
                        let base = (sy as usize * in_w as usize + sx as usize) * 4;
                        for c in 0..4 {
                            acc[c] += input[base + c] as f32 * wt;
                        }
                        wsum += wt;
                    }
                    // A Lanczos kernel has negative lobes, so normalise by the
                    // actual weight sum rather than assuming it is 1.0.
                    let n = if wsum.abs() < 1e-6 { 1.0 } else { wsum };
                    for c in 0..4 {
                        out_row[col * 4 + c] = (acc[c] / n).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        out
    };

    let mid = one_axis(src, src_w, src_h, dst_w, true);
    one_axis(&mid, dst_w, src_h, dst_h, false)
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
    use crate::modifiers::kinds::{Duotone, Grayscale, Invert, Sepia, Solarize, Temperature};
    use crate::modifiers::{Modifier, ModifierKind};

    fn apply(kind: ModifierKind, c: [f32; 4]) -> [f32; 4] {
        Modifier::new(kind).kind.apply_cpu(1, 1, [0.5, 0.5], c)
    }

    #[test]
    fn invert_flips_channels_and_scales_by_amount() {
        let full = apply(
            ModifierKind::Invert(Invert { amount: 1.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        assert!((full[0] - 0.8).abs() < 1e-5);
        assert!((full[1] - 0.4).abs() < 1e-5);
        assert!((full[2] - 0.1).abs() < 1e-5);
        assert_eq!(full[3], 1.0, "alpha untouched");

        let half = apply(
            ModifierKind::Invert(Invert { amount: 0.5 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        assert!(
            (half[0] - 0.5).abs() < 1e-5,
            "amount 0.5 is halfway to inverse"
        );

        let none = apply(
            ModifierKind::Invert(Invert { amount: 0.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
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
        assert!(
            (g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6,
            "all channels equal"
        );
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

        let none = apply(
            ModifierKind::Sepia(Sepia { intensity: 0.0 }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!((none[0] - 0.5).abs() < 1e-5, "intensity 0 is identity");
    }

    #[test]
    fn solarize_inverts_above_threshold_only() {
        let s = apply(
            ModifierKind::Solarize(Solarize { threshold: 0.5 }),
            [0.2, 0.8, 0.5, 1.0],
        );
        assert!((s[0] - 0.2).abs() < 1e-5, "0.2 < 0.5 stays");
        assert!((s[1] - 0.2).abs() < 1e-5, "0.8 >= 0.5 inverts to 0.2");
        assert!((s[2] - 0.5).abs() < 1e-5, "0.5 >= 0.5 inverts to 0.5");
        assert_eq!(s[3], 1.0);
    }

    #[test]
    fn duotone_maps_luma_endpoints_to_colors() {
        let shadow = [0.1, 0.15, 0.4];
        let highlight = [1.0, 0.95, 0.8];
        let d = |c| {
            apply(
                ModifierKind::Duotone(Duotone {
                    shadow,
                    highlight,
                    amount: 1.0,
                }),
                c,
            )
        };

        let black = d([0.0, 0.0, 0.0, 1.0]);
        for i in 0..3 {
            assert!((black[i] - shadow[i]).abs() < 1e-5, "black -> shadow color");
        }

        let white = d([1.0, 1.0, 1.0, 1.0]);
        for i in 0..3 {
            assert!(
                (white[i] - highlight[i]).abs() < 1e-5,
                "white -> highlight color"
            );
        }
        assert_eq!(white[3], 1.0);

        let neutral = apply(
            ModifierKind::Duotone(Duotone {
                shadow,
                highlight,
                amount: 0.0,
            }),
            [0.3, 0.7, 0.2, 1.0],
        );
        assert!((neutral[0] - 0.3).abs() < 1e-5, "amount 0 is identity");
    }
}

#[cfg(test)]
mod resample_tests {
    use super::{render_full, resample};
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::{Modifier, ModifierKind};

    /// Left half black, right half white -- a single vertical edge, which is
    /// what makes filter differences legible.
    fn split(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                let c = if x < w / 2 { 0u8 } else { 255u8 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    }

    fn px(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let b = ((y * w + x) * 4) as usize;
        [buf[b], buf[b + 1], buf[b + 2], buf[b + 3]]
    }

    #[test]
    fn resample_produces_the_requested_dimensions() {
        for f in ResizeFilter::ALL {
            let out = resample(&split(64, 32), 64, 32, 21, 47, f);
            assert_eq!(out.len(), 21 * 47 * 4, "{} produced a wrong-sized buffer", f.label());
        }
    }

    #[test]
    fn identity_resample_is_byte_exact() {
        let src = split(32, 16);
        for f in ResizeFilter::ALL {
            assert_eq!(resample(&src, 32, 16, 32, 16, f), src, "{}", f.label());
        }
    }

    /// Nearest must not invent intermediate values -- that blockiness is the
    /// only reason to choose it.
    #[test]
    fn nearest_preserves_the_source_palette() {
        let out = resample(&split(64, 8), 64, 8, 27, 8, ResizeFilter::Nearest);
        for i in (0..out.len()).step_by(4) {
            assert!(
                out[i] == 0 || out[i] == 255,
                "nearest produced a blended value {} at byte {i}",
                out[i]
            );
        }
    }

    /// Bilinear and Lanczos must blend across the edge; if they did not, they
    /// would be point sampling under another name.
    #[test]
    fn smooth_filters_blend_across_an_edge() {
        for f in [ResizeFilter::Bilinear, ResizeFilter::Lanczos] {
            let out = resample(&split(64, 8), 64, 8, 32, 8, f);
            let blended = (0..32).any(|x| {
                let v = px(&out, 32, x, 4)[0];
                v > 8 && v < 247
            });
            assert!(blended, "{} produced no intermediate values", f.label());
        }
    }

    /// A large downscale must average its input rather than point-sample it.
    /// With a fixed-radius kernel this aliases: the result depends on which
    /// source column each output pixel happens to land on. Averaging makes the
    /// two halves come out near the extremes with a smooth transition.
    #[test]
    fn large_downscale_averages_rather_than_aliases() {
        // 512 -> 8 is a 64x reduction; each output pixel covers 64 source px.
        let out = resample(&split(512, 8), 512, 8, 8, 8, ResizeFilter::Lanczos);
        let left = px(&out, 8, 0, 4)[0];
        let right = px(&out, 8, 7, 4)[0];
        assert!(left < 16, "left edge should stay dark, got {left}");
        assert!(right > 239, "right edge should stay light, got {right}");
    }

    #[test]
    fn alpha_survives_resampling() {
        let src = vec![255u8; 16 * 16 * 4];
        for f in ResizeFilter::ALL {
            let out = resample(&src, 16, 16, 9, 9, f);
            assert!(
                out.chunks(4).all(|p| p[3] == 255),
                "{} did not preserve opaque alpha",
                f.label()
            );
        }
    }

    /// End-to-end through the plan: `render_full` must return a buffer at the
    /// chain's output size, not the source size.
    #[test]
    fn render_full_returns_the_resized_buffer() {
        let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Pixels,
            width: 20.0,
            height: 10.0,
            filter: ResizeFilter::Bilinear,
            lock_aspect: false,
        }))];
        let out = render_full(&chain, &[], &[], &split(64, 32), 64, 32);
        assert_eq!(out.len(), 20 * 10 * 4);
    }

    /// A resize mid-chain must leave later stages operating at the new size.
    /// Before `infer_specs` was wired up this would panic on a buffer/geometry
    /// mismatch in the debug assert.
    #[test]
    fn a_stage_after_a_resize_runs_at_the_new_size() {
        use crate::modifiers::kinds::GaussianBlur;
        let chain = vec![
            Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 50.0,
                height: 50.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            })),
            Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 2.0 })),
        ];
        let out = render_full(&chain, &[], &[], &split(64, 32), 64, 32);
        assert_eq!(out.len(), 32 * 16 * 4);
    }
}

#[cfg(test)]
mod motion_blur_tests {
    use super::render_full;
    use crate::modifiers::kinds::MotionBlur;
    use crate::modifiers::{Modifier, ModifierKind};

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let on = (x + y) % 2 == 0;
                let v = if on { 255 } else { 0 };
                px[o] = v;
                px[o + 1] = v;
                px[o + 2] = v;
                px[o + 3] = 255;
            }
        }
        px
    }

    fn gpu_math_reference(src: &[u8], w: u32, h: u32, angle: f32, distance: f32) -> Vec<u8> {
        let (wi, hi) = (w as i32, h as i32);
        let rad = angle.to_radians();
        let du = rad.cos() * distance;
        let dv = rad.sin() * distance;
        let n = crate::modifiers::motion_blur_samples(distance) as i32;
        let load = |x: i32, y: i32| -> [f32; 4] {
            let cx = x.clamp(0, wi - 1);
            let cy = y.clamp(0, hi - 1);
            let o = ((cy * wi + cx) * 4) as usize;
            [
                src[o] as f32 / 255.0,
                src[o + 1] as f32 / 255.0,
                src[o + 2] as f32 / 255.0,
                src[o + 3] as f32 / 255.0,
            ]
        };
        let bilinear = |fx: f32, fy: f32| -> [f32; 4] {
            let px = fx - 0.5;
            let py = fy - 0.5;
            let x0 = px.floor() as i32;
            let y0 = py.floor() as i32;
            let tx = px - x0 as f32;
            let ty = py - y0 as f32;
            let c00 = load(x0, y0);
            let c10 = load(x0 + 1, y0);
            let c01 = load(x0, y0 + 1);
            let c11 = load(x0 + 1, y0 + 1);
            let mut o = [0.0f32; 4];
            for i in 0..4 {
                let top = c00[i] + (c10[i] - c00[i]) * tx;
                let bot = c01[i] + (c11[i] - c01[i]) * tx;
                o[i] = top + (bot - top) * ty;
            }
            o
        };
        let mut out = vec![0u8; src.len()];
        for y in 0..hi {
            for x in 0..wi {
                let cx = x as f32 + 0.5;
                let cy = y as f32 + 0.5;
                let mut acc = [0.0f32; 4];
                for i in 0..n {
                    let t = i as f32 / (n - 1) as f32 - 0.5;
                    let s = bilinear(cx + du * t, cy + dv * t);
                    for c in 0..4 {
                        acc[c] += s[c];
                    }
                }
                let o = ((y * wi + x) * 4) as usize;
                for c in 0..4 {
                    out[o + c] = ((acc[c] / n as f32).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
        out
    }

    #[test]
    fn export_matches_gpu_sample_math() {
        let (w, h) = (24u32, 18u32);
        let src = checker(w, h);
        for &(angle, dist) in &[(0.0, 20.0), (90.0, 16.0), (33.0, 25.0)] {
            let mods = vec![Modifier::new(ModifierKind::MotionBlur(MotionBlur {
                angle,
                distance: dist,
            }))];
            let bulk = render_full(&mods, &[], &[], &src, w, h);
            let reference = gpu_math_reference(&src, w, h, angle, dist);
            for (a, b) in bulk.iter().zip(&reference) {
                assert!(
                    (*a as i32 - *b as i32).abs() <= 1,
                    "angle {angle} dist {dist}: export vs gpu-math differ"
                );
            }
        }
    }

    #[test]
    fn horizontal_blur_smears_across_vertical_edge() {
        let (w, h) = (9u32, 1u32);
        let mut src = vec![0u8; (w * h * 4) as usize];
        for x in 0..w {
            let o = (x * 4) as usize;
            let v = if x < w / 2 { 0 } else { 255 };
            src[o] = v;
            src[o + 1] = v;
            src[o + 2] = v;
            src[o + 3] = 255;
        }
        let mods = vec![Modifier::new(ModifierKind::MotionBlur(MotionBlur {
            angle: 0.0,
            distance: 40.0,
        }))];
        let out = render_full(&mods, &[], &[], &src, w, h);
        let mid = ((w / 2) * 4) as usize;
        assert!(
            out[mid] > 10 && out[mid] < 245,
            "pixel at the edge should be a blend, got {}",
            out[mid]
        );
    }
}
