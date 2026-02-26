// could be merged into one shader with a `horizontal: u32` uniform flag if needed
struct LanczosUniforms {
    src_size: vec2<f32>,
    scale: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: LanczosUniforms;
@group(0) @binding(1) var t_input: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;

const PI: f32 = 3.141592653589793;
const LANCZOS_A: f32 = 3.0;

fn lanczos_weight(x: f32) -> f32 {
    if abs(x) < 1e-6 { return 1.0; }
    if abs(x) >= LANCZOS_A { return 0.0; }
    let px = x * PI;
    return (LANCZOS_A * sin(px) * sin(px / LANCZOS_A)) / (px * px);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((vi + 2u) / 3u % 2u), f32((vi + 1u) / 3u % 2u));
    let pos = uv * 2.0 - 1.0;
    return VertexOutput(vec4(pos, 0.0, 1.0), uv);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let src_size = vec2<f32>(textureDimensions(t_input, 0));
    let texels_per_pixel = 1.0 / uniforms.scale;

    if texels_per_pixel <= 1.0 {
        return textureSampleLevel(t_input, s_input, in.uv, 0.0);
    }

    let texel_width = 1.0 / src_size.x;
    let radius = i32(ceil(LANCZOS_A * texels_per_pixel));

    let src_x = in.uv.x * src_size.x;
    let center = src_x - 0.5;
    let center_i = floor(center);
    let frac = center - center_i;

    var color = vec4<f32>(0.0);
    var weight_sum = 0.0;

    for (var i = -radius; i <= radius; i++) {
        let weight = lanczos_weight((f32(i) - frac) / texels_per_pixel);
        if weight == 0.0 { continue; }

        let sample_u = (center_i + f32(i) + 0.5) * texel_width;
        if sample_u < 0.0 || sample_u > 1.0 { continue; }

        color += textureSampleLevel(t_input, s_input, vec2(sample_u, in.uv.y), 0.0) * weight;
        weight_sum += weight;
    }

    if weight_sum <= 1e-9 {
        return textureSampleLevel(t_input, s_input, in.uv, 0.0);
    }
    return color / weight_sum;
}