struct ModEntry {
    data: array<vec4<f32>, 3>,
}

struct ModUniforms {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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

// PCG-based hash — no visible period at any scale
fn hash_u(v: u32) -> u32 {
    var s = v * 747796405u + 2891336453u;
    s = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (s >> 22u) ^ s;
}

fn hash21(ix: i32, iy: i32, seed: i32) -> f32 {
    let h = hash_u(u32(ix) ^ (u32(iy) * 1664525u) ^ (u32(seed) * 22695477u));
    return f32(h) / 4294967295.0;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(t_image, s_image, in.uv);

    for (var i = 0u; i < u.count; i++) {
        let e = u.entries[i];
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
        let p9 = e.data[2].z;
        let p10 = e.data[2].w;

        switch kind {
            case 1u: { // Exposure
                c = vec4<f32>(c.rgb * pow(2.0, p0), c.a);
                c = clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
            }
            case 2u: { // Levels
                let hi = max(p2, p0 + 0.001);
                let range = hi - p0;
                let leveled = clamp((c.rgb - p0) / range, vec3<f32>(0.0), vec3<f32>(1.0));
                c = vec4<f32>(pow(leveled, vec3<f32>(1.0 / max(p1, 0.001))), c.a);
            }
            case 3u: { // BrightnessContrast
                var rgb = c.rgb + p0;
                rgb = (rgb - 0.5) * (1.0 + p1) + 0.5;
                if p2 != 0.0 {
                    let dx = in.uv.x - (p3 + p5 * 0.5);
                    let dy = in.uv.y - (p4 + p6 * 0.5);
                    let nx = dx / (p5 * 0.5 + 0.0001);
                    let ny = dy / (p6 * 0.5 + 0.0001);
                    let dist = sqrt(nx * nx + ny * ny);
                    let feather = max(p7, 0.001);
                    let weight = 1.0 - smoothstep(1.0 - feather, 1.0, dist);
                    rgb = mix(c.rgb, rgb, weight);
                }
                c = clamp(vec4<f32>(rgb, c.a), vec4<f32>(0.0), vec4<f32>(1.0));
            }
            case 4u: { // HueSaturation
                var hsl = rgb_to_hsl(c.rgb);
                hsl.x = fract(hsl.x + p0 / 360.0);
                hsl.y = clamp(hsl.y + p1, 0.0, 1.0);
                hsl.z = clamp(hsl.z + p2, 0.0, 1.0);
                c = vec4<f32>(hsl_to_rgb(hsl), c.a);
            }
            case 5u: { // Vignette
                let full_uv = in.uv * vec2<f32>(p5, p6) + vec2<f32>(p3, p4);
                let dist = length(full_uv - vec2<f32>(0.5, 0.5)) * 2.0;
                let inner = max(p1 - p2, 0.0);
                let vignette = 1.0 - smoothstep(inner, p1 + 0.0001, dist);
                c = vec4<f32>(c.rgb * mix(1.0 - p0, 1.0, vignette), c.a);
            }
            case 6u: { // Posterize
                let l = max(p0 - 1.0, 1.0);
                c = vec4<f32>(floor(c.rgb * l + 0.5) / l, c.a);
            }
            case 7u: { // Threshold
                let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
                let v = select(0.0, 1.0, luma >= p0);
                c = vec4<f32>(v, v, v, c.a);
            }
            case 8u: { // Vibrance
                // p0 = vibrance, p1 = saturation (additive)
                let avg = (c.r + c.g + c.b) / 3.0;
                let mx = max(max(c.r, c.g), c.b);
                let sat_proxy = mx - min(min(c.r, c.g), c.b);
                // Vibrance boosts low-saturation pixels more
                let vib_amount = p0 * (1.0 - sat_proxy);
                var rgb = mix(vec3<f32>(avg), c.rgb, 1.0 + vib_amount);
                // Additive saturation on top
                let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
                rgb = mix(vec3<f32>(luma), rgb, 1.0 + p1);
                c = clamp(vec4<f32>(rgb, c.a), vec4<f32>(0.0), vec4<f32>(1.0));
            }
            case 9u: { // ColorBalance
                // p0 = cyan_red, p1 = magenta_green, p2 = yellow_blue
                c = clamp(vec4<f32>(c.r + p0, c.g + p1, c.b + p2, c.a), vec4<f32>(0.0), vec4<f32>(1.0));
            }
            case 10u: { // Grain
                // p0=amount, p1=size(px), p2=roughness(0=smooth,1=sharp), p3=seed
                // p4=tile_x, p5=tile_y, p6=tile_w, p7=tile_h
                let full_px_x = p4 + in.uv.x * p6;
                let full_px_y = p5 + in.uv.y * p7;
                let iseed = i32(p3);
                let sz = max(p1, 0.5);
                // Sub-cell position within one grain block
                let gx = full_px_x / sz;
                let gy = full_px_y / sz;
                let cx = floor(gx);
                let cy = floor(gy);
                let fx = fract(gx);
                let fy = fract(gy);
                // Hash the four surrounding cells
                let n00 = hash21(i32(cx),     i32(cy),     iseed);
                let n10 = hash21(i32(cx) + 1, i32(cy),     iseed);
                let n01 = hash21(i32(cx),     i32(cy) + 1, iseed);
                let n11 = hash21(i32(cx) + 1, i32(cy) + 1, iseed);
                // Roughness blends interpolation weight: 0=smooth cubic, 1=nearest (hard cell)
                let t = clamp(p2, 0.0, 1.0);
                let wx = mix(fx * fx * (3.0 - 2.0 * fx), step(0.5, fx), t);
                let wy = mix(fy * fy * (3.0 - 2.0 * fy), step(0.5, fy), t);
                let noise = mix(mix(n00, n10, wx), mix(n01, n11, wx), wy);
                let grain = (noise - 0.5) * p0;
                c = clamp(vec4<f32>(c.rgb + grain, c.a), vec4<f32>(0.0), vec4<f32>(1.0));
            }
            case 11u: { // ChromaticAberration
                // p0 = amount (pixels, normalised as fraction of width), p1 = angle (radians)
                let offset = vec2<f32>(cos(p1), sin(p1)) * p0;
                let r = textureSample(t_image, s_image, clamp(in.uv + offset, vec2<f32>(0.0), vec2<f32>(1.0))).r;
                let b = textureSample(t_image, s_image, clamp(in.uv - offset, vec2<f32>(0.0), vec2<f32>(1.0))).b;
                c = vec4<f32>(r, c.g, b, c.a);
            }
            case 16u: { // Halftone
                // p0 = size (dot grid period in UV space), p1 = angle (radians)
                // p2 = tile_x, p3 = tile_y, p4 = tile_w, p5 = tile_h
                let full_uv = in.uv * vec2<f32>(p4, p5) + vec2<f32>(p2, p3);
                let cs = cos(p1);
                let sn = sin(p1);
                // Rotate full_uv into grid space
                let rot_uv = vec2<f32>(
                    full_uv.x * cs - full_uv.y * sn,
                    full_uv.x * sn + full_uv.y * cs,
                ) / max(p0, 0.001);
                // Distance from nearest grid center in rotated space
                let cell = floor(rot_uv) + 0.5;
                let dist = length(rot_uv - cell);
                // Luma of original pixel drives dot radius (0..0.5)
                let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
                let radius = sqrt(luma) * 0.5;
                let dot_val = select(0.0, 1.0, dist < radius);
                c = vec4<f32>(dot_val, dot_val, dot_val, c.a);
            }
            default: {}
        }
    }

    return clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
}
