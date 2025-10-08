struct Uniforms {
    resolution: vec2f,
    center: vec2f,
    scale: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var myTex: texture_2d<f32>;

@group(0) @binding(2)
var mySampler: sampler;

struct VertexIn {
    @builtin(vertex_index) vertex_index: u32,
}

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) color: vec3f,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var positions = array<vec2f, 3>(
        vec2f( 0.0,  0.5),
        vec2f(-0.5, -0.5),
        vec2f( 0.5, -0.5),
    );

    var colors = array<vec3f, 3>(
        vec3f(1.0, 0.0, 0.0),
        vec3f(0.0, 1.0, 0.0),
        vec3f(0.0, 0.0, 1.0),
    );

    var out: VertexOut;
    out.position = vec4f(positions[in.vertex_index], 0.0, 1.0);
    out.color = colors[in.vertex_index];

    _ = uniforms.scale;
    _ = uniforms.center;
    _ = uniforms.resolution;

    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return vec4f(in.color, 1.0);
}
