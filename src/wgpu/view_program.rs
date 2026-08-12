//! The viewport widget: pan, zoom, cursor readout, and the staged buffer the
//! eyedropper samples.
//!
//! The staged buffer comes from cpu::render_full, which returns the chain's
//! output size, not the source size. Anything indexing it must use the output
//! dimensions while reporting coordinates in source pixels, since that is what
//! the user is pointing at.
//!
//! effective_display_size and crop_origin are both in the resized document, and
//! crop fractions multiply the resized size, not the source -- mixing the two
//! put the pixel grid's transform in one space and its bounds in another.
//! Resize is applied before crop, matching export::geom_of, or the preview and
//! the file would disagree.
//!
//! The histogram renders on a *bounded* source, not the document. Rendering
//! the whole document costs time proportional to its area and is paid on every
//! modifier change: 431 ms at 12000px, ~2.7 s at 30000px, felt directly as lag
//! while dragging a resize slider. The source is strided down to
//! HISTOGRAM_PRERENDER_PIXELS first, which keeps a wide margin over the sample
//! target so a downscale still has real neighbors to average and fine detail is
//! not aliased into a different distribution than the export produces.
//!
//! Shrinking the source changes what a stage means, so the chain is adjusted to
//! match: kernel parameters in absolute pixels are scaled by the same factor
//! (the identity the reduced-scale blur path already relies on), and a resize
//! in Pixels mode is rewritten as the equivalent percentage so it follows the
//! smaller source. Text, Drawing, and PixelSort are not expressible that way --
//! their rasters and scanlines are built from the document's own size -- so a
//! chain containing them renders at full size instead.
//!
//! Two modifier hashes, and the difference is load-bearing. hash_modifiers is
//! exact, because the staged-render and eyedropper caches hold buffers whose
//! dimensions come from the chain, so a collision hands back a wrong-sized
//! buffer. hash_modifiers_for_histogram is deliberately coarser and must mirror
//! what compute_subsampled_histogram renders: disabled modifiers skipped, and
//! every resize measured against the source, which is the space the upscale
//! clamp decides in. An upscale is clamped to 100% before rendering because it
//! interpolates between pixels already present and moves the distribution
//! little, while costing a full resample per slider tick; a downscale averages
//! neighbors and measurably narrows the distribution, so it is kept.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, vec2, vec3, vec4};
use iced::{
    Event, Point, Rectangle,
    mouse::{self, Button, Cursor, Interaction},
    widget::{Action, shader::Program},
};
use rayon::prelude::*;

use crate::{
    app::Message,
    export::{ExportData, ExportFrame, ExportSource},
    modifiers::{
        Modifier, cpu,
        drawing_raster::{DrawingLayerCache, LayerView},
        kinds::{Resize, ResizeMode},
        plan::{ImageSpec, chain_output_spec, plan_modifiers},
        text_raster::TextRaster,
    },
    wgpu::{
        media::animation::Animation,
        media::exif_data::ExifData,
        media::image_data::ImageData,
        passes::{checkerboard::CheckerboardUniforms, pixel_grid::PixelGridUniforms},
        scale::Scale,
        view_pipeline::DisplayUniforms,
        view_primitive::ViewPrimitive,
    },
};

pub(crate) type Histogram = ([u32; 256], [u32; 256], [u32; 256]);

const HISTOGRAM_TARGET_SAMPLES: usize = 250_000;

const SCALE_COOLDOWN: Duration = Duration::from_millis(30);

pub struct ViewProgramState {
    pub drag: ViewDragState,
    pub last_scale: Option<Instant>,
}

impl Default for ViewProgramState {
    fn default() -> Self {
        Self {
            drag: ViewDragState::Idle,
            last_scale: None,
        }
    }
}

#[derive(Default)]
pub enum ViewDragState {
    #[default]
    Idle,
    Panning(Point, Button),
}

pub(crate) fn wheel_scale_msg(
    last_scale: &mut Option<Instant>,
    delta: &mouse::ScrollDelta,
    pos: Vec2,
) -> Option<Message> {
    let scale_msg = |y: f32| {
        if y > 0.0 {
            Message::ScaleUp(pos)
        } else {
            Message::ScaleDown(pos)
        }
    };
    match delta {
        mouse::ScrollDelta::Lines { y, .. } if *y != 0.0 => {
            *last_scale = None;
            Some(scale_msg(*y))
        }
        mouse::ScrollDelta::Pixels { y, .. } if *y != 0.0 => {
            let now = Instant::now();
            if last_scale.is_none_or(|t| now.duration_since(t) >= SCALE_COOLDOWN) {
                *last_scale = Some(now);
                Some(scale_msg(*y))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Clone)]
pub struct ViewProgram {
    offset: Vec2,
    image_size: Vec2,
    scale: Scale,
    fit_active: bool,
    bounds: Rectangle,
    image: Option<Arc<ImageData>>,
    animation: Option<Animation>,
    pub show_checkerboard: bool,
    pub checker_uniforms: CheckerboardUniforms,
    pub show_pixel_grid: bool,
    pub mipmap_zoom_out: bool,
    pub smooth_zoom_in: bool,
    pub loop_animations: bool,
    uploaded_mipmap_zoom_out: bool,
    cursor_image_pos: Option<Vec2>,
    panning: bool,
    rotation: u8,
    pub modifiers: Arc<Vec<Modifier>>,
    pub crop_tool_active: bool,
    dirty: Arc<std::sync::atomic::AtomicBool>,
    pre_clear_gpu: Arc<std::sync::atomic::AtomicBool>,
    reprocess_pending: Arc<std::sync::atomic::AtomicBool>,
    raster_cache: Arc<std::sync::Mutex<Option<RasterCache>>>,
    eyedropper_cache: Arc<std::sync::Mutex<Option<EyedropperCache>>>,
    staged_cache: Arc<std::sync::Mutex<Option<StagedCache>>>,
}

struct RasterCache {
    text_key: u64,
    w: u32,
    h: u32,
    text: Vec<Option<TextRaster>>,
    drawing: Vec<Option<DrawingLayerCache>>,
}

struct StagedCache {
    key: u64,
    w: u32,
    h: u32,
    pixels: Vec<u8>,
}

struct EyedropperCache {
    key: u64,
    info: Option<(u32, u32, Vec2, [u8; 4])>,
    pixels: std::collections::HashMap<u32, Vec<u8>>,
}

impl Default for ViewProgram {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            image_size: Vec2::ZERO,
            scale: Scale::default(),
            fit_active: true,
            bounds: Rectangle::default(),
            image: None,
            animation: None,
            show_checkerboard: false,
            checker_uniforms: CheckerboardUniforms {
                color_a: [0.8, 0.8, 0.8, 1.0],
                color_b: [0.6, 0.6, 0.6, 1.0],
                tile_size: 12.0,
                _pad: [0.0; 3],
            },
            show_pixel_grid: false,
            cursor_image_pos: None,
            panning: false,
            rotation: 0,
            mipmap_zoom_out: true,
            smooth_zoom_in: false,
            loop_animations: true,
            uploaded_mipmap_zoom_out: true,
            modifiers: Arc::new(Vec::new()),
            crop_tool_active: false,
            dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pre_clear_gpu: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reprocess_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            raster_cache: Arc::new(std::sync::Mutex::new(None)),
            eyedropper_cache: Arc::new(std::sync::Mutex::new(None)),
            staged_cache: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl ViewProgram {
    pub fn mark_dirty(&self) {
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn reprocess_pending(&self) -> bool {
        self.reprocess_pending
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn modifiers_mut(&mut self) -> &mut Vec<Modifier> {
        Arc::make_mut(&mut self.modifiers)
    }

    fn reset_crop_to_image(&mut self) {
        let size = self.image_size;
        for m in self.modifiers_mut() {
            if let Some(crop) = m.kind.as_crop_mut() {
                crop.x = 0.0;
                crop.y = 0.0;
                crop.width = size.x;
                crop.height = size.y;
            }
        }
    }

    pub fn set_bounds(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
        if self.fit_active {
            self.fit();
        }
        self.clamp_offset();
    }

    pub fn viewport_center(&self) -> Vec2 {
        vec2(self.bounds.width * 0.5, self.bounds.height * 0.5)
    }

    pub fn fit(&mut self) {
        self.fit_active = true;
        if self.image_size == Vec2::ZERO {
            return;
        }
        self.scale.custom(self.fit_scale());
        self.offset = Vec2::ZERO;
    }

    fn fit_scale(&self) -> f32 {
        let eff = self.effective_display_size();
        let (fw, fh) = if self.rotation.is_multiple_of(2) {
            (eff.x, eff.y)
        } else {
            (eff.y, eff.x)
        };
        (self.bounds.width / fw).min(self.bounds.height / fh)
    }

    pub fn set_base_rotation(&mut self, quarter_turns: u8) {
        self.rotation = quarter_turns % 4;
        self.fit();
    }

    pub fn rotate(&mut self) {
        self.rotation = (self.rotation + 1) % 4;
        self.fit();
    }

    pub fn rotate_ccw(&mut self) {
        self.rotation = (self.rotation + 3) % 4;
        self.fit();
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.fit_active = false;
        self.offset += 2.0 * delta / self.scale.value();
        self.clamp_offset();
    }

    pub fn scale_up(&mut self, cursor: Vec2) {
        self.fit_active = false;
        let prev = self.scale.up();
        self.scale_offset(cursor, prev);
        self.clamp_offset();
    }

    pub fn scale_down(&mut self, cursor: Vec2) {
        self.fit_active = false;
        let prev = self.scale.down();
        self.scale_offset(cursor, prev);
        self.clamp_offset();
    }

    pub fn set_scale(&mut self, scale: f32, cursor: Vec2) {
        self.fit_active = false;
        let prev = self.scale.value();
        self.scale.custom(scale);
        self.scale_offset(cursor, prev);
        self.clamp_offset();
    }

    fn scale_offset(&mut self, cursor: Vec2, prev: f32) {
        let viewport = vec2(self.bounds.width, self.bounds.height);
        let ndc = vec2(
            (cursor.x / viewport.x) * 2.0 - 1.0,
            1.0 - (cursor.y / viewport.y) * 2.0,
        );
        let factor = (1.0 / self.scale.value()) - (1.0 / prev);
        self.offset += viewport * ndc * factor;
    }

    fn clamp_offset(&mut self) {
        let eff = self.effective_display_size();
        let size = if self.rotation.is_multiple_of(2) {
            eff
        } else {
            vec2(eff.y, eff.x)
        };
        self.offset = self.offset.clamp(-size, size);
    }

    fn build_transform(&self, viewport: Vec2) -> Mat4 {
        let s = self.scale.value();
        let aspect = self.aspect(viewport);
        let pan_ndc = self.offset / viewport;
        let angle = -(self.rotation as f32) * std::f32::consts::FRAC_PI_2;
        Mat4::from_scale(vec3(s, s, 1.0))
            * Mat4::from_translation(vec3(pan_ndc.x, pan_ndc.y, 0.0))
            * Mat4::from_rotation_z(angle)
            * Mat4::from_scale(vec3(aspect.x, aspect.y, 1.0))
    }

    fn grid_uniforms(&self, bounds: Rectangle) -> Option<PixelGridUniforms> {
        let viewport = vec2(bounds.width, bounds.height);
        if !self.show_pixel_grid
            || self.image_size == Vec2::ZERO
            || viewport.x < 1.0
            || viewport.y < 1.0
        {
            return None;
        }
        let eff = self.effective_display_size();
        let origin = self.crop_origin();
        let to_pixels =
            Mat4::from_translation(vec3(0.5 * eff.x + origin.x, 0.5 * eff.y + origin.y, 0.0))
                * Mat4::from_scale(vec3(0.5 * eff.x, -0.5 * eff.y, 1.0));
        let screen_to_img = to_pixels * self.build_transform(viewport).inverse();
        Some(PixelGridUniforms {
            screen_to_img,
            viewport: [bounds.x, bounds.y, viewport.x, viewport.y],
            bounds_img: [origin.x, origin.y, origin.x + eff.x, origin.y + eff.y],
        })
    }

    fn aspect(&self, viewport: Vec2) -> Vec2 {
        let eff = self.effective_display_size();
        if self.rotation.is_multiple_of(2) {
            eff / viewport
        } else {
            vec2(eff.x / viewport.y, eff.y / viewport.x)
        }
    }

    pub fn set_image(&mut self, data: ImageData) {
        self.set_display_image(Arc::new(data));
    }

    fn set_display_image(&mut self, data: Arc<ImageData>) {
        self.image_size = vec2(data.width as f32, data.height as f32);
        self.image = Some(data);
        self.animation = None;
        self.cursor_image_pos = Some(self.image_size / 2.0);
        self.panning = false;
        self.rotation = 0;
        self.uploaded_mipmap_zoom_out = self.mipmap_zoom_out;
        self.reset_crop_to_image();
    }

    #[cfg(feature = "av")]
    pub fn set_video_frame(&mut self, data: Arc<ImageData>, first: bool) {
        if first {
            self.set_display_image(data);
        } else {
            self.image = Some(data);
        }
    }

    pub fn current_image(&self) -> Option<Arc<ImageData>> {
        self.image.clone()
    }

    pub fn exif(&self) -> Option<&ExifData> {
        self.image.as_deref().map(|d| &d.exif)
    }

    pub fn bit_depth(&self) -> Option<u8> {
        self.image.as_deref().map(|d| d.bit_depth)
    }

    pub fn color_space(&self) -> Option<&str> {
        self.image.as_deref().and_then(|d| {
            d.color_space
                .map(|s| s as &str)
                .or(d.exif.color_space.as_deref())
        })
    }

    pub fn set_animation(&mut self, mut anim: Animation) {
        anim.set_looping(self.loop_animations);
        let first = Arc::clone(anim.current_image());
        self.image_size = vec2(first.width as f32, first.height as f32);
        self.image = Some(first);
        self.animation = Some(anim);
        self.cursor_image_pos = Some(self.image_size / 2.0);
        self.panning = false;
        self.rotation = 0;
        self.uploaded_mipmap_zoom_out = self.mipmap_zoom_out;
        self.reset_crop_to_image();
    }

    pub fn set_cursor_pos(&mut self, pos: Option<Vec2>) {
        if !self.panning
            && let Some(new_pos) = pos.and_then(|p| {
                Some(
                    self.screen_to_image_coords(p)?
                        .clamp(Vec2::ZERO, self.image_size - Vec2::ONE),
                )
            })
        {
            self.cursor_image_pos = Some(new_pos);
        }
    }

    pub fn set_cursor_from_window(&mut self, window_pos: Vec2) {
        let local = window_pos - vec2(self.bounds.x, self.bounds.y);
        self.set_cursor_pos(Some(local));
    }

    pub fn set_panning(&mut self, panning: bool) {
        self.panning = panning;
    }

    pub fn seek_animation(&mut self, index: usize) {
        if let Some(ref mut anim) = self.animation {
            self.image = Some(anim.seek(index));
        }
    }

    pub fn resume_animation(&mut self) {
        if let Some(ref mut anim) = self.animation {
            anim.resume();
        }
    }

    pub fn tick_animation(&mut self, now: Instant) {
        if let Some(ref mut anim) = self.animation
            && let Some(frame) = anim.tick(now)
        {
            self.image = Some(frame);
        }
    }

    pub fn set_loop_animations(&mut self, looping: bool) {
        self.loop_animations = looping;
        if let Some(ref mut anim) = self.animation {
            anim.set_looping(looping);
        }
    }

    pub fn animation_ended(&self) -> bool {
        self.animation.as_ref().is_some_and(Animation::ended)
    }

    pub fn time_until_next_frame(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.time_until_next_frame())
    }

    pub fn scale(&self) -> f32 {
        self.scale.value()
    }

    pub fn fit_active(&self) -> bool {
        self.fit_active
    }

    pub fn set_fit_active(&mut self, active: bool) {
        self.fit_active = active;
    }

    pub fn rotation(&self) -> u8 {
        self.rotation
    }

    pub fn image_size(&self) -> Option<(u32, u32)> {
        if self.image_size == Vec2::ZERO {
            return None;
        }
        Some((self.image_size.x as u32, self.image_size.y as u32))
    }

    fn crop(&self) -> Option<[f32; 4]> {
        if self.image_size == Vec2::ZERO {
            return None;
        }
        self.modifiers.iter().find_map(|m| {
            if !m.enabled {
                return None;
            }
            let crop = m.kind.as_crop()?;
            let iw = self.image_size.x;
            let ih = self.image_size.y;
            Some([
                crop.x / iw,
                crop.y / ih,
                (crop.x + crop.width) / iw,
                (crop.y + crop.height) / ih,
            ])
        })
    }

    fn displayed_crop(&self) -> Option<[f32; 4]> {
        self.crop().filter(|_| !self.crop_tool_active)
    }

    pub fn active_trim(&self, duration: Duration) -> Option<(Duration, Duration)> {
        let trim = self
            .modifiers
            .iter()
            .find_map(|m| m.enabled.then(|| m.kind.as_trim()).flatten())?;
        (!trim.is_full()).then(|| trim.resolve(duration))
    }

    fn effective_display_size(&self) -> Vec2 {
        let resized = self.chain_output_size();
        if let Some([min_u, min_v, max_u, max_v]) = self.displayed_crop() {
            vec2((max_u - min_u) * resized.x, (max_v - min_v) * resized.y)
        } else {
            resized
        }
    }

    fn crop_origin(&self) -> Vec2 {
        match self.displayed_crop() {
            Some([min_u, min_v, ..]) => {
                let doc = self.chain_output_size();
                vec2(min_u * doc.x, min_v * doc.y)
            }
            None => Vec2::ZERO,
        }
    }

    fn chain_output_size(&self) -> Vec2 {
        if self.image_size == Vec2::ZERO {
            return self.image_size;
        }
        let out = chain_output_spec(
            ImageSpec::new(self.image_size.x as u32, self.image_size.y as u32),
            &plan_modifiers(&self.modifiers),
        );
        vec2(out.w as f32, out.h as f32)
    }

    pub fn animation_info(&self) -> Option<(usize, usize)> {
        self.animation
            .as_ref()
            .map(|a| (a.current_index(), a.frame_count()))
    }

    pub fn animation_duration(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.total_duration())
    }

    pub fn animation_delays(&self) -> impl Iterator<Item = Duration> + '_ {
        self.animation
            .iter()
            .flat_map(|a| a.frames().iter().map(|f| f.delay))
    }

    pub fn animation_timestamp(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.current_timestamp())
    }

    pub fn decoded_size_bytes(&self) -> Option<usize> {
        self.image.as_ref().map(|img| img.size_bytes())
    }

    pub fn vram_usage_bytes(&self) -> Option<usize> {
        let base = self.decoded_size_bytes()?;
        Some(if self.uploaded_mipmap_zoom_out {
            base * 4 / 3
        } else {
            base
        })
    }

    pub fn screen_to_image_uv(&self, screen_pos: Vec2) -> Option<Vec2> {
        let coords = self.screen_to_image_coords(screen_pos)?;
        Some(coords / self.image_size)
    }

    pub fn image_uv_to_screen(&self, uv: Vec2) -> Option<Vec2> {
        let viewport = vec2(self.bounds.width, self.bounds.height);
        if self.image_size == Vec2::ZERO || viewport.x < 1.0 || viewport.y < 1.0 {
            return None;
        }
        let display_uv = if let Some([min_u, min_v, max_u, max_v]) = self.displayed_crop() {
            let span = vec2((max_u - min_u).max(1e-6), (max_v - min_v).max(1e-6));
            vec2((uv.x - min_u) / span.x, (uv.y - min_v) / span.y)
        } else {
            uv
        };
        let img_ndc = vec4(display_uv.x * 2.0 - 1.0, 1.0 - display_uv.y * 2.0, 0.0, 1.0);
        let screen_ndc = self.build_transform(viewport) * img_ndc;
        Some(vec2(
            (screen_ndc.x + 1.0) * 0.5 * viewport.x,
            (1.0 - screen_ndc.y) * 0.5 * viewport.y,
        ))
    }

    fn screen_to_image_coords(&self, screen_pos: Vec2) -> Option<Vec2> {
        let viewport = vec2(self.bounds.width, self.bounds.height);
        if self.image_size == Vec2::ZERO || viewport.x < 1.0 || viewport.y < 1.0 {
            return None;
        }
        let screen_ndc = vec2(
            (screen_pos.x / viewport.x) * 2.0 - 1.0,
            1.0 - (screen_pos.y / viewport.y) * 2.0,
        );
        let img_ndc = (self.build_transform(viewport).inverse()
            * vec4(screen_ndc.x, screen_ndc.y, 0.0, 1.0))
        .truncate()
        .truncate();
        let eff = self.effective_display_size();
        let local_px = (img_ndc + 1.0) * 0.5 * vec2(eff.x, -eff.y) + vec2(0.0, eff.y);
        let doc_px = local_px + self.crop_origin();

        let doc = self.chain_output_size();
        if doc == Vec2::ZERO || doc == self.image_size {
            return Some(doc_px);
        }
        Some(doc_px * self.image_size / doc)
    }

    fn with_rasters<R>(
        &self,
        img_w: u32,
        img_h: u32,
        f: impl FnOnce(&[Option<TextRaster>], &[Option<LayerView<'_>>]) -> R,
    ) -> R {
        use crate::modifiers::ModifierKind;

        let text_key = hash_text_modifiers(&self.modifiers);
        let mut cache = self.raster_cache.lock().unwrap_or_else(|e| e.into_inner());
        let stale = cache
            .as_ref()
            .map(|c| (c.text_key, c.w, c.h) != (text_key, img_w, img_h))
            .unwrap_or(true);
        if stale {
            let drawing = cache
                .take()
                .filter(|c| (c.w, c.h) == (img_w, img_h))
                .map(|c| c.drawing)
                .unwrap_or_default();
            *cache = Some(RasterCache {
                text_key,
                w: img_w,
                h: img_h,
                text: crate::modifiers::text_raster::build_layers(&self.modifiers, img_w, img_h),
                drawing,
            });
        }
        let c = cache.as_mut().unwrap();
        if c.drawing.len() != self.modifiers.len() {
            c.drawing.clear();
            c.drawing.resize_with(self.modifiers.len(), || None);
        }
        for (i, m) in self.modifiers.iter().enumerate() {
            match &m.kind {
                ModifierKind::Drawing(d) if m.has_visible_effect() => {
                    let entry =
                        c.drawing[i].get_or_insert_with(|| DrawingLayerCache::new(img_w, img_h));
                    let _ = entry.sync(d);
                }
                _ => {
                    c.drawing[i] = None;
                }
            }
        }
        let views: Vec<Option<LayerView<'_>>> = c
            .drawing
            .iter()
            .map(|o| o.as_ref().map(|k| k.view()))
            .collect();
        f(&c.text, &views)
    }

    fn has_any_visible_modifier(&self) -> bool {
        self.modifiers.iter().any(|m| m.has_visible_effect())
    }

    fn sample_pixel(
        &self,
        text_layers: &[Option<TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        px: u32,
        py: u32,
    ) -> Option<[u8; 4]> {
        let image = self.image.as_ref()?;
        self.staged_pixel(text_layers, drawing_layers, image, px, py)
    }

    fn staged_rows(
        &self,
        text_layers: &[Option<TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        image: &ImageData,
        y0: u32,
        y1: u32,
    ) -> Option<(Vec<u8>, u32, u32)> {
        let (w, h) = (image.width, image.height);
        let plan = plan_modifiers(&self.modifiers);
        if !cpu::plan_is_bandable(ImageSpec::new(w, h), &plan) {
            return None;
        }
        let out = chain_output_spec(ImageSpec::new(w, h), &plan);
        let y0 = y0.min(out.h.saturating_sub(1));
        let y1 = y1.clamp(y0 + 1, out.h.max(1));
        let pixels = image.pixels_snapshot();
        if pixels.len() < image.size_bytes() {
            return None;
        }
        let band = cpu::render_band(
            &self.modifiers,
            text_layers,
            drawing_layers,
            &pixels,
            w,
            h,
            y0,
            y1,
        );
        Some((band, out.w, y0))
    }

    fn with_staged<R>(
        &self,
        text_layers: &[Option<TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        image: &ImageData,
        f: impl FnOnce(&[u8], u32, u32) -> R,
    ) -> Option<R> {
        let (w, h) = (image.width, image.height);
        let key = {
            let mut hasher = DefaultHasher::new();
            image.id.hash(&mut hasher);
            hash_modifiers(&self.modifiers).hash(&mut hasher);
            hasher.finish()
        };
        let mut guard = self.staged_cache.lock().unwrap_or_else(|e| e.into_inner());
        let stale = guard.as_ref().map(|c| c.key != key).unwrap_or(true);
        if stale {
            let pixels = image.pixels_snapshot();
            if pixels.len() < image.size_bytes() {
                return None;
            }
            let staged =
                cpu::render_full(&self.modifiers, text_layers, drawing_layers, &pixels, w, h);
            let out = chain_output_spec(ImageSpec::new(w, h), &plan_modifiers(&self.modifiers));
            *guard = Some(StagedCache {
                key,
                w: out.w,
                h: out.h,
                pixels: staged,
            });
        }
        let cache = guard.as_ref()?;
        Some(f(&cache.pixels, cache.w, cache.h))
    }

    fn staged_pixel(
        &self,
        text_layers: &[Option<TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        image: &ImageData,
        px: u32,
        py: u32,
    ) -> Option<[u8; 4]> {
        let (src_w, src_h) = (image.width.max(1), image.height.max(1));
        let out = chain_output_spec(
            ImageSpec::new(image.width, image.height),
            &plan_modifiers(&self.modifiers),
        );
        let map = |w: u32, h: u32| -> (u32, u32) {
            let sx = if w == src_w {
                px
            } else {
                (px as u64 * w as u64 / src_w as u64) as u32
            };
            let sy = if h == src_h {
                py
            } else {
                (py as u64 * h as u64 / src_h as u64) as u32
            };
            (sx.min(w.saturating_sub(1)), sy.min(h.saturating_sub(1)))
        };

        let (_, sy) = map(out.w, out.h);
        if let Some((band, bw, row_offset)) =
            self.staged_rows(text_layers, drawing_layers, image, sy, sy + 1)
        {
            let (sx, _) = map(bw, out.h);
            let local = sy.saturating_sub(row_offset);
            let idx = (local as usize * bw as usize + sx as usize) * 4;
            if let Some(p) = band.get(idx..idx + 4) {
                return Some([p[0], p[1], p[2], p[3]]);
            }
        }

        self.with_staged(text_layers, drawing_layers, image, |staged, w, h| {
            let (sx, sy) = map(w, h);
            let idx = (sy as usize * w as usize + sx as usize) * 4;
            staged.get(idx..idx + 4).map(|p| [p[0], p[1], p[2], p[3]])
        })?
    }

    pub fn color_at_window(&self, window_pos: Vec2) -> Option<[u8; 4]> {
        let local = window_pos - vec2(self.bounds.x, self.bounds.y);
        let img = self.screen_to_image_coords(local)?;
        if img.x < 0.0 || img.y < 0.0 || img.x >= self.image_size.x || img.y >= self.image_size.y {
            return None;
        }
        let (px, py) = (img.x as u32, img.y as u32);
        self.with_rasters(
            self.image_size.x as u32,
            self.image_size.y as u32,
            |text, drawing| self.sample_pixel(text, drawing, px, py),
        )
    }

    pub fn export_data(&self) -> Option<ExportData> {
        let anim = match &self.animation {
            Some(anim) => anim,
            None => return self.export_frame_data(),
        };

        let frames = anim
            .frames()
            .iter()
            .map(|f| ExportFrame {
                pixels: f.data.pixels_snapshot(),
                delay: f.delay,
            })
            .collect();
        let first = &anim.frames()[0].data;
        Some(self.build_export(frames, anim.current_index(), first.width, first.height))
    }

    pub fn export_frame_data(&self) -> Option<ExportData> {
        let image = self.image.as_ref()?;
        let frames = vec![ExportFrame {
            pixels: image.pixels_snapshot(),
            delay: Duration::ZERO,
        }];
        Some(self.build_export(frames, 0, image.width, image.height))
    }

    fn build_export(
        &self,
        frames: Vec<ExportFrame>,
        still_index: usize,
        width: u32,
        height: u32,
    ) -> ExportData {
        let duration = frames.iter().map(|f| f.delay).sum();
        ExportData {
            source: ExportSource::Frames {
                frames,
                still_index,
            },
            width,
            height,
            modifiers: self.modifiers.as_ref().clone(),
            crop: self.crop(),
            rotation: self.rotation,
            trim: self.active_trim(duration),
        }
    }

    #[cfg(feature = "av")]
    pub fn build_video_export(&self, info: &crate::wgpu::media::video::VideoInfo) -> ExportData {
        ExportData {
            source: ExportSource::Video(crate::export::VideoExportInfo {
                path: info.path.clone(),
                frame_count: info.frame_count,
                duration: info.duration,
            }),
            width: info.width,
            height: info.height,
            modifiers: self.modifiers.as_ref().clone(),
            crop: self.crop(),
            rotation: self.rotation,
            trim: self.active_trim(info.duration),
        }
    }

    fn eyedropper_key(&self) -> Option<u64> {
        let img = self.cursor_image_pos?;
        let image = self.image.as_ref()?;
        let mut hasher = DefaultHasher::new();
        (img.x as i64).hash(&mut hasher);
        (img.y as i64).hash(&mut hasher);
        image.id.hash(&mut hasher);
        self.rotation.hash(&mut hasher);
        hash_modifiers(&self.modifiers).hash(&mut hasher);
        Some(hasher.finish())
    }

    pub fn cursor_info(&self) -> Option<(u32, u32, Vec2, [u8; 4])> {
        let key = self.eyedropper_key();
        if let Some(key) = key
            && let Ok(guard) = self.eyedropper_cache.lock()
            && let Some(cache) = guard.as_ref()
            && cache.key == key
            && let Some(info) = cache.info
        {
            return Some(info);
        }

        let img = self.cursor_image_pos?;
        let (px, py) = (img.x as u32, img.y as u32);
        let uv = img / self.image_size;

        let rgba = if self.has_any_visible_modifier() {
            self.with_rasters(
                self.image_size.x as u32,
                self.image_size.y as u32,
                |text, drawing| self.sample_pixel(text, drawing, px, py),
            )?
        } else {
            let image = self.image.as_ref()?;
            let idx = (py as usize * image.width as usize + px as usize) * 4;
            let pixels = image.pixels_snapshot();
            let p = pixels.get(idx..idx + 4)?;
            [p[0], p[1], p[2], p[3]]
        };
        let info = Some((px, py, uv, rgba));

        if let Some(key) = key
            && let Ok(mut guard) = self.eyedropper_cache.lock()
        {
            match guard.as_mut() {
                Some(cache) if cache.key == key => cache.info = info,
                _ => {
                    *guard = Some(EyedropperCache {
                        key,
                        info,
                        pixels: std::collections::HashMap::new(),
                    })
                }
            }
        }
        info
    }

    pub fn cursor_pixels(&self, size: u32) -> Option<Vec<u8>> {
        let key = self.eyedropper_key();
        if let Some(key) = key
            && let Ok(guard) = self.eyedropper_cache.lock()
            && let Some(cache) = guard.as_ref()
            && cache.key == key
            && let Some(pixels) = cache.pixels.get(&size)
        {
            return Some(pixels.clone());
        }

        let img = self.cursor_image_pos?;
        let (cx, cy) = (img.x as i64, img.y as i64);
        let half = (size / 2) as i64;
        let image = self.image.as_ref()?;
        let (w, h) = (image.width as i64, image.height as i64);
        let buf = image.pixels_snapshot();
        if buf.is_empty() {
            return None;
        }

        let coord = |row: i64, col: i64| -> (i64, i64) {
            match self.rotation {
                0 => (cx - half + col, cy - half + row),
                1 => (cx - half + row, cy + half - col),
                2 => (cx + half - col, cy + half - row),
                3 => (cx + half - row, cy - half + col),
                _ => unreachable!(),
            }
        };

        let mut pixels = vec![0u8; (size * size * 4) as usize];

        if !self.has_any_visible_modifier() {
            for row in 0..size as i64 {
                for col in 0..size as i64 {
                    let (x, y) = coord(row, col);
                    if x < 0 || y < 0 || x >= w || y >= h {
                        continue;
                    }
                    let src = (y as usize * w as usize + x as usize) * 4;
                    let dst = ((row * size as i64 + col) * 4) as usize;
                    pixels[dst..dst + 4].copy_from_slice(&buf[src..src + 4]);
                }
            }
            self.store_cursor_pixels(key, size, &pixels);
            return Some(pixels);
        }

        let out = chain_output_spec(
            ImageSpec::new(image.width, image.height),
            &plan_modifiers(&self.modifiers),
        );
        let (ow, oh) = (out.w as i64, out.h as i64);
        let ocx = if ow == w { cx } else { cx * ow / w.max(1) };
        let ocy = if oh == h { cy } else { cy * oh / h.max(1) };

        let ocoord = |row: i64, col: i64| -> (i64, i64) {
            match self.rotation {
                0 => (ocx - half + col, ocy - half + row),
                1 => (ocx - half + row, ocy + half - col),
                2 => (ocx + half - col, ocy + half - row),
                3 => (ocx + half - row, ocy - half + col),
                _ => unreachable!(),
            }
        };

        let (y0, y1) = {
            let mut lo = i64::MAX;
            let mut hi = i64::MIN;
            for row in 0..size as i64 {
                for col in 0..size as i64 {
                    let (_, sy) = ocoord(row, col);
                    if sy < 0 || sy >= oh {
                        continue;
                    }
                    lo = lo.min(sy);
                    hi = hi.max(sy);
                }
            }
            if lo > hi {
                return None;
            }
            (lo.max(0) as u32, (hi + 1).max(0) as u32)
        };

        let banded = self.with_rasters(image.width, image.height, |text, drawing| {
            self.staged_rows(text, drawing, image, y0, y1)
        });

        let plot = |pixels: &mut [u8], staged: &[u8], sw: u32, sh: u32, row_offset: u32| {
            for row in 0..size as i64 {
                for col in 0..size as i64 {
                    let (sx, sy) = ocoord(row, col);
                    let sy = sy - row_offset as i64;
                    if sx < 0 || sy < 0 || sx >= sw as i64 || sy >= sh as i64 {
                        continue;
                    }
                    let src = (sy as usize * sw as usize + sx as usize) * 4;
                    let Some(p) = staged.get(src..src + 4) else {
                        continue;
                    };
                    let dst = ((row * size as i64 + col) * 4) as usize;
                    pixels[dst..dst + 4].copy_from_slice(p);
                }
            }
        };

        match banded {
            Some((band, sw, row_offset)) => {
                let rows = if sw == 0 {
                    0
                } else {
                    (band.len() / (sw as usize * 4)) as u32
                };
                plot(&mut pixels, &band, sw, rows, row_offset);
            }
            None => {
                self.with_rasters(image.width, image.height, |text, drawing| {
                    self.with_staged(text, drawing, image, |staged, sw, sh| {
                        plot(&mut pixels, staged, sw, sh, 0);
                    })
                })?;
            }
        }
        self.store_cursor_pixels(key, size, &pixels);
        Some(pixels)
    }

    #[cfg(test)]
    pub(crate) fn cursor_pixels_via_full_render(&self, size: u32) -> Option<Vec<u8>> {
        let img = self.cursor_image_pos?;
        let (cx, cy) = (img.x as i64, img.y as i64);
        let half = (size / 2) as i64;
        let image = self.image.as_ref()?;
        let (w, h) = (image.width as i64, image.height as i64);

        let mut pixels = vec![0u8; (size * size * 4) as usize];
        self.with_rasters(image.width, image.height, |text, drawing| {
            self.with_staged(text, drawing, image, |staged, sw, sh| {
                let ocx = if sw as i64 == w {
                    cx
                } else {
                    cx * sw as i64 / w.max(1)
                };
                let ocy = if sh as i64 == h {
                    cy
                } else {
                    cy * sh as i64 / h.max(1)
                };
                for row in 0..size as i64 {
                    for col in 0..size as i64 {
                        let (sx, sy) = match self.rotation {
                            0 => (ocx - half + col, ocy - half + row),
                            1 => (ocx - half + row, ocy + half - col),
                            2 => (ocx + half - col, ocy + half - row),
                            3 => (ocx + half - row, ocy - half + col),
                            _ => unreachable!(),
                        };
                        if sx < 0 || sy < 0 || sx >= sw as i64 || sy >= sh as i64 {
                            continue;
                        }
                        let src = (sy as usize * sw as usize + sx as usize) * 4;
                        let Some(p) = staged.get(src..src + 4) else {
                            continue;
                        };
                        let dst = ((row * size as i64 + col) * 4) as usize;
                        pixels[dst..dst + 4].copy_from_slice(p);
                    }
                }
            })
        })?;
        Some(pixels)
    }

    fn store_cursor_pixels(&self, key: Option<u64>, size: u32, pixels: &[u8]) {
        let Some(key) = key else { return };
        let Ok(mut guard) = self.eyedropper_cache.lock() else {
            return;
        };
        match guard.as_mut() {
            Some(cache) if cache.key == key => {
                cache.pixels.insert(size, pixels.to_vec());
            }
            _ => {
                let mut map = std::collections::HashMap::new();
                map.insert(size, pixels.to_vec());
                *guard = Some(EyedropperCache {
                    key,
                    info: None,
                    pixels: map,
                });
            }
        }
    }

    pub fn release_image_pixels(&self) {
        if let Some(image) = &self.image {
            image.release_pixels();
        }
        self.pre_clear_gpu
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Program<Message> for ViewProgram {
    type State = ViewProgramState;
    type Primitive = ViewPrimitive;

    fn draw(&self, _state: &Self::State, _cursor: Cursor, bounds: Rectangle) -> Self::Primitive {
        let viewport = vec2(bounds.width, bounds.height);
        let s = self.scale.value();
        let pan_ndc = self.offset / viewport;

        ViewPrimitive {
            uniforms: DisplayUniforms {
                transform: self.build_transform(viewport),
                crop_uv: self.displayed_crop().unwrap_or([0.0, 0.0, 1.0, 1.0]),
            },
            image: self.image.clone(),
            scale: s,
            pan_ndc,
            rotation: self.rotation,
            bounds,
            show_checkerboard: self.show_checkerboard,
            checker_uniforms: self.checker_uniforms,
            grid: self.grid_uniforms(bounds),
            mipmap_zoom_out: self.mipmap_zoom_out,
            smooth_zoom_in: self.smooth_zoom_in,
            modifiers: self.modifiers.clone(),
            dirty: self.dirty.swap(false, std::sync::atomic::Ordering::AcqRel),
            pre_clear_gpu: Arc::clone(&self.pre_clear_gpu),
            reprocess_pending: Arc::clone(&self.reprocess_pending),
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        if self.bounds != bounds {
            return Some(Action::publish(Message::BoundsChanged(bounds)));
        }

        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event
            && let Some(pos) = cursor.position_in(bounds)
        {
            let pos = Vec2::new(pos.x, pos.y);
            if let Some(msg) = wheel_scale_msg(&mut state.last_scale, delta, pos) {
                return Some(Action::publish(msg).and_capture());
            }
            return Some(Action::capture());
        }

        match state.drag {
            ViewDragState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(
                    button @ (Button::Left | Button::Middle),
                )) = event
                    && let Some(pos) = cursor.position_over(bounds)
                {
                    state.drag = ViewDragState::Panning(pos, *button);
                    return Some(Action::publish(Message::PanStarted));
                }
                if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event
                    && let Some(pos) = cursor.position_in(bounds)
                {
                    return Some(Action::publish(Message::CursorMoved(Vec2::new(
                        pos.x, pos.y,
                    ))));
                }
            }
            ViewDragState::Panning(prev, button) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(released)) if *released == button => {
                    state.drag = ViewDragState::Idle;
                    return Some(Action::publish(Message::PanEnded).and_capture());
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let delta = vec2(position.x - prev.x, prev.y - position.y);
                    state.drag = ViewDragState::Panning(*position, button);
                    return Some(Action::publish(Message::Pan(delta)).and_capture());
                }
                _ => {}
            },
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> Interaction {
        match state.drag {
            ViewDragState::Panning(..) => Interaction::Grabbing,
            ViewDragState::Idle => Interaction::Idle,
        }
    }
}

const HISTOGRAM_PRERENDER_PIXELS: usize = HISTOGRAM_TARGET_SAMPLES * 16;

fn chain_survives_prescale(modifiers: &[Modifier]) -> bool {
    use crate::modifiers::ModifierKind;
    !modifiers.iter().any(|m| {
        m.has_visible_effect()
            && matches!(
                m.kind,
                ModifierKind::Text(_) | ModifierKind::Drawing(_) | ModifierKind::PixelSort(_)
            )
    })
}

fn subsample_source(pixels: &[u8], width: u32, height: u32, stride: usize) -> (Vec<u8>, u32, u32) {
    let sw = (width as usize).div_ceil(stride).max(1);
    let sh = (height as usize).div_ceil(stride).max(1);
    let mut out = vec![0u8; sw * sh * 4];
    for y in 0..sh {
        let sy = y * stride;
        for x in 0..sw {
            let sx = x * stride;
            let s = (sy * width as usize + sx) * 4;
            let d = (y * sw + x) * 4;
            if let Some(p) = pixels.get(s..s + 4) {
                out[d..d + 4].copy_from_slice(p);
            }
        }
    }
    (out, sw as u32, sh as u32)
}

fn scale_chain_params(chain: &mut [Modifier], k: f32) {
    use crate::modifiers::ModifierKind;
    for m in chain.iter_mut() {
        match &mut m.kind {
            ModifierKind::GaussianBlur(b) => b.radius *= k,
            ModifierKind::MotionBlur(mb) => mb.distance *= k,
            _ => {}
        }
    }
}

pub(crate) fn compute_subsampled_histogram(
    pixels: &[u8],
    width: u32,
    height: u32,
    modifiers: &[Modifier],
) -> Histogram {
    let mut chain: Vec<Modifier> = modifiers
        .iter()
        .map(|m| {
            let mut m = m.clone();
            if let Some(r) = m.kind.as_resize_mut() {
                let out = r.output_for(ImageSpec::new(width, height));
                if out.w >= width && out.h >= height {
                    *r = Resize {
                        mode: ResizeMode::Percent,
                        width: 100.0,
                        height: 100.0,
                        filter: r.filter,
                        lock_aspect: true,
                    };
                }
            }
            m
        })
        .collect();

    let src_pixels = width as usize * height as usize;
    let mut shrunk: Option<Vec<u8>> = None;
    let (mut width, mut height) = (width, height);
    if src_pixels > HISTOGRAM_PRERENDER_PIXELS && chain_survives_prescale(&chain) {
        let stride = (src_pixels as f64 / HISTOGRAM_PRERENDER_PIXELS as f64)
            .sqrt()
            .ceil()
            .max(2.0) as usize;
        for m in chain.iter_mut() {
            if let Some(r) = m.kind.as_resize_mut()
                && r.mode == ResizeMode::Pixels
            {
                let out = r.output_for(ImageSpec::new(width, height));
                *r = Resize {
                    mode: ResizeMode::Percent,
                    width: out.w as f32 * 100.0 / width.max(1) as f32,
                    height: out.h as f32 * 100.0 / height.max(1) as f32,
                    filter: r.filter,
                    lock_aspect: false,
                };
            }
        }
        let (small, sw, sh) = subsample_source(pixels, width, height, stride);
        scale_chain_params(&mut chain, sw as f32 / width.max(1) as f32);
        shrunk = Some(small);
        (width, height) = (sw, sh);
    }
    let pixels: &[u8] = shrunk.as_deref().unwrap_or(pixels);

    let out = chain_output_spec(ImageSpec::new(width, height), &plan_modifiers(&chain));

    let pixel_count = (out.w as usize) * (out.h as usize);
    let stride = if pixel_count > HISTOGRAM_TARGET_SAMPLES {
        ((pixel_count as f64 / HISTOGRAM_TARGET_SAMPLES as f64)
            .sqrt()
            .round() as usize)
            .max(1)
    } else {
        1
    };
    let width_u = out.w as usize;
    let height_u = out.h as usize;
    let row_indices: Vec<usize> = (0..height_u).step_by(stride).collect();

    let text_layers = crate::modifiers::text_raster::build_layers(&chain, width, height);
    let drawing_rasters = crate::modifiers::drawing_raster::build_layers(&chain, width, height);
    let drawing_layers: Vec<Option<LayerView<'_>>> = drawing_rasters
        .iter()
        .map(|l| l.as_ref().map(|r| r.view()))
        .collect();
    let rendered = cpu::render_full(&chain, &text_layers, &drawing_layers, pixels, width, height);

    let (mut r, mut g, mut b) = row_indices
        .par_iter()
        .map(|&y| {
            let mut r = [0u32; 256];
            let mut g = [0u32; 256];
            let mut b = [0u32; 256];
            let mut x = 0;
            while x < width_u {
                let idx = (y * width_u + x) * 4;
                if let Some(p) = rendered.get(idx..idx + 4) {
                    r[p[0] as usize] += 1;
                    g[p[1] as usize] += 1;
                    b[p[2] as usize] += 1;
                }
                x += stride;
            }
            (r, g, b)
        })
        .reduce(
            || ([0u32; 256], [0u32; 256], [0u32; 256]),
            |(mut ra, mut ga, mut ba), (rb, gb, bb)| {
                for i in 0..256 {
                    ra[i] += rb[i];
                    ga[i] += gb[i];
                    ba[i] += bb[i];
                }
                (ra, ga, ba)
            },
        );

    smooth_bins(&mut r);
    smooth_bins(&mut g);
    smooth_bins(&mut b);
    (r, g, b)
}

fn smooth_bins(bins: &mut [u32; 256]) {
    let mut out = [0u32; 256];
    for i in 0usize..256 {
        let l = bins[i.saturating_sub(1)];
        let c = bins[i];
        let r = bins[(i + 1).min(255)];
        out[i] = (l + 2 * c + r + 2) / 4;
    }
    *bins = out;
}

pub(crate) fn hash_modifiers(modifiers: &[Modifier]) -> u64 {
    let mut hasher = DefaultHasher::new();
    modifiers.len().hash(&mut hasher);
    for m in modifiers {
        m.enabled.hash(&mut hasher);
        m.kind.hash_into(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn hash_modifiers_for_histogram(modifiers: &[Modifier], w: u32, h: u32) -> u64 {
    let source = ImageSpec::new(w, h);
    let mut hasher = DefaultHasher::new();
    let mut counted = 0usize;
    for m in modifiers {
        if !m.has_visible_effect() {
            continue;
        }
        match m.kind.as_resize() {
            Some(r) => {
                let out = r.output_for(source);
                if out.w >= source.w && out.h >= source.h {
                    continue;
                }
                counted += 1;
                (out.w, out.h).hash(&mut hasher);
                (r.filter as u8).hash(&mut hasher);
            }
            None => {
                counted += 1;
                m.kind.hash_into(&mut hasher);
            }
        }
    }
    counted.hash(&mut hasher);
    hasher.finish()
}

fn hash_text_modifiers(modifiers: &[Modifier]) -> u64 {
    use crate::modifiers::ModifierKind;
    let mut hasher = DefaultHasher::new();
    for (i, m) in modifiers.iter().enumerate() {
        if m.has_visible_effect()
            && let ModifierKind::Text(t) = &m.kind
        {
            i.hash(&mut hasher);
            t.hash_full(&mut hasher);
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod histogram_tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::{GaussianBlur, MotionBlur, PixelSort};

    fn noise(w: u32, h: u32) -> Vec<u8> {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 33) as u8
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        for p in px.chunks_exact_mut(4) {
            p[0] = rng();
            p[1] = rng();
            p[2] = rng();
            p[3] = 255;
        }
        px
    }

    fn assert_matches_render_full(modifiers: Vec<Modifier>, label: &str) {
        let (w, h) = (64u32, 48u32);
        let src = noise(w, h);

        let (r, g, b) = compute_subsampled_histogram(&src, w, h, &modifiers);

        let rendered = cpu::render_full(&modifiers, &[], &[], &src, w, h);
        let (mut er, mut eg, mut eb) = ([0u32; 256], [0u32; 256], [0u32; 256]);
        for p in rendered.chunks_exact(4) {
            er[p[0] as usize] += 1;
            eg[p[1] as usize] += 1;
            eb[p[2] as usize] += 1;
        }
        smooth_bins(&mut er);
        smooth_bins(&mut eg);
        smooth_bins(&mut eb);

        assert_eq!(r, er, "{label}: red channel disagrees with render_full");
        assert_eq!(g, eg, "{label}: green channel disagrees with render_full");
        assert_eq!(b, eb, "{label}: blue channel disagrees with render_full");
    }

    #[test]
    fn blur_after_blur_matches_export() {
        assert_matches_render_full(
            vec![
                Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
                Modifier::new(ModifierKind::MotionBlur(MotionBlur {
                    angle: 0.0,
                    distance: 20.0,
                })),
            ],
            "gaussian -> motion",
        );
    }

    #[test]
    fn pixel_sort_is_reflected() {
        assert_matches_render_full(
            vec![
                Modifier::new(ModifierKind::PixelSort(PixelSort {
                    threshold: 0.3,
                    angle: 0.0,
                })),
                Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
            ],
            "pixel sort -> blur",
        );
    }

    #[test]
    fn disabled_modifiers_are_ignored() {
        let mut m = Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 }));
        m.enabled = false;
        assert_matches_render_full(vec![m], "disabled blur");
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::Crop;

    fn program_with_crop() -> ViewProgram {
        let mut program = ViewProgram::default();
        program.set_image(ImageData::new(vec![0u8; 100 * 50 * 4], 100, 50));
        program
            .modifiers_mut()
            .push(Modifier::new(ModifierKind::Crop(Crop {
                x: 10.0,
                y: 20.0,
                width: 50.0,
                height: 25.0,
            })));
        program
    }

    #[test]
    fn crop_is_exported_while_the_crop_tool_is_active() {
        let mut program = program_with_crop();
        program.crop_tool_active = true;

        assert_eq!(program.displayed_crop(), None);
        assert_eq!(program.crop(), Some([0.1, 0.4, 0.6, 0.9]));
        assert_eq!(
            program.export_frame_data().expect("image is loaded").crop,
            Some([0.1, 0.4, 0.6, 0.9]),
        );
    }

    #[test]
    fn crop_applies_to_view_and_export_when_the_tool_is_inactive() {
        let program = program_with_crop();
        assert_eq!(program.displayed_crop(), program.crop());
        assert_eq!(
            program.export_frame_data().expect("image is loaded").crop,
            Some([0.1, 0.4, 0.6, 0.9]),
        );
    }

    #[test]
    fn disabled_crop_is_neither_shown_nor_exported() {
        let mut program = program_with_crop();
        program.modifiers_mut()[0].enabled = false;
        assert_eq!(program.crop(), None);
        assert_eq!(program.displayed_crop(), None);
        assert_eq!(
            program.export_frame_data().expect("image is loaded").crop,
            None,
        );
    }
}

#[cfg(test)]
mod eyedropper_resize_tests {
    #[test]
    fn the_cursor_maps_to_source_pixels_under_a_resize() {
        let (w, h) = (400u32, 300u32);
        for pct in [50.0f32, 200.0] {
            let mut program = program_with(vec![resize_pct(pct)], w, h);
            program.set_bounds(Rectangle {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            });
            program.fit();

            let centre = program
                .screen_to_image_coords(vec2(400.0, 300.0))
                .expect("the viewport centre is over the image");
            assert!(
                (centre.x - w as f32 / 2.0).abs() <= 1.5
                    && (centre.y - h as f32 / 2.0).abs() <= 1.5,
                "at {pct}% the viewport centre mapped to {centre:?}, not the                  middle of the {w}x{h} source"
            );
        }
    }

    #[test]
    fn the_screen_and_uv_conversions_round_trip_under_a_resize() {
        let (w, h) = (400u32, 300u32);
        for pct in [50.0f32, 200.0] {
            let mut program = program_with(vec![resize_pct(pct)], w, h);
            program.set_bounds(Rectangle {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            });
            program.fit();

            for probe in [vec2(400.0, 300.0), vec2(300.0, 250.0), vec2(500.0, 380.0)] {
                let Some(uv) = program.screen_to_image_uv(probe) else {
                    continue;
                };
                let back = program
                    .image_uv_to_screen(uv)
                    .expect("uv maps back to the screen");
                assert!(
                    (back - probe).length() <= 1.0,
                    "at {pct}% screen {probe:?} -> uv {uv:?} -> screen {back:?}"
                );
            }
        }
    }

    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

    fn banded(w: u32, h: u32) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                px[o] = (y * 4) as u8;
                px[o + 1] = (x * 2) as u8;
                px[o + 2] = 128;
                px[o + 3] = 255;
            }
        }
        px
    }

    fn program_with(modifiers: Vec<Modifier>, w: u32, h: u32) -> ViewProgram {
        let mut program = ViewProgram::default();
        program.set_image(ImageData::new(banded(w, h), w, h));
        for m in modifiers {
            program.modifiers_mut().push(m);
        }
        program
    }

    #[test]
    fn the_cursor_preview_steps_one_output_pixel_per_cell() {
        let (w, h) = (600u32, 600u32);
        for pct in [50.0f32, 10.0] {
            let mut program = program_with(vec![resize_pct(pct)], w, h);
            program.cursor_image_pos = Some(vec2(w as f32 * 0.5, h as f32 * 0.5));
            let px = program.cursor_pixels(9).expect("grid");

            let distinct: std::collections::HashSet<[u8; 4]> = px
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect();

            assert!(
                distinct.len() >= 9,
                "at {pct}% the 9x9 loupe shows only {} distinct colors. Adjacent \
                 source pixels collapse onto the same output pixel under a \
                 downscale, so the grid must step one *output* pixel per cell, \
                 not one source pixel.",
                distinct.len()
            );
        }
    }

    #[test]
    fn the_banded_eyedropper_matches_the_full_render() {
        use crate::modifiers::kinds::GaussianBlur;

        let (w, h) = (300u32, 220u32);
        let chains: Vec<(&str, Vec<Modifier>)> = vec![
            ("resize-down", vec![resize_pct(50.0)]),
            ("resize-up", vec![resize_pct(200.0)]),
            (
                "blur-then-resize",
                vec![
                    Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 5.0 })),
                    resize_pct(50.0),
                ],
            ),
            (
                "resize-then-blur",
                vec![
                    resize_pct(50.0),
                    Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 5.0 })),
                ],
            ),
        ];

        for (label, chain) in chains {
            for pos in [
                vec2(4.5, 3.5),
                vec2(w as f32 * 0.5, h as f32 * 0.5),
                vec2(w as f32 - 5.5, h as f32 - 4.5),
            ] {
                let mut banded = program_with(chain.clone(), w, h);
                banded.cursor_image_pos = Some(pos);
                let got = banded.cursor_pixels(9).expect("banded grid");

                let mut full = program_with(chain.clone(), w, h);
                full.cursor_image_pos = Some(pos);
                let want = full
                    .cursor_pixels_via_full_render(9)
                    .expect("full-render grid");

                assert_eq!(
                    got, want,
                    "{label} at {pos:?}: the banded eyedropper disagrees with the \
                     full render, so the readout depends on how much was rendered"
                );
            }
        }
    }

    #[test]
    #[ignore = "diagnostic; allocates aggressively; run with --release --ignored --nocapture"]
    fn eyedropper_scale_probe() {
        use std::time::Instant;

        println!("\nEyedropper cost vs source size (runs on the UI thread, in view())");
        println!("{:-<66}", "");
        println!(
            "  {:<12} {:>8} {:>12} {:>12} {:>14}",
            "source", "GB", "first ms", "cached ms", "per slider tick"
        );
        println!("{:-<66}", "");

        for dim in [2048u32, 4096, 8192, 12000] {
            let gb = (dim as f64 * dim as f64 * 4.0) / 1e9;
            let mut program = program_with(vec![resize_pct(50.0)], dim, dim);
            program.cursor_image_pos = Some(vec2(dim as f32 * 0.5, dim as f32 * 0.5));

            let t = Instant::now();
            let _ = program.cursor_pixels(9);
            let first = t.elapsed().as_secs_f64() * 1000.0;

            let t = Instant::now();
            let _ = program.cursor_pixels(9);
            let cached = t.elapsed().as_secs_f64() * 1000.0;

            if let Some(r) = program.modifiers_mut()[0].kind.as_resize_mut() {
                r.width = 49.0;
                r.height = 49.0;
            }
            let t = Instant::now();
            let _ = program.cursor_pixels(9);
            let tick = t.elapsed().as_secs_f64() * 1000.0;

            println!("  {dim:<12} {gb:>8.2} {first:>12.1} {cached:>12.1} {tick:>14.1}");
        }
        println!("{:-<66}", "");
    }

    fn resize_pct(pct: f32) -> Modifier {
        Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Bilinear,
            lock_aspect: true,
        }))
    }

    #[test]
    fn cursor_info_survives_a_resize() {
        let (w, h) = (64u32, 48u32);
        let mut program = program_with(vec![resize_pct(50.0)], w, h);

        for (px, py) in [(0u32, 0u32), (10, 10), (32, 24), (63, 47)] {
            program.cursor_image_pos = Some(vec2(px as f32 + 0.5, py as f32 + 0.5));
            let info = program.cursor_info();
            assert!(
                info.is_some(),
                "cursor info vanished at ({px}, {py}) with a 50% resize"
            );
            let (rx, ry, _, rgba) = info.unwrap();
            assert_eq!(
                (rx, ry),
                (px, py),
                "reported position must stay in source space"
            );
            assert_eq!(
                rgba[3], 255,
                "sampled a pixel outside the buffer at ({px}, {py})"
            );
        }
    }

    #[test]
    fn resized_sample_tracks_the_right_row() {
        let (w, h) = (64u32, 48u32);
        let mut program = program_with(vec![resize_pct(50.0)], w, h);

        for py in [0u32, 12, 24, 40] {
            program.cursor_image_pos = Some(vec2(32.5, py as f32 + 0.5));
            let (_, _, _, rgba) = program.cursor_info().expect("cursor info");
            let expected = (py * 4) as i32;
            assert!(
                (rgba[0] as i32 - expected).abs() <= 8,
                "row {py}: expected red near {expected}, got {}",
                rgba[0]
            );
        }
    }

    #[test]
    fn cursor_info_unchanged_without_resize() {
        let (w, h) = (64u32, 48u32);
        let mut program = program_with(vec![], w, h);
        program.cursor_image_pos = Some(vec2(20.5, 30.5));
        let (rx, ry, _, rgba) = program.cursor_info().expect("cursor info");
        assert_eq!((rx, ry), (20, 30));
        assert_eq!(rgba[0], (30 * 4) as u8);
        assert_eq!(rgba[1], (20 * 2) as u8);
    }

    #[test]
    fn cursor_pixels_grid_survives_a_resize() {
        let (w, h) = (64u32, 48u32);
        let mut program = program_with(vec![resize_pct(50.0)], w, h);
        program.cursor_image_pos = Some(vec2(60.5, 44.5));
        let px = program.cursor_pixels(5).expect("pixel grid");
        assert_eq!(px.len(), 5 * 5 * 4);
        assert!(
            px.chunks_exact(4).any(|p| p[3] == 255),
            "the grid near the far corner came back entirely empty"
        );
    }
}

#[cfg(test)]
mod histogram_cost_tests {
    fn resize_pct_h(pct: f32) -> Modifier {
        Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }))
    }

    #[test]
    fn the_histogram_cost_does_not_grow_with_the_source() {
        use std::time::Instant;

        let render = |dim: u32| {
            let n = dim as usize * dim as usize * 4;
            let pixels: Vec<u8> = (0..n)
                .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
                .collect();
            let t = Instant::now();
            let _ = compute_subsampled_histogram(&pixels, dim, dim, &[resize_pct_h(50.0)]);
            t.elapsed().as_secs_f64()
        };

        let _ = render(512);

        let small = render(1024);
        let large = render(4096);

        assert!(
            large <= small.max(0.001) * 6.0,
            "histogram cost grew {:.1}x for 16x the pixels ({:.1} ms -> {:.1} ms), \
             so it is still proportional to the document area rather than to the \
             number of samples it needs",
            large / small.max(1e-9),
            small * 1000.0,
            large * 1000.0
        );
    }

    #[test]
    fn sampling_preserves_the_distribution() {
        let (w, h) = (1024u32, 1024u32);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let v = if y < h / 4 {
                    64u8
                } else {
                    (x * 255 / w.max(1)) as u8
                };
                pixels[o] = v;
                pixels[o + 1] = v.wrapping_add(40);
                pixels[o + 2] = 255 - v;
                pixels[o + 3] = 255;
            }
        }

        for chain in [vec![], vec![resize_pct_h(50.0)]] {
            let (r, g, b) = compute_subsampled_histogram(&pixels, w, h, &chain);
            for (label, bins) in [("r", r), ("g", g), ("b", b)] {
                let total: u64 = bins.iter().map(|&c| c as u64).sum();
                assert!(
                    total > 0,
                    "{label}: the histogram is empty, so nothing was sampled"
                );
                let occupied = bins.iter().filter(|&&c| c > 0).count();
                assert!(
                    occupied > 32,
                    "{label}: only {occupied} bins occupied; the distribution \
                     collapsed, which means sampling aliased the content away"
                );
            }
        }
    }

    #[test]
    fn a_downscale_changes_the_histogram() {
        let (w, h) = (256u32, 256u32);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let on = (x / 2 + y / 2) % 2 == 0;
                let v = if on { 255 } else { 0 };
                pixels[o] = v;
                pixels[o + 1] = v;
                pixels[o + 2] = v;
                pixels[o + 3] = 255;
            }
        }
        let none = compute_subsampled_histogram(&pixels, w, h, &[]);
        let down = compute_subsampled_histogram(&pixels, w, h, &[resize_pct_h(25.0)]);
        assert_ne!(
            none, down,
            "a 25% downscale left the histogram unchanged; it averages pixels,              so the panel would disagree with the export"
        );
    }

    #[test]
    fn an_upscale_does_not_change_the_histogram() {
        let (w, h) = (128u32, 128u32);
        let pixels: Vec<u8> = (0..(w * h * 4))
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let none = compute_subsampled_histogram(&pixels, w, h, &[]);
        for pct in [200.0f32, 400.0] {
            assert_eq!(
                compute_subsampled_histogram(&pixels, w, h, &[resize_pct_h(pct)]),
                none,
                "a {pct}% upscale changed the histogram, but it is clamped to                  100% before rendering"
            );
        }
    }

    #[test]
    fn the_histogram_key_ignores_upscales_but_not_downscales() {
        let (w, h) = (500u32, 500u32);
        let none = hash_modifiers_for_histogram(&[], w, h);
        assert_eq!(
            hash_modifiers_for_histogram(&[resize_pct_h(200.0)], w, h),
            none,
            "an upscale changed the key, so every slider tick queues another              full CPU render"
        );
        assert_eq!(
            hash_modifiers_for_histogram(&[resize_pct_h(400.0)], w, h),
            none,
            "two upscales must share a key; neither is rendered"
        );
        assert_ne!(
            hash_modifiers_for_histogram(&[resize_pct_h(50.0)], w, h),
            none,
            "a downscale must invalidate: it is rendered and it changes the              histogram"
        );
    }

    #[test]
    fn a_disabled_resize_keys_like_no_resize() {
        let (w, h) = (500u32, 500u32);
        for pct in [25.0f32, 400.0] {
            let mut m = resize_pct_h(pct);
            m.enabled = false;
            assert_eq!(
                hash_modifiers_for_histogram(&[m], w, h),
                hash_modifiers_for_histogram(&[], w, h),
                "a disabled {pct}% resize keyed differently from no resize, but \
                 it is dropped before rendering, so both produce the same \
                 histogram"
            );
        }
    }

    #[test]
    fn the_key_classifies_chained_resizes_like_the_renderer() {
        let (w, h) = (400u32, 400u32);
        let pixels: Vec<u8> = (0..(w * h * 4))
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();

        let chain = vec![resize_pct_h(200.0), resize_pct_h(25.0)];
        let lone = vec![resize_pct_h(25.0)];

        assert_eq!(
            compute_subsampled_histogram(&pixels, w, h, &chain),
            compute_subsampled_histogram(&pixels, w, h, &lone),
            "the renderer clamps the upscale against the source, so these two \
             chains render the same histogram; if this fails the premise below \
             is wrong, not the key"
        );
        assert_eq!(
            hash_modifiers_for_histogram(&chain, w, h),
            hash_modifiers_for_histogram(&lone, w, h),
            "the two chains render an identical histogram but keyed \
             differently, so the cache cannot serve one for the other"
        );
    }

    #[test]
    fn the_general_key_distinguishes_every_resize() {
        let a = vec![resize_pct_h(50.0)];
        let b = vec![resize_pct_h(200.0)];
        assert_ne!(hash_modifiers(&a), hash_modifiers(&b));
        assert_ne!(hash_modifiers(&a), hash_modifiers(&[]));
        assert_ne!(hash_modifiers(&b), hash_modifiers(&[]));
    }

    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use std::time::Instant;

    #[test]
    #[ignore = "timing baseline; run with --release --ignored --nocapture"]
    fn histogram_cost_scales_with_the_resized_document() {
        let (w, h) = (1000u32, 1000u32);
        let pixels: Vec<u8> = (0..(w * h * 4))
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();

        println!(
            "
Histogram cost -- {w}x{h} source"
        );
        println!("  {:<22} {:>12} {:>14}", "chain", "doc", "ms");
        println!("  {:-<50}", "");

        for pct in [100.0f32, 200.0, 300.0, 400.0] {
            let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: pct,
                height: pct,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            }))];
            let t = Instant::now();
            let _ = compute_subsampled_histogram(&pixels, w, h, &chain);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let d = (w as f32 * pct / 100.0) as u32;
            println!(
                "  {:<22} {:>12} {:>14.1}",
                format!("resize {pct}%"),
                format!("{d}x{d}"),
                ms
            );
        }
        println!("  {:-<50}", "");
    }
}

#[cfg(test)]
mod document_size_tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::{Crop, Resize, ResizeFilter, ResizeMode};

    fn program(modifiers: Vec<Modifier>, w: u32, h: u32) -> ViewProgram {
        let mut p = ViewProgram::default();
        p.set_image(ImageData::new(vec![255u8; (w * h * 4) as usize], w, h));
        for m in modifiers {
            p.modifiers_mut().push(m);
        }
        p
    }

    fn resize_pct(pct: f32) -> Modifier {
        Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Bilinear,
            lock_aspect: true,
        }))
    }

    fn crop_of(x: f32, y: f32, w: f32, h: f32) -> Modifier {
        Modifier::new(ModifierKind::Crop(Crop {
            x,
            y,
            width: w,
            height: h,
        }))
    }

    #[test]
    fn no_modifiers_leaves_the_source_size() {
        let p = program(Vec::new(), 800, 600);
        assert_eq!(p.effective_display_size(), vec2(800.0, 600.0));
    }

    #[test]
    fn fit_tracks_the_resized_document_on_a_huge_image() {
        const SRC: u32 = 30000;
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };

        for pct in [1.0f32, 10.0, 50.0, 100.0, 200.0, 400.0] {
            let mut p = ViewProgram::default();
            p.set_image(ImageData::new(Vec::new(), SRC, SRC));
            p.modifiers_mut().push(resize_pct(pct));
            p.set_bounds(bounds);
            p.fit();

            let doc = p.effective_display_size();
            let s = p.scale();
            let on_screen = doc * s;

            assert!(
                s.is_finite() && s > 0.0,
                "{pct}%: fit produced a scale of {s}"
            );
            assert!(
                on_screen.x <= bounds.width + 1.0 && on_screen.y <= bounds.height + 1.0,
                "{pct}%: the fitted document is {on_screen:?} on screen, larger \
                 than the {}x{} viewport",
                bounds.width,
                bounds.height
            );
            assert!(
                on_screen.x >= bounds.width - 1.0 || on_screen.y >= bounds.height - 1.0,
                "{pct}%: the fitted document is {on_screen:?}, touching neither \
                 edge of the {}x{} viewport, so it is not filling it",
                bounds.width,
                bounds.height
            );
        }
    }

    #[test]
    fn a_resize_changes_the_document_size() {
        let p = program(vec![resize_pct(50.0)], 800, 600);
        assert_eq!(
            p.effective_display_size(),
            vec2(400.0, 300.0),
            "the viewport still reports the source size, so it would fit and \
             zoom against a document the export does not produce"
        );
    }

    #[test]
    fn crop_applies_to_the_resized_document() {
        let p = program(
            vec![resize_pct(50.0), crop_of(0.0, 0.0, 400.0, 300.0)],
            800,
            600,
        );
        assert_eq!(p.effective_display_size(), vec2(200.0, 150.0));
    }

    #[test]
    fn crop_origin_is_in_the_resized_space() {
        let p = program(
            vec![resize_pct(50.0), crop_of(400.0, 300.0, 400.0, 300.0)],
            800,
            600,
        );
        assert_eq!(
            p.crop_origin(),
            vec2(200.0, 150.0),
            "the crop origin is in source pixels while the extent is in              resized pixels, so the grid and the image disagree"
        );
    }

    #[test]
    fn crop_origin_without_a_resize_is_the_source_offset() {
        let p = program(vec![crop_of(400.0, 300.0, 400.0, 300.0)], 800, 600);
        assert_eq!(p.crop_origin(), vec2(400.0, 300.0));
    }

    #[test]
    fn a_disabled_resize_does_not_change_the_document() {
        let mut m = resize_pct(50.0);
        m.enabled = false;
        let p = program(vec![m], 800, 600);
        assert_eq!(p.effective_display_size(), vec2(800.0, 600.0));
    }

    #[test]
    fn fit_scale_uses_the_resized_document() {
        let mut p = program(vec![resize_pct(50.0)], 800, 600);
        p.set_bounds(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        });
        assert!(
            (p.fit_scale() - 1.0).abs() < 1e-4,
            "fit scale was {}, expected 1.0 for a 400x300 document in 400x300 \
             bounds",
            p.fit_scale()
        );
    }

    #[test]
    fn preview_size_matches_the_export_geometry() {
        for pct in [25.0f32, 50.0, 150.0] {
            let p = program(vec![resize_pct(pct)], 800, 600);
            let out = chain_output_spec(ImageSpec::new(800, 600), &plan_modifiers(&p.modifiers));
            assert_eq!(
                p.effective_display_size(),
                vec2(out.w as f32, out.h as f32),
                "at {pct}% the viewport and the export planner disagree"
            );
        }
    }
}
