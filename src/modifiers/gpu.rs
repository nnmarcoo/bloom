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

#[derive(Clone, Copy)]
pub struct UvRect {
    pub origin: [f32; 2],
    pub size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModUniforms {
    count: u32,
    _pad: [u32; 3],
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
    full_size_px: [f32; 2],
    _pad3: [f32; 2],
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

pub fn build_segment_uniforms(
    segment: &[&Modifier],
    tile: &TileInfo,
    proc: UvRect,
    src: UvRect,
) -> ModUniforms {
    let mut u = ModUniforms::zeroed();
    u.proc_origin = proc.origin;
    u.proc_size = proc.size;
    u.src_origin = src.origin;
    u.src_size = src.size;
    u.full_size_px = [tile.full_w as f32, tile.full_h as f32];
    for m in segment {
        if u.count >= 32 {
            break;
        }
        if let Some(entry) = m.kind.pack(tile) {
            u.entries[u.count as usize] = entry;
            u.count += 1;
        }
    }
    u
}
