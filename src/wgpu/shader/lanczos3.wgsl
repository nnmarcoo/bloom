struct Uniforms {
    resolution : vec2<f32>,
    center     : vec2<f32>,
    scale      : f32,
    _padding   : f32,
};

@group(0) @binding(0)
var<uniform> uniforms : Uniforms;

@group(0) @binding(1)
var myTexture : texture_2d<f32>;

@group(0) @binding(2)
var mySampler : sampler;

var<private> positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>( 3.0,  1.0),
    vec2<f32>(-1.0,  1.0),
);

struct VSOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) uv       : vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx : u32) -> VSOut {
    var out : VSOut;
    let pos = positions[idx];
    out.pos = vec4<f32>(pos, 0.0, 1.0);

    out.uv = (pos * 0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    let color = textureSample(myTexture, mySampler, in.uv);
    return color;
}
