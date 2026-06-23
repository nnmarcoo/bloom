struct TextUniforms {
    anchor: vec2<f32>,
    block_size: vec2<f32>,
    pivot: vec2<f32>,
    tile_origin: vec2<f32>,
    tile_size: vec2<f32>,
    block_min: vec2<f32>,
    rotation: f32,
    opacity: f32,
    px_range: f32,
    _pad0: f32,
    color: vec4<f32>,
}

struct Instance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: TextUniforms;
@group(0) @binding(1) var t_atlas: texture_2d<f32>;
@group(0) @binding(2) var s_atlas: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VertexOutput {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0)
    );
    let corner = corners[vi];

    let glyph_px = inst.rect.xy + corner * inst.rect.zw;
    let local = (glyph_px - u.block_min) - u.pivot * u.block_size;

    let cs = cos(u.rotation);
    let sn = sin(u.rotation);
    let rot = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let world = u.anchor + rot;

    let tile_local = world - u.tile_origin;
    let tuv = tile_local / u.tile_size;
    let ndc = vec2<f32>(tuv.x * 2.0 - 1.0, 1.0 - tuv.y * 2.0);

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = inst.uv.xy + corner * inst.uv.zw;
    return out;
}

fn median3(a: f32, b: f32, c: f32) -> f32 {
    return max(min(a, b), min(max(a, b), c));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(t_atlas, s_atlas, in.uv);
    let d = median3(s.r, s.g, s.b);
    let screen_px = max(fwidth(d), 1e-5);
    let coverage = clamp((d - 0.5) / screen_px + 0.5, 0.0, 1.0);
    return vec4<f32>(u.color.rgb, coverage * u.opacity);
}

struct CopyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_copy(@builtin(vertex_index) vi: u32) -> CopyOut {
    var quad = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0)
    );
    var out: CopyOut;
    out.position = vec4<f32>(quad[vi], 0.0, 1.0);
    out.uv = vec2<f32>((quad[vi].x + 1.0) * 0.5, 1.0 - (quad[vi].y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_copy(in: CopyOut) -> @location(0) vec4<f32> {
    return textureSample(t_atlas, s_atlas, in.uv);
}
