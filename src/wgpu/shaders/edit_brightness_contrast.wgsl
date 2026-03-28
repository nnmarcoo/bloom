struct Uniforms {
    brightness: f32,
    contrast: f32,
    _pad: vec2<f32>,
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
    var c = textureSample(t_input, s_input, in.uv);

    // Brightness: additive shift in linear light.
    c = vec4<f32>(c.rgb + vec3<f32>(u.brightness), c.a);

    // Contrast: scale around mid-grey (0.5).
    let factor = (1.0 + u.contrast);
    c = vec4<f32>((c.rgb - 0.5) * factor + 0.5, c.a);

    return clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
}
