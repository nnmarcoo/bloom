struct PixelSortDiagUniforms {
    width: u32,
    height: u32,
    row_words: u32,
    cutoff: u32,
    dx: u32,
    dy: u32,
    flip_x: u32,
    flip_y: u32,
    n_lines: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> u: PixelSortDiagUniforms;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;

fn luma(p: u32) -> u32 {
    let r = f32(p & 0xffu);
    let g = f32((p >> 8u) & 0xffu);
    let b = f32((p >> 16u) & 0xffu);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    return u32(clamp(y + 0.5, 0.0, 255.0));
}

fn line_addr(x0: u32, y0: u32, i: u32) -> u32 {
    let px = x0 + i * u.dx;
    let py = y0 + i * u.dy;
    var rx = px;
    if (u.flip_x != 0u) {
        rx = u.width - 1u - px;
    }
    var ry = py;
    if (u.flip_y != 0u) {
        ry = u.height - 1u - py;
    }
    return ry * u.row_words + rx;
}

var<private> hist: array<u32, 256>;
var<private> offset: array<u32, 256>;

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let line = gid.x;
    if (line >= u.n_lines) {
        return;
    }
    let bx = min(u.dx, u.width);
    let left = bx * u.height;
    var x0: u32;
    var y0: u32;
    if (line < left) {
        x0 = line % bx;
        y0 = line / bx;
    } else {
        let id2 = line - left;
        let span = u.width - bx;
        x0 = bx + id2 % span;
        y0 = id2 / span;
    }
    let steps_x = (u.width - 1u - x0) / u.dx;
    let steps_y = (u.height - 1u - y0) / u.dy;
    let n = min(steps_x, steps_y) + 1u;

    var i = 0u;
    loop {
        if (i >= n) { break; }
        let cur = src[line_addr(x0, y0, i)];
        if (luma(cur) <= u.cutoff) {
            dst[line_addr(x0, y0, i)] = cur;
            i = i + 1u;
            continue;
        }
        let start = i;
        var end = i;
        loop {
            if (end >= n) { break; }
            if (luma(src[line_addr(x0, y0, end)]) <= u.cutoff) { break; }
            end = end + 1u;
        }

        for (var k = 0u; k < 256u; k = k + 1u) { hist[k] = 0u; }
        for (var j = start; j < end; j = j + 1u) {
            let key = luma(src[line_addr(x0, y0, j)]);
            hist[key] = hist[key] + 1u;
        }
        var acc = 0u;
        for (var k = 0u; k < 256u; k = k + 1u) {
            offset[k] = acc;
            acc = acc + hist[k];
        }
        for (var j = start; j < end; j = j + 1u) {
            let v = src[line_addr(x0, y0, j)];
            let key = luma(v);
            let rank = offset[key];
            offset[key] = rank + 1u;
            dst[line_addr(x0, y0, start + rank)] = v;
        }
        i = end;
    }
}
