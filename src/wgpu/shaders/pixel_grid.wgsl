struct Uniforms {
    screen_to_img: mat4x4<f32>,
    viewport: vec4<f32>,
    bounds_img: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

const GRID_STRENGTH: f32 = 0.5;

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
    let vp = uniforms.viewport;
    let ndc = vec2<f32>(
        (in.position.x - vp.x) / vp.z * 2.0 - 1.0,
        1.0 - (in.position.y - vp.y) / vp.w * 2.0,
    );
    let img = (uniforms.screen_to_img * vec4<f32>(ndc, 0.0, 1.0)).xy;

    let b = uniforms.bounds_img;
    if img.x < b.x || img.y < b.y || img.x > b.z || img.y > b.w {
        discard;
    }

    let fw = max(fwidth(img), vec2<f32>(1e-6));
    let d = abs(fract(img - 0.5) - 0.5) / fw;
    let line = 1.0 - min(min(d.x, d.y), 1.0);

    let px_per_img = 1.0 / max(fw.x, fw.y);
    let fade = clamp((px_per_img - 10.0) / 6.0, 0.0, 1.0);

    let coverage = GRID_STRENGTH * line * fade;
    if coverage <= 0.0 {
        discard;
    }
    return vec4<f32>(vec3<f32>(coverage), 2.0 * coverage);
}
