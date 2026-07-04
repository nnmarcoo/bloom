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
    modifiers::{
        Modifier, cpu,
        drawing_raster::{DrawingLayerCache, LayerView},
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
    pixels: Vec<u8>,
}

const STAGED_EYEDROPPER_MAX_PX: u64 = 8_000_000;

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
        let origin = if let Some([min_u, min_v, ..]) = self.active_crop() {
            vec2(min_u * self.image_size.x, min_v * self.image_size.y)
        } else {
            Vec2::ZERO
        };
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

    fn active_crop(&self) -> Option<[f32; 4]> {
        if self.crop_tool_active || self.image_size == Vec2::ZERO {
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

    fn effective_display_size(&self) -> Vec2 {
        if let Some([min_u, min_v, max_u, max_v]) = self.active_crop() {
            vec2(
                (max_u - min_u) * self.image_size.x,
                (max_v - min_v) * self.image_size.y,
            )
        } else {
            self.image_size
        }
    }

    pub fn animation_info(&self) -> Option<(usize, usize)> {
        self.animation
            .as_ref()
            .map(|a| (a.current_index(), a.frame_count()))
    }

    pub fn animation_duration(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.total_duration())
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
        let display_uv = if let Some([min_u, min_v, max_u, max_v]) = self.active_crop() {
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
        let origin = if let Some([min_u, min_v, ..]) = self.active_crop() {
            vec2(min_u * self.image_size.x, min_v * self.image_size.y)
        } else {
            Vec2::ZERO
        };
        Some(local_px + origin)
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
        uv: [f32; 2],
    ) -> Option<[u8; 4]> {
        let image = self.image.as_ref()?;
        let w = image.width;
        let h = image.height;

        if (w as u64) * (h as u64) <= STAGED_EYEDROPPER_MAX_PX {
            if let Some(c) = self.staged_pixel(text_layers, drawing_layers, image, px, py) {
                return Some(c);
            }
        }

        let idx = (py as usize * w as usize + px as usize) * 4;
        let pixels = image.pixels_snapshot();
        let p = pixels.get(idx..idx + 4)?;
        Some(cpu::f32_to_pixel(self.apply_modifiers_cpu(
            text_layers,
            drawing_layers,
            &pixels,
            w,
            h,
            uv,
            cpu::pixel_to_f32(p),
        )))
    }

    fn staged_pixel(
        &self,
        text_layers: &[Option<TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        image: &ImageData,
        px: u32,
        py: u32,
    ) -> Option<[u8; 4]> {
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
            let staged =
                cpu::render_full(&self.modifiers, text_layers, drawing_layers, &pixels, w, h);
            *guard = Some(StagedCache {
                key,
                w,
                pixels: staged,
            });
        }
        let cache = guard.as_ref()?;
        let idx = (py as usize * cache.w as usize + px as usize) * 4;
        let p = cache.pixels.get(idx..idx + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }

    pub fn color_at_window(&self, window_pos: Vec2) -> Option<[u8; 4]> {
        let local = window_pos - vec2(self.bounds.x, self.bounds.y);
        let img = self.screen_to_image_coords(local)?;
        if img.x < 0.0 || img.y < 0.0 || img.x >= self.image_size.x || img.y >= self.image_size.y {
            return None;
        }
        let (px, py) = (img.x as u32, img.y as u32);
        let uv = [px as f32 / self.image_size.x, py as f32 / self.image_size.y];
        self.with_rasters(
            self.image_size.x as u32,
            self.image_size.y as u32,
            |text, drawing| self.sample_pixel(text, drawing, px, py, uv),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_modifiers_cpu(
        &self,
        text_layers: &[Option<crate::modifiers::text_raster::TextRaster>],
        drawing_layers: &[Option<LayerView<'_>>],
        pixels: &[u8],
        img_w: u32,
        img_h: u32,
        uv: [f32; 2],
        c: [f32; 4],
    ) -> [f32; 4] {
        cpu::apply_modifiers_with_layers(
            &self.modifiers,
            text_layers,
            drawing_layers,
            pixels,
            img_w,
            img_h,
            uv[0] * img_w as f32 + 0.5,
            uv[1] * img_h as f32 + 0.5,
            uv,
            c,
        )
    }

    pub fn export_data(&self) -> Option<crate::export::ExportData> {
        use crate::export::ExportFrame;

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

    pub fn export_frame_data(&self) -> Option<crate::export::ExportData> {
        use crate::export::ExportFrame;

        let image = self.image.as_ref()?;
        let frames = vec![ExportFrame {
            pixels: image.pixels_snapshot(),
            delay: std::time::Duration::ZERO,
        }];
        Some(self.build_export(frames, 0, image.width, image.height))
    }

    fn build_export(
        &self,
        frames: Vec<crate::export::ExportFrame>,
        still_index: usize,
        width: u32,
        height: u32,
    ) -> crate::export::ExportData {
        crate::export::ExportData {
            source: crate::export::ExportSource::Frames {
                frames,
                still_index,
            },
            width,
            height,
            modifiers: self.modifiers.as_ref().clone(),
            crop: self.active_crop(),
            rotation: self.rotation,
        }
    }

    #[cfg(feature = "av")]
    pub fn build_video_export(
        &self,
        info: &crate::wgpu::media::video::VideoInfo,
    ) -> crate::export::ExportData {
        crate::export::ExportData {
            source: crate::export::ExportSource::Video(crate::export::VideoExportInfo {
                path: info.path.clone(),
                frame_count: info.frame_count,
                duration: info.duration,
            }),
            width: info.width,
            height: info.height,
            modifiers: self.modifiers.as_ref().clone(),
            crop: self.active_crop(),
            rotation: self.rotation,
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
                |text, drawing| self.sample_pixel(text, drawing, px, py, [uv.x, uv.y]),
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

        self.with_rasters(image.width, image.height, |text_layers, drawing_layers| {
            for row in 0..size as i64 {
                for col in 0..size as i64 {
                    let (x, y) = coord(row, col);
                    if x < 0 || y < 0 || x >= w || y >= h {
                        continue;
                    }
                    let idx = (y as usize * w as usize + x as usize) * 4;
                    let p = &buf[idx..idx + 4];
                    let uv = [x as f32 / w as f32, y as f32 / h as f32];
                    let rgba = cpu::f32_to_pixel(self.apply_modifiers_cpu(
                        text_layers,
                        drawing_layers,
                        &buf,
                        w as u32,
                        h as u32,
                        uv,
                        cpu::pixel_to_f32(p),
                    ));
                    let dst = ((row * size as i64 + col) * 4) as usize;
                    pixels[dst..dst + 4].copy_from_slice(&rgba);
                }
            }
        });
        self.store_cursor_pixels(key, size, &pixels);
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
                crop_uv: self.active_crop().unwrap_or([0.0, 0.0, 1.0, 1.0]),
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

pub(crate) fn compute_subsampled_histogram(
    pixels: &[u8],
    width: u32,
    height: u32,
    modifiers: &[Modifier],
) -> Histogram {
    let pixel_count = (width as usize) * (height as usize);
    let stride = if pixel_count > HISTOGRAM_TARGET_SAMPLES {
        ((pixel_count as f64 / HISTOGRAM_TARGET_SAMPLES as f64)
            .sqrt()
            .round() as usize)
            .max(1)
    } else {
        1
    };
    let width_u = width as usize;
    let height_u = height as usize;
    let row_indices: Vec<usize> = (0..height_u).step_by(stride).collect();

    let (mut r, mut g, mut b) = row_indices
        .par_iter()
        .map(|&y| {
            let mut r = [0u32; 256];
            let mut g = [0u32; 256];
            let mut b = [0u32; 256];
            let mut x = 0;
            while x < width_u {
                let idx = (y * width_u + x) * 4;
                if let Some(p) = pixels.get(idx..idx + 4) {
                    let uv = [x as f32 / width as f32, y as f32 / height as f32];
                    let raw = cpu::pixel_to_f32(p);
                    let result = cpu::apply_modifiers(modifiers, pixels, width, height, uv, raw);
                    let out = cpu::f32_to_pixel(result);
                    r[out[0] as usize] += 1;
                    g[out[1] as usize] += 1;
                    b[out[2] as usize] += 1;
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
