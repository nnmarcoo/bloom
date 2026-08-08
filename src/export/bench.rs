//! Coarse timing baselines for the CPU render path.
//!
//! The render-architecture refactor claims to be performance-neutral through its
//! structural phases. This module makes that claim measurable rather than
//! asserted: it times representative chains so each phase can report a real
//! delta.
//!
//! These are deliberately *not* assertions. Wall-clock timing on a developer
//! machine is far too noisy to gate CI on, and a timing test that fails when a
//! laptop thermally throttles trains people to ignore failures. The bench is
//! `#[ignore]`d and prints a table for a human to compare across commits:
//!
//! ```text
//! cargo test --release bench_cpu_chains -- --ignored --nocapture
//! ```
//!
//! Always use `--release`. The debug build's timings are dominated by unoptimized
//! per-pixel math and say nothing useful about shipped performance.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::export::{ExportData, ExportFrame, ExportSource, render_still_rgba};
    use crate::modifiers::kinds::{
        ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort, Posterize,
    };
    use crate::modifiers::{Modifier, ModifierKind};

    const W: u32 = 1920;
    const H: u32 = 1080;

    const WARMUP: u32 = 1;
    const RUNS: u32 = 5;

    fn source_pixels(w: u32, h: u32) -> Arc<Vec<u8>> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut s = 0x9E3779B9u32;
        for i in 0..(w * h) as usize {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let o = i * 4;
            px[o] = (s >> 24) as u8;
            px[o + 1] = (s >> 16) as u8;
            px[o + 2] = (s >> 8) as u8;
            px[o + 3] = 255;
        }
        Arc::new(px)
    }

    fn time_chain(pixels: &Arc<Vec<u8>>, modifiers: &[Modifier]) -> Duration {
        let build = || ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: Arc::clone(pixels),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: W,
            height: H,
            modifiers: modifiers.to_vec(),
            crop: None,
            rotation: 0,
            trim: None,
        };

        for _ in 0..WARMUP {
            let _ = render_still_rgba(&build()).expect("bench render");
        }

        let mut samples: Vec<Duration> = (0..RUNS)
            .map(|_| {
                let data = build();
                let t = Instant::now();
                let out = render_still_rgba(&data).expect("bench render");
                let elapsed = t.elapsed();
                std::hint::black_box(&out);
                elapsed
            })
            .collect();
        samples.sort();
        samples[samples.len() / 2]
    }

    fn m(kind: ModifierKind) -> Modifier {
        Modifier::new(kind)
    }

    fn pointwise_20() -> Vec<Modifier> {
        (0..20)
            .map(|i| {
                m(ModifierKind::Exposure(Exposure {
                    exposure: 0.01 * (i % 5) as f32,
                }))
            })
            .collect()
    }

    #[test]
    #[ignore = "timing baseline; run explicitly with --release --ignored --nocapture"]
    fn bench_cpu_chains() {
        let pixels = source_pixels(W, H);

        let cases: Vec<(&str, Vec<Modifier>)> = vec![
            ("identity", vec![]),
            (
                "pointwise x1",
                vec![m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))],
            ),
            ("pointwise x20", pointwise_20()),
            (
                "blur r=8",
                vec![m(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 }))],
            ),
            (
                "blur x2 (r=8,8)",
                vec![
                    m(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 })),
                    m(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 })),
                ],
            ),
            (
                "blur x4 (r=8)",
                (0..4)
                    .map(|_| m(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 })))
                    .collect(),
            ),
            (
                "motion blur d=16",
                vec![m(ModifierKind::MotionBlur(MotionBlur {
                    angle: 30.0,
                    distance: 16.0,
                }))],
            ),
            (
                "chromatic aberration",
                vec![m(ModifierKind::ChromaticAberration(ChromaticAberration {
                    amount: 10.0,
                }))],
            ),
            (
                "pixel sort (cardinal)",
                vec![m(ModifierKind::PixelSort(PixelSort {
                    threshold: 0.5,
                    angle: 0.0,
                }))],
            ),
            (
                "pixel sort (diagonal)",
                vec![m(ModifierKind::PixelSort(PixelSort {
                    threshold: 0.5,
                    angle: 30.0,
                }))],
            ),
            (
                "mixed (pw+blur+sort+ca)",
                vec![
                    m(ModifierKind::Exposure(Exposure { exposure: 0.3 })),
                    m(ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 })),
                    m(ModifierKind::PixelSort(PixelSort {
                        threshold: 0.5,
                        angle: 0.0,
                    })),
                    m(ModifierKind::ChromaticAberration(ChromaticAberration {
                        amount: 6.0,
                    })),
                    m(ModifierKind::Posterize(Posterize { levels: 5 })),
                ],
            ),
        ];

        println!("\nCPU render baseline — {W}x{H}, median of {RUNS} runs");
        println!("{:-<52}", "");
        println!("{:<30} {:>10}", "chain", "median ms");
        println!("{:-<52}", "");
        for (label, modifiers) in &cases {
            let d = time_chain(&pixels, modifiers);
            println!("{:<30} {:>10.2}", label, d.as_secs_f64() * 1000.0);
        }
        println!("{:-<52}\n", "");
    }
}
