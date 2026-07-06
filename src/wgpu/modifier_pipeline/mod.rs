use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, BlendState,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::{
    modifiers::{
        Modifier, ModifierKind,
        gpu::{ModUniforms, TileInfo, build_segment_uniforms},
    },
    wgpu::{
        gpu,
        passes::{
            chromatic_aberration::ChromaticAberrationPass,
            drawing::{DrawingLayer, DrawingPass},
            gaussian_blur::{GaussianBlurPass, TileRect},
            motion_blur::{MotionBlurCompute, MotionBlurPass},
            pixel_sort::PixelSortCompute,
            text::{TextLayer, TextPass},
        },
        tiled_source::TiledSource,
        view_pipeline::tile_ndc_culled,
    },
};

mod blur;
mod geom;
mod sort;
mod tiled;

use geom::*;

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
            include_str!("../shaders/combined_modifiers.wgsl"),
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
    mb: usize,
    text: usize,
    drawing: usize,
}

const ROI_MARGIN_PX: f32 = 256.0;

const PIXEL_SORT_BUF_BUDGET: u64 = 64 * 1024 * 1024;
const PIXEL_SORT_LINES_PER_BAND: u32 = 64;
const PIXEL_SORT_BANDS_PER_FRAME: u32 = 4;
const PROCESS_VRAM_BUDGET_MIN: u64 = 512 * 1024 * 1024;
const PROCESS_VRAM_BUDGET_MAX: u64 = 4 * 1024 * 1024 * 1024;

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
use crate::modifiers::pixel_sort::{SortAxis, SortMode};

pub(super) struct ProcRect {
    px: [f32; 4],
    proc: UvRect,
    src: UvRect,
    w: u32,
    h: u32,
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
    mb_uniform_pool: Vec<iced::wgpu::Buffer>,
    mb_compute_uniform_pool: Vec<iced::wgpu::Buffer>,
    mb_buffers: Option<(iced::wgpu::Buffer, iced::wgpu::Buffer)>,
    blur_uniform_pool: Vec<iced::wgpu::Buffer>,
    text_uniform_pool: Vec<iced::wgpu::Buffer>,
    pixel_sort_uniform_pool: Vec<iced::wgpu::Buffer>,
    pixel_sort_diag_uniform_pool: Vec<iced::wgpu::Buffer>,
    sort_buffers: Option<(iced::wgpu::Buffer, iced::wgpu::Buffer)>,
    sort_band_cursor: u32,
    sort_progress_sig: u64,
    text_layers: Vec<Option<TextLayer>>,
    text_sigs: Vec<Option<u64>>,
    drawing_layers: Vec<Option<DrawingLayer>>,
    drawing_sigs: Vec<Option<u64>>,
    drawing_uniform_pool: Vec<iced::wgpu::Buffer>,
    combined: CombinedPass,
    chromatic_aberration: ChromaticAberrationPass,
    motion_blur: MotionBlurPass,
    motion_blur_compute: MotionBlurCompute,
    gaussian_blur: GaussianBlurPass,
    pixel_sort: PixelSortCompute,
    text: TextPass,
    drawing: DrawingPass,
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
            mb_uniform_pool: Vec::new(),
            mb_compute_uniform_pool: Vec::new(),
            mb_buffers: None,
            blur_uniform_pool: Vec::new(),
            text_uniform_pool: Vec::new(),
            pixel_sort_uniform_pool: Vec::new(),
            pixel_sort_diag_uniform_pool: Vec::new(),
            sort_buffers: None,
            sort_band_cursor: 0,
            sort_progress_sig: 0,
            text_layers: Vec::new(),
            text_sigs: Vec::new(),
            drawing_layers: Vec::new(),
            drawing_sigs: Vec::new(),
            drawing_uniform_pool: Vec::new(),
            combined: CombinedPass::new(device, format),
            chromatic_aberration: ChromaticAberrationPass::new(device, format),
            motion_blur: MotionBlurPass::new(device, format),
            motion_blur_compute: MotionBlurCompute::new(device),
            gaussian_blur: GaussianBlurPass::new(device, format),
            pixel_sort: PixelSortCompute::new(device),
            text: TextPass::new(device, format),
            drawing: DrawingPass::new(device, format),
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

        if self.drawing_layers.len() != modifiers.len() {
            self.drawing_layers.clear();
            self.drawing_layers.resize_with(modifiers.len(), || None);
            self.drawing_sigs.clear();
            self.drawing_sigs.resize(modifiers.len(), None);
        }
        let mut drawing_dirty: Option<[f32; 4]> = None;
        for (i, m) in modifiers.iter().enumerate() {
            match &m.kind {
                ModifierKind::Drawing(d) if m.has_visible_effect() => {
                    let sig = d.strokes_sig();
                    let stale = self.drawing_layers[i]
                        .as_ref()
                        .is_none_or(|l| !l.matches(source.full_width, source.full_height));
                    if !stale && self.drawing_sigs[i] == Some(sig) {
                        continue;
                    }
                    if stale {
                        self.drawing_layers[i] = Some(DrawingLayer::new(
                            device,
                            source.full_width,
                            source.full_height,
                        ));
                    }
                    if let Some(rect) = self.drawing_layers[i].as_mut().unwrap().sync(
                        queue,
                        d,
                        source.full_width,
                        source.full_height,
                    ) {
                        drawing_dirty = Some(match drawing_dirty {
                            Some(a) => [
                                a[0].min(rect[0]),
                                a[1].min(rect[1]),
                                a[2].max(rect[2]),
                                a[3].max(rect[3]),
                            ],
                            None => rect,
                        });
                    }
                    self.drawing_sigs[i] = Some(sig);
                }
                _ => {
                    if self.drawing_layers[i].take().is_some() {
                        raster_changed = true;
                    }
                    self.drawing_sigs[i] = None;
                }
            }
        }

        if raster_changed && !dirty {
            for o in self.tile_outputs.iter_mut().flatten() {
                o.valid = false;
            }
        } else if !dirty && let Some(dr) = drawing_dirty {
            for (ti, o) in self.tile_outputs.iter_mut().enumerate() {
                let Some(o) = o else { continue };
                let tile = &source.tiles[ti];
                let cover = o.proc_px.unwrap_or([
                    tile.x as f32,
                    tile.y as f32,
                    (tile.x + tile.width) as f32,
                    (tile.y + tile.height) as f32,
                ]);
                if cover[0] < dr[2] && dr[0] < cover[2] && cover[1] < dr[3] && dr[1] < cover[3] {
                    o.valid = false;
                }
            }
        }

        let plan_vec = plan_modifiers(modifiers);
        let has_blur = plan_vec
            .iter()
            .any(|p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some()));
        let has_pixel_sort = plan_vec.iter().any(
            |p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline()),
        );
        let has_motion_blur = plan_vec.iter().any(
            |p| matches!(p, PlanItem::Step(_, m) if matches!(m.kind, ModifierKind::MotionBlur(_))),
        );

        if n_tiles > 1 && (has_blur || has_pixel_sort || has_motion_blur) {
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

        let roi_ok = plan_vec.iter().all(|p| match p {
            PlanItem::Fused(_) => true,
            PlanItem::Step(_, m) => matches!(m.kind, ModifierKind::Drawing(_)),
        });

        let mut plan: Option<Vec<PlanItem>> = Some(plan_vec);
        let mut encoder: Option<CommandEncoder> = None;
        let mut pool_used = 0usize;
        let mut ca_pool_used = 0usize;
        let mut mb_pool_used = 0usize;
        let mut blur_pool_used = 0usize;
        let mut text_pool_used = 0usize;
        let mut drawing_pool_used = 0usize;
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
                            ModifierKind::MotionBlur(mb) => {
                                if mb_pool_used == self.mb_uniform_pool.len() {
                                    self.mb_uniform_pool
                                        .push(self.motion_blur.uniform_buffer(device));
                                }
                                let buffer = &self.mb_uniform_pool[mb_pool_used];
                                mb_pool_used += 1;
                                let enc = encoder.get_or_insert_with(|| {
                                    device.create_command_encoder(
                                        &iced::wgpu::CommandEncoderDescriptor {
                                            label: Some("modifier-pipeline-encoder"),
                                        },
                                    )
                                });
                                let src_rect = if k == 0 { pr.src } else { pr.proc };
                                self.motion_blur.record(
                                    device,
                                    queue,
                                    enc,
                                    buffer,
                                    mb.angle,
                                    mb.distance,
                                    source.full_width as f32,
                                    source.full_height as f32,
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
                            ModifierKind::Drawing(_) => {
                                if let Some(layer) =
                                    self.drawing_layers.get(*idx).and_then(|l| l.as_ref())
                                {
                                    if drawing_pool_used == self.drawing_uniform_pool.len() {
                                        self.drawing_uniform_pool
                                            .push(self.drawing.uniform_buffer(device));
                                    }
                                    let buffer = &self.drawing_uniform_pool[drawing_pool_used];
                                    drawing_pool_used += 1;
                                    let enc = encoder.get_or_insert_with(|| {
                                        device.create_command_encoder(
                                            &iced::wgpu::CommandEncoderDescriptor {
                                                label: Some("modifier-pipeline-encoder"),
                                            },
                                        )
                                    });
                                    let src_rect = if k == 0 { pr.src } else { pr.proc };
                                    self.drawing.record(
                                        device, queue, enc, buffer, layer, pr.proc, src_rect,
                                        &prev, out,
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

    pub(super) fn ensure_tile_output(
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

    pub(super) fn build_roi_display_bgs(
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
}
