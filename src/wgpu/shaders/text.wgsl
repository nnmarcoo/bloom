struct TextUniforms {
    anchor: vec2<f32>,       // full-image pixels: where the block pivot lands
    block_size: vec2<f32>,   // display pixels (w, h)
    pivot: vec2<f32>,        // 0..1 within block aligned to anchor (0.5,0.5 = center)
    tile_origin: vec2<f32>,  // this tile's offset in full-image pixels
    tile_size: vec2<f32>,    // this tile's size in full-image pixels
    rotation: f32,
    opacity: f32,
    color: vec3<f32>,
    _pad: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: TextUniforms;
@group(0) @binding(1) var t_glyph: texture_2d<f32>;
@group(0) @binding(2) var s_glyph: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0)
    );
    let corner = corners[vi];

    let local = (corner - u.pivot) * u.block_size;
    let cs = cos(u.rotation);
    let sn = sin(u.rotation);
    let rot = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let world = u.anchor + rot;

    let tile_local = world - u.tile_origin;
    let tuv = tile_local / u.tile_size;
    let ndc = vec2<f32>(tuv.x * 2.0 - 1.0, 1.0 - tuv.y * 2.0);

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let a = textureSample(t_glyph, s_glyph, in.uv).r * u.opacity;
    return vec4<f32>(u.color, a);
}

// Fullscreen passthrough: copy the prior segment's content into the output
// target so the text blend (LoadOp::Load) has the correct background.
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
    return textureSample(t_glyph, s_glyph, in.uv);
}
