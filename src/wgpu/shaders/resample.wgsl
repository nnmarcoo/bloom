// Separable resample, one axis per invocation.
//
// Mirrors `cpu::resample`. Three details must match it exactly or the two
// backends drift in ways that are invisible until you look closely:
//
//   1. Weights are normalized per output pixel, by the sum of the taps that
//      actually contributed. At the borders the tap range clamps to the source,
//      so fewer taps land and the sum differs from the interior. Dividing by a
//      constant instead shifts brightness along the edges.
//   2. When downscaling, the kernel widens by 1/scale (`inv`), and the weight
//      argument is divided by the same factor. Without it a large reduction
//      aliases, which is the whole reason to use a wide filter.
//   3. It is separable: horizontal then vertical, as two passes. A single 2D
//      gather over the same radius is a different filter, not just a slower one.

struct ResampleUniforms {
    // x: output length on the resampled axis, y: source length on that axis.
    // z: 0 for horizontal, 1 for vertical. w: filter (0 near, 1 bilin, 2 lanc).
    axis: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: ResampleUniforms;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_src: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var quad = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0)
    );
    let p = quad[vi];
    var out: VertexOutput;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return out;
}

fn lanczos3(x_in: f32) -> f32 {
    let a = 3.0;
    let x = abs(x_in);
    if (x < 1e-6) {
        return 1.0;
    }
    if (x >= a) {
        return 0.0;
    }
    let px = 3.14159265359 * x;
    return (sin(px) / px) * (sin(px / a) / (px / a));
}

fn weight_of(kind: f32, x: f32) -> f32 {
    if (kind < 0.5) {
        return 1.0;
    }
    if (kind < 1.5) {
        return max(1.0 - abs(x), 0.0);
    }
    return lanczos3(x);
}

fn radius_of(kind: f32) -> f32 {
    if (kind < 0.5) {
        return 0.0;
    }
    if (kind < 1.5) {
        return 1.0;
    }
    return 3.0;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let out_len = u.axis.x;
    let src_len = u.axis.y;
    let vertical = u.axis.z > 0.5;
    let kind = u.axis.w;

    // Index of this fragment along the axis being resampled, and the position
    // on the other axis which passes through untouched.
    var out_idx: f32;
    var other_uv: f32;
    if (vertical) {
        out_idx = floor(in.uv.y * out_len);
        other_uv = in.uv.x;
    } else {
        out_idx = floor(in.uv.x * out_len);
        other_uv = in.uv.y;
    }

    let scale = out_len / src_len;
    var inv = 1.0;
    if (scale < 1.0) {
        inv = 1.0 / scale;
    }
    let center = (out_idx + 0.5) / scale;

    if (kind < 0.5) {
        let s = clamp(floor(center), 0.0, src_len - 1.0);
        let t = (s + 0.5) / src_len;
        var uv = vec2<f32>(t, other_uv);
        if (vertical) {
            uv = vec2<f32>(other_uv, t);
        }
        return textureSample(t_src, s_src, uv);
    }

    let r = radius_of(kind) * inv;
    let lo = max(floor(center - r), 0.0);
    let hi = max(min(ceil(center + r), src_len), lo + 1.0);

    var acc = vec4<f32>(0.0);
    var sum = 0.0;
    var s = lo;
    loop {
        if (s >= hi) {
            break;
        }
        let w = weight_of(kind, (s + 0.5 - center) / inv);
        let t = (s + 0.5) / src_len;
        var uv = vec2<f32>(t, other_uv);
        if (vertical) {
            uv = vec2<f32>(other_uv, t);
        }
        acc = acc + w * textureSample(t_src, s_src, uv);
        sum = sum + w;
        s = s + 1.0;
    }

    // Per-pixel normalization, matching the CPU's `norm`. The guard mirrors its
    // `sum.abs() < 1e-6` case, which arises when every tap weight cancels.
    if (abs(sum) < 1e-6) {
        return acc;
    }
    return acc / sum;
}
