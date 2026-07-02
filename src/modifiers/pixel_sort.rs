use rayon::prelude::*;

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

const RATIONAL_SLOPES: [(i32, i32); 4] = [(1, 1), (1, 2), (2, 1), (1, 3)];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortMode {
    Cardinal(SortAxis),
    Diagonal { dx: i32, dy: i32 },
}

impl SortMode {
    pub fn from_angle(angle: f32) -> Self {
        let a = angle.rem_euclid(360.0);
        if (a % 90.0).min(90.0 - (a % 90.0)) <= 1.0 {
            return SortMode::Cardinal(SortAxis::from_angle(angle));
        }
        let target = a.to_radians();
        let (mut best, mut best_err) = ((1i32, 1i32), f32::INFINITY);
        for &(p, q) in &RATIONAL_SLOPES {
            for &(sx, sy) in &[(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                for &(dx, dy) in &[(p * sx, q * sy), (q * sx, p * sy)] {
                    let err = angle_diff(target, (dy as f32).atan2(dx as f32));
                    if err < best_err {
                        best_err = err;
                        best = (dx, dy);
                    }
                }
            }
        }
        SortMode::Diagonal {
            dx: best.0,
            dy: best.1,
        }
    }
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

pub fn diagonal_lines(width: usize, height: usize, dx: i32, dy: i32) -> Vec<Vec<usize>> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let g = gcd(dx.unsigned_abs(), dy.unsigned_abs()).max(1) as i32;
    let (dx, dy) = (dx / g, dy / g);

    let (w, h) = (width as i32, height as i32);
    let corners = [0, dy * (w - 1), -dx * (h - 1), dy * (w - 1) - dx * (h - 1)];
    let cross_min = *corners.iter().min().unwrap();
    let cross_max = *corners.iter().max().unwrap();
    let span = (cross_max - cross_min + 1) as usize;

    let mut bucket_of = vec![usize::MAX; span];
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let slot = (dy * x as i32 - dx * y as i32 - cross_min) as usize;
            let bucket = match bucket_of[slot] {
                usize::MAX => {
                    let b = lines.len();
                    bucket_of[slot] = b;
                    lines.push(Vec::new());
                    b
                }
                b => b,
            };
            lines[bucket].push(y * width + x);
        }
    }

    for line in &mut lines {
        line.sort_by_key(|&idx| {
            let (x, y) = ((idx % width) as i32, (idx / width) as i32);
            dx * x + dy * y
        });
    }
    lines
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub fn key_of(p: &[u8]) -> u8 {
    let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    (y + 0.5).clamp(0.0, 255.0) as u8
}

pub fn key_cutoff(threshold: f32) -> u8 {
    (threshold * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

type Rgba = [u8; 4];

fn counting_sort_run(run: &mut [Rgba], reverse: bool, sorted: &mut Vec<Rgba>) {
    let mut count = [0u32; 256];
    for px in run.iter() {
        count[key_of(px) as usize] += 1;
    }
    let mut offset = [0u32; 256];
    let mut acc = 0u32;
    for b in 0..256 {
        offset[b] = acc;
        acc += count[b];
    }

    let n = run.len() as u32;
    sorted.clear();
    sorted.resize(run.len(), Rgba::default());
    for px in run.iter() {
        let k = key_of(px) as usize;
        let dst = offset[k];
        offset[k] += 1;
        let rank = if reverse { n - 1 - dst } else { dst };
        sorted[rank as usize] = *px;
    }
    run.copy_from_slice(sorted);
}

fn sort_row(row: &mut [Rgba], cutoff: u8, reverse: bool, sorted: &mut Vec<Rgba>) {
    let n = row.len();
    let mut i = 0;
    while i < n {
        if key_of(&row[i]) <= cutoff {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && key_of(&row[i]) > cutoff {
            i += 1;
        }
        counting_sort_run(&mut row[start..i], reverse, sorted);
    }
}

fn transpose(src: &[Rgba], width: usize, height: usize) -> Vec<Rgba> {
    let mut out = vec![Rgba::default(); src.len()];
    out.par_chunks_mut(height)
        .enumerate()
        .for_each(|(x, col)| {
            for (y, dst) in col.iter_mut().enumerate() {
                *dst = src[y * width + x];
            }
        });
    out
}

pub fn pixel_sort_cpu(
    src: &[u8],
    width: usize,
    height: usize,
    threshold: f32,
    angle: f32,
) -> Vec<u8> {
    let mut out = src.to_vec();
    if width == 0 || height == 0 {
        return out;
    }
    let cutoff = key_cutoff(threshold);
    match SortMode::from_angle(angle) {
        SortMode::Cardinal(SortAxis::Horizontal { reverse }) => {
            let px: &mut [Rgba] = bytemuck::cast_slice_mut(&mut out);
            px.par_chunks_mut(width).for_each_init(Vec::new, |scratch, row| {
                sort_row(row, cutoff, reverse, scratch);
            });
        }
        SortMode::Cardinal(SortAxis::Vertical { reverse }) => {
            let px: &[Rgba] = bytemuck::cast_slice(&out);
            let mut t = transpose(px, width, height);
            t.par_chunks_mut(height).for_each_init(Vec::new, |scratch, col| {
                sort_row(col, cutoff, reverse, scratch);
            });
            let back = transpose(&t, height, width);
            out.copy_from_slice(bytemuck::cast_slice(&back));
        }
        SortMode::Diagonal { dx, dy } => {
            let lines = diagonal_lines(width, height, dx, dy);
            let px: &mut [Rgba] = bytemuck::cast_slice_mut(&mut out);
            let mut gather: Vec<Rgba> = Vec::new();
            let mut scratch: Vec<Rgba> = Vec::new();
            for idx in &lines {
                gather.clear();
                gather.extend(idx.iter().map(|&i| px[i]));
                sort_row(&mut gather, cutoff, false, &mut scratch);
                for (&i, &p) in idx.iter().zip(gather.iter()) {
                    px[i] = p;
                }
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
    fn horizontal_sorts_each_row_independently() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [200u8, 50, 150, 100, 10, 240, 30, 120] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 4, 2, 0.0, 0.0);
        let row0: Vec<u8> = (0..4).map(|x| out[x * 4]).collect();
        let row1: Vec<u8> = (4..8).map(|x| out[x * 4]).collect();
        assert_eq!(row0, vec![50, 100, 150, 200]);
        assert_eq!(row1, vec![10, 30, 120, 240]);
    }

    #[test]
    fn empty_dimensions_return_input_unchanged() {
        let src = vec![1u8, 2, 3, 4];
        assert_eq!(pixel_sort_cpu(&src, 0, 0, 0.0, 0.0), src);
        assert_eq!(pixel_sort_cpu(&[], 0, 5, 0.0, 90.0), Vec::<u8>::new());
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

    fn assert_partition(width: usize, height: usize, dx: i32, dy: i32) {
        let lines = diagonal_lines(width, height, dx, dy);
        let mut seen = vec![0u32; width * height];
        for line in &lines {
            for &idx in line {
                seen[idx] += 1;
            }
        }
        assert!(
            seen.iter().all(|&c| c == 1),
            "diagonal_lines({width}x{height}, {dx},{dy}) is not a partition: {:?}",
            seen.iter()
                .enumerate()
                .filter(|&(_, &c)| c != 1)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn diagonal_lines_partition_all_pixels() {
        for &(w, h) in &[(1, 1), (4, 4), (5, 3), (3, 5), (7, 11), (16, 9)] {
            for &(dx, dy) in &[
                (1, 1),
                (1, -1),
                (-1, 1),
                (2, 1),
                (1, 2),
                (3, 1),
                (1, 3),
                (2, 4),
            ] {
                assert_partition(w, h, dx, dy);
            }
        }
    }

    #[test]
    fn diagonal_lines_within_line_follow_direction() {
        let lines = diagonal_lines(3, 3, 1, 1);
        let main: Vec<usize> = lines
            .iter()
            .find(|l| l.contains(&0) && l.len() == 3)
            .expect("main diagonal present")
            .clone();
        assert_eq!(main, vec![0, 4, 8]);
    }

    #[test]
    fn diagonal_lines_reduces_slope_by_gcd() {
        let a = diagonal_lines(6, 6, 2, 2);
        let b = diagonal_lines(6, 6, 1, 1);
        assert_eq!(a, b, "(2,2) must reduce to (1,1)");
    }

    #[test]
    fn from_angle_snaps_cardinal_and_picks_diagonal() {
        assert!(matches!(SortMode::from_angle(0.0), SortMode::Cardinal(_)));
        assert!(matches!(SortMode::from_angle(90.5), SortMode::Cardinal(_)));
        assert!(matches!(
            SortMode::from_angle(45.0),
            SortMode::Diagonal { .. }
        ));
        if let SortMode::Diagonal { dx, dy } = SortMode::from_angle(45.0) {
            assert_eq!((dx.abs(), dy.abs()), (1, 1));
        }
    }

    #[test]
    fn diagonal_sort_orders_main_diagonal() {
        let g = |v: u8| [v, v, v, 255u8];
        let mut src = Vec::new();
        for v in [
            90u8, 10, 10, //
            10, 60, 10, //
            10, 10, 30,
        ] {
            src.extend_from_slice(&g(v));
        }
        let out = pixel_sort_cpu(&src, 3, 3, 0.0, 45.0);
        let diag: Vec<u8> = [0usize, 4, 8].iter().map(|&i| out[i * 4]).collect();
        assert_eq!(diag, vec![30, 60, 90]);
        for &off in &[1usize, 2, 3, 5, 6, 7] {
            assert_eq!(
                out[off * 4],
                10,
                "off-diagonal pixel {off} must be unchanged"
            );
        }
    }

    fn reference_sort(src: &[u8], w: usize, h: usize, cutoff: u8, vertical: bool, reverse: bool) -> Vec<u8> {
        let mut out = src.to_vec();
        let line = |k: usize| -> Vec<usize> {
            if vertical {
                (0..h).map(|y| y * w + k).collect()
            } else {
                (0..w).map(|x| k * w + x).collect()
            }
        };
        let count = if vertical { w } else { h };
        for k in 0..count {
            let idx = line(k);
            let n = idx.len();
            let mut i = 0;
            while i < n {
                let key = |p: usize| key_of(&out[idx[p] * 4..idx[p] * 4 + 4]);
                if key(i) <= cutoff {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < n && key(i) > cutoff {
                    i += 1;
                }
                let run: Vec<[u8; 4]> = (start..i)
                    .map(|p| {
                        let s = &out[idx[p] * 4..idx[p] * 4 + 4];
                        [s[0], s[1], s[2], s[3]]
                    })
                    .collect();
                let mut keyed: Vec<(u8, [u8; 4])> =
                    run.iter().map(|&px| (key_of(&px), px)).collect();
                keyed.sort_by_key(|&(k, _)| k);
                for (off, (_, px)) in keyed.into_iter().enumerate() {
                    let rank = if reverse { (i - start) - 1 - off } else { off };
                    let slot = idx[start + rank];
                    out[slot * 4..slot * 4 + 4].copy_from_slice(&px);
                }
            }
        }
        out
    }

    #[test]
    fn matches_reference_on_random_images() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 33) as u8
        };
        for &(w, h) in &[(1usize, 1usize), (7, 5), (5, 7), (16, 16), (31, 17)] {
            let src: Vec<u8> = (0..w * h * 4).map(|_| rng()).collect();
            for &thr in &[0.0f32, 0.3, 0.6] {
                let cutoff = key_cutoff(thr);
                for &(angle, vertical, reverse) in
                    &[(0.0f32, false, false), (180.0, false, true), (90.0, true, false), (270.0, true, true)]
                {
                    let got = pixel_sort_cpu(&src, w, h, thr, angle);
                    let want = reference_sort(&src, w, h, cutoff, vertical, reverse);
                    assert_eq!(
                        got, want,
                        "mismatch at {w}x{h} thr={thr} angle={angle}"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "timing benchmark; run with --release --ignored"]
    fn bench_large_image() {
        use std::time::Instant;
        let (w, h) = (4000usize, 3000usize);
        let mut state = 1u64;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 33) as u8
        };
        let src: Vec<u8> = (0..w * h * 4).map(|_| rng()).collect();

        for &(label, angle) in &[("horizontal", 0.0f32), ("vertical", 90.0), ("diagonal", 45.0)] {
            let t = Instant::now();
            let out = pixel_sort_cpu(&src, w, h, 0.3, angle);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(&out);
            println!("{label:>11}: {ms:.1} ms  ({w}x{h})");
        }
    }
}
