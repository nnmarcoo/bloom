struct ModEntry {
    data: array<vec4<f32>, 3>,
}

struct ModUniforms {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    proc_origin: vec2<f32>,
    proc_size: vec2<f32>,
    src_origin: vec2<f32>,
    src_size: vec2<f32>,
    full_size_px: vec2<f32>,
    _pad3: vec2<f32>,
    entries: array<ModEntry, 32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: ModUniforms;
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

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let max_c = max(max(rgb.r, rgb.g), rgb.b);
    let min_c = min(min(rgb.r, rgb.g), rgb.b);
    let l = (max_c + min_c) * 0.5;
    if max_c == min_c { return vec3<f32>(0.0, 0.0, l); }
    let d = max_c - min_c;
    let s = select(d / (2.0 - max_c - min_c), d / (max_c + min_c), l < 0.5);
    var h: f32;
    if max_c == rgb.r {
        h = (rgb.g - rgb.b) / d + select(6.0, 0.0, rgb.g >= rgb.b);
    } else if max_c == rgb.g {
        h = (rgb.b - rgb.r) / d + 2.0;
    } else {
        h = (rgb.r - rgb.g) / d + 4.0;
    }
    return vec3<f32>(h / 6.0, s, l);
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    var t = t_in;
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 0.5 { return q; }
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    return p;
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    if hsl.y == 0.0 { return vec3<f32>(hsl.z); }
    let q = select(hsl.z + hsl.y - hsl.z * hsl.y, hsl.z * (1.0 + hsl.y), hsl.z < 0.5);
    let p = 2.0 * hsl.z - q;
    return vec3<f32>(
        hue_to_rgb(p, q, hsl.x + 1.0 / 3.0),
        hue_to_rgb(p, q, hsl.x),
        hue_to_rgb(p, q, hsl.x - 1.0 / 3.0),
    );
}

fn hash_u(v: u32) -> u32 {
    var s = v * 747796405u + 2891336453u;
    s = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (s >> 22u) ^ s;
}

fn hash21(ix: i32, iy: i32, seed: i32) -> f32 {
    let h = hash_u(u32(ix) ^ (u32(iy) * 1664525u) ^ (u32(seed) * 22695477u));
    return f32(h) / 4294967295.0;
}

fn grain_value(cx: i32, cy: i32, wx: f32, wy: f32, seed: i32) -> f32 {
    let n00 = hash21(cx,     cy,     seed);
    let n10 = hash21(cx + 1, cy,     seed);
    let n01 = hash21(cx,     cy + 1, seed);
    let n11 = hash21(cx + 1, cy + 1, seed);
    return mix(mix(n00, n10, wx), mix(n01, n11, wx), wy);
}

fn apply_entry(e: ModEntry, full_uv: vec2<f32>, c_in: vec4<f32>) -> vec4<f32> {
    let kind = bitcast<u32>(e.data[0].x);
    let p0 = e.data[0].y;
    let p1 = e.data[0].z;
    let p2 = e.data[0].w;
    let p3 = e.data[1].x;
    let p4 = e.data[1].y;
    let p5 = e.data[1].z;
    let p6 = e.data[1].w;
    let p7 = e.data[2].x;
    let p8 = e.data[2].y;
    var c = c_in;

    switch kind {
        case 1u: {
            c = vec4<f32>(c.rgb * pow(2.0, p0), c.a);
        }
        case 2u: {
            let hi = max(p2, p0 + 0.001);
            let range = hi - p0;
            let leveled = clamp((c.rgb - p0) / range, vec3<f32>(0.0), vec3<f32>(1.0));
            c = vec4<f32>(pow(leveled, vec3<f32>(1.0 / max(p1, 0.001))), c.a);
        }
        case 3u: {
            var rgb = c.rgb + p0;
            rgb = (rgb - 0.5) * (1.0 + p1) + 0.5;
            c = vec4<f32>(rgb, c.a);
        }
        case 4u: {
            var hsl = rgb_to_hsl(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
            hsl.x = fract(hsl.x + p0 / 360.0);
            hsl.y = clamp(hsl.y + p1, 0.0, 1.0);
            hsl.z = clamp(hsl.z + p2, 0.0, 1.0);
            c = vec4<f32>(hsl_to_rgb(hsl), c.a);
        }
        case 5u: {
            let dist = length(full_uv - vec2<f32>(0.5, 0.5)) * 2.0;
            let inner = max(p1 - p2, 0.0);
            let vignette = 1.0 - smoothstep(inner, p1 + 0.0001, dist);
            c = vec4<f32>(c.rgb * mix(1.0 - p0, 1.0, vignette), c.a);
        }
        case 6u: {
            let l = max(p0 - 1.0, 1.0);
            c = vec4<f32>(floor(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * l + 0.5) / l, c.a);
        }
        case 7u: {
            let luma = dot(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
            let v = select(0.0, 1.0, luma >= p0);
            c = vec4<f32>(v, v, v, c.a);
        }
        case 8u: {
            let cin = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
            let luma = dot(cin, vec3<f32>(0.2126, 0.7152, 0.0722));
            let mx = max(max(cin.r, cin.g), cin.b);
            let sat_proxy = mx - min(min(cin.r, cin.g), cin.b);
            let vib_amount = p0 * (1.0 - sat_proxy);
            var rgb = mix(vec3<f32>(luma), cin, 1.0 + vib_amount);
            rgb = mix(vec3<f32>(luma), rgb, 1.0 + p1);
            c = vec4<f32>(rgb, c.a);
        }
        case 9u: {
            c = vec4<f32>(c.r + p0, c.g + p1, c.b + p2, c.a);
        }
        case 10u: {
            let full_px_x = full_uv.x * u.full_size_px.x;
            let full_px_y = full_uv.y * u.full_size_px.y;
            let iseed = i32(p2);
            let sz = max(p1, 0.5);
            let gx = full_px_x / sz;
            let gy = full_px_y / sz;
            let cx = i32(floor(gx));
            let cy = i32(floor(gy));
            let fx = fract(gx);
            let fy = fract(gy);
            let wx = fx * fx * (3.0 - 2.0 * fx);
            let wy = fy * fy * (3.0 - 2.0 * fy);
            let mono = grain_value(cx, cy, wx, wy, iseed);
            let color = clamp(p7, 0.0, 1.0);
            let noise = mix(
                vec3<f32>(mono),
                vec3<f32>(
                    grain_value(cx, cy, wx, wy, iseed + 101),
                    grain_value(cx, cy, wx, wy, iseed + 211),
                    grain_value(cx, cy, wx, wy, iseed + 307),
                ),
                color,
            );
            let luma = dot(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
            let luma_weight = mix(1.0, 4.0 * luma * (1.0 - luma), clamp(p8, 0.0, 1.0));
            let grain = (noise - vec3<f32>(0.5)) * p0 * luma_weight;
            c = vec4<f32>(c.rgb + grain, c.a);
        }
        case 16u: {
            let cs = cos(p1);
            let sn = sin(p1);
            let rot_uv = vec2<f32>(
                full_uv.x * cs - full_uv.y * sn,
                full_uv.x * sn + full_uv.y * cs,
            ) / max(p0, 0.001);
            let cell = floor(rot_uv) + 0.5;
            let dist = length(rot_uv - cell);
            let luma = dot(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
            let radius = sqrt(luma) * 0.5;
            let aa = 1.0 / max(p6, 1.0);
            let dot_val = 1.0 - smoothstep(radius - aa, radius + aa, dist);
            c = vec4<f32>(dot_val, dot_val, dot_val, c.a);
        }
        default: {}
    }

    return c;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let full_uv = u.proc_origin + in.uv * u.proc_size;
    let src_uv = (full_uv - u.src_origin) / u.src_size;
    var c = textureSample(t_image, s_image, src_uv);

    for (var i = 0u; i < u.count; i++) {
        c = apply_entry(u.entries[i], full_uv, c);
    }

    return clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
}
