struct Uniforms {
    transform: mat4x4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var t_image: texture_2d<f32>;
@group(0) @binding(2) var s_image: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var quad = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0)
    );

    let clip_pos = (uniforms.transform * vec4<f32>(quad[vi], 0.0, 1.0)).xy;

    var out: VertexOutput;
    out.position = vec4<f32>(clip_pos, 0.0, 1.0);
    out.uv = vec2<f32>((quad[vi].x + 1.0) * 0.5, 1.0 - (quad[vi].y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_image, s_image, in.uv);
}