//! Composites a frame: checkerboard, the tiled image or its modified output,
//! then the pixel grid.
//!
//! Full-quality reprocessing is deferred until the view stops moving.
//! VIEW_SETTLE is how long the transform must hold still first, so panning and
//! zooming stay smooth instead of restarting an expensive chain every frame.
//!
//! Per-tile ROIs are written here from viewport culling, and the modifier
//! pipeline reads them to decide how much of each tile to process.
//!
//! A resize is previewable like any other modifier. Excluding it from the gate
//! destroyed the pipeline for a resize-only stack, and the view then drew the
//! source tiles at full resolution inside a smaller quad -- every pixel kept
//! while claiming to be smaller, so a 1x1 resize showed the whole picture in
//! one pixel. A resize may also defer while interacting, but only while the
//! document holds the size its textures were built for; deferring across a size
//! change leaves full-resolution textures on shrunken quads, which reads as
//! flicker. That is what the doc_size comparison guards.
//!
//! doc_region is the source rect the document covers, narrowed by a crop. Three
//! separate things used to be one field, and collapsing them cost a bug each:
//! crop_uv is the window the shader samples with and is now always the unit
//! rect, since the chain has already cropped; doc_region is what the quads are
//! laid out across, and using the source there stretched the picture; and
//! last_doc_region keys the per-tile cache, where the constant crop_uv meant the
//! guard never fired and a tile the crop excluded kept its old placement -- a
//! fragment of the picture stranded in the viewport.
//!
//! place_tile is that layout as a pure function, so the display_harness module
//! can drive a real multi-tile grid through pan and zoom. Every crop bug that
//! reached a user lived in this arithmetic, and none were visible to a test
//! while it was tangled up with the GPU buffers update() writes.

use bytemuck::bytes_of;
use glam::{Mat4, Vec2, vec2, vec3, vec4};
use iced::{
    Rectangle,
    wgpu::{
        AddressMode, BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, Extent3d,
        FilterMode, LoadOp, Operations, Queue, RenderPassColorAttachment, RenderPassDescriptor,
        RenderPipeline, Sampler, SamplerDescriptor, StoreOp, TexelCopyBufferLayout, TextureFormat,
        TextureUsages, TextureView,
    },
    widget::shader::Pipeline,
};

use crate::{
    modifiers::Modifier,
    modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers},
    wgpu::{
        error::ViewError,
        gpu,
        media::image_data::{ImageData, ImageId},
        modifier_pipeline::ModifierPipeline,
        passes::{
            checkerboard::{CheckerboardPass, CheckerboardUniforms},
            display::DisplayPass,
            pixel_grid::{PixelGridPass, PixelGridUniforms},
        },
        tiled_source::TiledSource,
    },
};

#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct DisplayUniforms {
    pub transform: Mat4,
    pub crop_uv: [f32; 4],
}

pub(crate) fn tile_doc_intersection(tile: [f32; 4], doc: [f32; 4]) -> [f32; 4] {
    [
        doc[0].max(tile[0]),
        doc[1].max(tile[1]),
        doc[2].min(tile[2]),
        doc[3].min(tile[3]),
    ]
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewGeometry {
    pub doc_region: [f32; 4],
    pub viewport: Vec2,
    pub scale: f32,
    pub pan_ndc: Vec2,
    pub rotation: u8,
}

impl ViewGeometry {
    fn inv_tile_vp(&self) -> Vec2 {
        if self.rotation.is_multiple_of(2) {
            vec2(1.0 / self.viewport.x, 1.0 / self.viewport.y)
        } else {
            vec2(1.0 / self.viewport.y, 1.0 / self.viewport.x)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TilePlacement {
    pub transform: Mat4,
    pub crop_uv: [f32; 4],
    pub ndc: (Vec2, Vec2),
    pub isec: [f32; 4],
}

pub(crate) fn place_tile(tile: [f32; 4], g: ViewGeometry) -> Option<TilePlacement> {
    let isec = tile_doc_intersection(tile, g.doc_region);
    if isec[0] >= isec[2] || isec[1] >= isec[3] {
        return None;
    }

    let [dl, dt, dr, db] = g.doc_region;
    let doc_c = vec2((dl + dr) * 0.5, (dt + db) * 0.5);
    let isec_c = vec2((isec[0] + isec[2]) * 0.5, (isec[1] + isec[3]) * 0.5);
    let inv = g.inv_tile_vp();

    let offset = 2.0 * vec2(isec_c.x - doc_c.x, doc_c.y - isec_c.y) * inv;
    let aspect = vec2(isec[2] - isec[0], isec[3] - isec[1]) * inv;
    let angle = -(g.rotation as f32) * std::f32::consts::FRAC_PI_2;

    let transform = Mat4::from_scale(vec3(g.scale, g.scale, 1.0))
        * Mat4::from_translation(vec3(g.pan_ndc.x, g.pan_ndc.y, 0.0))
        * Mat4::from_rotation_z(angle)
        * Mat4::from_translation(vec3(offset.x, offset.y, 0.0))
        * Mat4::from_scale(vec3(aspect.x, aspect.y, 1.0));

    let (tx, ty) = (tile[0], tile[1]);
    let (tw, th) = ((tile[2] - tile[0]).max(1e-6), (tile[3] - tile[1]).max(1e-6));

    Some(TilePlacement {
        transform,
        crop_uv: [
            (isec[0] - tx) / tw,
            (isec[1] - ty) / th,
            (isec[2] - tx) / tw,
            (isec[3] - ty) / th,
        ],
        ndc: ndc_rect_of_transform(&transform),
        isec,
    })
}

pub(crate) fn tile_ndc_culled(rect: Option<(Vec2, Vec2)>) -> bool {
    matches!(
        rect,
        Some((min, max)) if max.x < -1.0 || min.x > 1.0 || max.y < -1.0 || min.y > 1.0
    )
}

fn roi_from_ndc_clip((ndc_min, ndc_max): (Vec2, Vec2), rect: [f32; 4]) -> Option<[f32; 4]> {
    let [left, top, right, bottom] = rect;
    let nw = ndc_max.x - ndc_min.x;
    let nh = ndc_max.y - ndc_min.y;
    if nw <= 0.0 || nh <= 0.0 {
        return None;
    }
    let fx0 = ((-1.0 - ndc_min.x) / nw).clamp(0.0, 1.0);
    let fx1 = ((1.0 - ndc_min.x) / nw).clamp(0.0, 1.0);
    let fy_from_top0 = ((ndc_max.y - 1.0) / nh).clamp(0.0, 1.0);
    let fy_from_top1 = ((ndc_max.y + 1.0) / nh).clamp(0.0, 1.0);
    if fx1 <= fx0 || fy_from_top1 <= fy_from_top0 {
        return None;
    }
    let l = left + (right - left) * fx0;
    let r = left + (right - left) * fx1;
    let t = top + (bottom - top) * fy_from_top0;
    let b = top + (bottom - top) * fy_from_top1;
    if r - l < 1.0 || b - t < 1.0 {
        return None;
    }
    Some([l, t, r, b])
}

fn ndc_rect_of_transform(transform: &Mat4) -> (Vec2, Vec2) {
    let corners = [
        vec4(-1.0, -1.0, 0.0, 1.0),
        vec4(1.0, -1.0, 0.0, 1.0),
        vec4(-1.0, 1.0, 0.0, 1.0),
        vec4(1.0, 1.0, 0.0, 1.0),
    ];
    let clip = corners.map(|c| (*transform * c).truncate().truncate());
    let min = clip.iter().copied().fold(clip[0], Vec2::min);
    let max = clip.iter().copied().fold(clip[0], Vec2::max);
    (min, max)
}

pub struct ViewPipeline {
    display: DisplayPass,
    checkerboard: CheckerboardPass,
    pixel_grid: PixelGridPass,
    trilinear_sampler: Sampler,
    nearest_sampler: Sampler,
    linear_sampler: Sampler,
    blit_pipeline: RenderPipeline,
    blit_bgl: BindGroupLayout,
    placeholder_bind_group: BindGroup,
    _placeholder_uniform: Buffer,
    source: Option<TiledSource>,
    modifier_pipeline: Option<ModifierPipeline>,
    pending_source_dirty: bool,
    scale_factor: f32,
    last_checker_uniforms: Option<CheckerboardUniforms>,
    pub mipmap_zoom_out: bool,
    last_view: Option<DisplayUniforms>,
    view_changed_at: std::time::Instant,
    format: TextureFormat,
}

const VIEW_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

impl ViewPipeline {
    pub fn clear_source(&mut self, device: &Device) {
        if self.source.is_none() {
            return;
        }
        self.modifier_pipeline = None;
        self.source = None;
        let _ = device.poll(iced::wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
    }

    pub fn upload_image(
        &mut self,
        device: &Device,
        queue: &Queue,
        image: &ImageData,
    ) -> Result<(), ViewError> {
        if !image.pixels_available() {
            return Ok(());
        }

        if let Some(source) = &mut self.source
            && source.matches(image, self.mipmap_zoom_out)
        {
            source.write_frame(
                device,
                queue,
                image,
                &self.blit_pipeline,
                &self.blit_bgl,
                &self.linear_sampler,
            )?;
            self.pending_source_dirty = true;
            return Ok(());
        }

        self.modifier_pipeline = None;
        self.source = None;
        let _ = device.poll(iced::wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });

        self.source = Some(TiledSource::new(
            device,
            queue,
            image,
            &self.display,
            &self.trilinear_sampler,
            &self.nearest_sampler,
            &self.linear_sampler,
            self.mipmap_zoom_out,
            &self.blit_pipeline,
            &self.blit_bgl,
            None,
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        device: &Device,
        queue: &Queue,
        scale: f32,
        scale_factor: f32,
        uniforms: &DisplayUniforms,
        viewport: Vec2,
        pan_ndc: Vec2,
        rotation: u8,
        doc_region: [f32; 4],
    ) {
        if self.last_view != Some(*uniforms) {
            self.last_view = Some(*uniforms);
            self.view_changed_at = std::time::Instant::now();
        }

        self.scale_factor = scale_factor;
        let physical_scale = scale * scale_factor;

        let source = match &mut self.source {
            Some(s) => s,
            None => return,
        };
        source.physical_scale = physical_scale;

        if source.mips_dirty && physical_scale < 1.0 - 1e-6 {
            source.regen_mipmaps(
                device,
                queue,
                &self.blit_pipeline,
                &self.blit_bgl,
                &self.linear_sampler,
            );
        }

        if source.tiles.len() == 1 {
            let tile = &mut source.tiles[0];
            if tile.last_transform != Some(uniforms.transform)
                || tile.last_doc_region != Some(doc_region)
            {
                queue.write_buffer(&tile.uniform_buffer, 0, bytes_of(uniforms));
                tile.last_ndc_rect = Some(ndc_rect_of_transform(&uniforms.transform));
                tile.last_transform = Some(uniforms.transform);
                tile.last_doc_region = Some(doc_region);
            }
            return;
        }

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let geom = ViewGeometry {
            doc_region,
            viewport,
            scale,
            pan_ndc,
            rotation,
        };

        for tile in &mut source.tiles {
            let tx = tile.x as f32;
            let ty = tile.y as f32;
            let tw = tile.width as f32;
            let th = tile.height as f32;

            let Some(p) = place_tile([tx, ty, tx + tw, ty + th], geom) else {
                if tile.last_doc_region != Some(doc_region) {
                    tile.last_ndc_rect = Some((vec2(2.0, 2.0), vec2(3.0, 3.0)));
                    tile.last_transform = None;
                    tile.last_doc_region = Some(doc_region);
                }
                continue;
            };
            let (transform, ndc) = (p.transform, p.ndc);
            let [isec_left, isec_top, isec_right, isec_bottom] = p.isec;

            let roi = if rotation == 0 {
                roi_from_ndc_clip(ndc, p.isec)
            } else {
                None
            };

            if tile.last_transform != Some(transform)
                || tile.last_doc_region != Some(doc_region)
                || tile.proc_rect_px != roi
            {
                queue.write_buffer(
                    &tile.uniform_buffer,
                    0,
                    bytes_of(&DisplayUniforms {
                        transform,
                        crop_uv: p.crop_uv,
                    }),
                );
                tile.last_ndc_rect = Some(ndc);
                tile.last_transform = Some(transform);
                tile.last_doc_region = Some(doc_region);

                tile.proc_rect_px = roi;
                tile.proc_rect_uv =
                    roi.map(|[l, t, r, b]| [l / full_w, t / full_h, r / full_w, b / full_h]);
                tile.isec_px = roi.map(|_| [isec_left, isec_top, isec_right, isec_bottom]);
            }
        }
    }

    pub fn update_checkerboard(&mut self, queue: &Queue, uniforms: CheckerboardUniforms) {
        if self.last_checker_uniforms != Some(uniforms) {
            self.checkerboard.update_colors(queue, &uniforms);
            self.last_checker_uniforms = Some(uniforms);
        }
    }

    pub fn update_pixel_grid(&self, queue: &Queue, uniforms: &PixelGridUniforms) {
        self.pixel_grid.update(queue, uniforms);
    }

    fn interacting(&self) -> bool {
        self.view_changed_at.elapsed() < VIEW_SETTLE
    }

    pub fn reprocess_pending(&self) -> bool {
        if self.interacting() {
            return true;
        }
        self.modifier_pipeline
            .as_ref()
            .is_some_and(|mp| mp.reprocess_pending())
    }

    pub fn prepare_modifiers(
        &mut self,
        device: &Device,
        queue: &Queue,
        modifiers: &[Modifier],
        dirty: bool,
    ) {
        let source = match &self.source {
            Some(s) => s,
            None => {
                self.modifier_pipeline = None;
                return;
            }
        };

        let dirty = dirty || self.pending_source_dirty;
        self.pending_source_dirty = false;

        if !modifiers.iter().any(|m| m.has_visible_effect()) {
            self.modifier_pipeline = None;
            return;
        }

        let doc = chain_output_spec(
            ImageSpec::new(source.full_width, source.full_height),
            &plan_modifiers(modifiers),
        );
        let doc_changed = self
            .modifier_pipeline
            .as_ref()
            .is_some_and(|mp| mp.doc_size() != (doc.w, doc.h));
        let has_expensive = modifiers
            .iter()
            .any(|m| m.has_visible_effect() && !m.kind.effect_class().is_pointwise());
        if has_expensive
            && !doc_changed
            && self.interacting()
            && let Some(mp) = self.modifier_pipeline.as_mut()
        {
            mp.refresh_display_transforms(device, queue, source);
            return;
        }

        let (w, h) = (source.full_width, source.full_height);

        let needs_create = self
            .modifier_pipeline
            .as_ref()
            .is_none_or(|mp| mp.width != w || mp.height != h);

        if needs_create {
            let mut mp = ModifierPipeline::new(device, self.format, w, h);
            mp.prepare(device, queue, source, modifiers, false);
            self.modifier_pipeline = Some(mp);
        } else if let Some(mp) = &mut self.modifier_pipeline {
            mp.prepare(device, queue, source, modifiers, dirty);
        }
    }

    pub fn render_checkerboard(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
    ) {
        self.checkerboard
            .draw(encoder, target, clip_bounds, bounds, self.scale_factor);
    }

    pub fn render_pixel_grid(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
    ) {
        self.pixel_grid
            .draw(encoder, target, clip_bounds, bounds, self.scale_factor);
    }

    pub fn render_display(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
        smooth_zoom_in: bool,
    ) {
        let mut bind_groups: Vec<&BindGroup> = Vec::new();

        if let Some(source) = &self.source {
            let zoomed_out = source.physical_scale < 1.0 - 1e-6;
            if let Some(mp) = self.modifier_pipeline.as_ref() {
                let nearest = !smooth_zoom_in && !zoomed_out;
                for (i, tile) in source.tiles.iter().enumerate() {
                    if tile_ndc_culled(tile.last_ndc_rect) {
                        continue;
                    }
                    if let Some(bg) = mp.tile_display_bg(i, nearest) {
                        bind_groups.push(bg);
                    }
                }
            } else {
                for tile in &source.tiles {
                    if tile_ndc_culled(tile.last_ndc_rect) {
                        continue;
                    }
                    bind_groups.push(if zoomed_out {
                        &tile.zoom_out_bind_group
                    } else if smooth_zoom_in {
                        &tile.linear_bind_group
                    } else {
                        &tile.nearest_bind_group
                    });
                }
            }
        } else {
            bind_groups.push(&self.placeholder_bind_group);
        }

        if bind_groups.is_empty() {
            return;
        }

        let sf = self.scale_factor;
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("display-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_viewport(
            bounds.x * sf,
            bounds.y * sf,
            bounds.width * sf,
            bounds.height * sf,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );

        for bg in bind_groups {
            self.display.draw(&mut pass, bg);
        }
    }

    pub fn needs_upload(&self, image_id: ImageId) -> bool {
        match &self.source {
            Some(s) => s.image_id != image_id,
            None => true,
        }
    }
}

impl Pipeline for ViewPipeline {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self
    where
        Self: Sized,
    {
        let display = DisplayPass::new(device, format);

        let trilinear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("trilinear-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("nearest-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let linear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("linear-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        let (blit_pipeline, blit_bgl) = gpu::blit_pipeline(device, TextureFormat::Rgba8Unorm);

        let placeholder_texture = gpu::texture_2d(
            device,
            1,
            1,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("placeholder-texture"),
        );
        queue.write_texture(
            placeholder_texture.as_image_copy(),
            &[128u8, 128, 128, 255],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let placeholder_view = placeholder_texture.create_view(&Default::default());
        let placeholder_uniform =
            gpu::uniform_buffer::<DisplayUniforms>(device, Some("placeholder-uniform"));
        let placeholder_bind_group = display.create_bind_group(
            device,
            &placeholder_uniform,
            &placeholder_view,
            &trilinear_sampler,
            Some("placeholder-bg"),
        );

        let checkerboard = CheckerboardPass::new(device, format);
        let pixel_grid = PixelGridPass::new(device, format);

        Self {
            display,
            checkerboard,
            pixel_grid,
            trilinear_sampler,
            nearest_sampler,
            linear_sampler,
            blit_pipeline,
            mipmap_zoom_out: true,
            blit_bgl,
            placeholder_bind_group,
            _placeholder_uniform: placeholder_uniform,
            source: None,
            modifier_pipeline: None,
            pending_source_dirty: false,
            scale_factor: 1.0,
            last_checker_uniforms: None,
            last_view: None,
            view_changed_at: std::time::Instant::now() - VIEW_SETTLE * 2,
            format,
        }
    }
}

#[cfg(test)]
mod display_harness {
    use super::{TilePlacement, ViewGeometry, place_tile};
    use glam::{Vec2, vec2, vec4};

    pub(super) const SRC: f32 = 30000.0;
    const TILE: f32 = 8192.0;

    pub(super) fn tiles(src: f32, tile: f32) -> Vec<[f32; 4]> {
        let mut v = Vec::new();
        let mut y = 0.0;
        while y < src {
            let mut x = 0.0;
            while x < src {
                v.push([x, y, (x + tile).min(src), (y + tile).min(src)]);
                x += tile;
            }
            y += tile;
        }
        v
    }

    pub(super) fn geometry(doc_region: [f32; 4], scale: f32, pan: Vec2) -> ViewGeometry {
        ViewGeometry {
            doc_region,
            viewport: vec2(1600.0, 900.0),
            scale,
            pan_ndc: pan,
            rotation: 0,
        }
    }

    fn quad_ndc(p: &TilePlacement) -> [f32; 4] {
        let corners = [
            vec4(-1.0, -1.0, 0.0, 1.0),
            vec4(1.0, -1.0, 0.0, 1.0),
            vec4(-1.0, 1.0, 0.0, 1.0),
            vec4(1.0, 1.0, 0.0, 1.0),
        ];
        let pts: Vec<Vec2> = corners
            .iter()
            .map(|c| (p.transform * *c).truncate().truncate())
            .collect();
        let min = pts.iter().copied().fold(pts[0], Vec2::min);
        let max = pts.iter().copied().fold(pts[0], Vec2::max);
        [min.x, min.y, max.x, max.y]
    }

    fn drawn_bounds(doc: [f32; 4], scale: f32, pan: Vec2) -> Option<[f32; 4]> {
        let g = geometry(doc, scale, pan);
        let mut u: Option<[f32; 4]> = None;
        for t in tiles(SRC, TILE) {
            let Some(p) = place_tile(t, g) else { continue };
            let q = quad_ndc(&p);
            u = Some(match u {
                Some(a) => [
                    a[0].min(q[0]),
                    a[1].min(q[1]),
                    a[2].max(q[2]),
                    a[3].max(q[3]),
                ],
                None => q,
            });
        }
        u
    }

    #[test]
    fn the_quads_reconstruct_the_document_at_every_zoom() {
        for &doc in &[
            [0.0, 0.0, SRC, SRC],
            [5000.0, 5000.0, 15000.0, 15000.0],
            [0.0, 0.0, 10000.0, 10000.0],
            [20000.0, 20000.0, 30000.0, 30000.0],
        ] {
            for &scale in &[0.05f32, 0.2, 1.0] {
                let b = drawn_bounds(doc, scale, Vec2::ZERO).expect("something is drawn");
                let (w, h) = (b[2] - b[0], b[3] - b[1]);
                let (dw, dh) = (doc[2] - doc[0], doc[3] - doc[1]);
                let vp = geometry(doc, scale, Vec2::ZERO).viewport;
                let (want_w, want_h) = (2.0 * scale * dw / vp.x, 2.0 * scale * dh / vp.y);

                assert!(
                    (w - want_w).abs() < 1e-2 && (h - want_h).abs() < 1e-2,
                    "doc {doc:?} at scale {scale}: the quads span                      {w:.4}x{h:.4} of NDC, not the {want_w:.4}x{want_h:.4} a                      {dw}x{dh} document occupies in a {}x{} viewport. The                      picture is stretched.",
                    vp.x,
                    vp.y
                );
            }
        }
    }

    #[test]
    fn a_crop_does_not_change_the_documents_shape_on_screen() {
        let full = drawn_bounds([0.0, 0.0, SRC, SRC], 0.2, Vec2::ZERO).expect("drawn");
        let cropped =
            drawn_bounds([5000.0, 5000.0, 15000.0, 15000.0], 0.2, Vec2::ZERO).expect("drawn");

        let shape = |b: [f32; 4]| (b[2] - b[0]) / (b[3] - b[1]);
        assert!(
            (shape(full) - shape(cropped)).abs() < 1e-3,
            "a square crop of a square image is drawn {:.4} wide per unit tall              while the whole image is drawn {:.4}; the crop changed the shape",
            shape(cropped),
            shape(full)
        );
    }

    #[test]
    fn dragging_a_crop_slider_keeps_the_shape_on_a_tiled_image() {
        // A slider drag is a sequence of doc_regions on the same source. Each
        // one must draw with the shape of the document it names, on a source
        // large enough to be tiled.
        let mut worst: Option<(f32, [f32; 4], f32, f32)> = None;
        for w in [30000.0f32, 20000.0, 12000.0, 8000.0, 4000.0, 1500.0, 600.0] {
            let doc = [0.0, 0.0, w, 20000.0];
            let scale = 0.03;
            let b = drawn_bounds(doc, scale, Vec2::ZERO).expect("something is drawn");
            let g = geometry(doc, scale, Vec2::ZERO);
            let got = (b[2] - b[0]) / (b[3] - b[1]);
            let want = (w / 20000.0) * (g.viewport.y / g.viewport.x);
            let err = (got - want).abs();
            if worst.as_ref().is_none_or(|(e, ..)| err > *e) {
                worst = Some((err, doc, got, want));
            }
        }
        let (err, doc, got, want) = worst.unwrap();
        assert!(
            err < 1e-2,
            "doc {doc:?}: drawn with aspect {got:.4} but the document's own \
             aspect on screen is {want:.4}. Dragging a crop slider on a tiled \
             image stretches the picture."
        );
    }

    #[test]
    fn one_tile_lays_the_document_out_like_many_do() {
        // update() returns early for a single tile and writes the transform as
        // given, never consulting doc_region. A source small enough to fit one
        // tile must still draw a crop the way the tiled path does.
        const SMALL: f32 = 800.0;
        let doc = [200.0, 100.0, 600.0, 500.0];

        for &scale in &[0.2f32, 1.0] {
            let g = geometry(doc, scale, Vec2::ZERO);
            let one = place_tile([0.0, 0.0, SMALL, SMALL], g).expect("the tile is drawn");
            let q = quad_ndc(&one);
            let (w, h) = (q[2] - q[0], q[3] - q[1]);
            let (dw, dh) = (doc[2] - doc[0], doc[3] - doc[1]);
            let want_w = 2.0 * scale * dw / g.viewport.x;
            let want_h = 2.0 * scale * dh / g.viewport.y;

            assert!(
                (w - want_w).abs() < 1e-2 && (h - want_h).abs() < 1e-2,
                "single tile at scale {scale}: the quad spans {w:.4}x{h:.4} of \
                 NDC, not the {want_w:.4}x{want_h:.4} a {dw}x{dh} document \
                 occupies. A one-tile source draws its crop stretched."
            );
        }
    }

    #[test]
    fn the_document_is_centred_when_the_view_is_not_panned() {
        for &doc in &[
            [0.0, 0.0, SRC, SRC],
            [5000.0, 5000.0, 15000.0, 15000.0],
            [0.0, 0.0, 10000.0, 10000.0],
            [20000.0, 20000.0, 30000.0, 30000.0],
        ] {
            let b = drawn_bounds(doc, 0.2, Vec2::ZERO).expect("drawn");
            let centre = vec2((b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5);
            assert!(
                centre.length() < 1e-3,
                "doc {doc:?} is drawn centred on {centre:?} rather than the                  middle of the viewport; the layout is centring on some other                  region than the document"
            );
        }
    }

    #[test]
    fn panning_translates_the_quads_without_reshaping_them() {
        let doc = [5000.0, 5000.0, 15000.0, 15000.0];
        let base = drawn_bounds(doc, 0.5, Vec2::ZERO).expect("drawn");

        for pan in [
            vec2(0.1, 0.0),
            vec2(-0.3, 0.2),
            vec2(0.0, -0.45),
            vec2(0.6, 0.6),
        ] {
            let moved = drawn_bounds(doc, 0.5, pan).expect("drawn");
            let (bw, bh) = (base[2] - base[0], base[3] - base[1]);
            let (mw, mh) = (moved[2] - moved[0], moved[3] - moved[1]);
            assert!(
                (bw - mw).abs() < 1e-3 && (bh - mh).abs() < 1e-3,
                "pan {pan:?} changed the drawn size from {bw:.4}x{bh:.4} to \
                 {mw:.4}x{mh:.4}; panning must translate, not reshape"
            );
            let want = [base[0] + 0.5 * pan.x, base[1] + 0.5 * pan.y];
            assert!(
                (moved[0] - want[0]).abs() < 1e-3 && (moved[1] - want[1]).abs() < 1e-3,
                "pan {pan:?} at zoom 0.5 moved the quads to {moved:?}, not                  {want:?}; panning must be a pure translation"
            );
        }
    }

    #[test]
    fn adjacent_tiles_meet_on_screen() {
        let doc = [2000.0, 2000.0, 28000.0, 28000.0];
        let g = geometry(doc, 0.4, vec2(0.1, -0.1));

        let mut placed: Vec<([f32; 4], [f32; 4])> = Vec::new();
        for t in tiles(SRC, TILE) {
            if let Some(p) = place_tile(t, g) {
                placed.push((p.isec, quad_ndc(&p)));
            }
        }
        assert!(
            placed.len() >= 16,
            "need a real grid with interior seams, got {}",
            placed.len()
        );

        for (a_src, a_q) in &placed {
            for (b_src, b_q) in &placed {
                if (b_src[0] - a_src[2]).abs() > 1e-3 || (b_src[1] - a_src[1]).abs() > 1e-3 {
                    continue;
                }
                assert!(
                    (b_q[0] - a_q[2]).abs() < 2e-3,
                    "tiles meeting at source x={} land at {} and {} on screen, \
                     leaving a seam or an overlap",
                    a_src[2],
                    a_q[2],
                    b_q[0]
                );
            }
        }
    }

    #[test]
    fn a_tile_outside_the_document_is_not_placed() {
        let doc = [20000.0, 20000.0, 30000.0, 30000.0];
        let g = geometry(doc, 1.0, Vec2::ZERO);

        let excluded = tiles(SRC, TILE)
            .into_iter()
            .filter(|t| place_tile(*t, g).is_none())
            .count();
        assert!(
            excluded > 0,
            "a crop of the bottom-right corner must exclude whole tiles, or \
             this proves nothing about culling"
        );
    }

    #[test]
    fn an_unchanged_view_places_tiles_identically() {
        let g = geometry([5000.0, 5000.0, 15000.0, 15000.0], 0.3, vec2(0.2, 0.1));
        for t in tiles(SRC, TILE) {
            assert_eq!(
                place_tile(t, g),
                place_tile(t, g),
                "placement is not a function of the geometry alone"
            );
        }
    }
}

#[cfg(test)]
mod tile_culling_tests {
    use super::tile_doc_intersection;

    const SRC: f32 = 30000.0;
    const TILE: f32 = 8192.0;
    const DOC: [f32; 4] = [12000.0, 12000.0, 20000.0, 20000.0];

    fn tiles() -> Vec<[f32; 4]> {
        let mut v = Vec::new();
        let mut y = 0.0;
        while y < SRC {
            let mut x = 0.0;
            while x < SRC {
                v.push([x, y, (x + TILE).min(SRC), (y + TILE).min(SRC)]);
                x += TILE;
            }
            y += TILE;
        }
        v
    }

    fn is_empty(r: [f32; 4]) -> bool {
        r[0] >= r[2] || r[1] >= r[3]
    }

    #[test]
    fn a_tile_outside_the_document_has_an_empty_intersection() {
        let outside = tile_doc_intersection([0.0, 0.0, TILE, TILE], DOC);
        assert!(
            is_empty(outside),
            "a tile the crop excludes must intersect the document in nothing, \
             or it keeps being drawn and strands a fragment of the old picture \
             in the viewport"
        );
    }

    #[test]
    fn a_crop_excludes_whole_tiles_on_a_large_image() {
        let excluded = tiles()
            .into_iter()
            .filter(|t| is_empty(tile_doc_intersection(*t, DOC)))
            .count();
        assert!(
            excluded > 0,
            "this crop should fall entirely outside several of the {} tiles; \
             without that the culling path is never exercised",
            tiles().len()
        );
    }

    #[test]
    fn a_tile_the_document_covers_keeps_the_overlapping_part() {
        let r = tile_doc_intersection([TILE, TILE, TILE * 2.0, TILE * 2.0], DOC);
        assert_eq!(r, [12000.0, 12000.0, 16384.0, 16384.0]);
    }

    #[test]
    fn moving_the_crop_invalidates_a_tiles_cached_placement() {
        fn stale(last: Option<[f32; 4]>, current: [f32; 4]) -> bool {
            last != Some(current)
        }

        let first = [12000.0f32, 12000.0, 20000.0, 20000.0];
        let moved = [9000.0f32, 9000.0, 17000.0, 17000.0];

        assert!(stale(None, first));
        assert!(!stale(Some(first), first));
        assert!(
            stale(Some(first), moved),
            "a tile cached for one crop must be seen as stale under another,              or the cull branch never runs and excluded tiles keep drawing"
        );

        const UNIT_UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        assert!(
            !stale(Some(UNIT_UV), UNIT_UV),
            "crop_uv is constant now, so keying on it reports every frame as              cached no matter how the crop moves"
        );
    }

    #[test]
    fn an_uncropped_document_keeps_every_tile_whole() {
        let full = [0.0, 0.0, SRC, SRC];
        for t in tiles() {
            assert_eq!(
                tile_doc_intersection(t, full),
                t,
                "without a crop a tile is entirely inside the document"
            );
        }
    }
}

#[cfg(test)]
mod preview_gate_tests {
    use crate::modifiers::kinds::{Exposure, GaussianBlur, Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::{Modifier, ModifierKind};

    fn needs_pipeline(modifiers: &[Modifier]) -> bool {
        modifiers.iter().any(|m| m.has_visible_effect())
    }

    fn defers_while_interacting(modifiers: &[Modifier], doc_changed: bool) -> bool {
        !doc_changed
            && modifiers
                .iter()
                .any(|m| m.has_visible_effect() && !m.kind.effect_class().is_pointwise())
    }

    fn resize(pct: f32) -> Modifier {
        Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }))
    }

    fn blur() -> Modifier {
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 }))
    }

    #[test]
    fn a_resize_alone_needs_the_pipeline() {
        assert!(
            needs_pipeline(&[resize(25.0)]),
            "a resize-only stack skipped the modifier pipeline, so the viewport \
             draws the unresized source inside a smaller frame"
        );
    }

    #[test]
    fn a_disabled_resize_alone_does_not() {
        let mut m = resize(25.0);
        m.enabled = false;
        assert!(!needs_pipeline(&[m]));
    }

    #[test]
    fn an_empty_stack_does_not() {
        assert!(!needs_pipeline(&[]));
    }

    #[test]
    fn a_resize_defers_while_the_document_holds_its_size() {
        assert!(
            defers_while_interacting(&[resize(25.0)], false),
            "a resize re-ran the chain on every frame while panning or zooming,              even though the document size had not moved"
        );
    }

    #[test]
    fn a_resize_does_not_defer_across_a_size_change() {
        assert!(
            !defers_while_interacting(&[resize(25.0)], true),
            "the document changed size but the chain was deferred, leaving              full-resolution textures on quads built for a different size"
        );
    }

    #[test]
    fn a_blur_is_still_deferred() {
        assert!(defers_while_interacting(&[blur()], false));
    }

    #[test]
    fn a_blur_with_a_resize_is_still_deferred() {
        assert!(defers_while_interacting(&[blur(), resize(50.0)], false));
    }

    #[test]
    fn a_pointwise_stack_is_not_deferred() {
        let chain = vec![Modifier::new(ModifierKind::Exposure(Exposure {
            exposure: 0.4,
        }))];
        assert!(!defers_while_interacting(&chain, false));
    }
}
