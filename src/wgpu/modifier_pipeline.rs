use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, BlendState,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::{
    modifiers::{
        Axis, EffectClass, Modifier, ModifierKind,
        gpu::{ModUniforms, TileInfo, build_segment_uniforms},
    },
    wgpu::{
        gpu,
        passes::{
            chromatic_aberration::ChromaticAberrationPass,
            gaussian_blur::{GaussianBlurPass, TileRect},
            pixel_sort::PixelSortCompute,
            text::{TextLayer, TextPass},
        },
        tiled_source::TiledSource,
        view_pipeline::tile_ndc_culled,
    },
};

struct CombinedPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
}

impl CombinedPass {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("combined-modifiers-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("shaders/combined_modifiers.wgsl"),
            Some("combined-modifiers-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        Self { pipeline, bgl }
    }

    fn run(&self, encoder: &mut CommandEncoder, bind_group: &BindGroup, dst: &TextureView) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("combined-modifiers-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: dst,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(iced::wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

struct TileOutput {
    _tex: Texture,
    view: TextureView,
    valid: bool,
    width: u32,
    height: u32,
    proc_px: Option<[f32; 4]>,
    proc_scale: f32,
    band_y: u32,
}

struct ScratchTarget {
    _tex: Texture,
    view: TextureView,
    width: u32,
    height: u32,
}

impl ScratchTarget {
    fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        let tex = gpu::texture_2d(
            device,
            width,
            height,
            format,
            TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST,
            Some("modifier-scratch"),
        );
        let view = tex.create_view(&Default::default());
        Self {
            _tex: tex,
            view,
            width,
            height,
        }
    }
}

enum PlanItem<'a> {
    Fused(Vec<&'a Modifier>),
    Step(usize, &'a Modifier),
}

#[derive(Clone, Copy)]
enum SortTarget {
    ScratchA,
    ScratchB,
    Output,
}

const TILE_BUDGET: usize = 2;

struct Scheduler {
    budget: usize,
    deferred: bool,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            budget: TILE_BUDGET,
            deferred: false,
        }
    }

    fn admit(&mut self) -> bool {
        if self.budget == 0 {
            self.deferred = true;
            false
        } else {
            self.budget -= 1;
            true
        }
    }

    fn pending(&self) -> bool {
        self.deferred
    }
}

#[derive(Default)]
struct StepPools {
    fused: usize,
    ca: usize,
    text: usize,
}

const ROI_MARGIN_PX: f32 = 256.0;

const PIXEL_SORT_BUF_BUDGET: u64 = 64 * 1024 * 1024;
const PIXEL_SORT_LINES_PER_BAND: u32 = 64;
const PIXEL_SORT_BANDS_PER_FRAME: u32 = 4;
const PROCESS_VRAM_BUDGET_MIN: u64 = 512 * 1024 * 1024;
const PROCESS_VRAM_BUDGET_MAX: u64 = 4 * 1024 * 1024 * 1024;

fn process_vram_budget(device: &Device) -> u64 {
    device
        .limits()
        .max_buffer_size
        .clamp(PROCESS_VRAM_BUDGET_MIN, PROCESS_VRAM_BUDGET_MAX)
}

fn fit_process_scale(
    unit_w: u32,
    unit_h: u32,
    n_units: u64,
    banks: u64,
    budget: u64,
    base: f32,
) -> f32 {
    let mut scale = base.clamp(1.0 / 4096.0, 1.0).log2().floor().exp2();
    loop {
        let w = ((unit_w as f32 * scale).round() as u64).max(1);
        let h = ((unit_h as f32 * scale).round() as u64).max(1);
        let total = w * h * 4 * banks * n_units.max(1);
        if total <= budget || scale <= 1.0 / 4096.0 {
            break;
        }
        scale *= 0.5;
    }
    scale
}

fn scaled(coord: u32, scale: f32) -> u32 {
    (coord as f32 * scale).round() as u32
}

const BLUR_WORK_BUDGET: u32 = 24_000_000;
const BLUR_MIN_BAND_H: u32 = 8;
const BLUR_MAX_BAND_H: u32 = 1024;

#[derive(Default, Clone, Copy)]
struct Neighbors {
    left: Option<usize>,
    right: Option<usize>,
    up: Option<usize>,
    down: Option<usize>,
}

use crate::modifiers::gpu::UvRect;
use crate::modifiers::pixel_sort::SortAxis;

struct ProcRect {
    px: [f32; 4],
    proc: UvRect,
    src: UvRect,
    w: u32,
    h: u32,
}

fn tile_proc_rect(
    tile: &crate::wgpu::tiled_source::Tile,
    full_w: f32,
    full_h: f32,
    proc_scale: f32,
    downscale: bool,
    apron_px: f32,
    roi_enabled: bool,
) -> ProcRect {
    let fw = tile.x as f32 + tile.width as f32;
    let fh = tile.y as f32 + tile.height as f32;
    let tl = tile.x as f32;
    let tt = tile.y as f32;

    let margin_px = if downscale { 0.0 } else { ROI_MARGIN_PX };
    let roi = if roi_enabled { tile.proc_rect_px } else { None };
    let px = match roi {
        Some([l, t, r, b]) => {
            let grow = apron_px + margin_px;
            [
                (l - grow).floor().max(tl),
                (t - grow).floor().max(tt),
                (r + grow).ceil().min(fw),
                (b + grow).ceil().min(fh),
            ]
        }
        None => [tl, tt, fw, fh],
    };

    let pw_px = (px[2] - px[0]).max(1.0);
    let ph_px = (px[3] - px[1]).max(1.0);
    let scale = if downscale { proc_scale } else { 1.0 };
    let w = ((pw_px * scale).ceil() as u32).max(1);
    let h = ((ph_px * scale).ceil() as u32).max(1);

    let proc = UvRect {
        origin: [px[0] / full_w, px[1] / full_h],
        size: [(px[2] - px[0]) / full_w, (px[3] - px[1]) / full_h],
    };
    let src = UvRect {
        origin: [tl / full_w, tt / full_h],
        size: [tile.width as f32 / full_w, tile.height as f32 / full_h],
    };
    ProcRect {
        px,
        proc,
        src,
        w,
        h,
    }
}

fn inscribe_transform(base: glam::Mat4, isec: [f32; 4], sub: [f32; 4]) -> glam::Mat4 {
    let [il, it, ir, ib] = isec;
    let iw = (ir - il).max(1e-6);
    let ih = (ib - it).max(1e-6);
    let qx = |x: f32| -1.0 + 2.0 * (x - il) / iw;
    let qy = |y: f32| 1.0 - 2.0 * (y - it) / ih;
    let qx0 = qx(sub[0]);
    let qx1 = qx(sub[2]);
    let qy_top = qy(sub[1]);
    let qy_bot = qy(sub[3]);
    let cx = (qx0 + qx1) * 0.5;
    let cy = (qy_top + qy_bot) * 0.5;
    let hx = (qx1 - qx0) * 0.5;
    let hy = (qy_top - qy_bot) * 0.5;
    base * glam::Mat4::from_translation(glam::vec3(cx, cy, 0.0))
        * glam::Mat4::from_scale(glam::vec3(hx, hy, 1.0))
}

fn rect_contains(outer: [f32; 4], inner: [f32; 4]) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[2] <= outer[2] + 0.5
        && inner[3] <= outer[3] + 0.5
}

fn proc_rect_from_px(
    proc_px: Option<[f32; 4]>,
    tile: &crate::wgpu::tiled_source::Tile,
    full_w: f32,
    full_h: f32,
    w: u32,
    h: u32,
) -> ProcRect {
    let px = proc_px.unwrap_or([
        tile.x as f32,
        tile.y as f32,
        tile.x as f32 + tile.width as f32,
        tile.y as f32 + tile.height as f32,
    ]);
    let proc = UvRect {
        origin: [px[0] / full_w, px[1] / full_h],
        size: [(px[2] - px[0]) / full_w, (px[3] - px[1]) / full_h],
    };
    let src = UvRect {
        origin: [tile.x as f32 / full_w, tile.y as f32 / full_h],
        size: [tile.width as f32 / full_w, tile.height as f32 / full_h],
    };
    ProcRect {
        px,
        proc,
        src,
        w,
        h,
    }
}

fn full_rect(tile: &TileInfo) -> TileRect {
    TileRect {
        origin: [
            tile.tile_x as f32 / tile.full_w as f32,
            tile.tile_y as f32 / tile.full_h as f32,
        ],
        size: [
            tile.tile_w as f32 / tile.full_w as f32,
            tile.tile_h as f32 / tile.full_h as f32,
        ],
    }
}

fn tile_neighbors(tiles: &[crate::wgpu::tiled_source::Tile], ti: usize) -> Neighbors {
    let t = &tiles[ti];
    let mut n = Neighbors::default();
    for (j, o) in tiles.iter().enumerate() {
        if j == ti {
            continue;
        }
        if o.y == t.y && o.x + o.width == t.x {
            n.left = Some(j);
        } else if o.y == t.y && t.x + t.width == o.x {
            n.right = Some(j);
        } else if o.x == t.x && o.y + o.height == t.y {
            n.up = Some(j);
        } else if o.x == t.x && t.y + t.height == o.y {
            n.down = Some(j);
        }
    }
    n
}

fn tex_copy_info(
    tex: &Texture,
    origin: iced::wgpu::Origin3d,
) -> iced::wgpu::TexelCopyTextureInfo<'_> {
    iced::wgpu::TexelCopyTextureInfo {
        texture: tex,
        mip_level: 0,
        origin,
        aspect: iced::wgpu::TextureAspect::All,
    }
}

fn pixel_sort_groups(source: &TiledSource, proc_set: &[usize], vertical: bool) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut keys: Vec<u32> = Vec::new();
    for &ti in proc_set {
        let key = if vertical {
            source.tiles[ti].x
        } else {
            source.tiles[ti].y
        };
        let gi = match keys.iter().position(|&k| k == key) {
            Some(i) => i,
            None => {
                keys.push(key);
                groups.push(Vec::new());
                groups.len() - 1
            }
        };
        groups[gi].push(ti);
    }
    for g in &mut groups {
        if vertical {
            g.sort_by_key(|&ti| source.tiles[ti].y);
        } else {
            g.sort_by_key(|&ti| source.tiles[ti].x);
        }
    }
    groups
}

fn plan_modifiers(modifiers: &[Modifier]) -> Vec<PlanItem<'_>> {
    let mut plan: Vec<PlanItem> = Vec::new();
    let mut current: Vec<&Modifier> = Vec::new();
    for (i, m) in modifiers.iter().enumerate() {
        if !m.has_visible_effect() {
            continue;
        }
        if !m.kind.effect_class().is_pointwise() {
            if !current.is_empty() {
                plan.push(PlanItem::Fused(std::mem::take(&mut current)));
            }
            plan.push(PlanItem::Step(i, m));
        } else {
            current.push(m);
        }
    }
    if !current.is_empty() {
        plan.push(PlanItem::Fused(current));
    }
    plan
}

pub struct ModifierPipeline {
    tile_outputs: Vec<Option<TileOutput>>,
    tile_display_bgs_linear: Vec<Option<BindGroup>>,
    tile_display_bgs_nearest: Vec<Option<BindGroup>>,

    scratch_a: Option<ScratchTarget>,
    scratch_b: Option<ScratchTarget>,

    scratch_blur: Option<ScratchTarget>,
    bank_a: Vec<Option<ScratchTarget>>,
    bank_b: Vec<Option<ScratchTarget>>,
    blur_hmid: Vec<Option<ScratchTarget>>,
    blur_vstrip_top: Vec<Option<ScratchTarget>>,
    blur_vstrip_bot: Vec<Option<ScratchTarget>>,
    roi_display_uniforms: Vec<Option<iced::wgpu::Buffer>>,
    reprocess_pending: bool,

    uniform_pool: Vec<iced::wgpu::Buffer>,
    ca_uniform_pool: Vec<iced::wgpu::Buffer>,
    blur_uniform_pool: Vec<iced::wgpu::Buffer>,
    text_uniform_pool: Vec<iced::wgpu::Buffer>,
    pixel_sort_uniform_pool: Vec<iced::wgpu::Buffer>,
    sort_buffers: Option<(iced::wgpu::Buffer, iced::wgpu::Buffer)>,
    sort_band_cursor: u32,
    sort_progress_sig: u64,
    text_layers: Vec<Option<TextLayer>>,
    text_sigs: Vec<Option<u64>>,
    combined: CombinedPass,
    chromatic_aberration: ChromaticAberrationPass,
    gaussian_blur: GaussianBlurPass,
    pixel_sort: PixelSortCompute,
    text: TextPass,
    display_bgl: BindGroupLayout,
    trilinear_sampler: Sampler,
    linear_sampler: Sampler,
    nearest_sampler: Sampler,

    format: TextureFormat,
    pub width: u32,
    pub height: u32,
}

impl ModifierPipeline {
    pub fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        let display_bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("modifier-display-bgl"),
        );

        let trilinear_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-trilinear-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            mipmap_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let linear_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-linear-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("modifier-nearest-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Nearest,
            min_filter: iced::wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            tile_outputs: Vec::new(),
            tile_display_bgs_linear: Vec::new(),
            tile_display_bgs_nearest: Vec::new(),
            scratch_a: None,
            scratch_b: None,
            scratch_blur: None,
            bank_a: Vec::new(),
            bank_b: Vec::new(),
            blur_hmid: Vec::new(),
            blur_vstrip_top: Vec::new(),
            blur_vstrip_bot: Vec::new(),
            roi_display_uniforms: Vec::new(),
            reprocess_pending: false,
            uniform_pool: Vec::new(),
            ca_uniform_pool: Vec::new(),
            blur_uniform_pool: Vec::new(),
            text_uniform_pool: Vec::new(),
            pixel_sort_uniform_pool: Vec::new(),
            sort_buffers: None,
            sort_band_cursor: 0,
            sort_progress_sig: 0,
            text_layers: Vec::new(),
            text_sigs: Vec::new(),
            combined: CombinedPass::new(device, format),
            chromatic_aberration: ChromaticAberrationPass::new(device, format),
            gaussian_blur: GaussianBlurPass::new(device, format),
            pixel_sort: PixelSortCompute::new(device),
            text: TextPass::new(device, format),
            display_bgl,
            trilinear_sampler,
            linear_sampler,
            nearest_sampler,
            format,
            width,
            height,
        }
    }

    pub fn reprocess_pending(&self) -> bool {
        self.reprocess_pending
    }

    pub fn tile_display_bg(&self, i: usize, nearest: bool) -> Option<&BindGroup> {
        if nearest {
            self.tile_display_bgs_nearest.get(i)?.as_ref()
        } else {
            self.tile_display_bgs_linear.get(i)?.as_ref()
        }
    }

    fn ensure_scratch(&mut self, device: &Device, w: u32, h: u32) {
        let stale =
            |s: &Option<ScratchTarget>| s.as_ref().is_none_or(|t| t.width != w || t.height != h);
        if stale(&self.scratch_a) {
            self.scratch_a = Some(ScratchTarget::new(device, self.format, w, h));
        }
        if stale(&self.scratch_b) {
            self.scratch_b = Some(ScratchTarget::new(device, self.format, w, h));
        }
        if stale(&self.scratch_blur) {
            self.scratch_blur = Some(ScratchTarget::new(device, self.format, w, h));
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        modifiers: &[Modifier],
        dirty: bool,
    ) {
        let n_tiles = source.tiles.len();

        self.reprocess_pending = false;

        self.tile_outputs.resize_with(n_tiles, || None);
        self.tile_display_bgs_linear.resize_with(n_tiles, || None);
        self.tile_display_bgs_nearest.resize_with(n_tiles, || None);
        self.roi_display_uniforms.resize_with(n_tiles, || None);

        if dirty {
            for o in self.tile_outputs.iter_mut().flatten() {
                o.valid = false;
            }
        }

        let physical_scale = source.physical_scale;
        let proc_scale = if physical_scale > 0.0 {
            physical_scale.log2().ceil().exp2().min(1.0)
        } else {
            1.0
        };
        let downscale = proc_scale < 1.0;

        if self.text_layers.len() != modifiers.len() {
            self.text_layers.clear();
            self.text_layers.resize_with(modifiers.len(), || None);
            self.text_sigs.clear();
            self.text_sigs.resize(modifiers.len(), None);
        }

        let mut raster_changed = false;
        for (i, m) in modifiers.iter().enumerate() {
            let sig = if m.has_visible_effect()
                && let ModifierKind::Text(t) = &m.kind
            {
                Some(t.raster_hash())
            } else {
                None
            };

            let unchanged =
                self.text_sigs[i] == sig && self.text_layers[i].is_some() == sig.is_some();
            if unchanged {
                if let (Some(layer), ModifierKind::Text(t)) = (&mut self.text_layers[i], &m.kind) {
                    layer.refresh_transform(t);
                }
                continue;
            }

            self.text_layers[i] = match (sig, &m.kind) {
                (Some(_), ModifierKind::Text(t)) => self.text.build_layer(device, queue, t),
                _ => None,
            };
            self.text_sigs[i] = sig;
            raster_changed = true;
        }

        if raster_changed && !dirty {
            for o in self.tile_outputs.iter_mut().flatten() {
                o.valid = false;
            }
        }

        let plan_vec = plan_modifiers(modifiers);
        let has_blur = plan_vec
            .iter()
            .any(|p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some()));
        let has_pixel_sort = plan_vec.iter().any(
            |p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline()),
        );

        if n_tiles > 1 && (has_blur || has_pixel_sort) {
            self.prepare_tiled(
                device,
                queue,
                source,
                &plan_vec,
                proc_scale,
                downscale,
                dirty,
                has_pixel_sort,
            );
            return;
        }

        let roi_ok = plan_vec.iter().all(|p| matches!(p, PlanItem::Fused(_)));

        let mut plan: Option<Vec<PlanItem>> = Some(plan_vec);
        let mut encoder: Option<CommandEncoder> = None;
        let mut pool_used = 0usize;
        let mut ca_pool_used = 0usize;
        let mut blur_pool_used = 0usize;
        let mut text_pool_used = 0usize;
        let mut scheduler = Scheduler::new();

        for ti in 0..n_tiles {
            let tile = &source.tiles[ti];

            if tile_ndc_culled(tile.last_ndc_rect) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }

            let cur_scale = if downscale { proc_scale } else { 1.0 };
            let visible_roi = if roi_ok { tile.proc_rect_px } else { None };
            let reuse = match (self.tile_outputs[ti].as_ref(), visible_roi) {
                (Some(o), Some(roi)) => {
                    o.proc_px.is_some_and(|p| rect_contains(p, roi))
                        && (o.proc_scale - cur_scale).abs() < 1e-4
                }
                (Some(o), None) => o.proc_px.is_none() && (o.proc_scale - cur_scale).abs() < 1e-4,
                _ => false,
            };

            let pr = if reuse {
                let o = self.tile_outputs[ti].as_ref().unwrap();
                proc_rect_from_px(
                    o.proc_px,
                    tile,
                    source.full_width as f32,
                    source.full_height as f32,
                    o.width,
                    o.height,
                )
            } else {
                tile_proc_rect(
                    tile,
                    source.full_width as f32,
                    source.full_height as f32,
                    proc_scale,
                    downscale,
                    0.0,
                    roi_ok,
                )
            };
            let (eff_w, eff_h) = (pr.w, pr.h);

            if !reuse {
                let tex = gpu::texture_2d(
                    device,
                    eff_w,
                    eff_h,
                    self.format,
                    TextureUsages::RENDER_ATTACHMENT
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC
                        | TextureUsages::COPY_DST,
                    Some(&format!("modifier-tile{ti}-output")),
                );
                let view = tex.create_view(&Default::default());
                self.tile_outputs[ti] = Some(TileOutput {
                    _tex: tex,
                    view,
                    valid: false,
                    width: eff_w,
                    height: eff_h,
                    proc_px: if roi_ok { Some(pr.px) } else { None },
                    proc_scale: cur_scale,
                    band_y: 0,
                });
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }

            let needs_reprocess = !self.tile_outputs[ti].as_ref().unwrap().valid;
            let roi_active = roi_ok && tile.isec_px.is_some();
            let needs_bg_rebuild =
                self.tile_display_bgs_linear[ti].is_none() || needs_reprocess || roi_active;

            if !needs_bg_rebuild {
                continue;
            }

            if needs_reprocess && !scheduler.admit() {
                continue;
            }

            if needs_reprocess {
                let tile_info = TileInfo {
                    tile_x: tile.x,
                    tile_y: tile.y,
                    tile_w: tile.width,
                    tile_h: tile.height,
                    full_w: source.full_width,
                    full_h: source.full_height,
                };

                let plan = plan.get_or_insert_with(|| plan_modifiers(modifiers));
                let n_items = plan.len();
                let plan_has_blur = plan.iter().any(|p| {
                    matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some())
                });
                if n_items > 1 || plan_has_blur {
                    self.ensure_scratch(device, eff_w, eff_h);
                }

                let mut prev: TextureView = tile.source_view.clone();
                for (k, item) in plan.iter().enumerate() {
                    let out: TextureView = if k == n_items - 1 {
                        self.tile_outputs[ti].as_ref().unwrap().view.clone()
                    } else if k % 2 == 0 {
                        self.scratch_a.as_ref().unwrap().view.clone()
                    } else {
                        self.scratch_b.as_ref().unwrap().view.clone()
                    };
                    let out = &out;

                    let src_rect = if k == 0 { pr.src } else { pr.proc };

                    match item {
                        PlanItem::Fused(seg) => {
                            let uniforms =
                                build_segment_uniforms(seg, &tile_info, pr.proc, src_rect);
                            if pool_used == self.uniform_pool.len() {
                                self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                                    device,
                                    Some("combined-modifiers-uniform"),
                                ));
                            }
                            let buffer = &self.uniform_pool[pool_used];
                            pool_used += 1;
                            gpu::write_uniform(queue, buffer, &uniforms);
                            let bg = gpu::standard_bind_group(
                                device,
                                &self.combined.bgl,
                                buffer,
                                &prev,
                                &self.trilinear_sampler,
                                Some("combined-modifiers-bg"),
                            );

                            let enc = encoder.get_or_insert_with(|| {
                                device.create_command_encoder(
                                    &iced::wgpu::CommandEncoderDescriptor {
                                        label: Some("modifier-pipeline-encoder"),
                                    },
                                )
                            });
                            self.combined.run(enc, &bg, out);
                        }
                        PlanItem::Step(idx, m)
                            if m.kind.effect_class().separable_apron().is_some() =>
                        {
                            let radius = m.kind.effect_class().separable_apron().unwrap();
                            {
                                while self.blur_uniform_pool.len() < blur_pool_used + 2 {
                                    self.blur_uniform_pool
                                        .push(self.gaussian_blur.uniform_buffer(device));
                                }
                                let radius_px = radius * eff_w as f32 / tile.width.max(1) as f32;
                                let sigma = (radius_px / 3.0).max(0.5);
                                let step_x = tile.width.max(1) as f32 / eff_w as f32;
                                let step_y = tile.height.max(1) as f32 / eff_h as f32;
                                let mid = &self.scratch_blur.as_ref().unwrap().view;
                                let enc = encoder.get_or_insert_with(|| {
                                    device.create_command_encoder(
                                        &iced::wgpu::CommandEncoderDescriptor {
                                            label: Some("modifier-pipeline-encoder"),
                                        },
                                    )
                                });
                                let (h_pool, v_pool) =
                                    self.blur_uniform_pool.split_at(blur_pool_used + 1);
                                let h_buffer = &h_pool[blur_pool_used];
                                let v_buffer = &v_pool[0];
                                blur_pool_used += 2;
                                let proc = TileRect {
                                    origin: pr.proc.origin,
                                    size: pr.proc.size,
                                };
                                let src0 = TileRect {
                                    origin: pr.src.origin,
                                    size: pr.src.size,
                                };
                                let src_lod = step_x.max(1.0).log2();
                                self.gaussian_blur.record(
                                    device,
                                    queue,
                                    enc,
                                    h_buffer,
                                    [step_x / source.full_width as f32, 0.0],
                                    radius_px,
                                    sigma,
                                    proc,
                                    src0,
                                    None,
                                    None,
                                    &prev,
                                    mid,
                                    None,
                                    src_lod,
                                );
                                self.gaussian_blur.record(
                                    device,
                                    queue,
                                    enc,
                                    v_buffer,
                                    [0.0, step_y / source.full_height as f32],
                                    radius_px,
                                    sigma,
                                    proc,
                                    proc,
                                    None,
                                    None,
                                    mid,
                                    out,
                                    None,
                                    0.0,
                                );
                            }
                        }
                        PlanItem::Step(idx, m) => match &m.kind {
                            ModifierKind::ChromaticAberration(ca) => {
                                let amount = ca.amount;
                                if ca_pool_used == self.ca_uniform_pool.len() {
                                    self.ca_uniform_pool
                                        .push(self.chromatic_aberration.uniform_buffer(device));
                                }
                                let buffer = &self.ca_uniform_pool[ca_pool_used];
                                ca_pool_used += 1;
                                let enc = encoder.get_or_insert_with(|| {
                                    device.create_command_encoder(
                                        &iced::wgpu::CommandEncoderDescriptor {
                                            label: Some("modifier-pipeline-encoder"),
                                        },
                                    )
                                });
                                let src_rect = if k == 0 { pr.src } else { pr.proc };
                                self.chromatic_aberration.record(
                                    device,
                                    queue,
                                    enc,
                                    buffer,
                                    amount,
                                    source.full_width as f32,
                                    pr.proc,
                                    src_rect,
                                    &prev,
                                    out,
                                );
                            }
                            ModifierKind::Text(_) => {
                                if let Some(layer) =
                                    self.text_layers.get(*idx).and_then(|l| l.as_ref())
                                {
                                    if text_pool_used == self.text_uniform_pool.len() {
                                        self.text_uniform_pool
                                            .push(self.text.uniform_buffer(device));
                                    }
                                    let buffer = &self.text_uniform_pool[text_pool_used];
                                    text_pool_used += 1;
                                    let enc = encoder.get_or_insert_with(|| {
                                        device.create_command_encoder(
                                            &iced::wgpu::CommandEncoderDescriptor {
                                                label: Some("modifier-pipeline-encoder"),
                                            },
                                        )
                                    });
                                    let src_rect = if k == 0 { pr.src } else { pr.proc };
                                    self.text.record(
                                        device, queue, enc, buffer, layer, &tile_info, pr.proc,
                                        src_rect, &prev, out,
                                    );
                                }
                            }
                            ModifierKind::PixelSort(ps) => {
                                let (threshold, angle) = (ps.threshold, ps.angle);
                                let src_rect = if k == 0 { pr.src } else { pr.proc };
                                let uniforms =
                                    build_segment_uniforms(&[], &tile_info, pr.proc, src_rect);
                                if pool_used == self.uniform_pool.len() {
                                    self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                                        device,
                                        Some("combined-modifiers-uniform"),
                                    ));
                                }
                                let buffer = &self.uniform_pool[pool_used];
                                pool_used += 1;
                                gpu::write_uniform(queue, buffer, &uniforms);
                                let bg = gpu::standard_bind_group(
                                    device,
                                    &self.combined.bgl,
                                    buffer,
                                    &prev,
                                    &self.trilinear_sampler,
                                    Some("pixel-sort-copy-bg"),
                                );
                                let enc = encoder.get_or_insert_with(|| {
                                    device.create_command_encoder(
                                        &iced::wgpu::CommandEncoderDescriptor {
                                            label: Some("modifier-pipeline-encoder"),
                                        },
                                    )
                                });
                                self.combined.run(enc, &bg, out);
                                if n_tiles == 1 {
                                    if let Some(enc) = encoder.take() {
                                        queue.submit([enc.finish()]);
                                    }
                                    let target = if k == n_items - 1 {
                                        SortTarget::Output
                                    } else if k % 2 == 0 {
                                        SortTarget::ScratchA
                                    } else {
                                        SortTarget::ScratchB
                                    };
                                    self.sort_target(
                                        device, queue, ti, target, eff_w, eff_h, threshold, angle,
                                    );
                                }
                            }
                            _ => {}
                        },
                    }

                    prev = out.clone();
                }

                self.tile_outputs[ti].as_mut().unwrap().valid = true;
            }

            self.build_roi_display_bgs(device, queue, ti, tile, &pr, roi_ok);
        }

        self.reprocess_pending |= scheduler.pending();

        if let Some(encoder) = encoder {
            queue.submit([encoder.finish()]);
        }
    }

    fn ensure_tile_output(
        &mut self,
        device: &Device,
        ti: usize,
        w: u32,
        h: u32,
        proc_px: Option<[f32; 4]>,
    ) {
        let needs_alloc = self.tile_outputs[ti]
            .as_ref()
            .is_none_or(|o| o.width != w || o.height != h);
        if needs_alloc {
            let tex = gpu::texture_2d(
                device,
                w,
                h,
                self.format,
                TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC
                    | TextureUsages::COPY_DST,
                Some(&format!("modifier-tile{ti}-output")),
            );
            let view = tex.create_view(&Default::default());
            self.tile_outputs[ti] = Some(TileOutput {
                _tex: tex,
                view,
                valid: false,
                width: w,
                height: h,
                proc_px,
                proc_scale: 1.0,
                band_y: 0,
            });
            self.tile_display_bgs_linear[ti] = None;
            self.tile_display_bgs_nearest[ti] = None;
        }
    }

    pub fn refresh_display_transforms(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
    ) {
        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;
        for ti in 0..source.tiles.len() {
            let tile = &source.tiles[ti];
            if tile_ndc_culled(tile.last_ndc_rect) {
                continue;
            }
            let Some(o) = self.tile_outputs[ti].as_ref() else {
                continue;
            };
            if !o.valid {
                continue;
            }
            let (proc_px, w, h) = (o.proc_px, o.width, o.height);
            let pr = proc_rect_from_px(proc_px, tile, full_w, full_h, w, h);
            let roi_active = proc_px.is_some() && tile.isec_px.is_some();
            self.build_roi_display_bgs(device, queue, ti, tile, &pr, roi_active);
        }
    }

    fn build_roi_display_bgs(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        tile: &crate::wgpu::tiled_source::Tile,
        pr: &ProcRect,
        roi_active: bool,
    ) {
        let display_uniform: &iced::wgpu::Buffer = if roi_active
            && let (Some(isec), Some(base)) = (tile.isec_px, tile.last_transform)
        {
            let t = inscribe_transform(base, isec, pr.px);
            if self.roi_display_uniforms[ti].is_none() {
                self.roi_display_uniforms[ti] =
                    Some(gpu::uniform_buffer::<
                        crate::wgpu::view_pipeline::DisplayUniforms,
                    >(device, Some("roi-display-uniform")));
            }
            let buf = self.roi_display_uniforms[ti].as_ref().unwrap();
            gpu::write_uniform(
                queue,
                buf,
                &crate::wgpu::view_pipeline::DisplayUniforms {
                    transform: t,
                    crop_uv: [0.0, 0.0, 1.0, 1.0],
                },
            );
            buf
        } else {
            &tile.uniform_buffer
        };

        let output_view = &self.tile_outputs[ti].as_ref().unwrap().view;
        let make_bg = |sampler: &Sampler, label: &str| {
            device.create_bind_group(&BindGroupDescriptor {
                label: Some(label),
                layout: &self.display_bgl,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: display_uniform.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(output_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        self.tile_display_bgs_linear[ti] = Some(make_bg(
            &self.linear_sampler,
            &format!("modifier-tile{ti}-display-linear"),
        ));
        self.tile_display_bgs_nearest[ti] = Some(make_bg(
            &self.nearest_sampler,
            &format!("modifier-tile{ti}-display-nearest"),
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_tiled(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        plan: &[PlanItem],
        proc_scale: f32,
        downscale: bool,
        dirty: bool,
        has_pixel_sort: bool,
    ) {
        if has_pixel_sort {
            self.prepare_tiled_scanlines(device, queue, source, plan, proc_scale, dirty);
            return;
        }

        let n_tiles = source.tiles.len();
        self.bank_a.resize_with(n_tiles, || None);
        self.bank_b.resize_with(n_tiles, || None);
        self.blur_hmid.resize_with(n_tiles, || None);
        self.blur_vstrip_top.resize_with(n_tiles, || None);
        self.blur_vstrip_bot.resize_with(n_tiles, || None);

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let mut apron_px: f32 = 0.0;
        for item in plan {
            if let PlanItem::Step(_, m) = item
                && let Some(radius) = m.kind.effect_class().separable_apron()
            {
                apron_px = apron_px.max(radius);
            }
        }

        let blur_is_sole_step = plan.len() == 1
            && matches!(
                plan.first(),
                Some(PlanItem::Step(_, m)) if m.kind.effect_class().separable_apron().is_some()
            );

        let roi_active = !downscale
            && blur_is_sole_step
            && source
                .tiles
                .iter()
                .any(|t| !tile_ndc_culled(t.last_ndc_rect))
            && source
                .tiles
                .iter()
                .all(|t| tile_ndc_culled(t.last_ndc_rect) || t.proc_rect_px.is_some());

        let (mut proc_scale, mut downscale) = (proc_scale, downscale);
        if !roi_active {
            let mut in_set = vec![false; n_tiles];
            for ti in 0..n_tiles {
                if tile_ndc_culled(source.tiles[ti].last_ndc_rect) {
                    continue;
                }
                in_set[ti] = true;
                let nb = tile_neighbors(&source.tiles, ti);
                for nn in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    in_set[nn] = true;
                }
            }
            let mut n_proc = 0u64;
            let mut tw = 1u32;
            let mut th = 1u32;
            for (ti, &keep) in in_set.iter().enumerate() {
                if keep {
                    n_proc += 1;
                    tw = tw.max(source.tiles[ti].width);
                    th = th.max(source.tiles[ti].height);
                }
            }
            let fit = fit_process_scale(tw, th, n_proc, 3, process_vram_budget(device), proc_scale);
            if fit < proc_scale {
                proc_scale = fit;
                downscale = true;
            }
        }

        let proc_rect_for =
            |ti: usize, reuse: bool, prev_px: Option<[f32; 4]>, prev_wh: (u32, u32)| -> ProcRect {
                if reuse {
                    proc_rect_from_px(
                        prev_px,
                        &source.tiles[ti],
                        full_w,
                        full_h,
                        prev_wh.0,
                        prev_wh.1,
                    )
                } else {
                    tile_proc_rect(
                        &source.tiles[ti],
                        full_w,
                        full_h,
                        proc_scale,
                        downscale,
                        apron_px,
                        roi_active,
                    )
                }
            };

        let cur_scale = if downscale { proc_scale } else { 1.0 };

        let mut proc_rects: Vec<Option<ProcRect>> = (0..n_tiles).map(|_| None).collect();
        let mut stale: Vec<usize> = Vec::new();
        #[allow(clippy::needless_range_loop)]
        for ti in 0..n_tiles {
            let tile = &source.tiles[ti];
            if tile_ndc_culled(tile.last_ndc_rect) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }
            let roi = if roi_active { tile.proc_rect_px } else { None };
            let (prev_px, prev_scale, prev_wh) = self.tile_outputs[ti]
                .as_ref()
                .map(|o| (o.proc_px, o.proc_scale, (o.width, o.height)))
                .unwrap_or((None, 0.0, (0, 0)));
            let same_scale = (prev_scale - cur_scale).abs() < 1e-4;
            let reuse_valid = !dirty
                && match (prev_px, roi) {
                    (Some(p), Some(r)) => rect_contains(p, r) && same_scale,
                    (None, None) => same_scale,
                    _ => false,
                };
            let pr = proc_rect_for(ti, reuse_valid, prev_px, prev_wh);
            self.ensure_tile_output(
                device,
                ti,
                pr.w,
                pr.h,
                if roi_active { Some(pr.px) } else { None },
            );
            if !reuse_valid {
                let o = self.tile_outputs[ti].as_mut().unwrap();
                o.valid = false;
                o.proc_px = if roi_active { Some(pr.px) } else { None };
                o.band_y = 0;
            }
            self.tile_outputs[ti].as_mut().unwrap().proc_scale = cur_scale;

            let needs = !self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid)
                || self.tile_display_bgs_linear[ti].is_none()
                || !reuse_valid
                || (roi_active && tile.isec_px.is_some());
            if needs {
                stale.push(ti);
            }
            proc_rects[ti] = Some(pr);
        }

        if stale.is_empty() {
            return;
        }

        let mut scheduler = Scheduler::new();
        let mut reprocess: Vec<usize> = Vec::new();
        let mut display_set: Vec<usize> = Vec::new();
        for &ti in &stale {
            let needs_reprocess = !self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid);
            if needs_reprocess {
                if !scheduler.admit() {
                    continue;
                }
                reprocess.push(ti);
            }
            display_set.push(ti);
        }
        self.reprocess_pending = scheduler.pending();

        let mut in_process = vec![false; n_tiles];
        let mut blur_set: Vec<usize> = Vec::new();
        for &ti in &reprocess {
            if !in_process[ti] {
                in_process[ti] = true;
                blur_set.push(ti);
            }
            if !roi_active {
                let nb = tile_neighbors(&source.tiles, ti);
                for n in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    if !in_process[n] {
                        in_process[n] = true;
                        blur_set.push(n);
                        if proc_rects[n].is_none() {
                            proc_rects[n] = Some(tile_proc_rect(
                                &source.tiles[n],
                                full_w,
                                full_h,
                                proc_scale,
                                downscale,
                                apron_px,
                                false,
                            ));
                        }
                    }
                }
            }
        }

        let ensure_bank = |bank: &mut Vec<Option<ScratchTarget>>,
                           ti: usize,
                           w: u32,
                           h: u32,
                           fmt: TextureFormat| {
            let stale = bank[ti]
                .as_ref()
                .is_none_or(|t| t.width != w || t.height != h);
            if stale {
                bank[ti] = Some(ScratchTarget::new(device, fmt, w, h));
            }
        };
        for &ti in &blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            self.ensure_tile_output(
                device,
                ti,
                pr.w,
                pr.h,
                if roi_active { Some(pr.px) } else { None },
            );
            ensure_bank(&mut self.bank_a, ti, pr.w, pr.h, self.format);
            ensure_bank(&mut self.bank_b, ti, pr.w, pr.h, self.format);
            ensure_bank(&mut self.blur_hmid, ti, pr.w, pr.h, self.format);
        }

        let plan_steps = plan.len();
        let mut encoder = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("modifier-step-major-encoder"),
        });
        let mut pools = StepPools::default();
        let mut blur_pool_used = 0usize;

        let mut prev_in_a = true;

        for (k, item) in plan.iter().enumerate() {
            let last = k == plan_steps - 1;
            let is_blur = matches!(item, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some());
            if !is_blur {
                for &ti in &blur_set {
                    let tile = &source.tiles[ti];
                    let pr = proc_rects[ti].as_ref().unwrap();
                    let tile_info = TileInfo {
                        tile_x: tile.x,
                        tile_y: tile.y,
                        tile_w: tile.width,
                        tile_h: tile.height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    };

                    let prev: TextureView = if k == 0 {
                        tile.source_view.clone()
                    } else if prev_in_a {
                        self.bank_a[ti].as_ref().unwrap().view.clone()
                    } else {
                        self.bank_b[ti].as_ref().unwrap().view.clone()
                    };

                    let dst_in_a = !prev_in_a;
                    let out: TextureView = if last {
                        self.tile_outputs[ti].as_ref().unwrap().view.clone()
                    } else if dst_in_a {
                        self.bank_a[ti].as_ref().unwrap().view.clone()
                    } else {
                        self.bank_b[ti].as_ref().unwrap().view.clone()
                    };

                    let src_rect = if k == 0 { pr.src } else { pr.proc };
                    let proc = pr.proc;
                    self.run_ordinary_step(
                        device,
                        queue,
                        &mut encoder,
                        item,
                        &tile_info,
                        source.full_width as f32,
                        proc,
                        src_rect,
                        &prev,
                        &out,
                        &mut pools,
                    );
                }
            }

            if let PlanItem::Step(_, m) = item
                && let Some(radius) = m.kind.effect_class().separable_apron()
            {
                let deferred = self.run_separable_step(
                    device,
                    queue,
                    &mut encoder,
                    source,
                    &blur_set,
                    &proc_rects,
                    roi_active,
                    radius,
                    k == 0,
                    prev_in_a,
                    last,
                    &mut blur_pool_used,
                );
                self.reprocess_pending |= deferred;
            }

            if !last {
                prev_in_a = !prev_in_a;
            }
        }

        for &ti in &reprocess {
            let o = self.tile_outputs[ti].as_mut().unwrap();
            let complete = o.band_y == 0 || o.band_y >= o.height;
            o.valid = complete;
        }
        for &ti in &display_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            self.build_roi_display_bgs(device, queue, ti, &source.tiles[ti], pr, roi_active);
        }
        queue.submit([encoder.finish()]);
    }

    fn prepare_tiled_scanlines(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        plan: &[PlanItem],
        proc_scale: f32,
        dirty: bool,
    ) {
        let n_tiles = source.tiles.len();
        self.bank_a.resize_with(n_tiles, || None);
        self.bank_b.resize_with(n_tiles, || None);
        self.blur_hmid.resize_with(n_tiles, || None);

        let scale = fit_process_scale(
            source.full_width,
            source.full_height,
            1,
            3,
            process_vram_budget(device),
            proc_scale,
        );
        let stile = |ti: usize, source: &TiledSource| -> (u32, u32, u32, u32) {
            let t = &source.tiles[ti];
            let sx0 = scaled(t.x, scale);
            let sy0 = scaled(t.y, scale);
            let sx1 = scaled(t.x + t.width, scale);
            let sy1 = scaled(t.y + t.height, scale);
            (sx0, sy0, (sx1 - sx0).max(1), (sy1 - sy0).max(1))
        };

        let mut has_h = false;
        let mut has_v = false;
        for p in plan {
            if let PlanItem::Step(_, m) = p
                && let EffectClass::ComputeScanline { axis } = m.kind.effect_class()
            {
                match axis {
                    Axis::Vertical => has_v = true,
                    Axis::Horizontal => has_h = true,
                }
            }
        }

        let visible: Vec<usize> = (0..n_tiles)
            .filter(|&ti| !tile_ndc_culled(source.tiles[ti].last_ndc_rect))
            .collect();
        if visible.is_empty() {
            for ti in 0..n_tiles {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }
            return;
        }

        let plan_has_blur = plan
            .iter()
            .any(|p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some()));

        let mut in_set = vec![false; n_tiles];
        for &v in &visible {
            let (vx, vy) = (source.tiles[v].x, source.tiles[v].y);
            for (ti, slot) in in_set.iter_mut().enumerate() {
                let same_row = source.tiles[ti].y == vy;
                let same_col = source.tiles[ti].x == vx;
                if (has_h && same_row) || (has_v && same_col) {
                    *slot = true;
                }
            }
        }
        if plan_has_blur {
            let strip: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();
            for ti in strip {
                let nb = tile_neighbors(&source.tiles, ti);
                for n in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    in_set[n] = true;
                }
            }
            let expanded: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();
            for ei in expanded {
                let (ex, ey) = (source.tiles[ei].x, source.tiles[ei].y);
                for (ti, slot) in in_set.iter_mut().enumerate() {
                    let same_row = source.tiles[ti].y == ey;
                    let same_col = source.tiles[ti].x == ex;
                    if (has_h && same_row) || (has_v && same_col) {
                        *slot = true;
                    }
                }
            }
        }
        let proc_set: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();

        for (ti, &keep) in in_set.iter().enumerate() {
            if !keep {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                self.bank_a[ti] = None;
                self.bank_b[ti] = None;
            }
        }

        if dirty {
            for &ti in &proc_set {
                if let Some(o) = self.tile_outputs[ti].as_mut() {
                    o.valid = false;
                }
            }
        }

        let ensure_bank = |bank: &mut Vec<Option<ScratchTarget>>,
                           ti: usize,
                           w: u32,
                           h: u32,
                           fmt: TextureFormat| {
            let stale = bank[ti]
                .as_ref()
                .is_none_or(|t| t.width != w || t.height != h);
            if stale {
                bank[ti] = Some(ScratchTarget::new(device, fmt, w, h));
            }
        };

        let plan_steps = plan.len();

        let mut proc_rects: Vec<Option<ProcRect>> = (0..n_tiles).map(|_| None).collect();
        for &ti in &proc_set {
            let (_, _, sw, sh) = stile(ti, source);
            self.ensure_tile_output(device, ti, sw, sh, None);
            ensure_bank(&mut self.bank_a, ti, sw, sh, self.format);
            ensure_bank(&mut self.bank_b, ti, sw, sh, self.format);
            if plan_has_blur {
                ensure_bank(&mut self.blur_hmid, ti, sw, sh, self.format);
                let t = &source.tiles[ti];
                let fw = source.full_width as f32;
                let fh = source.full_height as f32;
                proc_rects[ti] = Some(ProcRect {
                    px: [
                        t.x as f32,
                        t.y as f32,
                        (t.x + t.width) as f32,
                        (t.y + t.height) as f32,
                    ],
                    proc: UvRect {
                        origin: [t.x as f32 / fw, t.y as f32 / fh],
                        size: [t.width as f32 / fw, t.height as f32 / fh],
                    },
                    src: UvRect {
                        origin: [t.x as f32 / fw, t.y as f32 / fh],
                        size: [t.width as f32 / fw, t.height as f32 / fh],
                    },
                    w: sw,
                    h: sh,
                });
            }
        }

        let all_valid = proc_set
            .iter()
            .all(|&ti| self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid));
        let needs_bg_only = all_valid && !dirty;

        let mut sig: u64 = 1469598103934665603;
        for &ti in &proc_set {
            sig = (sig ^ ti as u64).wrapping_mul(1099511628211);
        }
        sig = (sig ^ scale.to_bits() as u64).wrapping_mul(1099511628211);
        if dirty || sig != self.sort_progress_sig {
            self.sort_band_cursor = 0;
            self.sort_progress_sig = sig;
        }

        let last_sort_k = plan.iter().enumerate().rev().find_map(|(k, p)| {
            matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline())
                .then_some(k)
        });

        let continuation = self.sort_band_cursor > 0;

        if !needs_bg_only {
            let mut encoder =
                device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                    label: Some("pixel-sort-multitile-encoder"),
                });

            if self.uniform_pool.is_empty() {
                self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                    device,
                    Some("combined-modifiers-uniform"),
                ));
            }
            for &ti in &proc_set {
                if continuation {
                    break;
                }
                let src: TextureView = source.tiles[ti].source_view.clone();
                let dst: TextureView = self.bank_a[ti].as_ref().unwrap().view.clone();
                let uniforms = build_segment_uniforms(
                    &[],
                    &TileInfo {
                        tile_x: source.tiles[ti].x,
                        tile_y: source.tiles[ti].y,
                        tile_w: source.tiles[ti].width,
                        tile_h: source.tiles[ti].height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    },
                    UvRect {
                        origin: [0.0, 0.0],
                        size: [1.0, 1.0],
                    },
                    UvRect {
                        origin: [0.0, 0.0],
                        size: [1.0, 1.0],
                    },
                );
                let buffer = &self.uniform_pool[0];
                gpu::write_uniform(queue, buffer, &uniforms);
                let bg = gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    &src,
                    &self.trilinear_sampler,
                    Some("psort-downscale-bg"),
                );
                self.combined.run(&mut encoder, &bg, &dst);
            }

            let mut pools = StepPools {
                fused: 1,
                ..StepPools::default()
            };
            let mut blur_pool_used = 0usize;
            let mut prev_in_a = true;
            let mut sort_complete = true;

            for (k, item) in plan.iter().enumerate() {
                let last = k == plan_steps - 1;
                let is_sort = matches!(item, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline());
                let blur_radius = match item {
                    PlanItem::Step(_, m) => m.kind.effect_class().separable_apron(),
                    _ => None,
                };

                let post_sort = last_sort_k.is_some_and(|s| k > s);

                if !is_sort && ((continuation && !post_sort) || (post_sort && !sort_complete)) {
                    if !last {
                        prev_in_a = !prev_in_a;
                    }
                    continue;
                }

                if let Some(radius) = blur_radius {
                    self.run_separable_step(
                        device,
                        queue,
                        &mut encoder,
                        source,
                        &proc_set,
                        &proc_rects,
                        false,
                        radius,
                        false,
                        prev_in_a,
                        last,
                        &mut blur_pool_used,
                    );
                } else if !is_sort {
                    for &ti in &proc_set {
                        let tile = &source.tiles[ti];
                        let tile_info = TileInfo {
                            tile_x: tile.x,
                            tile_y: tile.y,
                            tile_w: tile.width,
                            tile_h: tile.height,
                            full_w: source.full_width,
                            full_h: source.full_height,
                        };
                        let whole = tile_proc_rect(
                            tile,
                            source.full_width as f32,
                            source.full_height as f32,
                            1.0,
                            false,
                            0.0,
                            false,
                        );
                        let prev: TextureView = self.step_input_view(ti, prev_in_a).clone();
                        let out: TextureView = self.step_output_view(ti, last, prev_in_a).clone();

                        self.run_ordinary_step(
                            device,
                            queue,
                            &mut encoder,
                            item,
                            &tile_info,
                            source.full_width as f32,
                            whole.proc,
                            whole.src,
                            &prev,
                            &out,
                            &mut pools,
                        );
                    }
                } else if let PlanItem::Step(_, m) = item
                    && let ModifierKind::PixelSort(ps) = &m.kind
                {
                    let step_vertical =
                        matches!(SortAxis::from_angle(ps.angle), SortAxis::Vertical { .. });
                    let groups = pixel_sort_groups(source, &proc_set, step_vertical);
                    queue.submit([encoder.finish()]);
                    let budgeted = Some(k) == last_sort_k;
                    let done = self.sort_cross_tile(
                        device,
                        queue,
                        source,
                        &groups,
                        last,
                        prev_in_a,
                        step_vertical,
                        ps.threshold,
                        ps.angle,
                        budgeted,
                        scale,
                    );
                    if budgeted {
                        sort_complete = done;
                    }
                    encoder =
                        device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                            label: Some("pixel-sort-multitile-encoder"),
                        });
                }

                if !last {
                    prev_in_a = !prev_in_a;
                }
            }

            queue.submit([encoder.finish()]);

            if sort_complete {
                self.sort_band_cursor = 0;
                for &ti in &proc_set {
                    if let Some(o) = self.tile_outputs[ti].as_mut() {
                        o.valid = true;
                    }
                }
            } else {
                self.reprocess_pending = true;
            }
        }

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;
        for &ti in &visible {
            let tile = &source.tiles[ti];
            let pr = proc_rect_from_px(None, tile, full_w, full_h, tile.width, tile.height);
            self.build_roi_display_bgs(device, queue, ti, tile, &pr, false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_ordinary_step(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        item: &PlanItem,
        tile_info: &TileInfo,
        full_w: f32,
        proc: UvRect,
        src: UvRect,
        prev: &TextureView,
        out: &TextureView,
        pools: &mut StepPools,
    ) {
        match item {
            PlanItem::Fused(seg) => {
                let uniforms = build_segment_uniforms(seg, tile_info, proc, src);
                if pools.fused == self.uniform_pool.len() {
                    self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                        device,
                        Some("combined-modifiers-uniform"),
                    ));
                }
                let buffer = &self.uniform_pool[pools.fused];
                pools.fused += 1;
                gpu::write_uniform(queue, buffer, &uniforms);
                let bg = gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    prev,
                    &self.trilinear_sampler,
                    Some("combined-modifiers-bg"),
                );
                self.combined.run(encoder, &bg, out);
            }
            PlanItem::Step(idx, m) => match &m.kind {
                ModifierKind::ChromaticAberration(ca) => {
                    if pools.ca == self.ca_uniform_pool.len() {
                        self.ca_uniform_pool
                            .push(self.chromatic_aberration.uniform_buffer(device));
                    }
                    let buffer = &self.ca_uniform_pool[pools.ca];
                    pools.ca += 1;
                    self.chromatic_aberration.record(
                        device, queue, encoder, buffer, ca.amount, full_w, proc, src, prev, out,
                    );
                }
                ModifierKind::Text(_) => {
                    if let Some(layer) = self.text_layers.get(*idx).and_then(|l| l.as_ref()) {
                        if pools.text == self.text_uniform_pool.len() {
                            self.text_uniform_pool
                                .push(self.text.uniform_buffer(device));
                        }
                        let buffer = &self.text_uniform_pool[pools.text];
                        pools.text += 1;
                        self.text.record(
                            device, queue, encoder, buffer, layer, tile_info, proc, src, prev, out,
                        );
                    }
                }
                _ => {
                    let uniforms = build_segment_uniforms(&[], tile_info, proc, src);
                    if pools.fused == self.uniform_pool.len() {
                        self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                            device,
                            Some("combined-modifiers-uniform"),
                        ));
                    }
                    let buffer = &self.uniform_pool[pools.fused];
                    pools.fused += 1;
                    gpu::write_uniform(queue, buffer, &uniforms);
                    let bg = gpu::standard_bind_group(
                        device,
                        &self.combined.bgl,
                        buffer,
                        prev,
                        &self.trilinear_sampler,
                        Some("passthrough-bg"),
                    );
                    self.combined.run(encoder, &bg, out);
                }
            },
        }
    }

    fn step_input_view(&self, ti: usize, prev_in_a: bool) -> &TextureView {
        if prev_in_a {
            &self.bank_a[ti].as_ref().unwrap().view
        } else {
            &self.bank_b[ti].as_ref().unwrap().view
        }
    }

    fn step_output_view(&self, ti: usize, last: bool, prev_in_a: bool) -> &TextureView {
        if last {
            &self.tile_outputs[ti].as_ref().unwrap().view
        } else if prev_in_a {
            &self.bank_b[ti].as_ref().unwrap().view
        } else {
            &self.bank_a[ti].as_ref().unwrap().view
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sort_cross_tile(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        groups: &[Vec<usize>],
        last: bool,
        prev_in_a: bool,
        vertical: bool,
        threshold: f32,
        angle: f32,
        budgeted: bool,
        scale: f32,
    ) -> bool {
        let budget = device.limits().max_buffer_size.min(PIXEL_SORT_BUF_BUDGET);
        let line_len = if vertical {
            scaled(source.full_height, scale)
        } else {
            scaled(source.full_width, scale)
        };
        let stile = |ti: usize| -> (u32, u32, u32, u32) {
            let t = &source.tiles[ti];
            let sx0 = scaled(t.x, scale);
            let sy0 = scaled(t.y, scale);
            (
                sx0,
                sy0,
                (scaled(t.x + t.width, scale) - sx0).max(1),
                (scaled(t.y + t.height, scale) - sy0).max(1),
            )
        };

        let mut units: Vec<(usize, u32, u32)> = Vec::new();
        for (gi, group) in groups.iter().enumerate() {
            let (_, _, sw0, sh0) = stile(group[0]);
            let cross = if vertical { sw0 } else { sh0 };
            let per_line_bytes = if vertical {
                (line_len as u64) * 4
            } else {
                ((line_len * 4).div_ceil(256) * 256) as u64
            };
            let mem_band = (budget / per_line_bytes.max(1)).max(1) as u32;
            let band = mem_band.min(PIXEL_SORT_LINES_PER_BAND).min(cross).max(1);
            let mut c0 = 0u32;
            while c0 < cross {
                let c1 = (c0 + band).min(cross);
                units.push((gi, c0, c1));
                c0 = c1;
            }
        }

        let total = units.len() as u32;
        let (start, end) = if budgeted {
            let start = self.sort_band_cursor.min(total);
            let end = (start + PIXEL_SORT_BANDS_PER_FRAME).min(total);
            self.sort_band_cursor = end;
            (start, end)
        } else {
            (0, total)
        };

        for &(gi, c0, c1) in &units[start as usize..end as usize] {
            let group = &groups[gi];
            {
                let band_n = c1 - c0;

                let (sort_w, sort_h) = if vertical {
                    (band_n, line_len)
                } else {
                    (line_len, band_n)
                };
                let row_bytes = (sort_w * 4).div_ceil(256) * 256;
                let row_words = row_bytes / 4;
                let bytes = (row_bytes * sort_h) as u64;
                let need_new = self
                    .sort_buffers
                    .as_ref()
                    .is_none_or(|(s, _)| s.size() < bytes);
                if need_new {
                    self.sort_buffers = Some((
                        gpu::storage_buffer(device, bytes, Some("pixel-sort-src")),
                        gpu::storage_buffer(device, bytes, Some("pixel-sort-dst")),
                    ));
                }
                if self.pixel_sort_uniform_pool.is_empty() {
                    self.pixel_sort_uniform_pool
                        .push(self.pixel_sort.uniform_buffer(device));
                }

                let mut enc =
                    device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                        label: Some("pixel-sort-cross-tile"),
                    });
                let (src_buf, dst_buf) = self.sort_buffers.as_ref().unwrap();
                let uniform = &self.pixel_sort_uniform_pool[0];

                let copy_origin = if vertical {
                    iced::wgpu::Origin3d { x: c0, y: 0, z: 0 }
                } else {
                    iced::wgpu::Origin3d { x: 0, y: c0, z: 0 }
                };
                let copy_extent = |sw: u32, sh: u32| {
                    if vertical {
                        iced::wgpu::Extent3d {
                            width: band_n,
                            height: sh,
                            depth_or_array_layers: 1,
                        }
                    } else {
                        iced::wgpu::Extent3d {
                            width: sw,
                            height: band_n,
                            depth_or_array_layers: 1,
                        }
                    }
                };
                let tile_offset = |sx0: u32, sy0: u32| -> u64 {
                    if vertical {
                        (sy0 as u64) * (row_bytes as u64)
                    } else {
                        (sx0 as u64) * 4
                    }
                };
                for &ti in group {
                    let (sx0, sy0, sw, sh) = stile(ti);
                    let tex = self.sort_in_tex(ti, prev_in_a);
                    enc.copy_texture_to_buffer(
                        tex_copy_info(tex, copy_origin),
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: src_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset: tile_offset(sx0, sy0),
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        copy_extent(sw, sh),
                    );
                }

                self.pixel_sort.record(
                    device, queue, &mut enc, uniform, src_buf, dst_buf, sort_w, sort_h, row_words,
                    threshold, angle,
                );

                for &ti in group {
                    let (sx0, sy0, sw, sh) = stile(ti);
                    let tex = self.sort_out_tex(ti, last, prev_in_a);
                    enc.copy_buffer_to_texture(
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: dst_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset: tile_offset(sx0, sy0),
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        tex_copy_info(tex, copy_origin),
                        copy_extent(sw, sh),
                    );
                }

                queue.submit([enc.finish()]);
            }
        }

        end >= total
    }

    fn sort_in_tex(&self, ti: usize, prev_in_a: bool) -> &Texture {
        if prev_in_a {
            &self.bank_a[ti].as_ref().unwrap()._tex
        } else {
            &self.bank_b[ti].as_ref().unwrap()._tex
        }
    }

    fn sort_out_tex(&self, ti: usize, last: bool, prev_in_a: bool) -> &Texture {
        if last {
            &self.tile_outputs[ti].as_ref().unwrap()._tex
        } else if prev_in_a {
            &self.bank_b[ti].as_ref().unwrap()._tex
        } else {
            &self.bank_a[ti].as_ref().unwrap()._tex
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sort_target(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        target: SortTarget,
        w: u32,
        h: u32,
        threshold: f32,
        angle: f32,
    ) {
        let row_bytes = (w * 4).div_ceil(256) * 256;
        let row_words = row_bytes / 4;
        let bytes = (row_bytes * h) as u64;

        let need_new = self
            .sort_buffers
            .as_ref()
            .is_none_or(|(s, _)| s.size() < bytes);
        if need_new {
            self.sort_buffers = Some((
                gpu::storage_buffer(device, bytes, Some("pixel-sort-src")),
                gpu::storage_buffer(device, bytes, Some("pixel-sort-dst")),
            ));
        }
        let (src, dst) = self.sort_buffers.as_ref().unwrap();

        let tex = match target {
            SortTarget::ScratchA => &self.scratch_a.as_ref().unwrap()._tex,
            SortTarget::ScratchB => &self.scratch_b.as_ref().unwrap()._tex,
            SortTarget::Output => &self.tile_outputs[ti].as_ref().unwrap()._tex,
        };
        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("pixel-sort-bridge"),
        });
        let copy_layout = iced::wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row_bytes),
            rows_per_image: Some(h),
        };
        let extent = iced::wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            iced::wgpu::TexelCopyBufferInfo {
                buffer: src,
                layout: copy_layout,
            },
            extent,
        );
        if self.pixel_sort_uniform_pool.is_empty() {
            self.pixel_sort_uniform_pool
                .push(self.pixel_sort.uniform_buffer(device));
        }
        let uniform = &self.pixel_sort_uniform_pool[0];
        self.pixel_sort.record(
            device, queue, &mut enc, uniform, src, dst, w, h, row_words, threshold, angle,
        );
        enc.copy_buffer_to_texture(
            iced::wgpu::TexelCopyBufferInfo {
                buffer: dst,
                layout: copy_layout,
            },
            tex.as_image_copy(),
            extent,
        );
        queue.submit([enc.finish()]);
    }

    #[allow(clippy::too_many_arguments)]
    fn run_separable_step(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        blur_set: &[usize],
        proc_rects: &[Option<ProcRect>],
        roi_active: bool,
        radius: f32,
        is_first: bool,
        prev_in_a: bool,
        last: bool,
        blur_pool_used: &mut usize,
    ) -> bool {
        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let bank_rect = |i: usize| -> Option<TileRect> {
            proc_rects[i].as_ref().map(|p| TileRect {
                origin: p.proc.origin,
                size: p.proc.size,
            })
        };

        if roi_active && last && is_first {
            let mut deferred = false;
            for &ti in blur_set {
                if self.blur_tile_banded(
                    device,
                    queue,
                    encoder,
                    source,
                    proc_rects,
                    ti,
                    radius,
                    full_w,
                    full_h,
                    blur_pool_used,
                ) {
                    deferred = true;
                }
            }
            return deferred;
        }

        for &ti in blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            let proc_img_w = (pr.px[2] - pr.px[0]).max(1.0);
            let radius_px = radius * pr.w as f32 / proc_img_w;
            let sigma = (radius_px / 3.0).max(0.5);
            let step = proc_img_w / pr.w as f32;
            let rect = bank_rect(ti).unwrap();
            let src = if is_first {
                TileRect {
                    origin: pr.src.origin,
                    size: pr.src.size,
                }
            } else {
                rect
            };
            let nb = if roi_active {
                Neighbors::default()
            } else {
                tile_neighbors(&source.tiles, ti)
            };

            while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let h_buffer = &self.blur_uniform_pool[*blur_pool_used];
            *blur_pool_used += 1;

            let p_view = |i: usize| -> Option<&TextureView> {
                if is_first {
                    Some(&source.tiles[i].source_view)
                } else if prev_in_a {
                    self.bank_a[i].as_ref().map(|t| &t.view)
                } else {
                    self.bank_b[i].as_ref().map(|t| &t.view)
                }
            };
            let nbr_rect = |i: usize| -> Option<TileRect> {
                if is_first {
                    Some(full_rect(&TileInfo {
                        tile_x: source.tiles[i].x,
                        tile_y: source.tiles[i].y,
                        tile_w: source.tiles[i].width,
                        tile_h: source.tiles[i].height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    }))
                } else {
                    bank_rect(i)
                }
            };

            let input = p_view(ti).unwrap();
            let lo = nb.left.and_then(|i| Some((p_view(i)?, nbr_rect(i)?)));
            let hi = nb.right.and_then(|i| Some((p_view(i)?, nbr_rect(i)?)));
            let h_out = &self.blur_hmid[ti].as_ref().unwrap().view;

            let h_lod = if is_first { step.max(1.0).log2() } else { 0.0 };
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                h_buffer,
                [step / full_w, 0.0],
                radius_px,
                sigma,
                rect,
                src,
                lo,
                hi,
                input,
                h_out,
                None,
                h_lod,
            );
        }

        for &ti in blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            let proc_img_h = (pr.px[3] - pr.px[1]).max(1.0);
            let radius_px = radius * pr.h as f32 / proc_img_h;
            let sigma = (radius_px / 3.0).max(0.5);
            let step = proc_img_h / pr.h as f32;
            let rect = bank_rect(ti).unwrap();
            let nb = if roi_active {
                Neighbors::default()
            } else {
                tile_neighbors(&source.tiles, ti)
            };

            while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let v_buffer = &self.blur_uniform_pool[*blur_pool_used];
            *blur_pool_used += 1;

            let h_view =
                |i: usize| -> Option<&TextureView> { self.blur_hmid[i].as_ref().map(|t| &t.view) };

            let input = h_view(ti).unwrap();
            let lo = nb.up.and_then(|i| Some((h_view(i)?, bank_rect(i)?)));
            let hi = nb.down.and_then(|i| Some((h_view(i)?, bank_rect(i)?)));

            let v_out: &TextureView = if last {
                &self.tile_outputs[ti].as_ref().unwrap().view
            } else if prev_in_a {
                &self.bank_b[ti].as_ref().unwrap().view
            } else {
                &self.bank_a[ti].as_ref().unwrap().view
            };

            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                v_buffer,
                [0.0, step / full_h],
                radius_px,
                sigma,
                rect,
                rect,
                lo,
                hi,
                input,
                v_out,
                None,
                0.0,
            );
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn blur_tile_banded(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        proc_rects: &[Option<ProcRect>],
        ti: usize,
        radius: f32,
        full_w: f32,
        full_h: f32,
        blur_pool_used: &mut usize,
    ) -> bool {
        let pr = proc_rects[ti].as_ref().unwrap();
        let w = pr.w;
        let h = pr.h;
        let proc_img_w = (pr.px[2] - pr.px[0]).max(1.0);
        let proc_img_h = (pr.px[3] - pr.px[1]).max(1.0);
        let radius_h = radius * w as f32 / proc_img_w;
        let radius_v = radius * h as f32 / proc_img_h;
        let sigma_h = (radius_h / 3.0).max(0.5);
        let sigma_v = (radius_v / 3.0).max(0.5);
        let step_h = proc_img_w / w as f32;
        let step_v = proc_img_h / h as f32;
        let apron = radius_v.ceil() as u32;

        let rect = TileRect {
            origin: pr.proc.origin,
            size: pr.proc.size,
        };
        let src = TileRect {
            origin: pr.src.origin,
            size: pr.src.size,
        };

        let tile_top = source.tiles[ti].y as f32;
        let tile_bot = (source.tiles[ti].y + source.tiles[ti].height) as f32;
        let apron_img = radius.ceil();
        let nb = tile_neighbors(&source.tiles, ti);
        let top_strip = match nb.up {
            Some(up) if pr.px[1] <= tile_top + 0.5 => self.record_v_neighbor_strip(
                device,
                queue,
                encoder,
                source,
                ti,
                up,
                pr,
                apron,
                apron_img,
                w,
                tile_top - apron_img,
                tile_top,
                full_w,
                full_h,
                step_h,
                radius_h,
                sigma_h,
                blur_pool_used,
                true,
            ),
            _ => None,
        };
        let bot_strip = match nb.down {
            Some(down) if pr.px[3] >= tile_bot - 0.5 => self.record_v_neighbor_strip(
                device,
                queue,
                encoder,
                source,
                ti,
                down,
                pr,
                apron,
                apron_img,
                w,
                tile_bot,
                tile_bot + apron_img,
                full_w,
                full_h,
                step_h,
                radius_h,
                sigma_h,
                blur_pool_used,
                false,
            ),
            _ => None,
        };

        let taps = 2 * (radius_h.ceil() as u32 + radius_v.ceil() as u32) + 2;
        let per_row = (w.max(1)) * taps.max(1);
        let band_h = (BLUR_WORK_BUDGET / per_row).clamp(BLUR_MIN_BAND_H, BLUR_MAX_BAND_H);

        let start = self.tile_outputs[ti].as_ref().unwrap().band_y;
        let mut by = start;
        let mut bands_done = 0u32;
        while by < h && bands_done < 1 {
            let by1 = (by + band_h).min(h);
            let h0 = by.saturating_sub(apron);
            let h1 = (by1 + apron).min(h);
            while self.blur_uniform_pool.len() < *blur_pool_used + 2 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let (h_pool, v_pool) = self.blur_uniform_pool.split_at(*blur_pool_used + 1);
            let h_buffer = &h_pool[*blur_pool_used];
            let v_buffer = &v_pool[0];
            *blur_pool_used += 2;

            let input = &source.tiles[ti].source_view;
            let h_out = &self.blur_hmid[ti].as_ref().unwrap().view;
            let h_lod = step_h.max(1.0).log2();
            let nb = tile_neighbors(&source.tiles, ti);
            let tile_rect = |i: usize| -> TileRect {
                full_rect(&TileInfo {
                    tile_x: source.tiles[i].x,
                    tile_y: source.tiles[i].y,
                    tile_w: source.tiles[i].width,
                    tile_h: source.tiles[i].height,
                    full_w: source.full_width,
                    full_h: source.full_height,
                })
            };
            let lo = nb
                .left
                .map(|i| (&source.tiles[i].source_view, tile_rect(i)));
            let hi = nb
                .right
                .map(|i| (&source.tiles[i].source_view, tile_rect(i)));
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                h_buffer,
                [step_h / full_w, 0.0],
                radius_h,
                sigma_h,
                rect,
                src,
                lo,
                hi,
                input,
                h_out,
                Some([0, h0, w, h1 - h0]),
                h_lod,
            );

            let hmid = &self.blur_hmid[ti].as_ref().unwrap().view;
            let out = &self.tile_outputs[ti].as_ref().unwrap().view;
            let lo_v =
                top_strip.and_then(|r| self.blur_vstrip_top[ti].as_ref().map(|s| (&s.view, r)));
            let hi_v =
                bot_strip.and_then(|r| self.blur_vstrip_bot[ti].as_ref().map(|s| (&s.view, r)));
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                v_buffer,
                [0.0, step_v / full_h],
                radius_v,
                sigma_v,
                rect,
                rect,
                lo_v,
                hi_v,
                hmid,
                out,
                Some([0, by, w, by1 - by]),
                0.0,
            );

            by = by1;
            bands_done += 1;
        }

        let o = self.tile_outputs[ti].as_mut().unwrap();
        o.band_y = by;
        by < h
    }

    #[allow(clippy::too_many_arguments)]
    fn record_v_neighbor_strip(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        ti: usize,
        nbr: usize,
        pr: &ProcRect,
        apron: u32,
        apron_img: f32,
        w: u32,
        y0_img: f32,
        y1_img: f32,
        full_w: f32,
        full_h: f32,
        step_h: f32,
        radius_h: f32,
        sigma_h: f32,
        blur_pool_used: &mut usize,
        top: bool,
    ) -> Option<TileRect> {
        let strip_h = apron.max(1);
        if y1_img <= y0_img + 0.5 || apron_img < 0.5 || w == 0 {
            return None;
        }

        {
            let slot = if top {
                &mut self.blur_vstrip_top[ti]
            } else {
                &mut self.blur_vstrip_bot[ti]
            };
            if slot
                .as_ref()
                .is_none_or(|s| s.width != w || s.height != strip_h)
            {
                *slot = Some(ScratchTarget::new(device, self.format, w, strip_h));
            }
        }

        let strip_rect = TileRect {
            origin: [pr.proc.origin[0], y0_img / full_h],
            size: [pr.proc.size[0], (y1_img - y0_img) / full_h],
        };
        let nsrc = full_rect(&TileInfo {
            tile_x: source.tiles[nbr].x,
            tile_y: source.tiles[nbr].y,
            tile_w: source.tiles[nbr].width,
            tile_h: source.tiles[nbr].height,
            full_w: source.full_width,
            full_h: source.full_height,
        });

        while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
            self.blur_uniform_pool
                .push(self.gaussian_blur.uniform_buffer(device));
        }
        let h_buffer = &self.blur_uniform_pool[*blur_pool_used];
        *blur_pool_used += 1;

        let strip_view = if top {
            &self.blur_vstrip_top[ti].as_ref().unwrap().view
        } else {
            &self.blur_vstrip_bot[ti].as_ref().unwrap().view
        };
        let input = &source.tiles[nbr].source_view;
        let h_lod = step_h.max(1.0).log2();
        self.gaussian_blur.record(
            device,
            queue,
            encoder,
            h_buffer,
            [step_h / full_w, 0.0],
            radius_h,
            sigma_h,
            strip_rect,
            nsrc,
            None,
            None,
            input,
            strip_view,
            None,
            h_lod,
        );
        Some(strip_rect)
    }
}
