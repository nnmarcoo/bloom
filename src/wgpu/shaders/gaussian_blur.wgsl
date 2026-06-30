struct BlurUniforms {
    direction: vec2<f32>,
    radius: f32,
    sigma: f32,
    proc_origin: vec2<f32>,
    proc_size: vec2<f32>,
    src_origin: vec2<f32>,
    src_size: vec2<f32>,
    lo_origin: vec2<f32>,
    lo_size: vec2<f32>,
    hi_origin: vec2<f32>,
    hi_size: vec2<f32>,
    has_lo: f32,
    has_hi: f32,
    src_lod: f32,
    _pad: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: BlurUniforms;
@group(0) @binding(1) var t_image: texture_2d<f32>;
@group(0) @binding(2) var s_image: sampler;
@group(0) @binding(3) var t_lo: texture_2d<f32>;
@group(0) @binding(4) var t_hi: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var quad = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0)
    );
    var out: VertexOutput;
    out.position = vec4<f32>(quad[vi], 0.0, 1.0);
    out.uv = vec2<f32>((quad[vi].x + 1.0) * 0.5, 1.0 - (quad[vi].y + 1.0) * 0.5);
    return out;
}

fn in_rect(full_uv: vec2<f32>, origin: vec2<f32>, size: vec2<f32>) -> bool {
    let lo = origin;
    let hi = origin + size;
    return all(full_uv >= lo) && all(full_uv < hi);
}

fn sample_tap(full_uv: vec2<f32>) -> vec4<f32> {
    if (u.has_lo > 0.5 && in_rect(full_uv, u.lo_origin, u.lo_size)) {
        let local = (full_uv - u.lo_origin) / u.lo_size;
        return textureSampleLevel(t_lo, s_image, local, 0.0);
    }
    if (u.has_hi > 0.5 && in_rect(full_uv, u.hi_origin, u.hi_size)) {
        let local = (full_uv - u.hi_origin) / u.hi_size;
        return textureSampleLevel(t_hi, s_image, local, 0.0);
    }
    let local = clamp((full_uv - u.src_origin) / u.src_size, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(t_image, s_image, local, u.src_lod);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = i32(ceil(u.radius));
    let full_uv = u.proc_origin + in.uv * u.proc_size;
    if (r <= 0) {
        let local = clamp((full_uv - u.src_origin) / u.src_size, vec2<f32>(0.0), vec2<f32>(1.0));
        return textureSampleLevel(t_image, s_image, local, u.src_lod);
    }
    let inv_two_sigma_sq = 1.0 / (2.0 * u.sigma * u.sigma);
    var sum = vec4<f32>(0.0);
    var weight_sum = 0.0;
    for (var i = -r; i <= r; i = i + 1) {
        let fi = f32(i);
        let w = exp(-fi * fi * inv_two_sigma_sq);
        let tap_uv = full_uv + u.direction * fi;
        sum = sum + sample_tap(tap_uv) * w;
        weight_sum = weight_sum + w;
    }
    return sum / weight_sum;
}
