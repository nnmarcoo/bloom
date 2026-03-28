// Non-destructive crop — masks pixels outside the normalised rect to alpha=0.
struct Uniforms {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var t_input: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;

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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_input, s_input, in.uv);
    let uv = in.uv;
    let inside = uv.x >= u.x && uv.x <= (u.x + u.w)
              && uv.y >= u.y && uv.y <= (u.y + u.h);
    return select(vec4<f32>(c.rgb, 0.0), c, inside);
}
