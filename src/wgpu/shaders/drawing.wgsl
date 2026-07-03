struct DrawingUniforms {
    proc_origin: vec2<f32>,
    proc_size: vec2<f32>,
    src_origin: vec2<f32>,
    src_size: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: DrawingUniforms;
@group(0) @binding(1) var t_image: texture_2d<f32>;
@group(0) @binding(2) var t_layer: texture_2d<f32>;
@group(0) @binding(3) var s_image: sampler;

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
    let full_uv = u.proc_origin + in.uv * u.proc_size;
    let src_uv = (full_uv - u.src_origin) / u.src_size;
    let base = textureSample(t_image, s_image, src_uv);
    let paint = textureSample(t_layer, s_image, clamp(full_uv, vec2<f32>(0.0), vec2<f32>(1.0)));

    let sa = paint.a;
    let da = base.a;
    let out_a = sa + da * (1.0 - sa);
    if (out_a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let rgb = (paint.rgb * sa + base.rgb * da * (1.0 - sa)) / out_a;
    return vec4<f32>(rgb, out_a);
}
