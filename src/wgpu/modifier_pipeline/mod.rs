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
    /// The document this output was built for.
    ///
    ///  is expressed in it, so an output made for a different one
    /// cannot be reused: its region would be reinterpreted in the new document
    /// and the band copy would run off the texture.
    doc: (u32, u32),
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

use crate::modifiers::plan::{ImageSpec, PlanItem, infer_specs, plan_modifiers};

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

/// The fraction of the chain's output resolution worth rendering.
///
/// `physical_scale` is how large the document is drawn on screen, so it already
/// says how much of the chain's output survives to a pixel. Rounding up to the
/// next power of two renders at least as many document pixels as the screen
/// shows, at any zoom and with or without a resize in the chain -- a 2x upscale
/// fitted at 0.42 renders 1179 of its 2358 columns for 990 screen pixels.
///
/// This deliberately has no special case for an upscale. One was tried, on the
/// theory that a resized document was being rendered below the source's
/// resolution; the arithmetic says otherwise, and the floor only forced 4-8x
/// the necessary work once zoomed out. Whatever softens an upscaled preview is
/// not decided here.
fn quality_scale_for(physical_scale: f32) -> f32 {
    if physical_scale > 0.0 {
        physical_scale.log2().ceil().exp2().min(1.0)
    } else {
        1.0
    }
}

const ROI_MARGIN_PX: f32 = 256.0;

const PROCESS_VRAM_BUDGET_MIN: u64 = 512 * 1024 * 1024;
const PROCESS_VRAM_BUDGET_MAX: u64 = 4 * 1024 * 1024 * 1024;

const BLUR_WORK_BUDGET: u32 = 24_000_000;
const BLUR_MIN_BAND_H: u32 = 8;
const BLUR_MAX_BAND_H: u32 = 1024;
const MAX_BLUR_FRAMES: u32 = 4;

use crate::modifiers::gpu::UvRect;

/// The source and output document sizes a display quad is placed against.
///
/// Carried together because they are only meaningful as a pair: a rect is
/// mapped from one into the other.
#[derive(Clone, Copy)]
pub(super) struct DocScale {
    pub src: (u32, u32),
    pub out: (u32, u32),
    /// Whether the tile has a visible sub-rect to inscribe the quad into.
    pub roi_active: bool,
}

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
    /// The document the last prepare produced, so refresh_display_transforms
    /// can place quads without the modifier list.
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

        let plan_vec = plan_modifiers(modifiers);

        if plan_vec.is_empty() {
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
            self.build_roi_display_bgs(
                device,
                queue,
                ti,
                tile,
                &pr,
                DocScale {
                    src: (source.full_width, source.full_height),
                    out: self.doc_size,
                    roi_active,
                },
            );
        }
    }

    pub(super) fn build_roi_display_bgs(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        tile: &crate::wgpu::tiled_source::Tile,
        pr: &ProcRect,
        doc: DocScale,
    ) {
        //  places pr.px within isec, so both must be in one
        // space. pr.px is in the output document once the chain resizes, while
        // isec is the tile's visible rect in the source, so isec is mapped the
        // same way. Mixing them offsets each tile's quad by a different amount,
        // which is what shows as seams between tiles.
        let display_uniform: &iced::wgpu::Buffer = if doc.roi_active
            && let (Some(isec), Some(base)) = (tile.isec_px, tile.last_transform)
        {
            let isec = tile_out_rect(isec, doc.src.0, doc.src.1, doc.out.0, doc.out.1);
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

/// The rule the preview's resolution rests on: rendering at the next power of
/// two above `physical_scale` always covers the screen.
///
/// Marco reported that an upscale looked blurrier the further he zoomed out,
/// and a floor was briefly added here on the theory that a 2x document was
/// being rendered at the source's pixel count. The arithmetic says otherwise --
/// the plain stepped value already covers the screen at every zoom -- and the
/// floor only bought 4-8x the work once zoomed out. It was removed. These tests
/// exist so the same wrong fix is not attempted twice.
#[cfg(test)]
mod quality_scale_tests {
    use super::quality_scale_for;

    #[test]
    fn zoom_rounds_up_to_a_power_of_two() {
        assert_eq!(quality_scale_for(0.42), 0.5);
        assert_eq!(quality_scale_for(0.3), 0.5);
        assert_eq!(quality_scale_for(0.25), 0.25);
        assert_eq!(quality_scale_for(1.0), 1.0);
        assert_eq!(quality_scale_for(4.0), 1.0, "never renders above 1:1");
    }

    /// The invariant that makes a floor unnecessary.
    ///
    /// Whatever the zoom and whatever the resize, the rendered document must be
    /// at least as many pixels as the screen shows it in -- otherwise the
    /// display magnifies a proxy, which is what "blurry" means. A 2x upscale
    /// fitted at 0.42 renders 1179 of its 2358 columns for 990 screen pixels.
    #[test]
    fn the_render_always_covers_the_screen() {
        let doc_px = 2358.0_f32;
        for &phys in &[1.0, 0.5, 0.42, 0.25, 0.2, 0.1, 0.05, 0.01] {
            let rendered = doc_px * quality_scale_for(phys);
            let on_screen = doc_px * phys;
            assert!(
                rendered >= on_screen,
                "at zoom {phys} the document rendered {rendered:.0}px for                  {on_screen:.0}px of screen; the display would be magnifying a proxy"
            );
        }
    }

    /// Zooming out must keep saving work, which is the property the floor broke.
    #[test]
    fn zooming_out_keeps_reducing_the_work() {
        assert_eq!(quality_scale_for(0.2), 0.25);
        assert_eq!(quality_scale_for(0.1), 0.125);
        assert_eq!(quality_scale_for(0.05), 0.0625);
        assert!(
            quality_scale_for(0.05) < quality_scale_for(0.2),
            "further out must not cost more"
        );
    }

    /// A degenerate scale must not produce a NaN or a zero-sized render.
    #[test]
    fn a_nonpositive_scale_falls_back_to_full() {
        assert_eq!(quality_scale_for(0.0), 1.0);
        assert_eq!(quality_scale_for(-1.0), 1.0);
    }
}
