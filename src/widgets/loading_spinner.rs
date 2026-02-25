use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::time::Instant;
use iced::widget::canvas;
use iced::window;
use iced::{Background, Color, Element, Event, Length, Radians, Rectangle, Renderer, Size, Vector};

use std::f32::consts::PI;
use std::time::Duration;

pub struct Circular<Theme = iced::Theme>
where
    Theme: StyleSheet,
{
    size: f32,
    bar_height: f32,
    style: <Theme as StyleSheet>::Style,
    cycle_duration: Duration,
    rotation_duration: Duration,
}

pub struct Linear<Theme = iced::Theme>
where
    Theme: StyleSheet,
{
    width: Length,
    height: Length,
    style: <Theme as StyleSheet>::Style,
    cycle_duration: Duration,
}

impl<Theme> Circular<Theme>
where
    Theme: StyleSheet,
{
    pub fn new() -> Self {
        Self {
            size: 40.0,
            bar_height: 4.0,
            style: <Theme as StyleSheet>::Style::default(),
            cycle_duration: Duration::from_millis(600),
            rotation_duration: Duration::from_secs(2),
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn bar_height(mut self, height: f32) -> Self {
        self.bar_height = height;
        self
    }

    pub fn style(mut self, style: impl Into<<Theme as StyleSheet>::Style>) -> Self {
        self.style = style.into();
        self
    }

    pub fn cycle_duration(mut self, duration: Duration) -> Self {
        self.cycle_duration = duration / 2;
        self
    }

    pub fn rotation_duration(mut self, duration: Duration) -> Self {
        self.rotation_duration = duration;
        self
    }
}

impl<Theme> Linear<Theme>
where
    Theme: StyleSheet,
{
    pub fn new() -> Self {
        Self {
            width: Length::Fixed(100.0),
            height: Length::Fixed(4.0),
            style: <Theme as StyleSheet>::Style::default(),
            cycle_duration: Duration::from_millis(600),
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn style(mut self, style: impl Into<<Theme as StyleSheet>::Style>) -> Self {
        self.style = style.into();
        self
    }

    pub fn cycle_duration(mut self, duration: Duration) -> Self {
        self.cycle_duration = duration / 2;
        self
    }
}

impl<Theme> Default for Circular<Theme>
where
    Theme: StyleSheet,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Theme> Default for Linear<Theme>
where
    Theme: StyleSheet,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Appearance {
    pub track_color: Color,
    pub bar_color: Color,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            track_color: Color::TRANSPARENT,
            bar_color: Color::from_rgb(0.0, 0.5, 1.0),
        }
    }
}

pub trait StyleSheet {
    type Style: Default + Clone;

    fn appearance(&self, style: &Self::Style) -> Appearance;
}

impl StyleSheet for iced::Theme {
    type Style = ();

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        let palette = self.extended_palette();

        Appearance {
            track_color: palette.background.weak.color,
            bar_color: palette.primary.base.color,
        }
    }
}

const MIN_ANGLE: Radians = Radians(PI / 8.0);
const WRAP_ANGLE: Radians = Radians(2.0 * PI - PI / 4.0);
const BASE_ROTATION_SPEED: u32 = u32::MAX / 80;

#[derive(Clone, Copy)]
enum CircularAnimation {
    Expanding {
        start: Instant,
        progress: f32,
        rotation: u32,
        last: Instant,
    },
    Contracting {
        start: Instant,
        progress: f32,
        rotation: u32,
        last: Instant,
    },
}

impl Default for CircularAnimation {
    fn default() -> Self {
        Self::Expanding {
            start: Instant::now(),
            progress: 0.0,
            rotation: 0,
            last: Instant::now(),
        }
    }
}

impl CircularAnimation {
    fn next(&self, additional_rotation: u32, now: Instant) -> Self {
        match self {
            Self::Expanding { rotation, .. } => Self::Contracting {
                start: now,
                progress: 0.0,
                rotation: rotation.wrapping_add(additional_rotation),
                last: now,
            },
            Self::Contracting { rotation, .. } => {
                Self::Expanding {
                    start: now,
                    progress: 0.0,
                    rotation: rotation.wrapping_add(BASE_ROTATION_SPEED.wrapping_add(
                        (f64::from(WRAP_ANGLE.0 / (2.0 * PI)) * u32::MAX as f64) as u32,
                    )),
                    last: now,
                }
            }
        }
    }

    fn start(&self) -> Instant {
        match self {
            Self::Expanding { start, .. } | Self::Contracting { start, .. } => *start,
        }
    }

    fn last(&self) -> Instant {
        match self {
            Self::Expanding { last, .. } | Self::Contracting { last, .. } => *last,
        }
    }

    fn timed_transition(
        &self,
        cycle_duration: Duration,
        rotation_duration: Duration,
        now: Instant,
    ) -> Self {
        let elapsed = now.duration_since(self.start());
        let additional_rotation = ((now - self.last()).as_secs_f32()
            / rotation_duration.as_secs_f32()
            * (u32::MAX) as f32) as u32;

        match elapsed {
            elapsed if elapsed > cycle_duration => self.next(additional_rotation, now),
            _ => self.with_elapsed(cycle_duration, additional_rotation, elapsed, now),
        }
    }

    fn with_elapsed(
        &self,
        cycle_duration: Duration,
        additional_rotation: u32,
        elapsed: Duration,
        now: Instant,
    ) -> Self {
        let progress = elapsed.as_secs_f32() / cycle_duration.as_secs_f32();
        let eased = ease_in_out_cubic(progress);

        match self {
            Self::Expanding {
                start, rotation, ..
            } => Self::Expanding {
                start: *start,
                progress: eased,
                rotation: rotation.wrapping_add(additional_rotation),
                last: now,
            },
            Self::Contracting {
                start, rotation, ..
            } => Self::Contracting {
                start: *start,
                progress: eased,
                rotation: rotation.wrapping_add(additional_rotation),
                last: now,
            },
        }
    }

    fn rotation(&self) -> f32 {
        match self {
            Self::Expanding { rotation, .. } | Self::Contracting { rotation, .. } => {
                *rotation as f32 / u32::MAX as f32
            }
        }
    }
}

#[derive(Default)]
struct CircularState {
    animation: CircularAnimation,
    cache: canvas::Cache,
}

impl<Message, Theme> Widget<Message, Theme, Renderer> for Circular<Theme>
where
    Message: Clone,
    Theme: StyleSheet,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<CircularState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(CircularState::default())
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
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<CircularState>();

        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            state.animation =
                state
                    .animation
                    .timed_transition(self.cycle_duration, self.rotation_duration, *now);

            state.cache.clear();
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<CircularState>();
        let bounds = layout.bounds();
        let custom_style = theme.appearance(&self.style);

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let track_radius = frame.width() / 2.0 - self.bar_height;
            let track_path = canvas::Path::circle(frame.center(), track_radius);

            frame.stroke(
                &track_path,
                canvas::Stroke::default()
                    .with_color(custom_style.track_color)
                    .with_width(self.bar_height),
            );

            let mut builder = canvas::path::Builder::new();

            let start = Radians(state.animation.rotation() * 2.0 * PI);

            match state.animation {
                CircularAnimation::Expanding { progress, .. } => {
                    builder.arc(canvas::path::Arc {
                        center: frame.center(),
                        radius: track_radius,
                        start_angle: start,
                        end_angle: start + MIN_ANGLE + WRAP_ANGLE * progress,
                    });
                }
                CircularAnimation::Contracting { progress, .. } => {
                    builder.arc(canvas::path::Arc {
                        center: frame.center(),
                        radius: track_radius,
                        start_angle: start + WRAP_ANGLE * progress,
                        end_angle: start + MIN_ANGLE + WRAP_ANGLE,
                    });
                }
            }

            let bar_path = builder.build();

            frame.stroke(
                &bar_path,
                canvas::Stroke::default()
                    .with_color(custom_style.bar_color)
                    .with_width(self.bar_height),
            );
        });

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            use iced::advanced::graphics::geometry::Renderer as _;
            renderer.draw_geometry(geometry);
        });
    }
}

impl<'a, Message, Theme> From<Circular<Theme>> for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: StyleSheet + 'a,
{
    fn from(circular: Circular<Theme>) -> Self {
        Self::new(circular)
    }
}

#[derive(Clone, Copy)]
enum LinearState {
    Expanding { start: Instant, progress: f32 },
    Contracting { start: Instant, progress: f32 },
}

impl Default for LinearState {
    fn default() -> Self {
        Self::Expanding {
            start: Instant::now(),
            progress: 0.0,
        }
    }
}

impl LinearState {
    fn next(&self, now: Instant) -> Self {
        match self {
            Self::Expanding { .. } => Self::Contracting {
                start: now,
                progress: 0.0,
            },
            Self::Contracting { .. } => Self::Expanding {
                start: now,
                progress: 0.0,
            },
        }
    }

    fn start(&self) -> Instant {
        match self {
            Self::Expanding { start, .. } | Self::Contracting { start, .. } => *start,
        }
    }

    fn timed_transition(&self, cycle_duration: Duration, now: Instant) -> Self {
        let elapsed = now.duration_since(self.start());

        match elapsed {
            elapsed if elapsed > cycle_duration => self.next(now),
            _ => self.with_elapsed(cycle_duration, elapsed),
        }
    }

    fn with_elapsed(&self, cycle_duration: Duration, elapsed: Duration) -> Self {
        let progress = elapsed.as_secs_f32() / cycle_duration.as_secs_f32();
        let eased = ease_in_out_cubic(progress);

        match self {
            Self::Expanding { start, .. } => Self::Expanding {
                start: *start,
                progress: eased,
            },
            Self::Contracting { start, .. } => Self::Contracting {
                start: *start,
                progress: eased,
            },
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Linear<Theme>
where
    Message: Clone,
    Theme: StyleSheet,
    Renderer: advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LinearState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(LinearState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<LinearState>();

        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            *state = state.timed_transition(self.cycle_duration, *now);
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let custom_style = theme.appearance(&self.style);
        let state = tree.state.downcast_ref::<LinearState>();

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                },
                ..renderer::Quad::default()
            },
            Background::Color(custom_style.track_color),
        );

        match state {
            LinearState::Expanding { progress, .. } => renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y,
                        width: *progress * bounds.width,
                        height: bounds.height,
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(custom_style.bar_color),
            ),
            LinearState::Contracting { progress, .. } => renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: bounds.x + *progress * bounds.width,
                        y: bounds.y,
                        width: (1.0 - *progress) * bounds.width,
                        height: bounds.height,
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(custom_style.bar_color),
            ),
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Linear<Theme>> for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: StyleSheet + 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(linear: Linear<Theme>) -> Self {
        Self::new(linear)
    }
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - ((-2.0 * t + 2.0).powi(3)) / 2.0
    }
}
