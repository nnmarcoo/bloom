//! The GPU modifier pipeline: per-tile render targets, cached bind groups, and
//! the signature that decides when work must be redone.
//!
//! Preview quality is scaled down when the zoom level or the VRAM budget calls
//! for it, so a complex stack on a large image stays interactive. VRAM size is
//! not discoverable from wgpu, so the budget is policy rather than measurement.

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
            motion_blur::MotionBlurPass,
            pixel_sort::PixelSortCompute,
            text::{TextLayer, TextPass},
        },
        tiled_source::TiledSource,
        view_pipeline::tile_ndc_culled,
    },
};

mod executor;
mod geom;
#[cfg(test)]
mod goldens;
#[cfg(test)]
mod gpu_bench;
#[cfg(test)]
mod parity;

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
        self.run_pieces(encoder, dst, std::iter::once((bind_group, None)));
    }

    fn run_pieces<'a>(
        &self,
        encoder: &mut CommandEncoder,
        dst: &TextureView,
        pieces: impl IntoIterator<Item = (&'a BindGroup, Option<[u32; 4]>)>,
    ) {
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
        for (bind_group, scissor) in pieces {
            pass.set_bind_group(0, bind_group, &[]);
            if let Some([x, y, w, h]) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
    }
}

struct TileOutput {
    _tex: Texture,
    view: TextureView,
    valid: bool,
    width: u32,
    height: u32,
    proc_px: Option<[f32; 4]>,
    quality_scale: f32,
    /// Set once `resample_outputs` has replaced the texture with a smaller one.
    ///
    /// `proc_px` and `quality_scale` still describe the geometry the chain
    /// rendered at, so they no longer predict this texture's size. The executor
    /// must not reuse it as a render target.
    resampled: bool,
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

use crate::modifiers::plan::{ImageSpec, PlanItem, chain_output_spec, infer_specs, plan_modifiers};

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

fn quality_scale_for(physical_scale: f32) -> f32 {
    if physical_scale > 0.0 {
        physical_scale.log2().ceil().exp2().min(1.0)
    } else {
        1.0
    }
}

pub(crate) fn is_resize(kind: &ModifierKind) -> bool {
    matches!(kind, ModifierKind::Resize(_))
}

/// The filter of the last enabled resize, which is the one that decides how the
/// final downsample looks.
///
/// With several stacked resizes the CPU applies each in turn; the preview does
/// them as one step, so it takes the last filter rather than blending them.
fn resize_filter(modifiers: &[Modifier]) -> crate::modifiers::kinds::ResizeFilter {
    modifiers
        .iter()
        .rev()
        .filter(|m| m.enabled)
        .find_map(|m| match &m.kind {
            ModifierKind::Resize(r) => Some(r.filter),
            _ => None,
        })
        .unwrap_or(crate::modifiers::kinds::ResizeFilter::Lanczos)
}

const ROI_MARGIN_PX: f32 = 256.0;

const PROCESS_VRAM_BUDGET_MIN: u64 = 512 * 1024 * 1024;
const PROCESS_VRAM_BUDGET_MAX: u64 = 4 * 1024 * 1024 * 1024;

const BLUR_WORK_BUDGET: u32 = 24_000_000;
const BLUR_MIN_BAND_H: u32 = 8;
const BLUR_MAX_BAND_H: u32 = 1024;
const MAX_BLUR_FRAMES: u32 = 4;

use crate::modifiers::gpu::UvRect;

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

    roi_display_uniforms: Vec<Option<iced::wgpu::Buffer>>,
    reprocess_pending: bool,

    uniform_pool: Vec<iced::wgpu::Buffer>,
    ca_uniform_pool: Vec<iced::wgpu::Buffer>,
    mb_uniform_pool: Vec<iced::wgpu::Buffer>,
    blur_uniform_pool: Vec<iced::wgpu::Buffer>,
    text_uniform_pool: Vec<iced::wgpu::Buffer>,
    pixel_sort_uniform_pool: Vec<iced::wgpu::Buffer>,
    pixel_sort_diag_uniform_pool: Vec<iced::wgpu::Buffer>,
    sort_buffers: Option<(iced::wgpu::Buffer, iced::wgpu::Buffer)>,
    text_layers: Vec<Option<TextLayer>>,
    text_sigs: Vec<Option<u64>>,
    drawing_layers: Vec<Option<DrawingLayer>>,
    drawing_sigs: Vec<Option<u64>>,
    drawing_uniform_pool: Vec<iced::wgpu::Buffer>,
    combined: CombinedPass,
    chromatic_aberration: ChromaticAberrationPass,
    motion_blur: MotionBlurPass,
    gaussian_blur: GaussianBlurPass,
    pixel_sort: PixelSortCompute,
    text: TextPass,
    drawing: DrawingPass,
    resample: crate::wgpu::passes::resample::ResamplePass,
    resample_uniforms: Vec<iced::wgpu::Buffer>,
    display_bgl: BindGroupLayout,
    trilinear_sampler: Sampler,
    linear_sampler: Sampler,
    nearest_sampler: Sampler,
    /// The chain's output size, so  can normalize a
    /// resampled tile's quad without re-deriving the plan.
    doc_size: (u32, u32),
    exec_band_cursor: u32,
    exec_sig: u64,
    exec_slab_pool: Vec<Option<ScratchTarget>>,

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
            roi_display_uniforms: Vec::new(),
            reprocess_pending: false,
            uniform_pool: Vec::new(),
            ca_uniform_pool: Vec::new(),
            mb_uniform_pool: Vec::new(),
            blur_uniform_pool: Vec::new(),
            text_uniform_pool: Vec::new(),
            pixel_sort_uniform_pool: Vec::new(),
            pixel_sort_diag_uniform_pool: Vec::new(),
            sort_buffers: None,
            text_layers: Vec::new(),
            text_sigs: Vec::new(),
            drawing_layers: Vec::new(),
            drawing_sigs: Vec::new(),
            drawing_uniform_pool: Vec::new(),
            combined: CombinedPass::new(device, format),
            chromatic_aberration: ChromaticAberrationPass::new(device, format),
            motion_blur: MotionBlurPass::new(device, format),
            gaussian_blur: GaussianBlurPass::new(device, format),
            pixel_sort: PixelSortCompute::new(device),
            text: TextPass::new(device, format),
            drawing: DrawingPass::new(device, format),
            resample: crate::wgpu::passes::resample::ResamplePass::new(device, format),
            resample_uniforms: Vec::new(),
            display_bgl,
            trilinear_sampler,
            linear_sampler,
            nearest_sampler,
            doc_size: (width, height),
            exec_band_cursor: 0,
            exec_sig: 0,
            exec_slab_pool: Vec::new(),
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
        let quality_scale = quality_scale_for(physical_scale);
        let downscale = quality_scale < 1.0;

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

        let mut plan_vec = plan_modifiers(modifiers);

        // Resizes at the tail of the chain are separated out rather than
        // dropped. Everything before them runs at source geometry, which is
        // what this executor can express, and their combined effect is a change
        // to the document's size. The viewport already fits that size, so the
        // display draws the chain's output across the resized quad.
        //
        // A resize in the *middle* is still dropped: stages after it would have
        // to run in the post-resize space, and one coordinate space is all the
        // ROI walk and tiling can carry today.
        let trailing_resizes = plan_vec
            .iter()
            .rev()
            .take_while(|item| matches!(item, PlanItem::Step(_, m) if is_resize(&m.kind)))
            .count();
        plan_vec.truncate(plan_vec.len() - trailing_resizes);
        plan_vec.retain(|item| !matches!(item, PlanItem::Step(_, m) if is_resize(&m.kind)));

        // Nothing to render *and* nothing to resize: clear the outputs so the
        // view falls back to drawing the source directly.
        //
        // A resize-only stack must not take this path. Its plan is empty once
        // the trailing resizes are split off, but falling back to the source
        // draws full-resolution tiles inside the shrunken quad, so the image
        // keeps all its detail while claiming to be smaller. At an extreme
        // resize that reads as the whole picture crammed into a few pixels.
        if plan_vec.is_empty() && trailing_resizes == 0 {
            for o in self.tile_outputs.iter_mut() {
                *o = None;
            }
            for bg in self.tile_display_bgs_linear.iter_mut() {
                *bg = None;
            }
            for bg in self.tile_display_bgs_nearest.iter_mut() {
                *bg = None;
            }
            return;
        }

        let source_spec = ImageSpec::new(source.full_width, source.full_height);
        debug_assert!(
            infer_specs(source_spec, &plan_vec)
                .iter()
                .all(|s| s.is_passthrough()),
            "a modifier changes dimensions, but the GPU executor still sizes \
             every stage from the source"
        );

        let mut n_proc = 0u64;
        let (mut tw, mut th) = (1u32, 1u32);
        for t in &source.tiles {
            if !tile_ndc_culled(t.last_ndc_rect) {
                n_proc += 1;
                tw = tw.max(t.width);
                th = th.max(t.height);
            }
        }
        let fit = fit_process_scale(
            tw,
            th,
            n_proc,
            1,
            process_vram_budget(device),
            quality_scale,
        );
        let (ps, ds) = if fit < quality_scale {
            (fit, true)
        } else {
            (quality_scale, downscale)
        };
        if let [PlanItem::Fused(seg)] = plan_vec.as_slice() {
            let seg = seg.clone();
            self.execute_pointwise(device, queue, source, &seg, ps, ds);
        } else {
            self.execute_kernel_chain(device, queue, source, &plan_vec, ps, ds);
        }

        if trailing_resizes > 0 {
            let out = chain_output_spec(source_spec, &plan_modifiers(modifiers));
            self.resample_outputs(device, queue, source, out, resize_filter(modifiers));
        }
    }

    /// Resize each tile's output to the extent it occupies in the resized
    /// document.
    ///
    /// The target is an absolute size derived from the tile's `proc_px`, not a
    /// ratio applied to the current texture. Those differ whenever the chain
    /// rendered at reduced quality: the texture is already smaller than the
    /// region it represents, so scaling it by the document ratio would shrink
    /// it twice and leave the tiles mismatched against their neighbors.
    ///
    /// Rounding the region's edges rather than its extent keeps adjacent tiles
    /// sharing a boundary. Rounding widths independently lets two tiles that
    /// met exactly end up a pixel apart.
    fn resample_outputs(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        out: ImageSpec,
        filter: crate::modifiers::kinds::ResizeFilter,
    ) {
        let sx = out.w as f32 / source.full_width.max(1) as f32;
        let sy = out.h as f32 / source.full_height.max(1) as f32;
        if (sx - 1.0).abs() < 1e-6 && (sy - 1.0).abs() < 1e-6 {
            return;
        }

        let mut encoder = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resize-outputs"),
        });
        let usage = TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST;
        let mut resampled_tiles: Vec<usize> = Vec::new();

        for ti in 0..source.tiles.len() {
            let Some(o) = self.tile_outputs[ti].as_ref() else {
                continue;
            };
            if !o.valid {
                continue;
            }
            let tile = &source.tiles[ti];
            let px = o.proc_px.unwrap_or([
                tile.x as f32,
                tile.y as f32,
                (tile.x + tile.width) as f32,
                (tile.y + tile.height) as f32,
            ]);
            let (src_w, src_h) = (o.width, o.height);
            let x0 = (px[0] * sx).round();
            let y0 = (px[1] * sy).round();
            let dst_w = (((px[2] * sx).round() - x0) as u32).max(1);
            let dst_h = (((px[3] * sy).round() - y0) as u32).max(1);
            if (dst_w, dst_h) == (src_w, src_h) {
                // The texture is already the right size, which happens when the
                // quality scale has coincidentally shrunk it by the same factor
                // the resize asks for. No resampling is needed, but `proc_px`
                // must still move into the resized document or the display quad
                // stays full-size and stretches the tile back over the original
                // area.
                let slot = self.tile_outputs[ti].as_mut().unwrap();
                slot.proc_px = Some([px[0] * sx, px[1] * sy, px[2] * sx, px[3] * sy]);
                slot.resampled = true;
                resampled_tiles.push(ti);
                continue;
            }

            while self.resample_uniforms.len() < (ti + 1) * 2 {
                self.resample_uniforms
                    .push(self.resample.uniform_buffer(device));
            }

            // Horizontal into an intermediate, then vertical into the final.
            // Both axes in one texture would apply a 2D gather, which is a
            // different filter than the CPU's separable pair.
            let mid = gpu::texture_2d(device, dst_w, src_h, self.format, usage, Some("resize-mid"));
            let mid_view = mid.create_view(&Default::default());
            let dst = gpu::texture_2d(device, dst_w, dst_h, self.format, usage, Some("resize-out"));
            let dst_view = dst.create_view(&Default::default());

            self.resample.record(
                device,
                queue,
                &mut encoder,
                &self.resample_uniforms[ti * 2],
                &o.view,
                &mid_view,
                dst_w,
                src_w,
                false,
                filter,
            );
            self.resample.record(
                device,
                queue,
                &mut encoder,
                &self.resample_uniforms[ti * 2 + 1],
                &mid_view,
                &dst_view,
                dst_h,
                src_h,
                true,
                filter,
            );

            let slot = self.tile_outputs[ti].as_mut().unwrap();
            slot._tex = dst;
            slot.view = dst_view;
            slot.width = dst_w;
            slot.height = dst_h;
            // The chain writes bands into this texture sized from `proc_px` at
            // `quality_scale`, and those still describe the pre-resample
            // geometry. Reusing a resampled output would copy a full-size band
            // into a shrunken texture, which wgpu rejects outright: "Copy of Y
            // 1024..1024 would end up overrunning the bounds of the Destination
            // texture of Y size 3". Marking it resampled forces a fresh
            // allocation next frame.
            // `proc_px` is the region this output covers, and the display quad
            // is built from it. Leaving it in source space makes the quad
            // full-size while the texture is not, so the resized pixels get
            // stretched back across the original area -- the resize does its
            // work and is then undone at draw time.
            //
            // Moving it into the resized document keeps the quad and the
            // texture describing the same region.
            slot.proc_px = Some([px[0] * sx, px[1] * sy, px[2] * sx, px[3] * sy]);
            slot.resampled = true;
            resampled_tiles.push(ti);
        }
        queue.submit([encoder.finish()]);

        // Rebuild the display bind groups against the new textures.
        //
        // They point at the texture view, so replacing it invalidates them. The
        // executor builds them before this runs, and nothing rebuilds them
        // afterward, so simply clearing them left `tile_display_bg` returning
        // None. The draw loop pushes nothing for such a tile and falls through
        // without drawing it, which is what produced the flicker: the tile
        // appeared on frames where the bind group survived and vanished on
        // frames where it did not.
        // `proc_px` is now in the resized document, so the quad is normalized
        // against that rather than the source.
        self.doc_size = (out.w, out.h);
        let (doc_w, doc_h) = (out.w as f32, out.h as f32);
        for ti in resampled_tiles {
            let tile = &source.tiles[ti];
            let (proc_px, w, h) = {
                let o = self.tile_outputs[ti].as_ref().unwrap();
                (o.proc_px, o.width, o.height)
            };
            let pr = proc_rect_from_px(proc_px, tile, doc_w, doc_h, w, h);
            let roi_active = proc_px.is_some() && tile.isec_px.is_some();
            self.build_roi_display_bgs(device, queue, ti, tile, &pr, roi_active);
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
            let (proc_px, w, h, resampled) = (o.proc_px, o.width, o.height, o.resampled);
            // A resampled tile carries proc_px in the resized document, so the
            // quad must be normalized against that instead of the source.
            let (dw, dh) = if resampled {
                (self.doc_size.0 as f32, self.doc_size.1 as f32)
            } else {
                (full_w, full_h)
            };
            let pr = proc_rect_from_px(proc_px, tile, dw, dh, w, h);
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
