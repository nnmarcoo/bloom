#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortAxis {
    Horizontal { reverse: bool },
    Vertical { reverse: bool },
}

impl SortAxis {
    pub fn from_angle(angle: f32) -> Self {
        let q = (((angle / 90.0).round() as i32) % 4 + 4) % 4;
        match q {
            0 => SortAxis::Horizontal { reverse: false },
            1 => SortAxis::Vertical { reverse: false },
            2 => SortAxis::Horizontal { reverse: true },
            _ => SortAxis::Vertical { reverse: true },
        }
    }
}

pub fn key_of(p: &[u8]) -> u8 {
    let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    (y + 0.5).clamp(0.0, 255.0) as u8
}

pub fn key_cutoff(threshold: f32) -> u8 {
    (threshold * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn sort_line(pixels: &mut [u8], idx: &[usize], cutoff: u8, reverse: bool) {
    let n = idx.len();
    let mut i = 0;
    while i < n {
        if key_of(&pixels[idx[i] * 4..idx[i] * 4 + 4]) <= cutoff {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && key_of(&pixels[idx[i] * 4..idx[i] * 4 + 4]) > cutoff {
            i += 1;
        }
        let run = &idx[start..i];
        let mut count = [0u32; 256];
        let keys: Vec<u8> = run
            .iter()
            .map(|&p| key_of(&pixels[p * 4..p * 4 + 4]))
            .collect();
        for &k in &keys {
            count[k as usize] += 1;
        }
        let mut offset = [0u32; 256];
        let mut acc = 0u32;
        for b in 0..256 {
            offset[b] = acc;
            acc += count[b];
        }
        let snapshot: Vec<[u8; 4]> = run
            .iter()
            .map(|&p| {
                let s = &pixels[p * 4..p * 4 + 4];
                [s[0], s[1], s[2], s[3]]
            })
            .collect();
        let run_len = run.len() as u32;
        for (j, &k) in keys.iter().enumerate() {
            let dst_rank = offset[k as usize];
            offset[k as usize] += 1;
            let rank = if reverse {
                run_len - 1 - dst_rank
            } else {
                dst_rank
            };
            let slot = run[rank as usize];
            pixels[slot * 4..slot * 4 + 4].copy_from_slice(&snapshot[j]);
        }
    }
}

pub fn pixel_sort_cpu(
    src: &[u8],
    width: usize,
    height: usize,
    threshold: f32,
    angle: f32,
) -> Vec<u8> {
    let mut out = src.to_vec();
    let cutoff = key_cutoff(threshold);
    match SortAxis::from_angle(angle) {
        SortAxis::Horizontal { reverse } => {
            for y in 0..height {
                let idx: Vec<usize> = (0..width).map(|x| y * width + x).collect();
                sort_line(&mut out, &idx, cutoff, reverse);
            }
        }
        SortAxis::Vertical { reverse } => {
            for x in 0..width {
                let idx: Vec<usize> = (0..height).map(|y| y * width + x).collect();
                sort_line(&mut out, &idx, cutoff, reverse);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_sorts_above_threshold_run_ascending() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [200u8, 50, 150, 100] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 4, 1, 0.0, 0.0);
        let vals: Vec<u8> = (0..4).map(|x| out[x * 4]).collect();
        assert_eq!(vals, vec![50, 100, 150, 200]);
    }

    #[test]
    fn threshold_pins_dark_pixels_and_segments() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [200u8, 10, 200, 100] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 4, 1, 0.1, 0.0);
        let vals: Vec<u8> = (0..4).map(|x| out[x * 4]).collect();
        assert_eq!(vals, vec![200, 10, 100, 200]);
    }

    #[test]
    fn reverse_sorts_descending() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [50u8, 200, 100, 150] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 4, 1, 0.0, 180.0);
        let vals: Vec<u8> = (0..4).map(|x| out[x * 4]).collect();
        assert_eq!(vals, vec![200, 150, 100, 50]);
    }

    #[test]
    fn vertical_axis_sorts_columns() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [150u8, 50, 100] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 1, 3, 0.0, 90.0);
        let vals: Vec<u8> = (0..3).map(|y| out[y * 4]).collect();
        assert_eq!(vals, vec![50, 100, 150]);
    }
}
