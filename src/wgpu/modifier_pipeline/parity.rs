//! Per-modifier CPU/GPU equivalence.
//!
//! Every modifier that packs into `combined_modifiers.wgsl` is written twice:
//! once as `apply_cpu` in Rust and once as a `case` arm in WGSL. They are
//! independent implementations of the same math, and nothing forces them to
//! agree.
//!
//! The chain goldens compare whole stacks, so a divergence in one modifier can
//! be masked by a later stage or diluted below tolerance. These tests render a
//! single modifier at a time over a gradient that sweeps the full color range,
//! so each implementation pair is checked in isolation.
//!
//! Tolerance is per-channel on 8-bit output. A few levels of difference are
//! expected and fine: the GPU works in f32 with its own rounding, and some
//! shader intrinsics (`smoothstep`, `pow`) are permitted to differ slightly
//! from their Rust equivalents. What these catch is a *structural* difference,
//! where one path implements a different formula.
//!
//! CASES is the single list. It drives one `#[test]` per modifier and the
//! coverage check that fails when a modifier is added without one, so the suite
//! cannot silently stop covering something.

use super::goldens::{ParityOutcome, parity_probe};
use crate::modifiers::kinds::{
    BrightnessContrast, ColorBalance, Duotone, Exposure, Grain, Grayscale, Halftone, HueSaturation,
    Invert, Levels, Posterize, Sepia, Solarize, Temperature, Threshold, Vibrance, Vignette,
};
use crate::modifiers::{Modifier, ModifierKind, ids};

const TOL: u8 = 4;

struct Case {
    id: u32,
    label: &'static str,
    kind: fn() -> ModifierKind,
}

const CASES: &[Case] = &[
    Case {
        id: ids::EXPOSURE,
        label: "exposure",
        kind: || ModifierKind::Exposure(Exposure { exposure: 0.75 }),
    },
    Case {
        id: ids::LEVELS,
        label: "levels",
        kind: || {
            ModifierKind::Levels(Levels {
                shadows: 0.15,
                midtones: 1.4,
                highlights: 0.85,
            })
        },
    },
    Case {
        id: ids::BRIGHTNESS_CONTRAST,
        label: "brightness_contrast",
        kind: || {
            ModifierKind::BrightnessContrast(BrightnessContrast {
                brightness: 0.2,
                contrast: 0.35,
            })
        },
    },
    Case {
        id: ids::HUE_SATURATION,
        label: "hue_saturation",
        kind: || {
            ModifierKind::HueSaturation(HueSaturation {
                hue: 40.0,
                saturation: 0.4,
                lightness: 0.1,
            })
        },
    },
    Case {
        id: ids::VIGNETTE,
        label: "vignette",
        kind: || {
            ModifierKind::Vignette(Vignette {
                strength: 0.7,
                size: 0.6,
                softness: 0.3,
            })
        },
    },
    Case {
        id: ids::POSTERIZE,
        label: "posterize",
        kind: || ModifierKind::Posterize(Posterize { levels: 5 }),
    },
    Case {
        id: ids::THRESHOLD,
        label: "threshold",
        kind: || ModifierKind::Threshold(Threshold { cutoff: 0.45 }),
    },
    Case {
        id: ids::VIBRANCE,
        label: "vibrance",
        kind: || {
            ModifierKind::Vibrance(Vibrance {
                vibrance: 0.6,
                saturation: 0.25,
            })
        },
    },
    Case {
        id: ids::COLOR_BALANCE,
        label: "color_balance",
        kind: || {
            ModifierKind::ColorBalance(ColorBalance {
                cyan_red: 0.25,
                magenta_green: -0.15,
                yellow_blue: 0.35,
            })
        },
    },
    Case {
        id: ids::GRAIN,
        label: "grain",
        kind: || {
            ModifierKind::Grain(Grain {
                amount: 0.35,
                size: 2.0,
                seed: 7.0,
                color: 0.5,
                response: 0.5,
            })
        },
    },
    Case {
        id: ids::INVERT,
        label: "invert",
        kind: || ModifierKind::Invert(Invert { amount: 0.8 }),
    },
    Case {
        id: ids::GRAYSCALE,
        label: "grayscale",
        kind: || ModifierKind::Grayscale(Grayscale { amount: 0.9 }),
    },
    Case {
        id: ids::TEMPERATURE,
        label: "temperature",
        kind: || {
            ModifierKind::Temperature(Temperature {
                temperature: 0.4,
                tint: -0.25,
            })
        },
    },
    Case {
        id: ids::SEPIA,
        label: "sepia",
        kind: || ModifierKind::Sepia(Sepia { intensity: 0.85 }),
    },
    Case {
        id: ids::SOLARIZE,
        label: "solarize",
        kind: || ModifierKind::Solarize(Solarize { threshold: 0.55 }),
    },
    Case {
        id: ids::HALFTONE,
        label: "halftone",
        kind: || {
            ModifierKind::Halftone(Halftone {
                size: 6.0,
                angle: 30.0,
            })
        },
    },
    Case {
        id: ids::DUOTONE,
        label: "duotone",
        kind: || {
            ModifierKind::Duotone(Duotone {
                shadow: [0.1, 0.05, 0.4],
                highlight: [1.0, 0.85, 0.3],
                amount: 0.9,
            })
        },
    },
];

fn run(id: u32) {
    let case = CASES
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no parity case registered for id {id}"));
    match parity_probe(&[Modifier::new((case.kind)())], TOL) {
        ParityOutcome::NoDevice => {}
        ParityOutcome::Checked { max_diff, pct_over } => {
            let label = case.label;
            assert!(
                max_diff <= TOL,
                "{label}: CPU and GPU disagree. max channel diff {max_diff} > \
                 tol {TOL}, {pct_over:.2}% of channels over. The two \
                 implementations have drifted; compare `apply_cpu` in \
                 modifiers/kinds/{label}.rs against its arm in \
                 combined_modifiers.wgsl."
            );
        }
    }
}

#[test]
fn covers_every_shared_shader_modifier() {
    let missing: Vec<&str> = ids::ALL
        .iter()
        .filter(|(_, id)| !CASES.iter().any(|c| c.id == *id))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        missing.is_empty(),
        "these modifiers pack into the shared shader but have no parity case: \
         {missing:?}. Add one to CASES in this file so the two implementations \
         are checked against each other."
    );

    let unknown: Vec<u32> = CASES
        .iter()
        .filter(|c| !ids::ALL.iter().any(|(_, id)| *id == c.id))
        .map(|c| c.id)
        .collect();
    assert!(
        unknown.is_empty(),
        "parity cases reference ids that no longer exist: {unknown:?}"
    );
}

#[test]
fn case_ids_are_unique() {
    let mut ids: Vec<u32> = CASES.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "CASES contains a duplicate id");
}

#[test]
fn every_case_has_a_test() {
    const SRC: &str = include_str!("parity.rs");
    let missing: Vec<&str> = CASES
        .iter()
        .filter(|c| !SRC.contains(&format!("fn parity_{}()", c.label)))
        .map(|c| c.label)
        .collect();
    assert!(
        missing.is_empty(),
        "these parity cases are registered but never run: {missing:?}. \
         Add `#[test] fn parity_<label>() {{ run(ids::<ID>); }}` for each."
    );
}

#[test]
fn parity_exposure() {
    run(ids::EXPOSURE);
}

#[test]
fn parity_levels() {
    run(ids::LEVELS);
}

#[test]
fn parity_brightness_contrast() {
    run(ids::BRIGHTNESS_CONTRAST);
}

#[test]
fn parity_hue_saturation() {
    run(ids::HUE_SATURATION);
}

#[test]
fn parity_vignette() {
    run(ids::VIGNETTE);
}

#[test]
fn parity_posterize() {
    run(ids::POSTERIZE);
}

#[test]
fn parity_threshold() {
    run(ids::THRESHOLD);
}

#[test]
fn parity_vibrance() {
    run(ids::VIBRANCE);
}

#[test]
fn parity_color_balance() {
    run(ids::COLOR_BALANCE);
}

#[test]
fn parity_grain() {
    run(ids::GRAIN);
}

#[test]
fn parity_invert() {
    run(ids::INVERT);
}

#[test]
fn parity_grayscale() {
    run(ids::GRAYSCALE);
}

#[test]
fn parity_temperature() {
    run(ids::TEMPERATURE);
}

#[test]
fn parity_sepia() {
    run(ids::SEPIA);
}

#[test]
fn parity_solarize() {
    run(ids::SOLARIZE);
}

#[test]
fn parity_halftone() {
    run(ids::HALFTONE);
}

#[test]
fn parity_duotone() {
    run(ids::DUOTONE);
}
