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

    let aspect = uniforms.image_size / uniforms.viewport_size;
    let clip_pos = (quad[vi] * aspect + uniforms.pan / uniforms.viewport_size) * uniforms.scale;

    var out: VertexOut;
    out.position = vec4f(clip_pos, 0.0, 1.0);
    out.uv = vec2f((quad[vi].x + 1.0) * 0.5, 1.0 - (quad[vi].y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(source, 0));
    let coord = clamp(vec2i(floor(in.uv * size)), vec2i(0), vec2i(size) - vec2i(1));
    return textureLoad(source, coord, 0);
}
