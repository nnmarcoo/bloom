struct Uniforms {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    tile_size_pad: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var quad = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0)
    );
    var out: VertexOutput;
    out.position = vec4<f32>(quad[vi], 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let cell = vec2<i32>(floor(in.position.xy / uniforms.tile_size_pad.x));
    let checker = (cell.x + cell.y) % 2;
    if checker == 0 {
        return uniforms.color_a;
    } else {
        return uniforms.color_b;
    }
}
