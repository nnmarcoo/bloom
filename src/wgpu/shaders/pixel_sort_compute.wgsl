struct PixelSortUniforms {
    width: u32,
    height: u32,
    cutoff: u32,
    reverse: u32,
    vertical: u32,
    row_words: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> u: PixelSortUniforms;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;

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
    let n = select(u.height, u.width, u.vertical == 0u);
    let n_lines = select(u.width, u.height, u.vertical == 0u);
    if (line >= n_lines) {
        return;
    }
    var base: u32;
    var stride: u32;
    if (u.vertical == 0u) {
        base = line * u.row_words;
        stride = 1u;
    } else {
        base = line;
        stride = u.row_words;
    }

    var i = 0u;
    loop {
        if (i >= n) { break; }
        let cur = src[base + i * stride];
        if (luma(cur) <= u.cutoff) {
            dst[base + i * stride] = cur;
            i = i + 1u;
            continue;
        }
        let start = i;
        var end = i;
        loop {
            if (end >= n) { break; }
            if (luma(src[base + end * stride]) <= u.cutoff) { break; }
            end = end + 1u;
        }
        let run_len = end - start;

        for (var k = 0u; k < 256u; k = k + 1u) { hist[k] = 0u; }
        for (var j = start; j < end; j = j + 1u) {
            let key = luma(src[base + j * stride]);
            hist[key] = hist[key] + 1u;
        }
        var acc = 0u;
        for (var k = 0u; k < 256u; k = k + 1u) {
            offset[k] = acc;
            acc = acc + hist[k];
        }
        for (var j = start; j < end; j = j + 1u) {
            let v = src[base + j * stride];
            let key = luma(v);
            let rank = offset[key];
            offset[key] = rank + 1u;
            var dst_rank = rank;
            if (u.reverse != 0u) {
                dst_rank = run_len - 1u - rank;
            }
            dst[base + (start + dst_rank) * stride] = v;
        }
        i = end;
    }
}
