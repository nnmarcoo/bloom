use iced::{
    widget::{
        canvas::{self, Canvas, Cursor, Event, Frame, Geometry},
        container, image,
    },
    mouse, Rectangle, Length, Element, Theme, Renderer, Point, Vector,
};
use glam::Vec2;
use crate::app::Message;

pub fn main_panel<'a>(pos: Vec2) -> Element<'a, Message> {
    const IMAGE_BYTES: &[u8] = include_bytes!("../assets/debug.jpg");
    let handle = image::Handle::from_bytes(IMAGE_BYTES);
    let image_canvas = ImageCanvas::new(handle, pos.x, pos.y);

    container(
        Canvas::new(image_canvas)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .into()
}

#[derive(Debug)]
struct ImageCanvas {
    handle: image::Handle,
    initial_x: f32,
    initial_y: f32,
}

impl ImageCanvas {
    pub fn new(handle: image::Handle, x: f32, y: f32) -> Self {
        Self { handle, initial_x: x, initial_y: y }
    }
}

#[derive(Debug, Clone)]
enum MouseInteraction {
    Idle,
    Panning(Vec2),
}

impl Default for MouseInteraction {
    fn default() -> Self {
        MouseInteraction::Idle
    }
}

#[derive(Debug, Default)]
struct State {
    interaction: MouseInteraction,
    translation: Vec2,
    zoom: f32,
}

impl<Message> canvas::Program<Message> for ImageCanvas {
    type State = State;

    fn update(
        &self,
        state: &mut Self::State,
        event: Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        // --- Handle scroll wheel zoom ---
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            if let Some(_pos) = cursor.position_in(&bounds) {
                let delta_y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 100.0,
                };
                state.zoom = (state.zoom + delta_y * 0.1).clamp(0.1, 5.0);
                return (canvas::event::Status::Captured, None);
            }
        }

        // --- Handle dragging (panning) ---
        match state.interaction {
            MouseInteraction::Idle => {
                if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                    if let Some(pos) = cursor.position_in(&bounds) {
                        state.interaction = MouseInteraction::Panning(vec2(pos.x, pos.y));
                        return (canvas::event::Status::Captured, None);
                    }
                }
            }
            MouseInteraction::Panning(prev) => match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    state.interaction = MouseInteraction::Idle;
                    return (canvas::event::Status::Captured, None);
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    let pos = vec2(position.x, position.y);
                    let delta = pos - prev;
                    state.translation += delta;
                    state.interaction = MouseInteraction::Panning(pos);
                    return (canvas::event::Status::Captured, None);
                }
                _ => {}
            },
        }

        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let x = self.initial_x + state.translation.x;
        let y = self.initial_y + state.translation.y;
        let size = 200.0 * state.zoom;

        frame.draw_image(
            Rectangle {
                x,
                y,
                width: size,
                height: size,
            },
            canvas::Image {
                handle: self.handle.clone(),
                filter_method: image::FilterMethod::Linear,
                rotation: iced::Radians(0.0),
                opacity: 1.0,
                snap: false,
            },
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> mouse::Interaction {
        match state.interaction {
            MouseInteraction::Idle => mouse::Interaction::Grab,
            MouseInteraction::Panning(_) => mouse::Interaction::Grabbing,
        }
    }
}
