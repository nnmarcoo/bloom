@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

struct Uniforms {
    viewport_size: vec2f,
    pan: vec2f,
    scale: f32,
    _pad: f32,
    image_size: vec2f,
}

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
    var quad = array<vec2f, 4>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0),
        vec2f(-1.0,  1.0), vec2f(1.0,  1.0)
    );
    var out: VertexOut;
    out.position = vec4f(quad[vi], 0.0, 1.0);
    out.uv = vec2f((quad[vi].x + 1.0) * 0.5, 1.0 - (quad[vi].y + 1.0) * 0.5);
    return out;
}

const PI: f32 = 3.141592653589793;
const LANCZOS_A: f32 = 3.0;

fn lanczos_weight(x: f32) -> f32 {
    if abs(x) < 1e-6 { return 1.0; }
    if abs(x) >= LANCZOS_A { return 0.0; }
    let px = x * PI;
    return (LANCZOS_A * sin(px) * sin(px / LANCZOS_A)) / (px * px);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let src_size = vec2f(textureDimensions(source, 0));
    let texels_per_pixel = 1.0 / uniforms.scale;

    let src_y = in.uv.y * src_size.y;
    let pass_through_u = in.uv.x;

    if texels_per_pixel <= 1.0 {
        return textureSampleLevel(source, tex_sampler, vec2f(pass_through_u, in.uv.y), 0.0);
    }

    let texel_height = 1.0 / src_size.y;
    let radius = i32(ceil(LANCZOS_A * texels_per_pixel));

    let center = src_y - 0.5;
    let center_i = floor(center);
    let frac = center - center_i;

    var color = vec4f(0.0);
    var weight_sum = 0.0;

    for (var j = -radius; j <= radius; j++) {
        let weight = lanczos_weight((f32(j) - frac) / texels_per_pixel);
        if weight == 0.0 { continue; }

        let sample_v = (center_i + f32(j) + 0.5) * texel_height;
        if sample_v < 0.0 || sample_v > 1.0 { continue; }

        color += textureSampleLevel(source, tex_sampler, vec2f(pass_through_u, sample_v), 0.0) * weight;
        weight_sum += weight;
    }

    if weight_sum <= 1e-9 {
        return textureSampleLevel(source, tex_sampler, vec2f(pass_through_u, in.uv.y), 0.0);
    }
    return color / weight_sum;
}
