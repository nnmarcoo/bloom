use crate::modifiers::{InputClass, Modifier, ModifierKind};

pub(crate) fn apply_modifiers(
    modifiers: &[Modifier],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    mut c: [f32; 4],
) -> [f32; 4] {
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        if let ModifierKind::ChromaticAberration(ca) = &m.kind {
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
            let cr = apply_prior_non_resampling(
                prior,
                img_w,
                img_h,
                r_uv,
                sample_pixel(pixels, img_w, img_h, r_uv[0], r_uv[1]),
            );
            let cb = apply_prior_non_resampling(
                prior,
                img_w,
                img_h,
                b_uv,
                sample_pixel(pixels, img_w, img_h, b_uv[0], b_uv[1]),
            );
            c[0] = cr[0];
            c[2] = cb[2];
        } else {
            c = m.kind.apply_cpu(img_w, img_h, uv, c);
        }
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

fn apply_prior_non_resampling(
    modifiers: &[Modifier],
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    mut c: [f32; 4],
) -> [f32; 4] {
    for m in modifiers {
        if !m.has_visible_effect() || matches!(m.kind.input_class(), InputClass::NonPointwise) {
            continue;
        }
        c = m.kind.apply_cpu(img_w, img_h, uv, c);
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
