struct Uniforms {
    hue: f32,        // degrees, -180..180
    saturation: f32, // -1..1
    lightness: f32,  // -1..1
    _pad: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var t_input: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let p = positions[vi];
    var out: VertexOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv  = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let r = rgb.r; let g = rgb.g; let b = rgb.b;
    let mx = max(max(r, g), b);
    let mn = min(min(r, g), b);
    let l  = (mx + mn) * 0.5;
    if mx == mn {
        return vec3<f32>(0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = select(d / (2.0 - mx - mn), d / (mx + mn), l < 0.5);
    var h: f32;
    if mx == r {
        h = (g - b) / d + select(6.0, 0.0, g >= b);
    } else if mx == g {
        h = (b - r) / d + 2.0;
    } else {
        h = (r - g) / d + 4.0;
    }
    return vec3<f32>(h / 6.0, s, l);
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    var t = t_in;
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.0 / 2.0 { return q; }
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    return p;
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let h = hsl.x; let s = hsl.y; let l = hsl.z;
    if s == 0.0 {
        return vec3<f32>(l);
    }
    let q = select(l + s - l * s, l * (1.0 + s), l < 0.5);
    let p = 2.0 * l - q;
    return vec3<f32>(
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    );
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_input, s_input, in.uv);
    var hsl = rgb_to_hsl(c.rgb);

    hsl.x = fract(hsl.x + u.hue / 360.0);
    hsl.y = clamp(hsl.y + u.saturation, 0.0, 1.0);
    hsl.z = clamp(hsl.z + u.lightness,  0.0, 1.0);

    return vec4<f32>(hsl_to_rgb(hsl), c.a);
}
