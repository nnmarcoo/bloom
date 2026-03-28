// Curves pass — four 256-entry 1D LUT textures: RGB master, R, G, B.
// Each LUT is uploaded as a 256x1 Rgba8Unorm texture, one channel used.

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var t_lut_rgb: texture_2d<f32>;
@group(0) @binding(3) var t_lut_r:   texture_2d<f32>;
@group(0) @binding(4) var t_lut_g:   texture_2d<f32>;
@group(0) @binding(5) var t_lut_b:   texture_2d<f32>;
@group(0) @binding(6) var s_lut:     sampler;

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let p = positions[vi];
    var out: VertexOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv  = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

fn lut_sample(lut: texture_2d<f32>, s: sampler, v: f32) -> f32 {
    return textureSample(lut, s, vec2<f32>(v, 0.5)).r;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_input, s_input, in.uv);

    // Apply RGB master curve first, then per-channel.
    let r = lut_sample(t_lut_r, s_lut, lut_sample(t_lut_rgb, s_lut, c.r));
    let g = lut_sample(t_lut_g, s_lut, lut_sample(t_lut_rgb, s_lut, c.g));
    let b = lut_sample(t_lut_b, s_lut, lut_sample(t_lut_rgb, s_lut, c.b));

    return vec4<f32>(r, g, b, c.a);
}
