//! Byte-exact change detectors for the CPU render oracle.
//!
//! The GPU goldens assert that the GPU path agrees with [`cpu::render_full`],
//! which makes the CPU path the oracle for the whole renderer. That comparison
//! cannot notice the CPU path itself moving: a change that shifts both paths the
//! same way keeps the goldens green while the rendered image silently changes.
//!
//! These tests pin the oracle to fixed hashes, one per [`StepClass`] in the ROI
//! taxonomy plus mixed chains. They prove nothing about correctness -- a wrong
//! render hashes just as stably as a right one. Their job is to make any change
//! in rendered output *visible* during the render-architecture refactor, so a
//! phase that claims to be pure plumbing can be shown to be exactly that.
//!
//! A failure here is not automatically a bug. It means the output moved, and the
//! change must be justified: if the move is intended, re-run with
//! `BLOOM_BLESS_ORACLE=1` to print the new values, then update them in the same
//! commit that explains why.
//!
//! Text is the one modifier with no fixed hash. It shapes glyphs through
//! whatever fonts the host has installed, so its output legitimately differs
//! between machines and a pinned hash would fail on any host but the one that
//! blessed it. Its tests assert the properties that do hold everywhere: the
//! render is deterministic, it changes the image, and the changed pixels carry
//! the requested color.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::export::{ExportData, ExportFrame, ExportSource, render_still_rgba};
    use crate::modifiers::kinds::{
        ChromaticAberration, Drawing, Exposure, GaussianBlur, Grain, Halftone, HueSaturation,
        Levels, MotionBlur, PixelSort, Posterize, Stroke, Text, Vignette,
    };
    use crate::modifiers::{Modifier, ModifierKind};

    const W: u32 = 96;
    const H: u32 = 72;

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn source_pixels(w: u32, h: u32) -> Arc<Vec<u8>> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut s = 0x9E3779B9u32;
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let speckle = (s >> 27) as u8;
                let in_block = x >= w / 3 && x < 2 * w / 3 && y >= h / 4 && y < 3 * h / 4;
                px[o] = ((x * 255 / w.max(1)) as u8).saturating_add(speckle);
                px[o + 1] = if in_block {
                    230
                } else {
                    (y * 255 / h.max(1)) as u8
                };
                px[o + 2] = ((x + y) * 255 / (w + h).max(1)) as u8;
                px[o + 3] = 255;
            }
        }
        Arc::new(px)
    }

    fn render(modifiers: Vec<Modifier>) -> (u32, u32, Vec<u8>) {
        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: source_pixels(W, H),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: W,
            height: H,
            modifiers,
            crop: None,
            rotation: 0,
            trim: None,
        };
        render_still_rgba(&data).expect("oracle render should succeed")
    }

    fn assert_oracle(label: &str, modifiers: Vec<Modifier>, expected: u64) {
        let (w, h, rgba) = render(modifiers);
        assert_eq!((w, h), (W, H), "{label}: unexpected output dimensions");
        assert_eq!(
            rgba.len(),
            (W * H * 4) as usize,
            "{label}: unexpected buffer length"
        );

        let actual = fnv1a(&rgba);
        if actual == expected {
            return;
        }
        if std::env::var("BLOOM_BLESS_ORACLE").is_ok_and(|v| v == "1") {
            panic!("{label}: BLESS -> 0x{actual:016x} (was 0x{expected:016x})");
        }
        panic!(
            "{label}: CPU oracle output changed.\n  \
             expected 0x{expected:016x}\n  \
             actual   0x{actual:016x}\n\
             The rendered image moved. If that was intended, re-run with \
             BLOOM_BLESS_ORACLE=1 to get the new value and update it in the same \
             commit, explaining the change. If it was not intended, this is a \
             regression."
        );
    }

    fn m(kind: ModifierKind) -> Modifier {
        Modifier::new(kind)
    }

    #[test]
    fn oracle_identity_no_modifiers() {
        assert_oracle("identity", vec![], 0xecc2b74a2202806b);
    }

    #[test]
    fn oracle_pointwise_chain() {
        assert_oracle(
            "pointwise",
            vec![
                m(ModifierKind::Exposure(Exposure { exposure: 0.4 })),
                m(ModifierKind::Levels(Levels {
                    shadows: 0.1,
                    midtones: 1.2,
                    highlights: 0.9,
                })),
                m(ModifierKind::HueSaturation(HueSaturation {
                    hue: 20.0,
                    saturation: 0.3,
                    lightness: -0.1,
                })),
                m(ModifierKind::Posterize(Posterize { levels: 6 })),
            ],
            0xa9e0c267c69ab2e1,
        );
    }

    #[test]
    fn oracle_pointwise_beyond_fusion_cap() {
        let mut chain = Vec::new();
        for i in 0..40 {
            chain.push(m(ModifierKind::Exposure(Exposure {
                exposure: 0.01 * (i % 7) as f32,
            })));
        }
        assert_oracle("pointwise-40", chain, 0x3a3bab73381050fb);
    }

    #[test]
    fn oracle_gaussian_blur() {
        assert_oracle(
            "blur",
            vec![m(ModifierKind::GaussianBlur(GaussianBlur { radius: 5.0 }))],
            0xff15ea1878db6990,
        );
    }

    #[test]
    fn oracle_double_blur() {
        assert_oracle(
            "blur-blur",
            vec![
                m(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
                m(ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 })),
            ],
            0x3d88ae9483bb1609,
        );
    }

    #[test]
    fn oracle_motion_blur() {
        assert_oracle(
            "motion-blur",
            vec![m(ModifierKind::MotionBlur(MotionBlur {
                angle: 35.0,
                distance: 14.0,
            }))],
            0xb63a3be5d95c2772,
        );
    }

    #[test]
    fn oracle_chromatic_aberration() {
        assert_oracle(
            "chromatic-aberration",
            vec![m(ModifierKind::ChromaticAberration(ChromaticAberration {
                amount: 9.0,
            }))],
            0x00761bcfc412ee2a,
        );
    }

    #[test]
    fn oracle_pixel_sort_cardinal() {
        assert_oracle(
            "pixel-sort-cardinal",
            vec![m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.45,
                angle: 0.0,
            }))],
            0xbe3d22af14f22a53,
        );
    }

    #[test]
    fn oracle_pixel_sort_vertical() {
        assert_oracle(
            "pixel-sort-vertical",
            vec![m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.45,
                angle: 90.0,
            }))],
            0xd1772554e8cfe8ab,
        );
    }

    #[test]
    fn oracle_pixel_sort_diagonal() {
        assert_oracle(
            "pixel-sort-diagonal",
            vec![m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.45,
                angle: 30.0,
            }))],
            0xdf7d6e7871c69feb,
        );
    }

    fn text_modifier() -> Modifier {
        m(ModifierKind::Text(Text {
            content: "Bloom".to_string(),
            size: 42.0,
            x: 0.5,
            y: 0.5,
            r: 1.0,
            g: 0.9,
            b: 0.2,
            opacity: 1.0,
            ..Text::default()
        }))
    }

    #[test]
    fn text_renders_and_is_deterministic() {
        let a = render(vec![text_modifier()]);
        let b = render(vec![text_modifier()]);
        assert_eq!(
            fnv1a(&a.2),
            fnv1a(&b.2),
            "text: two renders of the same chain disagree"
        );

        let plain = render(vec![]);
        assert_ne!(
            fnv1a(&a.2),
            fnv1a(&plain.2),
            "text: the chain produced the untouched source, so nothing was drawn"
        );
    }

    #[test]
    fn text_is_drawn_in_its_own_color() {
        let (_, _, rgba) = render(vec![text_modifier()]);
        let (_, _, plain) = render(vec![]);

        let mut changed = 0usize;
        let mut yellowish = 0usize;
        for (a, b) in rgba.chunks_exact(4).zip(plain.chunks_exact(4)) {
            if a == b {
                continue;
            }
            changed += 1;
            if a[0] > a[2] && a[1] > a[2] {
                yellowish += 1;
            }
        }

        assert!(
            changed > 200,
            "text: only {changed} pixels changed, which is too few to be glyphs"
        );
        assert!(
            yellowish * 10 >= changed * 8,
            "text: {yellowish} of {changed} changed pixels carry the requested \
             color, expected most of them"
        );
    }

    #[test]
    fn oracle_drawing() {
        assert_oracle(
            "drawing",
            vec![m(ModifierKind::Drawing(Drawing {
                strokes: vec![Stroke {
                    points: vec![[0.15, 0.3], [0.5, 0.7], [0.85, 0.35]],
                    size: 10.0,
                    hardness: 0.7,
                    opacity: 0.9,
                    color: [0.1, 0.6, 1.0],
                }],
                ..Drawing::default()
            }))],
            0xcb24d1f0942e90cf,
        );
    }

    #[test]
    fn oracle_halftone() {
        assert_oracle(
            "halftone",
            vec![m(ModifierKind::Halftone(Halftone {
                size: 6.0,
                angle: 25.0,
            }))],
            0x5c421586912dad54,
        );
    }

    #[test]
    fn oracle_vignette() {
        assert_oracle(
            "vignette",
            vec![m(ModifierKind::Vignette(Vignette {
                strength: 0.7,
                size: 0.6,
                softness: 0.4,
            }))],
            0x30d7808e5896f84a,
        );
    }

    #[test]
    fn oracle_grain() {
        assert_oracle(
            "grain",
            vec![m(ModifierKind::Grain(Grain {
                amount: 0.35,
                size: 2.0,
                seed: 7.0,
                color: 0.2,
                response: 0.5,
            }))],
            0x254e429236e848d5,
        );
    }

    #[test]
    fn oracle_pointwise_kernel_pointwise() {
        assert_oracle(
            "pointwise-kernel-pointwise",
            vec![
                m(ModifierKind::Exposure(Exposure { exposure: 0.3 })),
                m(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
                m(ModifierKind::Posterize(Posterize { levels: 5 })),
            ],
            0x6ddf42fc5560c162,
        );
    }

    #[test]
    fn oracle_all_classes_mixed() {
        assert_oracle(
            "all-classes",
            vec![
                m(ModifierKind::Exposure(Exposure { exposure: 0.25 })),
                m(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
                m(ModifierKind::PixelSort(PixelSort {
                    threshold: 0.5,
                    angle: 0.0,
                })),
                m(ModifierKind::ChromaticAberration(ChromaticAberration {
                    amount: 6.0,
                })),
                m(ModifierKind::MotionBlur(MotionBlur {
                    angle: 15.0,
                    distance: 8.0,
                })),
                m(ModifierKind::Posterize(Posterize { levels: 4 })),
            ],
            0xdc0279ea156407ce,
        );
    }

    #[test]
    fn oracle_disabled_modifier_is_inert() {
        let enabled = render(vec![m(ModifierKind::Exposure(Exposure { exposure: 0.4 }))]);
        let mut disabled_blur = m(ModifierKind::GaussianBlur(GaussianBlur { radius: 9.0 }));
        disabled_blur.enabled = false;
        let with_disabled = render(vec![
            m(ModifierKind::Exposure(Exposure { exposure: 0.4 })),
            disabled_blur,
        ]);
        assert_eq!(
            fnv1a(&enabled.2),
            fnv1a(&with_disabled.2),
            "a disabled modifier changed the render"
        );
    }

    #[test]
    fn oracle_render_is_deterministic() {
        let chain = || {
            vec![
                m(ModifierKind::Exposure(Exposure { exposure: 0.25 })),
                m(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.5 })),
                m(ModifierKind::PixelSort(PixelSort {
                    threshold: 0.5,
                    angle: 45.0,
                })),
            ]
        };
        let a = render(chain());
        let b = render(chain());
        assert_eq!(fnv1a(&a.2), fnv1a(&b.2), "render is not deterministic");
    }
}
