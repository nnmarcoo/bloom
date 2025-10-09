struct Uniforms {
    res: vec2f,
    pos: vec2f,
    scale: f32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var sampl: sampler;

struct VertexIn {
    @builtin(vertex_index) vertex_index: u32,
}

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var positions = array<vec2f, 4>(
        vec2f(-1.0, -1.0),
        vec2f( 1.0, -1.0),
        vec2f(-1.0,  1.0),
        vec2f( 1.0,  1.0)
    );

    var out: VertexOut;
    out.position = vec4f((positions[in.vertex_index] + uniforms.pos / uniforms.res) * uniforms.scale, 0.0, 1.0);
    out.uv = vec2f((positions[in.vertex_index].x + 1.0) * 0.5, 1.0 - ((positions[in.vertex_index].y + 1.0) * 0.5)
);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return textureSample(tex, sampl, in.uv);
}
