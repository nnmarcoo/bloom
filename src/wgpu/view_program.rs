use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, vec2, vec3, vec4};
use iced::{
    Event, Point, Rectangle,
    mouse::{self, Button, Cursor, Interaction},
    widget::{Action, shader::Program},
};

use crate::{
    app::Message,
    wgpu::{
        media::animation::Animation, media::exif_data::ExifData, media::image_data::ImageData,
        scale::Scale, view_pipeline::Uniforms, view_primitive::ViewPrimitive,
    },
};

const SCALE_COOLDOWN: Duration = Duration::from_millis(10);

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
    pub lanczos_enabled: bool,
    cursor_pos: Option<Vec2>,
    rotation: u8, // 0=0°, 1=90°CW, 2=180°, 3=270°CW
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
            lanczos_enabled: false,
            cursor_pos: None,
            rotation: 0,
        }
    }
}

impl ViewProgram {
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
        let (fw, fh) = if self.rotation % 2 == 0 {
            (self.image_size.x, self.image_size.y)
        } else {
            (self.image_size.y, self.image_size.x)
        };
        self.scale.fit_dims(fw, fh, self.bounds);
        self.offset = Vec2::ZERO;
    }

    pub fn rotate(&mut self) {
        self.rotation = (self.rotation + 1) % 4;
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
        self.offset = self.offset.clamp(-self.image_size, self.image_size);
    }

    // Returns aspect vec2 that maps the image quad to correct proportions given
    // the current rotation. At 90/270 the image axes are swapped on screen,
    // so each image dimension is divided by the opposite viewport dimension.
    fn aspect(&self, viewport: Vec2) -> Vec2 {
        if self.rotation % 2 == 0 {
            self.image_size / viewport
        } else {
            vec2(
                self.image_size.x / viewport.y,
                self.image_size.y / viewport.x,
            )
        }
    }

    pub fn set_image(&mut self, data: ImageData) {
        self.image_size = vec2(data.width as f32, data.height as f32);
        self.image = Some(Arc::new(data));
        self.animation = None;
        self.cursor_pos = None;
        self.rotation = 0;
    }

    pub fn histogram(&self) -> Option<&([u32; 256], [u32; 256], [u32; 256])> {
        if let Some(anim) = &self.animation {
            return Some(anim.current_histogram());
        }
        self.image.as_deref().map(|d| &d.histogram)
    }

    pub fn exif(&self) -> Option<&ExifData> {
        self.image.as_deref().map(|d| &d.exif)
    }

    pub fn set_animation(&mut self, anim: Animation) {
        let first = Arc::clone(anim.current_image());
        self.image_size = vec2(first.width as f32, first.height as f32);
        self.image = Some(first);
        self.animation = Some(anim);
        self.cursor_pos = None;
        self.rotation = 0;
    }

    pub fn set_cursor_pos(&mut self, pos: Option<Vec2>) {
        self.cursor_pos = pos;
    }

    pub fn tick_animation(&mut self, now: Instant) {
        if let Some(ref mut anim) = self.animation {
            if let Some(frame) = anim.tick(now) {
                self.image = Some(frame);
            }
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

    pub fn animation_info(&self) -> Option<(usize, usize)> {
        self.animation
            .as_ref()
            .map(|a| (a.current_index(), a.frame_count()))
    }

    pub fn animation_duration(&self) -> Option<Duration> {
        self.animation.as_ref().map(|a| a.total_duration())
    }

    pub fn decoded_size_bytes(&self) -> Option<usize> {
        self.image.as_ref().map(|img| img.size_bytes())
    }

    pub fn screen_to_image_pixel(&self, screen_pos: Vec2) -> Option<(u32, u32)> {
        let viewport = vec2(self.bounds.width, self.bounds.height);
        if self.image_size == Vec2::ZERO || viewport.x < 1.0 || viewport.y < 1.0 {
            return None;
        }
        let screen_ndc = vec2(
            (screen_pos.x / viewport.x) * 2.0 - 1.0,
            1.0 - (screen_pos.y / viewport.y) * 2.0,
        );
        let s = self.scale.value();
        let aspect = self.aspect(viewport);
        let pan_ndc = self.offset / viewport;
        let angle = -(self.rotation as f32) * std::f32::consts::FRAC_PI_2;
        let transform = Mat4::from_scale(vec3(s, s, 1.0))
            * Mat4::from_translation(vec3(pan_ndc.x, pan_ndc.y, 0.0))
            * Mat4::from_rotation_z(angle)
            * Mat4::from_scale(vec3(aspect.x, aspect.y, 1.0));
        let img_ndc = (transform.inverse() * vec4(screen_ndc.x, screen_ndc.y, 0.0, 1.0))
            .truncate()
            .truncate();
        let img = (img_ndc + 1.0) * 0.5 * vec2(self.image_size.x, -self.image_size.y)
            + vec2(0.0, self.image_size.y);
        if img.x < 0.0 || img.y < 0.0 || img.x >= self.image_size.x || img.y >= self.image_size.y {
            return None;
        }
        Some((img.x as u32, img.y as u32))
    }

    pub fn cursor_info(&self) -> Option<(u32, u32, [u8; 4])> {
        self.color_at(self.cursor_pos?)
    }

    pub fn color_at(&self, pos: Vec2) -> Option<(u32, u32, [u8; 4])> {
        let (px, py) = self.screen_to_image_pixel(pos)?;
        let image = self.image.as_ref()?;
        let idx = (py as usize * image.width as usize + px as usize) * 4;
        let p = image.pixels.get(idx..idx + 4)?;
        Some((px, py, [p[0], p[1], p[2], p[3]]))
    }
}

impl Program<Message> for ViewProgram {
    type State = ViewProgramState;
    type Primitive = ViewPrimitive;

    fn draw(&self, _state: &Self::State, _cursor: Cursor, bounds: Rectangle) -> Self::Primitive {
        let viewport = vec2(bounds.width, bounds.height);
        let s = self.scale.value();
        let aspect = self.aspect(viewport);
        let pan_ndc = self.offset / viewport;
        let angle = -(self.rotation as f32) * std::f32::consts::FRAC_PI_2;

        let transform = Mat4::from_scale(vec3(s, s, 1.0))
            * Mat4::from_translation(vec3(pan_ndc.x, pan_ndc.y, 0.0))
            * Mat4::from_rotation_z(angle)
            * Mat4::from_scale(vec3(aspect.x, aspect.y, 1.0));

        ViewPrimitive {
            uniforms: Uniforms { transform },
            image: self.image.clone(),
            scale: s,
            pan_ndc,
            rotation: self.rotation,
            bounds,
            lanczos_enabled: self.lanczos_enabled,
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

        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let pos = Vec2::new(pos.x, pos.y);
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                let now = Instant::now();
                let ready = state
                    .last_scale
                    .map_or(true, |t| now.duration_since(t) >= SCALE_COOLDOWN);
                if ready && y != 0.0 {
                    state.last_scale = Some(now);
                    let msg = if y > 0.0 {
                        Message::ScaleUp(pos)
                    } else {
                        Message::ScaleDown(pos)
                    };
                    return Some(Action::publish(msg).and_capture());
                }
                return Some(Action::capture());
            }
        }

        match state.drag {
            ViewDragState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(Button::Right)) = event {
                    if let Some(pos) = cursor.position_in(bounds) {
                        return Some(Action::publish(Message::ContextMenuOpened(Vec2::new(
                            pos.x, pos.y,
                        ))));
                    }
                }
                if let Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) = event {
                    if let Some(pos) = cursor.position_over(bounds) {
                        state.drag = ViewDragState::Panning(pos);
                        return Some(Action::capture());
                    }
                }
                if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
                    if let Some(pos) = cursor.position_in(bounds) {
                        return Some(Action::publish(Message::CursorMoved(Vec2::new(
                            pos.x, pos.y,
                        ))));
                    }
                }
                if let Event::Mouse(mouse::Event::CursorLeft) = event {
                    return Some(Action::publish(Message::CursorLeft));
                }
            }
            ViewDragState::Panning(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) => {
                    state.drag = ViewDragState::Idle;
                    return Some(Action::capture());
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
