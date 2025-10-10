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

const pi = 3.1415926535897932384626433832795;

fn sinc(x: f32) -> f32 {
    if abs(x) < 1e-5 {
        return 1.0;
    }
    return sin(x) / x;
}

fn lanczos(x: f32, a: f32) -> f32 {
    if abs(x) >= a {
        return 0.0;
    }
    return sinc(x * pi) * sinc(x * pi / a);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let texSize = vec2f(textureDimensions(tex, 0));
    let pixel = 1.0 / texSize;

    let a = 2.0;
    var color = vec4f(0.0);
    var weightSum = 0.0;

    let radius = i32(ceil(a / uniforms.scale));

    for (var x: i32 = -radius; x <= radius; x = x + 1) {
        for (var y: i32 = -radius; y <= radius; y = y + 1) {
            let offset = vec2f(f32(x), f32(y)) * pixel;

            let wx = lanczos(f32(x) * uniforms.scale, a);
            let wy = lanczos(f32(y) * uniforms.scale, a);
            let w = wx * wy;

            color += textureSample(tex, sampl, in.uv + offset) * w;
            weightSum += w;
        }
    }

    return color / weightSum;
}
