use bytemuck::Zeroable;

use crate::modifiers::Modifier;

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

pub(crate) fn make_entry(kind: u32, params: &[f32]) -> ModEntry {
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
        if let Some(entry) = m.kind.pack(tile) {
            u.entries[u.count as usize] = entry;
            u.count += 1;
        }
    }
    u
}
