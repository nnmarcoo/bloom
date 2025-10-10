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
    let uvPerTexel = 1.0 / texSize;

    let uvF = fwidth(in.uv);
    let texelsPerPixel = uvF * texSize;
    var S = max(texelsPerPixel.x, texelsPerPixel.y);

    if S < 1.0 { S = 1.0; }

    let a = 3.0;

    var radius_f = ceil(a * S);
    if radius_f > 64.0 { radius_f = 64.0; }
    let radius = i32(radius_f);

    var color = vec4f(0.0);
    var weightSum = 0.0;

    let _touch = textureSampleLevel(tex, sampl, in.uv, 0.0).r * 0.0;

    for (var j: i32 = -radius; j <= radius; j = j + 1) {
        for (var i: i32 = -radius; i <= radius; i = i + 1) {
            let offset_uv = vec2f(f32(i), f32(j)) * uvPerTexel;

            let dx = f32(i) / S;
            let dy = f32(j) / S;

            let wx = lanczos(dx, a);
            let wy = lanczos(dy, a);
            let w = wx * wy;

            let sampUV = clamp(in.uv + offset_uv, vec2f(0.0, 0.0), vec2f(1.0, 1.0));

            color += textureSampleLevel(tex, sampl, sampUV, 0.0) * w;
            weightSum += w;
        }
    }

    if weightSum <= 1e-9 {
        return textureSampleLevel(tex, sampl, in.uv, 0.0);
    }

    return color / weightSum;
}
