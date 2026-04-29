use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, vec2, vec3, vec4};
use iced::{
    Event, Point, Rectangle,
    mouse::{self, Button, Cursor, Interaction},
    widget::{Action, shader::Program},
};

use crate::{
    app::Message,
    modifiers::Modifier,
    wgpu::{
        media::animation::Animation, media::exif_data::ExifData, media::image_data::ImageData,
        passes::checkerboard::CheckerboardUniforms, scale::Scale, view_pipeline::Uniforms,
        view_primitive::ViewPrimitive,
    },
};

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
    Panning(Point),
}

#[derive(Clone)]
pub struct ViewProgram {
    offset: Vec2,
    image_size: Vec2,
    scale: Scale,
    bounds: Rectangle,
    image: Option<Arc<ImageData>>,
    animation: Option<Animation>,
    pub show_checkerboard: bool,
    pub checker_uniforms: CheckerboardUniforms,
    pub mipmap_zoom_out: bool,
    pub smooth_zoom_in: bool,
    uploaded_mipmap_zoom_out: bool,
    cursor_image_pos: Option<Vec2>,
    panning: bool,
    rotation: u8,
    pub modifiers: Vec<Modifier>,
    pub crop_tool_active: bool,
    dirty_from: Arc<Mutex<Option<usize>>>,
    pre_clear_gpu: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for ViewProgram {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            image_size: Vec2::ZERO,
            scale: Scale::default(),
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
            cursor_image_pos: None,
            panning: false,
            rotation: 0,
            mipmap_zoom_out: true,
            smooth_zoom_in: false,
            uploaded_mipmap_zoom_out: true,
            modifiers: Vec::new(),
            crop_tool_active: false,
            dirty_from: Arc::new(Mutex::new(None)),
            pre_clear_gpu: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

impl ViewProgram {
    pub fn mark_dirty(&self, i: usize) {
        let mut guard = self.dirty_from.lock().unwrap();
        *guard = Some(guard.map_or(i, |p| p.min(i)));
    }

    pub fn set_bounds(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
        self.clamp_offset();
    }

    pub fn viewport_center(&self) -> Vec2 {
        vec2(self.bounds.width * 0.5, self.bounds.height * 0.5)
    }

    pub fn fit(&mut self) {
        if self.image_size == Vec2::ZERO {
            return;
        }
        let eff = self.effective_display_size();
        let (fw, fh) = if self.rotation.is_multiple_of(2) {
            (eff.x, eff.y)
        } else {
            (eff.y, eff.x)
        };
        self.scale.fit_dims(fw, fh, self.bounds);
        self.offset = Vec2::ZERO;
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
        self.offset += 2.0 * delta / self.scale.value();
        self.clamp_offset();
    }

    pub fn scale_up(&mut self, cursor: Vec2) {
        let prev = self.scale.up();
        self.scale_offset(cursor, prev);
        self.clamp_offset();
    }

    pub fn scale_down(&mut self, cursor: Vec2) {
        let prev = self.scale.down();
        self.scale_offset(cursor, prev);
        self.clamp_offset();
    }

    pub fn set_scale(&mut self, scale: f32, cursor: Vec2) {
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

    fn aspect(&self, viewport: Vec2) -> Vec2 {
        let eff = self.effective_display_size();
        if self.rotation.is_multiple_of(2) {
            eff / viewport
        } else {
            vec2(eff.x / viewport.y, eff.y / viewport.x)
        }
    }

    pub fn set_image(&mut self, data: ImageData) {
        self.image_size = vec2(data.width as f32, data.height as f32);
        self.image = Some(Arc::new(data));
        self.animation = None;
        self.cursor_image_pos = Some(self.image_size / 2.0);
        self.panning = false;
        self.rotation = 0;
        self.uploaded_mipmap_zoom_out = self.mipmap_zoom_out;
    }

    pub fn histogram(&self) -> Option<&([u32; 256], [u32; 256], [u32; 256])> {
        if let Some(anim) = &self.animation {
            return Some(anim.current_histogram());
        }
        self.image.as_deref().map(|d| d.histogram())
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

    pub fn set_animation(&mut self, anim: Animation) {
        let first = Arc::clone(anim.current_image());
        self.image_size = vec2(first.width as f32, first.height as f32);
        self.image = Some(first);
        self.animation = Some(anim);
        self.cursor_image_pos = Some(self.image_size / 2.0);
        self.panning = false;
        self.rotation = 0;
        self.uploaded_mipmap_zoom_out = self.mipmap_zoom_out;
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

    pub fn time_until_next_frame(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.time_until_next_frame())
    }

    pub fn scale(&self) -> f32 {
        self.scale.value()
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
        use crate::modifiers::ModifierKind;
        if self.crop_tool_active || self.image_size == Vec2::ZERO {
            return None;
        }
        self.modifiers.iter().find_map(|m| {
            if !m.enabled {
                return None;
            }
            if let ModifierKind::Crop {
                x,
                y,
                width,
                height,
            } = m.kind
            {
                let iw = self.image_size.x;
                let ih = self.image_size.y;
                Some([x / iw, y / ih, (x + width) / iw, (y + height) / ih])
            } else {
                None
            }
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
        let img_ndc = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
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

    pub fn screen_to_image_pixel(&self, screen_pos: Vec2) -> Option<(u32, u32)> {
        let img = self.screen_to_image_coords(screen_pos)?;
        if img.x < 0.0 || img.y < 0.0 || img.x >= self.image_size.x || img.y >= self.image_size.y {
            return None;
        }
        Some((img.x as u32, img.y as u32))
    }

    fn apply_single_cpu(
        kind: &crate::modifiers::ModifierKind,
        img_w: u32,
        img_h: u32,
        uv: [f32; 2],
        mut c: [f32; 4],
    ) -> [f32; 4] {
        use crate::modifiers::ModifierKind;
        match kind {
            ModifierKind::BrightnessContrast {
                brightness,
                contrast,
            } => {
                for v in c.iter_mut().take(3) {
                    *v = (*v + brightness - 0.5) * (1.0 + contrast) + 0.5;
                }
            }
            ModifierKind::Exposure { exposure } => {
                let scale = 2.0f32.powf(*exposure);
                c[0] *= scale;
                c[1] *= scale;
                c[2] *= scale;
            }
            ModifierKind::Levels {
                shadows,
                midtones,
                highlights,
            } => {
                let hi = highlights.max(shadows + 0.001);
                let range = hi - shadows;
                for v in c.iter_mut().take(3) {
                    *v = ((*v - shadows) / range).clamp(0.0, 1.0);
                }
                let gamma = midtones.max(0.001);
                for v in c.iter_mut().take(3) {
                    *v = v.powf(1.0 / gamma);
                }
            }
            ModifierKind::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                let [h, s, l] = Self::rgb_to_hsl([c[0], c[1], c[2]]);
                let rgb = Self::hsl_to_rgb([
                    (h + hue / 360.0).fract(),
                    (s + saturation).clamp(0.0, 1.0),
                    (l + lightness).clamp(0.0, 1.0),
                ]);
                c[0] = rgb[0];
                c[1] = rgb[1];
                c[2] = rgb[2];
            }
            ModifierKind::Vignette {
                strength,
                size,
                softness,
            } => {
                let dx = uv[0] - 0.5;
                let dy = uv[1] - 0.5;
                let dist = (dx * dx + dy * dy).sqrt() * 2.0;
                let inner = (size - softness).max(0.0);
                let t = ((dist - inner) / (size + 0.0001 - inner)).clamp(0.0, 1.0);
                let vignette = 1.0 - t * t * (3.0 - 2.0 * t);
                let factor = (1.0 - strength).max(0.0) * (1.0 - vignette) + vignette;
                c[0] *= factor;
                c[1] *= factor;
                c[2] *= factor;
            }
            ModifierKind::Threshold { cutoff } => {
                let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
                let v = if luma >= *cutoff { 1.0 } else { 0.0 };
                c[0] = v;
                c[1] = v;
                c[2] = v;
            }
            ModifierKind::Posterize { levels } => {
                let l = (*levels as f32 - 1.0).max(1.0);
                for v in c.iter_mut().take(3) {
                    *v = (*v * l + 0.5).floor() / l;
                }
            }
            ModifierKind::Vibrance {
                vibrance,
                saturation,
            } => {
                let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
                let max_c = c[0].max(c[1]).max(c[2]);
                let sat_proxy = max_c - c[0].min(c[1]).min(c[2]);
                let vib_amount = vibrance * (1.0 - sat_proxy);
                for v in c.iter_mut().take(3) {
                    *v = luma + (*v - luma) * (1.0 + vib_amount);
                }
                for v in c.iter_mut().take(3) {
                    *v = luma + (*v - luma) * (1.0 + saturation);
                }
            }
            ModifierKind::ColorBalance {
                cyan_red,
                magenta_green,
                yellow_blue,
            } => {
                c[0] += cyan_red;
                c[1] += magenta_green;
                c[2] += yellow_blue;
            }
            ModifierKind::Grain {
                amount,
                size,
                roughness,
                seed,
            } => {
                let gx = uv[0] * img_w as f32 / size.max(0.5);
                let gy = uv[1] * img_h as f32 / size.max(0.5);
                let iseed = *seed as i32;
                let (cx, cy) = (gx.floor(), gy.floor());
                let (fx, fy) = (gx.fract(), gy.fract());
                let n00 = Self::hash21_i(cx as i32, cy as i32, iseed);
                let n10 = Self::hash21_i(cx as i32 + 1, cy as i32, iseed);
                let n01 = Self::hash21_i(cx as i32, cy as i32 + 1, iseed);
                let n11 = Self::hash21_i(cx as i32 + 1, cy as i32 + 1, iseed);
                let t = roughness.clamp(0.0, 1.0);
                let wx =
                    fx * fx * (3.0 - 2.0 * fx) * (1.0 - t) + if fx >= 0.5 { 1.0 } else { 0.0 } * t;
                let wy =
                    fy * fy * (3.0 - 2.0 * fy) * (1.0 - t) + if fy >= 0.5 { 1.0 } else { 0.0 } * t;
                let noise =
                    (n00 * (1.0 - wx) + n10 * wx) * (1.0 - wy) + (n01 * (1.0 - wx) + n11 * wx) * wy;
                let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
                let luma_weight = 4.0 * luma * (1.0 - luma);
                let grain = (noise - 0.5) * amount * luma_weight;
                for v in c.iter_mut().take(3) {
                    *v = (*v + grain).clamp(0.0, 1.0);
                }
            }
            ModifierKind::Halftone { size, angle } => {
                let angle_rad = *angle * std::f32::consts::PI / 180.0;
                let cs = angle_rad.cos();
                let sn = angle_rad.sin();
                let period = (*size / img_w.min(img_h) as f32).max(0.001);
                let rot_x = (uv[0] * cs - uv[1] * sn) / period;
                let rot_y = (uv[0] * sn + uv[1] * cs) / period;
                let cell_x = rot_x.floor() + 0.5;
                let cell_y = rot_y.floor() + 0.5;
                let dist = ((rot_x - cell_x).powi(2) + (rot_y - cell_y).powi(2)).sqrt();
                let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
                let v = if dist < luma.sqrt() * 0.5 { 1.0 } else { 0.0 };
                c[0] = v;
                c[1] = v;
                c[2] = v;
            }
            _ => {}
        }
        c.map(|v| v.clamp(0.0, 1.0))
    }

    fn apply_modifiers_cpu(
        &self,
        pixels: &[u8],
        img_w: u32,
        img_h: u32,
        uv: [f32; 2],
        mut c: [f32; 4],
    ) -> [f32; 4] {
        use crate::modifiers::ModifierKind;
        for (i, m) in self.modifiers.iter().enumerate() {
            if !m.enabled {
                continue;
            }
            if let ModifierKind::ChromaticAberration { amount } = &m.kind {
                let scale = amount / img_w as f32;
                let r_uv = [
                    (uv[0] + (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                    (uv[1] + (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
                ];
                let b_uv = [
                    (uv[0] - (uv[0] - 0.5) * scale).clamp(0.0, 1.0),
                    (uv[1] - (uv[1] - 0.5) * scale).clamp(0.0, 1.0),
                ];
                let mut cr = Self::sample_pixel(pixels, img_w, img_h, r_uv[0], r_uv[1]);
                let mut cb = Self::sample_pixel(pixels, img_w, img_h, b_uv[0], b_uv[1]);
                for prev in &self.modifiers[..i] {
                    if !prev.enabled
                        || matches!(prev.kind, ModifierKind::ChromaticAberration { .. })
                    {
                        continue;
                    }
                    cr = Self::apply_single_cpu(&prev.kind, img_w, img_h, r_uv, cr);
                    cb = Self::apply_single_cpu(&prev.kind, img_w, img_h, b_uv, cb);
                }
                c[0] = cr[0];
                c[2] = cb[2];
            } else {
                c = Self::apply_single_cpu(&m.kind, img_w, img_h, uv, c);
            }
            c = c.map(|v| v.clamp(0.0, 1.0));
        }
        c
    }

    fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
        let max_c = rgb[0].max(rgb[1]).max(rgb[2]);
        let min_c = rgb[0].min(rgb[1]).min(rgb[2]);
        let l = (max_c + min_c) * 0.5;
        if max_c == min_c {
            return [0.0, 0.0, l];
        }
        let d = max_c - min_c;
        let s = if l < 0.5 {
            d / (max_c + min_c)
        } else {
            d / (2.0 - max_c - min_c)
        };
        let h = if max_c == rgb[0] {
            (rgb[1] - rgb[2]) / d + if rgb[1] >= rgb[2] { 0.0 } else { 6.0 }
        } else if max_c == rgb[1] {
            (rgb[2] - rgb[0]) / d + 2.0
        } else {
            (rgb[0] - rgb[1]) / d + 4.0
        };
        [h / 6.0, s, l]
    }

    fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
        if hsl[1] == 0.0 {
            return [hsl[2]; 3];
        }
        let q = if hsl[2] < 0.5 {
            hsl[2] * (1.0 + hsl[1])
        } else {
            hsl[2] + hsl[1] - hsl[2] * hsl[1]
        };
        let p = 2.0 * hsl[2] - q;
        [
            Self::hue_to_rgb(p, q, hsl[0] + 1.0 / 3.0),
            Self::hue_to_rgb(p, q, hsl[0]),
            Self::hue_to_rgb(p, q, hsl[0] - 1.0 / 3.0),
        ]
    }

    fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
        let mut t = t_in;
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 0.5 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    }

    fn pixel_to_f32(p: &[u8]) -> [f32; 4] {
        [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            p[3] as f32 / 255.0,
        ]
    }

    fn f32_to_pixel(c: [f32; 4]) -> [u8; 4] {
        [
            (c[0] * 255.0).round() as u8,
            (c[1] * 255.0).round() as u8,
            (c[2] * 255.0).round() as u8,
            (c[3] * 255.0).round() as u8,
        ]
    }

    fn hash_u(v: u32) -> u32 {
        let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
        let s = ((s >> ((s >> 28).wrapping_add(4))) ^ s).wrapping_mul(277803737);
        (s >> 22) ^ s
    }

    fn hash21_i(ix: i32, iy: i32, seed: i32) -> f32 {
        let h = Self::hash_u(
            (ix as u32) ^ (iy as u32).wrapping_mul(1664525) ^ (seed as u32).wrapping_mul(22695477),
        );
        h as f32 / 4294967295.0
    }

    fn sample_channel(pixels: &[u8], w: u32, h: u32, u: f32, v: f32, ch: usize) -> f32 {
        let x = (u * w as f32).clamp(0.0, w as f32 - 1.0) as usize;
        let y = (v * h as f32).clamp(0.0, h as f32 - 1.0) as usize;
        let idx = (y * w as usize + x) * 4 + ch;
        pixels.get(idx).map_or(0.0, |&b| b as f32 / 255.0)
    }

    fn sample_pixel(pixels: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
        [
            Self::sample_channel(pixels, w, h, u, v, 0),
            Self::sample_channel(pixels, w, h, u, v, 1),
            Self::sample_channel(pixels, w, h, u, v, 2),
            Self::sample_channel(pixels, w, h, u, v, 3),
        ]
    }

    pub fn cursor_info(&self) -> Option<(u32, u32, Vec2, [u8; 4])> {
        let img = self.cursor_image_pos?;
        let (px, py) = (img.x as u32, img.y as u32);
        let uv = img / self.image_size;
        let image = self.image.as_ref()?;
        let idx = (py as usize * image.width as usize + px as usize) * 4;
        let guard = image.pixels.lock().unwrap();
        let p = guard.get(idx..idx + 4)?;
        let rgba = Self::f32_to_pixel(self.apply_modifiers_cpu(
            &guard,
            image.width,
            image.height,
            [uv.x, uv.y],
            Self::pixel_to_f32(p),
        ));
        Some((px, py, uv, rgba))
    }

    pub fn cursor_pixels(&self, size: u32) -> Option<Vec<u8>> {
        let img = self.cursor_image_pos?;
        let (cx, cy) = (img.x as i64, img.y as i64);
        let half = (size / 2) as i64;
        let image = self.image.as_ref()?;
        let (w, h) = (image.width as i64, image.height as i64);
        let guard = image.pixels.lock().unwrap();
        if guard.is_empty() {
            return None;
        }
        let mut pixels = Vec::with_capacity((size * size * 4) as usize);
        for row in 0..size as i64 {
            for col in 0..size as i64 {
                let (x, y) = match self.rotation {
                    0 => (cx - half + col, cy - half + row),
                    1 => (cx - half + row, cy + half - col),
                    2 => (cx + half - col, cy + half - row),
                    3 => (cx + half - row, cy - half + col),
                    _ => unreachable!(),
                };
                if x < 0 || y < 0 || x >= w || y >= h {
                    pixels.extend_from_slice(&[0, 0, 0, 0]);
                    continue;
                }
                let idx = (y as usize * w as usize + x as usize) * 4;
                let p = &guard[idx..idx + 4];
                let uv = [x as f32 / w as f32, y as f32 / h as f32];
                let rgba = Self::f32_to_pixel(self.apply_modifiers_cpu(
                    &guard,
                    w as u32,
                    h as u32,
                    uv,
                    Self::pixel_to_f32(p),
                ));
                pixels.extend_from_slice(&rgba);
            }
        }
        Some(pixels)
    }

    pub fn color_at(&self, pos: Vec2) -> Option<(u32, u32, [u8; 4])> {
        let (px, py) = self.screen_to_image_pixel(pos)?;
        let image = self.image.as_ref()?;
        let idx = (py as usize * image.width as usize + px as usize) * 4;
        let guard = image.pixels.lock().unwrap();
        let p = guard.get(idx..idx + 4)?;
        let uv = [
            px as f32 / image.width as f32,
            py as f32 / image.height as f32,
        ];
        let rgba = Self::f32_to_pixel(self.apply_modifiers_cpu(
            &guard,
            image.width,
            image.height,
            uv,
            Self::pixel_to_f32(p),
        ));
        Some((px, py, rgba))
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
            uniforms: Uniforms {
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
            mipmap_zoom_out: self.mipmap_zoom_out,
            smooth_zoom_in: self.smooth_zoom_in,
            modifiers: self.modifiers.clone(),
            dirty_from: self.dirty_from.lock().unwrap().take(),
            pre_clear_gpu: Arc::clone(&self.pre_clear_gpu),
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
            let scale_msg = |y: f32| {
                if y > 0.0 {
                    Message::ScaleUp(pos)
                } else {
                    Message::ScaleDown(pos)
                }
            };
            let msg = match delta {
                mouse::ScrollDelta::Lines { y, .. } if *y != 0.0 => {
                    state.last_scale = None;
                    Some(scale_msg(*y))
                }
                mouse::ScrollDelta::Pixels { y, .. } if *y != 0.0 => {
                    let now = Instant::now();
                    if state
                        .last_scale
                        .is_none_or(|t| now.duration_since(t) >= SCALE_COOLDOWN)
                    {
                        state.last_scale = Some(now);
                        Some(scale_msg(*y))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(msg) = msg {
                return Some(Action::publish(msg).and_capture());
            }
            return Some(Action::capture());
        }

        match state.drag {
            ViewDragState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(Button::Right)) = event
                    && let Some(pos) = cursor.position_in(bounds)
                {
                    return Some(Action::publish(Message::ContextMenuOpened(Vec2::new(
                        pos.x, pos.y,
                    ))));
                }
                if let Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) = event
                    && let Some(pos) = cursor.position_over(bounds)
                {
                    state.drag = ViewDragState::Panning(pos);
                    return Some(Action::publish(Message::PanStarted));
                }
                if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event
                    && let Some(pos) = cursor.position_in(bounds)
                {
                    return Some(Action::publish(Message::CursorMoved(Vec2::new(
                        pos.x, pos.y,
                    ))));
                }
                if let Event::Mouse(mouse::Event::CursorLeft) = event {
                    return Some(Action::publish(Message::CursorLeft));
                }
            }
            ViewDragState::Panning(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) => {
                    state.drag = ViewDragState::Idle;
                    return Some(Action::publish(Message::PanEnded).and_capture());
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let delta = vec2(position.x - prev.x, prev.y - position.y);
                    state.drag = ViewDragState::Panning(*position);
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
            ViewDragState::Panning(_) => Interaction::Grabbing,
            ViewDragState::Idle => Interaction::Idle,
        }
    }
}
