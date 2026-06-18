struct CaUniforms {
    amount: f32,
    tile_origin_x: f32,
    tile_origin_y: f32,
    tile_size_x: f32,
    tile_size_y: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: CaUniforms;
@group(0) @binding(1) var t_image: texture_2d<f32>;
@group(0) @binding(2) var s_image: sampler;

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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(t_image, s_image, in.uv);

    let tile_origin = vec2<f32>(u.tile_origin_x, u.tile_origin_y);
    let tile_size = vec2<f32>(u.tile_size_x, u.tile_size_y);
    let full_uv = in.uv * tile_size + tile_origin;
    let offset = full_uv - vec2<f32>(0.5);
    let r_full = clamp(vec2<f32>(0.5) + offset * (1.0 + u.amount), vec2<f32>(0.0), vec2<f32>(1.0));
    let b_full = clamp(vec2<f32>(0.5) + offset * (1.0 - u.amount), vec2<f32>(0.0), vec2<f32>(1.0));
    let r_tile = clamp((r_full - tile_origin) / tile_size, vec2<f32>(0.0), vec2<f32>(1.0));
    let b_tile = clamp((b_full - tile_origin) / tile_size, vec2<f32>(0.0), vec2<f32>(1.0));

    let cr = textureSample(t_image, s_image, r_tile);
    let cb = textureSample(t_image, s_image, b_tile);
    return clamp(vec4<f32>(cr.r, c.g, cb.b, c.a), vec4<f32>(0.0), vec4<f32>(1.0));
}
