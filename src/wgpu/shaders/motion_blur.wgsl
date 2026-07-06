struct MbUniforms {
    dir: vec2<f32>,
    samples: f32,
    _pad0: f32,
    proc_origin: vec2<f32>,
    proc_size: vec2<f32>,
    src_origin: vec2<f32>,
    src_size: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: MbUniforms;
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
    let full_uv = u.proc_origin + in.uv * u.proc_size;

    let n = i32(u.samples);
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i = i + 1) {
        let t = f32(i) / f32(n - 1) - 0.5;
        let s_full = full_uv + u.dir * t;
        let s_src = (s_full - u.src_origin) / u.src_size;
        acc = acc + textureSample(t_image, s_image, clamp(s_src, vec2<f32>(0.0), vec2<f32>(1.0)));
    }
    return acc / f32(n);
}
