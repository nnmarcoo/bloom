struct MbComputeUniforms {
    width: u32,
    height: u32,
    row_words: u32,
    samples: u32,
    du: f32,
    dv: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: MbComputeUniforms;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;

fn unpack(p: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(p & 0xffu),
        f32((p >> 8u) & 0xffu),
        f32((p >> 16u) & 0xffu),
        f32((p >> 24u) & 0xffu),
    ) / 255.0;
}

fn pack(c: vec4<f32>) -> u32 {
    let q = clamp(c, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5);
    return u32(q.x) | (u32(q.y) << 8u) | (u32(q.z) << 16u) | (u32(q.w) << 24u);
}

fn load(x: i32, y: i32) -> vec4<f32> {
    let cx = u32(clamp(x, 0, i32(u.width) - 1));
    let cy = u32(clamp(y, 0, i32(u.height) - 1));
    return unpack(src[cy * u.row_words + cx]);
}

fn sample_bilinear(fx: f32, fy: f32) -> vec4<f32> {
    let px = fx - 0.5;
    let py = fy - 0.5;
    let x0 = i32(floor(px));
    let y0 = i32(floor(py));
    let tx = px - f32(x0);
    let ty = py - f32(y0);
    let c00 = load(x0, y0);
    let c10 = load(x0 + 1, y0);
    let c01 = load(x0, y0 + 1);
    let c11 = load(x0 + 1, y0 + 1);
    let top = mix(c00, c10, tx);
    let bot = mix(c01, c11, tx);
    return mix(top, bot, ty);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.width || gid.y >= u.height) {
        return;
    }
    let cx = f32(gid.x) + 0.5;
    let cy = f32(gid.y) + 0.5;
    let n = i32(u.samples);
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i = i + 1) {
        let t = f32(i) / f32(n - 1) - 0.5;
        let sx = cx + u.du * t;
        let sy = cy + u.dv * t;
        acc = acc + sample_bilinear(sx, sy);
    }
    let out = acc / f32(n);
    dst[gid.y * u.row_words + gid.x] = pack(out);
}
