struct PixelSortDiagUniforms {
    n_lines: u32,
    cutoff: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> u: PixelSortDiagUniforms;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;
@group(0) @binding(3) var<storage, read> line_index: array<u32>;
@group(0) @binding(4) var<storage, read> line_offset: array<u32>;

fn luma(p: u32) -> u32 {
    let r = f32(p & 0xffu);
    let g = f32((p >> 8u) & 0xffu);
    let b = f32((p >> 16u) & 0xffu);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    return u32(clamp(y + 0.5, 0.0, 255.0));
}

var<private> hist: array<u32, 256>;
var<private> offset: array<u32, 256>;

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let line = gid.x;
    if (line >= u.n_lines) {
        return;
    }
    let base = line_offset[line * 2u];
    let n = line_offset[line * 2u + 1u];

    var i = 0u;
    loop {
        if (i >= n) { break; }
        let cur = src[line_index[base + i]];
        if (luma(cur) <= u.cutoff) {
            dst[line_index[base + i]] = cur;
            i = i + 1u;
            continue;
        }
        let start = i;
        var end = i;
        loop {
            if (end >= n) { break; }
            if (luma(src[line_index[base + end]]) <= u.cutoff) { break; }
            end = end + 1u;
        }

        for (var k = 0u; k < 256u; k = k + 1u) { hist[k] = 0u; }
        for (var j = start; j < end; j = j + 1u) {
            let key = luma(src[line_index[base + j]]);
            hist[key] = hist[key] + 1u;
        }
        var acc = 0u;
        for (var k = 0u; k < 256u; k = k + 1u) {
            offset[k] = acc;
            acc = acc + hist[k];
        }
        for (var j = start; j < end; j = j + 1u) {
            let v = src[line_index[base + j]];
            let key = luma(v);
            let rank = offset[key];
            offset[key] = rank + 1u;
            dst[line_index[base + start + rank]] = v;
        }
        i = end;
    }
}
