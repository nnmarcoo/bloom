use bytemuck::Zeroable;

use crate::modifiers::{Modifier, ModifierKind, ids};

pub struct TileInfo {
    pub tile_x: u32,
    pub tile_y: u32,
    pub tile_w: u32,
    pub tile_h: u32,
    pub full_w: u32,
    pub full_h: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModEntry {
    data: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModUniforms {
    count: u32,
    _pad: [u32; 3],
    entries: [ModEntry; 32],
}

fn make_entry(kind: u32, params: &[f32]) -> ModEntry {
    debug_assert!(
        params.len() <= 11,
        "make_entry: params overflow ModEntry ({} > 11)",
        params.len()
    );
    let mut e = ModEntry::zeroed();
    e.data[0][0] = f32::from_bits(kind);
    for (i, &p) in params.iter().enumerate() {
        let slot = i + 1;
        e.data[slot / 4][slot % 4] = p;
    }
    e
}

pub fn build_mod_uniforms(modifiers: &[Modifier], tile: &TileInfo) -> ModUniforms {
    let mut u = ModUniforms::zeroed();
    for m in modifiers {
        if !m.has_visible_effect() || u.count >= 32 {
            continue;
        }
        let entry = match &m.kind {
            ModifierKind::Exposure { exposure } => make_entry(ids::EXPOSURE, &[*exposure]),
            ModifierKind::Levels {
                shadows,
                midtones,
                highlights,
            } => make_entry(ids::LEVELS, &[*shadows, *midtones, *highlights]),
            ModifierKind::BrightnessContrast {
                brightness,
                contrast,
            } => make_entry(ids::BRIGHTNESS_CONTRAST, &[*brightness, *contrast]),
            ModifierKind::HueSaturation {
                hue,
                saturation,
                lightness,
            } => make_entry(ids::HUE_SATURATION, &[*hue, *saturation, *lightness]),
            ModifierKind::Vignette {
                strength,
                size,
                softness,
            } => make_entry(
                ids::VIGNETTE,
                &[
                    *strength,
                    *size,
                    *softness,
                    tile.tile_x as f32 / tile.full_w as f32,
                    tile.tile_y as f32 / tile.full_h as f32,
                    tile.tile_w as f32 / tile.full_w as f32,
                    tile.tile_h as f32 / tile.full_h as f32,
                ],
            ),
            ModifierKind::Posterize { levels } => make_entry(ids::POSTERIZE, &[*levels as f32]),
            ModifierKind::Threshold { cutoff } => make_entry(ids::THRESHOLD, &[*cutoff]),
            ModifierKind::Vibrance {
                vibrance,
                saturation,
            } => make_entry(ids::VIBRANCE, &[*vibrance, *saturation]),
            ModifierKind::ColorBalance {
                cyan_red,
                magenta_green,
                yellow_blue,
            } => make_entry(
                ids::COLOR_BALANCE,
                &[*cyan_red, *magenta_green, *yellow_blue],
            ),
            ModifierKind::Grain {
                amount,
                size,
                roughness,
                seed,
            } => make_entry(
                ids::GRAIN,
                &[
                    *amount,
                    *size,
                    *roughness,
                    *seed,
                    tile.tile_x as f32,
                    tile.tile_y as f32,
                    tile.tile_w as f32,
                    tile.tile_h as f32,
                ],
            ),
            ModifierKind::ChromaticAberration { amount } => make_entry(
                ids::CHROMATIC_ABERRATION,
                &[
                    *amount / tile.full_w as f32,
                    tile.tile_x as f32 / tile.full_w as f32,
                    tile.tile_y as f32 / tile.full_h as f32,
                    tile.tile_w as f32 / tile.full_w as f32,
                    tile.tile_h as f32 / tile.full_h as f32,
                ],
            ),
            ModifierKind::Halftone { size, angle } => make_entry(
                ids::HALFTONE,
                &[
                    *size / tile.full_w.min(tile.full_h) as f32,
                    *angle * std::f32::consts::PI / 180.0,
                    tile.tile_x as f32 / tile.full_w as f32,
                    tile.tile_y as f32 / tile.full_h as f32,
                    tile.tile_w as f32 / tile.full_w as f32,
                    tile.tile_h as f32 / tile.full_h as f32,
                ],
            ),
            _ => continue,
        };
        u.entries[u.count as usize] = entry;
        u.count += 1;
    }
    u
}
