use crate::modifiers::{Modifier, ModifierKind};

pub(crate) fn apply_single(
    kind: &ModifierKind,
    img_w: u32,
    img_h: u32,
    uv: [f32; 2],
    mut c: [f32; 4],
) -> [f32; 4] {
    match kind {
        ModifierKind::BrightnessContrast {
            brightness,
            contrast,
        } => {
            for v in c.iter_mut().take(3) {
                *v = (*v + brightness - 0.5) * (1.0 + contrast) + 0.5;
            }
        }
        ModifierKind::Exposure { exposure } => {
            let scale = 2.0f32.powf(*exposure);
            c[0] *= scale;
            c[1] *= scale;
            c[2] *= scale;
        }
        ModifierKind::Levels {
            shadows,
            midtones,
            highlights,
        } => {
            let hi = highlights.max(shadows + 0.001);
            let range = hi - shadows;
            for v in c.iter_mut().take(3) {
                *v = ((*v - shadows) / range).clamp(0.0, 1.0);
            }
            let gamma = midtones.max(0.001);
            for v in c.iter_mut().take(3) {
                *v = v.powf(1.0 / gamma);
            }
        }
        ModifierKind::HueSaturation {
            hue,
            saturation,
            lightness,
        } => {
            let [h, s, l] = rgb_to_hsl([c[0], c[1], c[2]]);
            let rgb = hsl_to_rgb([
                (h + hue / 360.0).fract(),
                (s + saturation).clamp(0.0, 1.0),
                (l + lightness).clamp(0.0, 1.0),
            ]);
            c[0] = rgb[0];
            c[1] = rgb[1];
            c[2] = rgb[2];
        }
        ModifierKind::Vignette {
            strength,
            size,
            softness,
        } => {
            let dx = uv[0] - 0.5;
            let dy = uv[1] - 0.5;
            let dist = (dx * dx + dy * dy).sqrt() * 2.0;
            let inner = (size - softness).max(0.0);
            let t = ((dist - inner) / (size + 0.0001 - inner)).clamp(0.0, 1.0);
            let vignette = 1.0 - t * t * (3.0 - 2.0 * t);
            let factor = (1.0 - strength).max(0.0) * (1.0 - vignette) + vignette;
            c[0] *= factor;
            c[1] *= factor;
            c[2] *= factor;
        }
        ModifierKind::Threshold { cutoff } => {
            let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
            let v = if luma >= *cutoff { 1.0 } else { 0.0 };
            c[0] = v;
            c[1] = v;
            c[2] = v;
        }
        ModifierKind::Posterize { levels } => {
            let l = (*levels as f32 - 1.0).max(1.0);
            for v in c.iter_mut().take(3) {
                *v = (*v * l + 0.5).floor() / l;
            }
        }
        ModifierKind::Vibrance {
            vibrance,
            saturation,
        } => {
            let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
            let max_c = c[0].max(c[1]).max(c[2]);
            let sat_proxy = max_c - c[0].min(c[1]).min(c[2]);
            let vib_amount = vibrance * (1.0 - sat_proxy);
            for v in c.iter_mut().take(3) {
                *v = luma + (*v - luma) * (1.0 + vib_amount);
            }
            for v in c.iter_mut().take(3) {
                *v = luma + (*v - luma) * (1.0 + saturation);
            }
        }
        ModifierKind::ColorBalance {
            cyan_red,
            magenta_green,
            yellow_blue,
        } => {
            c[0] += cyan_red;
            c[1] += magenta_green;
            c[2] += yellow_blue;
        }
        ModifierKind::Grain {
            amount,
            size,
            roughness,
            seed,
        } => {
            let gx = uv[0] * img_w as f32 / size.max(0.5);
            let gy = uv[1] * img_h as f32 / size.max(0.5);
            let iseed = *seed as i32;
            let (cx, cy) = (gx.floor(), gy.floor());
            let (fx, fy) = (gx.fract(), gy.fract());
            let n00 = hash21_i(cx as i32, cy as i32, iseed);
            let n10 = hash21_i(cx as i32 + 1, cy as i32, iseed);
            let n01 = hash21_i(cx as i32, cy as i32 + 1, iseed);
            let n11 = hash21_i(cx as i32 + 1, cy as i32 + 1, iseed);
            let t = roughness.clamp(0.0, 1.0);
            let wx = fx * fx * (3.0 - 2.0 * fx) * (1.0 - t) + if fx >= 0.5 { 1.0 } else { 0.0 } * t;
            let wy = fy * fy * (3.0 - 2.0 * fy) * (1.0 - t) + if fy >= 0.5 { 1.0 } else { 0.0 } * t;
            let noise =
                (n00 * (1.0 - wx) + n10 * wx) * (1.0 - wy) + (n01 * (1.0 - wx) + n11 * wx) * wy;
            let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
            let luma_weight = 4.0 * luma * (1.0 - luma);
            let grain = (noise - 0.5) * amount * luma_weight;
            for v in c.iter_mut().take(3) {
                *v = (*v + grain).clamp(0.0, 1.0);
            }
        }
        ModifierKind::Halftone { size, angle } => {
            let angle_rad = *angle * std::f32::consts::PI / 180.0;
            let cs = angle_rad.cos();
            let sn = angle_rad.sin();
            let period = (*size / img_w.min(img_h) as f32).max(0.001);
            let rot_x = (uv[0] * cs - uv[1] * sn) / period;
            let rot_y = (uv[0] * sn + uv[1] * cs) / period;
            let cell_x = rot_x.floor() + 0.5;
            let cell_y = rot_y.floor() + 0.5;
            let dist = ((rot_x - cell_x).powi(2) + (rot_y - cell_y).powi(2)).sqrt();
            let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
            let radius = luma.sqrt() * 0.5;
            let aa = 1.0 / size.max(1.0);
            let t = ((dist - (radius - aa)) / (2.0 * aa)).clamp(0.0, 1.0);
            let v = 1.0 - t * t * (3.0 - 2.0 * t);
            c[0] = v;
            c[1] = v;
            c[2] = v;
        }
        _ => {}
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

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
        if let ModifierKind::ChromaticAberration { amount } = &m.kind {
            let scale = amount / img_w as f32;
            let r_uv = [
                (uv[0] + (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                (uv[1] + (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let b_uv = [
                (uv[0] - (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                (uv[1] - (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let mut cr = sample_pixel(pixels, img_w, img_h, r_uv[0], r_uv[1]);
            let mut cb = sample_pixel(pixels, img_w, img_h, b_uv[0], b_uv[1]);
            for prev in &modifiers[..i] {
                if !prev.has_visible_effect()
                    || matches!(prev.kind, ModifierKind::ChromaticAberration { .. })
                {
                    continue;
                }
                cr = apply_single(&prev.kind, img_w, img_h, r_uv, cr);
                cb = apply_single(&prev.kind, img_w, img_h, b_uv, cb);
            }
            c[0] = cr[0];
            c[2] = cb[2];
        } else {
            c = apply_single(&m.kind, img_w, img_h, uv, c);
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

pub(crate) fn hash21_i(ix: i32, iy: i32, seed: i32) -> f32 {
    let h = hash_u(
        (ix as u32) ^ (iy as u32).wrapping_mul(1664525) ^ (seed as u32).wrapping_mul(22695477),
    );
    h as f32 / 4294967295.0
}

fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
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

fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
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
