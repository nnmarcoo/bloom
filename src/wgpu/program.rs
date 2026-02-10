use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use glam::{Vec2, vec2};
use iced::Rectangle;
use iced::advanced::mouse;
use iced::Event;
use iced::widget::shader::{Action, Program};

use crate::app::Message;
use crate::wgpu::primitive::{ImagePrimitive, ViewState};

#[derive(Debug, Clone, Default)]
pub enum DragState {
    #[default]
    Idle,
    Dragging(Vec2),
}

pub struct ImageProgram {
    pub view: ViewState,
    pending_image: RefCell<Option<Vec<u8>>>,
    fit_scale: Arc<AtomicU32>,
}

impl std::fmt::Debug for ImageProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageProgram")
            .field("view", &self.view)
            .finish()
    }
}

impl Default for ImageProgram {
    fn default() -> Self {
        let debug_bytes = include_bytes!("../assets/debug.jpg");
        let mut view = ViewState::default();
        if let Ok(reader) =
            image::ImageReader::new(std::io::Cursor::new(debug_bytes as &[u8]))
                .with_guessed_format()
        {
            if let Ok((w, h)) = reader.into_dimensions() {
                view.image_size = vec2(w as f32, h as f32);
            }
        }

        Self {
            view,
            pending_image: RefCell::new(None),
            fit_scale: Arc::new(AtomicU32::new(0f32.to_bits())),
        }
    }
}

impl ImageProgram {
    pub fn set_pending_image(&self, bytes: Vec<u8>) {
        *self.pending_image.borrow_mut() = Some(bytes);
    }

    pub fn resolve_scale(&mut self) {
        if self.view.scale == 0.0 {
            let fit = f32::from_bits(self.fit_scale.load(Ordering::Relaxed));
            if fit > 0.0 {
                self.view.scale = fit;
            }
        }
    }
}

impl Program<Message> for ImageProgram {
    type State = DragState;
    type Primitive = ImagePrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        ImagePrimitive::new(
            self.view,
            self.pending_image.borrow_mut().take(),
            Arc::clone(&self.fit_scale),
        )
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let pos = Vec2::new(pos.x, pos.y);
                let delta = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };

                return Some(
                    Action::publish(Message::ZoomDelta(pos, bounds, delta)).and_capture(),
                );
            }
        }

        match state {
            DragState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                    if let Some(pos) = cursor.position_over(bounds) {
                        *state = DragState::Dragging(Vec2::new(pos.x, pos.y));
                        return Some(Action::capture());
                    }
                }
            }
            DragState::Dragging(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    *state = DragState::Idle;
                    return Some(Action::capture());
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let current = vec2(position.x, position.y);
                    let delta = vec2(current.x - prev.x, prev.y - current.y);

                    *state = DragState::Dragging(current);
                    return Some(Action::publish(Message::PanDelta(delta)).and_capture());
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
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state {
            DragState::Idle => mouse::Interaction::Idle,
            DragState::Dragging(_) => mouse::Interaction::Grabbing,
        }
    }
}
