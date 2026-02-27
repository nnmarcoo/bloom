use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, vec2, vec3};
use iced::{
    Event, Point, Rectangle,
    keyboard::{
        self,
        key::{self, Physical},
    },
    mouse::{self, Button, Cursor, Interaction},
    widget::{Action, shader::Program},
};

use crate::{
    app::Message,
    wgpu::{
        media::animation::Animation, media::image_data::ImageData, scale::Scale,
        view_pipeline::Uniforms, view_primitive::ViewPrimitive,
    },
};

#[derive(Default)]
pub enum ViewProgramState {
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
        self.scale.fit(self.image_size, self.bounds);
        self.offset = Vec2::ZERO;
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

    pub fn set_image(&mut self, data: ImageData) {
        self.image_size = vec2(data.width as f32, data.height as f32);
        self.image = Some(Arc::new(data));
        self.animation = None;
    }

    pub fn set_animation(&mut self, anim: Animation) {
        let first = Arc::clone(anim.current_image());
        self.image_size = vec2(first.width as f32, first.height as f32);
        self.image = Some(first);
        self.animation = Some(anim);
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

    pub fn image_size(&self) -> Option<(u32, u32)> {
        if self.image_size == Vec2::ZERO {
            return None;
        }
        Some((self.image_size.x as u32, self.image_size.y as u32))
    }

    pub fn animation_info(&self) -> Option<(usize, usize)> {
        self.animation.as_ref().map(|a| (a.current_index(), a.frame_count()))
    }
}

impl Program<Message> for ViewProgram {
    type State = ViewProgramState;
    type Primitive = ViewPrimitive;

    fn draw(&self, _state: &Self::State, _cursor: Cursor, bounds: Rectangle) -> Self::Primitive {
        let viewport = vec2(bounds.width, bounds.height);
        let s = self.scale.value();
        let aspect = self.image_size / viewport;
        let pan_ndc = self.offset / viewport;

        let transform = Mat4::from_scale(vec3(s, s, 1.0))
            * Mat4::from_translation(vec3(pan_ndc.x, pan_ndc.y, 0.0))
            * Mat4::from_scale(vec3(aspect.x, aspect.y, 1.0));

        ViewPrimitive {
            uniforms: Uniforms { transform },
            image: self.image.clone(),
            scale: s,
            pan_ndc,
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

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            physical_key,
            modifiers,
            ..
        }) = event
        {
            if modifiers.control() {
                if let Physical::Code(code) = physical_key {
                    let center = vec2(bounds.width * 0.5, bounds.height * 0.5);
                    let msg = match code {
                        key::Code::Equal => Some(Message::ScaleUp(center)),
                        key::Code::Minus => Some(Message::ScaleDown(center)),
                        key::Code::Digit0 => Some(Message::Fit),
                        key::Code::Digit1 => Some(Message::Scale(1.0)),
                        key::Code::Digit2 => Some(Message::Scale(2.0)),
                        key::Code::Digit3 => Some(Message::Scale(3.0)),
                        key::Code::Digit4 => Some(Message::Scale(4.0)),
                        key::Code::Digit5 => Some(Message::Scale(5.0)),
                        key::Code::Digit6 => Some(Message::Scale(6.0)),
                        key::Code::Digit7 => Some(Message::Scale(7.0)),
                        key::Code::Digit8 => Some(Message::Scale(8.0)),
                        key::Code::Digit9 => Some(Message::Scale(9.0)),
                        _ => None,
                    };
                    if let Some(msg) = msg {
                        return Some(Action::publish(msg).and_capture());
                    }
                }
            }
        }

        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let pos = Vec2::new(pos.x, pos.y);
                let delta = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                let msg = if delta > 0.0 {
                    Message::ScaleUp(pos)
                } else {
                    Message::ScaleDown(pos)
                };
                return Some(Action::publish(msg).and_capture());
            }
        }

        match state {
            ViewProgramState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) = event {
                    if let Some(pos) = cursor.position_over(bounds) {
                        *state = ViewProgramState::Panning(pos);
                        return Some(Action::capture());
                    }
                }
            }
            ViewProgramState::Panning(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) => {
                    *state = ViewProgramState::Idle;
                    return Some(Action::capture());
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let delta = vec2(position.x - prev.x, prev.y - position.y);
                    *state = ViewProgramState::Panning(*position);
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
        match state {
            ViewProgramState::Panning(_) => Interaction::Grabbing,
            ViewProgramState::Idle => Interaction::Idle,
        }
    }
}
