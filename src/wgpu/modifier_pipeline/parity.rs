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

use super::goldens::{ParityOutcome, parity_probe};
use crate::modifiers::kinds::{
    BrightnessContrast, ColorBalance, Duotone, Exposure, Grain, Grayscale, Halftone, HueSaturation,
    Invert, Levels, Posterize, Sepia, Solarize, Temperature, Threshold, Vibrance, Vignette,
};
use crate::modifiers::{Modifier, ModifierKind};

/// Channel difference allowed before a modifier is considered divergent.
///
/// 4/255 tolerates f32 rounding and intrinsic differences while still failing
/// on a wrong formula, which shifts output by tens of levels.
const TOL: u8 = 4;

fn check(label: &str, kind: ModifierKind) {
    match parity_probe(&[Modifier::new(kind)], TOL) {
        ParityOutcome::NoDevice => {}
        ParityOutcome::Checked { max_diff, pct_over } => {
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
fn parity_exposure() {
    check(
        "exposure",
        ModifierKind::Exposure(Exposure { exposure: 0.75 }),
    );
}

#[test]
fn parity_levels() {
    check(
        "levels",
        ModifierKind::Levels(Levels {
            shadows: 0.15,
            midtones: 1.4,
            highlights: 0.85,
        }),
    );
}

#[test]
fn parity_brightness_contrast() {
    check(
        "brightness_contrast",
        ModifierKind::BrightnessContrast(BrightnessContrast {
            brightness: 0.2,
            contrast: 0.35,
        }),
    );
}

#[test]
fn parity_hue_saturation() {
    check(
        "hue_saturation",
        ModifierKind::HueSaturation(HueSaturation {
            hue: 40.0,
            saturation: 0.4,
            lightness: 0.1,
        }),
    );
}

#[test]
fn parity_vignette() {
    check(
        "vignette",
        ModifierKind::Vignette(Vignette {
            strength: 0.7,
            size: 0.6,
            softness: 0.3,
        }),
    );
}

#[test]
fn parity_posterize() {
    check(
        "posterize",
        ModifierKind::Posterize(Posterize { levels: 5 }),
    );
}

#[test]
fn parity_threshold() {
    check(
        "threshold",
        ModifierKind::Threshold(Threshold { cutoff: 0.45 }),
    );
}

#[test]
fn parity_vibrance() {
    check(
        "vibrance",
        ModifierKind::Vibrance(Vibrance {
            vibrance: 0.6,
            saturation: 0.25,
        }),
    );
}

#[test]
fn parity_color_balance() {
    check(
        "color_balance",
        ModifierKind::ColorBalance(ColorBalance {
            cyan_red: 0.25,
            magenta_green: -0.15,
            yellow_blue: 0.35,
        }),
    );
}

#[test]
fn parity_grain() {
    check(
        "grain",
        ModifierKind::Grain(Grain {
            amount: 0.35,
            size: 2.0,
            seed: 7.0,
            color: 0.5,
            response: 0.5,
        }),
    );
}

#[test]
fn parity_invert() {
    check("invert", ModifierKind::Invert(Invert { amount: 0.8 }));
}

#[test]
fn parity_grayscale() {
    check(
        "grayscale",
        ModifierKind::Grayscale(Grayscale { amount: 0.9 }),
    );
}

#[test]
fn parity_temperature() {
    check(
        "temperature",
        ModifierKind::Temperature(Temperature {
            temperature: 0.4,
            tint: -0.25,
        }),
    );
}

#[test]
fn parity_sepia() {
    check("sepia", ModifierKind::Sepia(Sepia { intensity: 0.85 }));
}

#[test]
fn parity_solarize() {
    check(
        "solarize",
        ModifierKind::Solarize(Solarize { threshold: 0.55 }),
    );
}

#[test]
fn parity_halftone() {
    check(
        "halftone",
        ModifierKind::Halftone(Halftone {
            size: 6.0,
            angle: 30.0,
        }),
    );
}

#[test]
fn parity_duotone() {
    check(
        "duotone",
        ModifierKind::Duotone(Duotone {
            shadow: [0.1, 0.05, 0.4],
            highlight: [1.0, 0.85, 0.3],
            amount: 0.9,
        }),
    );
}
