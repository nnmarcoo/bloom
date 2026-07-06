use std::time::Duration;

use iced::advanced::image::{self, FilterMethod, Image};
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::time::Instant;
use iced::{Element, Event, Length, Radians, Rectangle, Renderer, Size};
use iced::{mouse, window};

const DURATION: Duration = Duration::from_millis(1000);
const ECHOES: usize = 5;
const REACH: f32 = 1.0;
const OPACITY: f32 = 0.92;
const FADE_EXPONENT: f32 = 1.6;
const LAG: f32 = 0.18;
const FAN_OUT: f32 = 0.4;
const HOLD: f32 = 0.6;
const MIN_OPACITY: f32 = 1.0 / 255.0;

pub struct LogoBloom {
    handle: image::Handle,
    size: f32,
}

impl LogoBloom {
    pub fn new(handle: image::Handle, size: f32) -> Self {
        Self { handle, size }
    }
}

#[derive(Default)]
struct State {
    playing: Option<Instant>,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in(t: f32) -> f32 {
    t * t * t
}

fn spread(t: f32) -> f32 {
    if t < FAN_OUT {
        ease_out(t / FAN_OUT)
    } else if t < HOLD {
        1.0
    } else {
        1.0 - ease_in((t - HOLD) / (1.0 - HOLD))
    }
}

impl<Message, Theme> Widget<Message, Theme, Renderer> for LogoBloom {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size),
            height: Length::Fixed(self.size),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.size, self.size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                state.playing = Some(Instant::now());
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if let Some(start) = state.playing {
                    if now.duration_since(start) >= DURATION {
                        state.playing = None;
                    } else {
                        shell.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::image::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        let draw_copy = |renderer: &mut Renderer, dx: f32, opacity: f32| {
            renderer.draw_image(
                Image {
                    handle: self.handle.clone(),
                    filter_method: FilterMethod::Linear,
                    rotation: Radians(0.0),
                    border_radius: iced::border::Radius::default(),
                    opacity,
                    snap: false,
                },
                Rectangle {
                    x: bounds.x + dx,
                    ..bounds
                },
                *viewport,
            );
        };

        let spread = match state.playing {
            Some(start) => {
                let t = (Instant::now().duration_since(start).as_secs_f32()
                    / DURATION.as_secs_f32())
                .clamp(0.0, 1.0);
                spread(t)
            }
            None => 0.0,
        };

        if spread > 0.0 {
            let travel = bounds.width * REACH * spread;
            for i in (1..=ECHOES).rev() {
                let f = i as f32 / ECHOES as f32;
                let opacity = OPACITY * (1.0 - f).powf(FADE_EXPONENT) * spread;
                if opacity < MIN_OPACITY {
                    continue;
                }
                let dx = -travel * f * (1.0 - LAG * (1.0 - f));
                draw_copy(renderer, dx, opacity);
            }
        }

        draw_copy(renderer, 0.0, 1.0);
    }
}

impl<'a, Message, Theme> From<LogoBloom> for Element<'a, Message, Theme, Renderer> {
    fn from(logo: LogoBloom) -> Self {
        Self::new(logo)
    }
}
