use std::time::Instant;

use glam::vec2;
use iced::advanced::Shell;
use iced::keyboard::{self, key};
use iced::mouse;
use iced::{Event, Point, Rectangle};

use crate::app::Message;
use crate::wgpu::view_program::wheel_scale_msg;

#[derive(Default)]
pub struct NavState {
    last_scale: Option<Instant>,
    space_held: bool,
    panning: Option<(Point, mouse::Button)>,
}

impl NavState {
    pub fn interaction(&self) -> Option<mouse::Interaction> {
        if self.panning.is_some() {
            Some(mouse::Interaction::Grabbing)
        } else if self.space_held {
            Some(mouse::Interaction::Grab)
        } else {
            None
        }
    }
}

pub fn handle(
    state: &mut NavState,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    allow_space: bool,
    shell: &mut Shell<'_, Message>,
) -> bool {
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed { physical_key, .. })
            if allow_space && *physical_key == key::Physical::Code(key::Code::Space) =>
        {
            state.space_held = true;
            shell.capture_event();
            true
        }
        Event::Keyboard(keyboard::Event::KeyReleased { physical_key, .. })
            if *physical_key == key::Physical::Code(key::Code::Space) =>
        {
            let was_held = state.space_held;
            state.space_held = false;
            if was_held {
                shell.capture_event();
            }
            was_held
        }
        Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
            let Some(pos) = cursor.position_in(bounds) else {
                return false;
            };
            if let Some(msg) = wheel_scale_msg(&mut state.last_scale, delta, vec2(pos.x, pos.y)) {
                shell.publish(msg);
            }
            shell.capture_event();
            true
        }
        Event::Mouse(mouse::Event::ButtonPressed(button))
            if state.panning.is_none()
                && (*button == mouse::Button::Middle
                    || (*button == mouse::Button::Left && state.space_held)) =>
        {
            let Some(pos) = cursor.position_over(bounds) else {
                return false;
            };
            state.panning = Some((pos, *button));
            shell.publish(Message::PanStarted);
            shell.capture_event();
            true
        }
        Event::Mouse(mouse::Event::CursorMoved { position }) if state.panning.is_some() => {
            let (prev, button) = state.panning.unwrap();
            let delta = vec2(position.x - prev.x, prev.y - position.y);
            state.panning = Some((*position, button));
            shell.publish(Message::Pan(delta));
            shell.capture_event();
            true
        }
        Event::Mouse(mouse::Event::ButtonReleased(released))
            if state.panning.is_some_and(|(_, b)| b == *released) =>
        {
            state.panning = None;
            shell.publish(Message::PanEnded);
            shell.capture_event();
            true
        }
        _ => false,
    }
}
