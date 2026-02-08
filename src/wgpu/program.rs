use glam::{Vec2, vec2};
use iced::Rectangle;
use iced::advanced::{Shell, mouse};
use iced::event::Status;
use iced::widget::shader::{Event, Program};

use crate::app::Message;
use crate::wgpu::primitive::{ImagePrimitive, ViewState};

#[derive(Debug, Clone)]
pub enum DragState {
    Idle,
    Dragging(Vec2),
}

impl Default for DragState {
    fn default() -> Self {
        DragState::Idle
    }
}

#[derive(Debug, Default)]
pub struct ImageProgram {
    pub view: ViewState,
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
        ImagePrimitive::new(self.view)
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        _shell: &mut Shell<'_, Message>,
    ) -> (Status, Option<Message>) {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                let pos = Vec2::new(pos.x, pos.y);
                let delta = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y,
                };

                return (
                    Status::Captured,
                    Some(Message::ZoomDelta(pos, bounds, delta)),
                );
            }
        }

        match state {
            DragState::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                    if let Some(pos) = cursor.position_over(bounds) {
                        *state = DragState::Dragging(Vec2::new(pos.x, pos.y));
                        return (Status::Captured, None);
                    }
                }
            }
            DragState::Dragging(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    *state = DragState::Idle;
                    return (Status::Captured, None);
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let current = vec2(position.x, position.y);
                    let delta = vec2(current.x - prev.x, prev.y - current.y);

                    *state = DragState::Dragging(current);
                    return (Status::Captured, Some(Message::PanDelta(delta)));
                }
                _ => {}
            },
        }

        (Status::Ignored, None)
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
