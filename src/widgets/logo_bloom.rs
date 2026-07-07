use std::time::Duration;

use iced::advanced::image::{self, FilterMethod, Image};
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::time::Instant;
use iced::{Element, Event, Length, Radians, Rectangle, Renderer, Size};
use iced::{mouse, window};

use crate::easing::{ease_in_cubic, ease_out_cubic};

const DURATION: Duration = Duration::from_millis(1100);
const ECHOES: usize = 5;
const REACH: f32 = 1.0;
const MAX_ANGLE: f32 = 5.0 * std::f32::consts::PI / 180.0;
const OPACITY: f32 = 0.92;
const FADE_EXPONENT: f32 = 1.6;
const LAG: f32 = 0.18;
const MIN_OPACITY: f32 = 1.0 / 255.0;
const FAN_OUT: f32 = 0.4;
const HOLD: f32 = 0.6;
const MAIN_WOBBLE: f32 = 2.5 * std::f32::consts::PI / 180.0;

const PRESS_DURATION: Duration = Duration::from_millis(220);
const PRESS_SHRINK: f32 = 0.12;

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
    wave: Option<Instant>,
    press: Option<Instant>,
}

fn spread(t: f32) -> f32 {
    if t < FAN_OUT {
        ease_out_cubic(t / FAN_OUT)
    } else if t < HOLD {
        1.0
    } else {
        1.0 - ease_in_cubic((t - HOLD) / (1.0 - HOLD))
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
                let now = Instant::now();
                state.wave = Some(now);
                state.press = Some(now);
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let mut animating = false;
                if let Some(start) = state.wave {
                    if now.duration_since(start) >= DURATION {
                        state.wave = None;
                    } else {
                        animating = true;
                    }
                }
                if let Some(start) = state.press {
                    if now.duration_since(start) >= PRESS_DURATION {
                        state.press = None;
                    } else {
                        animating = true;
                    }
                }
                if animating {
                    shell.request_redraw();
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
        let now = Instant::now();
        let bounds = layout.bounds();

        let press_scale = match state.press {
            Some(start) => {
                let p = (now.duration_since(start).as_secs_f32() / PRESS_DURATION.as_secs_f32())
                    .clamp(0.0, 1.0);
                let dip = (p * std::f32::consts::PI).sin();
                1.0 - PRESS_SHRINK * dip
            }
            None => 1.0,
        };

        let inset = bounds.width * (1.0 - press_scale) * 0.5;
        let stage = Rectangle {
            x: bounds.x + inset,
            y: bounds.y + inset,
            width: bounds.width * press_scale,
            height: bounds.height * press_scale,
        };

        let draw_copy = |renderer: &mut Renderer, dx: f32, angle: f32, opacity: f32| {
            renderer.draw_image(
                Image {
                    handle: self.handle.clone(),
                    filter_method: FilterMethod::Linear,
                    rotation: Radians(angle),
                    border_radius: iced::border::Radius::default(),
                    opacity,
                    snap: false,
                },
                Rectangle {
                    x: stage.x + dx,
                    ..stage
                },
                *viewport,
            );
        };

        let t = state.wave.map(|start| {
            (now.duration_since(start).as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0)
        });

        if let Some(t) = t {
            let spread = spread(t);
            let travel = stage.width * REACH * spread;

            for i in (1..=ECHOES).rev() {
                let f = i as f32 / ECHOES as f32;
                let opacity = OPACITY * (1.0 - f).powf(FADE_EXPONENT) * spread;
                if opacity < MIN_OPACITY {
                    continue;
                }
                let dx = -travel * f * (1.0 - LAG * (1.0 - f));
                let angle = MAX_ANGLE * spread * (std::f32::consts::TAU * (t - f)).sin();
                draw_copy(renderer, dx, angle, opacity);
            }

            let main_angle = MAIN_WOBBLE * spread * (std::f32::consts::TAU * t).sin();
            draw_copy(renderer, 0.0, main_angle, 1.0);
        } else {
            draw_copy(renderer, 0.0, 0.0, 1.0);
        }
    }
}

impl<'a, Message, Theme> From<LogoBloom> for Element<'a, Message, Theme, Renderer> {
    fn from(logo: LogoBloom) -> Self {
        Self::new(logo)
    }
}
