use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::window;
use iced::{Background, Border, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::styles::radius;

// how close the cursor must be to a trim edge to grab it instead of seeking
const HANDLE_GRAB_PX: f32 = 6.0;
const HANDLE_W: f32 = 3.0;

pub struct Timeline<Message> {
    playing: bool,
    position: f32,
    total: usize,
    on_seek: Box<dyn Fn(usize) -> Message>,
    on_drag_start: Option<Message>,
    on_drag_end: Option<Message>,
    height: f32,
    trim: Option<(f32, f32)>,
    on_trim: Option<Box<dyn Fn(TrimEdge, f32) -> Message>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    Start,
    End,
}

impl<Message> Timeline<Message> {
    pub fn new(
        playing: bool,
        position: f32,
        total: usize,
        on_seek: impl Fn(usize) -> Message + 'static,
    ) -> Self {
        Self {
            playing,
            position,
            total,
            on_seek: Box::new(on_seek),
            on_drag_start: None,
            on_drag_end: None,
            height: 28.0,
            trim: None,
            on_trim: None,
        }
    }

    pub fn on_drag_start(mut self, msg: Message) -> Self {
        self.on_drag_start = Some(msg);
        self
    }

    pub fn on_drag_end(mut self, msg: Message) -> Self {
        self.on_drag_end = Some(msg);
        self
    }

    // draggable trim edges as fractions of the full media, with their change handler
    pub fn trim(
        mut self,
        range: (f32, f32),
        on_trim: impl Fn(TrimEdge, f32) -> Message + 'static,
    ) -> Self {
        self.trim = Some(range);
        self.on_trim = Some(Box::new(on_trim));
        self
    }

    fn edge_x(&self, bounds: Rectangle, edge: TrimEdge) -> Option<f32> {
        let (start, end) = self.trim?;
        let frac = match edge {
            TrimEdge::Start => start,
            TrimEdge::End => end,
        };
        Some(bounds.x + bounds.width * frac.clamp(0.0, 1.0))
    }

    fn edge_at(&self, bounds: Rectangle, x: f32) -> Option<TrimEdge> {
        self.on_trim.as_ref()?;
        let candidates = [TrimEdge::Start, TrimEdge::End];
        candidates
            .into_iter()
            .filter_map(|e| self.edge_x(bounds, e).map(|ex| (e, (x - ex).abs())))
            .filter(|(_, d)| *d <= HANDLE_GRAB_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e)
    }
}

#[derive(Default)]
struct State {
    drag_x: Option<f32>,
    trim_drag: Option<TrimEdge>,
}

impl<Message> Widget<Message, iced::Theme, Renderer> for Timeline<Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, limits.max().width, self.height)
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

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            if self.playing && state.drag_x.is_none() && state.trim_drag.is_none() {
                shell.request_redraw();
            }
            return;
        }

        let bounds = layout.bounds();

        let frame_from_x = |x: f32| -> usize {
            if self.total <= 1 {
                return 0;
            }
            let t = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
            (t * (self.total - 1) as f32).round() as usize
        };

        let frac_from_x =
            |x: f32| -> f32 { ((x - bounds.x) / bounds.width.max(1.0)).clamp(0.0, 1.0) };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    // grabbing a trim edge takes priority over seeking
                    if let Some(edge) = self.edge_at(bounds, pos.x) {
                        state.trim_drag = Some(edge);
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    state.drag_x = Some(pos.x);
                    if let Some(msg) = self.on_drag_start.clone() {
                        shell.publish(msg);
                    }
                    shell.publish((self.on_seek)(frame_from_x(pos.x)));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.trim_drag.is_some() => {
                if let (Some(edge), Some(on_trim)) = (state.trim_drag, self.on_trim.as_ref()) {
                    shell.publish(on_trim(edge, frac_from_x(position.x)));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.drag_x.is_some() => {
                state.drag_x = Some(position.x);
                shell.publish((self.on_seek)(frame_from_x(position.x)));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.trim_drag.take().is_some() =>
            {
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag_x.take().is_some() =>
            {
                if let Some(msg) = self.on_drag_end.clone() {
                    shell.publish(msg);
                }
                shell.capture_event();
            }
            _ => {}
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

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let palette = theme.extended_palette();

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: radius().into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.background.base.color),
        );

        let track_h = 4.0_f32;
        let track_y = bounds.center_y() - track_h / 2.0;

        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: track_h,
                },
                border: Border::default(),
                ..Quad::default()
            },
            Background::Color(palette.background.strong.color),
        );

        let progress = if let Some(x) = state.drag_x {
            ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
        } else {
            self.position
        };

        if progress > 0.0 {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: track_y,
                        width: bounds.width * progress,
                        height: track_h,
                    },
                    border: Border::default(),
                    ..Quad::default()
                },
                Background::Color(palette.primary.base.color),
            );
        }

        if let Some((trim_start, trim_end)) = self.trim {
            let x_of = |f: f32| bounds.x + bounds.width * f.clamp(0.0, 1.0);
            let (sx, ex) = (x_of(trim_start), x_of(trim_end));
            let excluded = palette.background.base.color.scale_alpha(0.55);

            // shade the spans the trim discards
            for (x, width) in [
                (bounds.x, sx - bounds.x),
                (ex, bounds.x + bounds.width - ex),
            ] {
                if width > 0.0 {
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x,
                                y: bounds.y,
                                width,
                                height: bounds.height,
                            },
                            border: Border::default(),
                            ..Quad::default()
                        },
                        Background::Color(excluded),
                    );
                }
            }

            for x in [sx, ex] {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: (x - HANDLE_W / 2.0).round(),
                            y: bounds.y,
                            width: HANDLE_W,
                            height: bounds.height,
                        },
                        border: Border {
                            radius: radius().into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(palette.success.base.color),
                );
            }
        }

        if self.total > 1 {
            let max_ticks = (bounds.width / 4.0) as usize;
            let step = ((self.total - 1) / max_ticks.max(1)).max(1);
            let tick_h_major = 5.0_f32;
            let tick_h_minor = 3.0_f32;
            let tick_w = 1.0_f32;
            let tick_top = track_y - tick_h_major - 1.0;
            let color_minor = palette.background.base.text.scale_alpha(0.25);
            let color_major = palette.background.base.text.scale_alpha(0.45);

            for i in (step..self.total.saturating_sub(step)).step_by(step) {
                let t = i as f32 / (self.total - 1) as f32;
                let x = (bounds.x + bounds.width * t).round();
                let is_major = i % (step * 5) == 0;
                let tick_h = if is_major { tick_h_major } else { tick_h_minor };
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: x - tick_w / 2.0,
                            y: tick_top + (tick_h_major - tick_h),
                            width: tick_w,
                            height: tick_h,
                        },
                        border: Border::default(),
                        ..Quad::default()
                    },
                    Background::Color(if is_major { color_major } else { color_minor }),
                );
            }
        }

        let thumb_w = 4.0_f32;
        let thumb_cx = (bounds.x + bounds.width * progress)
            .min(bounds.x + bounds.width - thumb_w / 2.0)
            .max(bounds.x + thumb_w / 2.0)
            .round();
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: thumb_cx - thumb_w / 2.0,
                    y: bounds.y,
                    width: thumb_w,
                    height: bounds.height,
                },
                border: Border {
                    radius: radius().into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(palette.primary.strong.color),
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        if state.trim_drag.is_some() {
            return mouse::Interaction::ResizingHorizontally;
        }
        if let Some(pos) = cursor.position_over(bounds)
            && self.edge_at(bounds, pos.x).is_some()
        {
            return mouse::Interaction::ResizingHorizontally;
        }
        if state.drag_x.is_some() || cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message> From<Timeline<Message>> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(widget: Timeline<Message>) -> Self {
        Self::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 28.0,
    };

    fn timeline_with_trim(start: f32, end: f32) -> Timeline<()> {
        Timeline::new(false, 0.0, 100, |_| ()).trim((start, end), |_, _| ())
    }

    #[test]
    fn edges_map_to_pixel_positions() {
        let t = timeline_with_trim(0.25, 0.75);
        assert_eq!(t.edge_x(BOUNDS, TrimEdge::Start), Some(25.0));
        assert_eq!(t.edge_x(BOUNDS, TrimEdge::End), Some(75.0));
    }

    #[test]
    fn cursor_grabs_the_nearest_edge() {
        let t = timeline_with_trim(0.25, 0.75);
        assert_eq!(t.edge_at(BOUNDS, 25.0), Some(TrimEdge::Start));
        assert_eq!(t.edge_at(BOUNDS, 74.0), Some(TrimEdge::End));
        assert_eq!(
            t.edge_at(BOUNDS, 50.0),
            None,
            "midpoint should seek, not trim"
        );
    }

    #[test]
    fn adjacent_edges_resolve_to_the_closest() {
        let t = timeline_with_trim(0.50, 0.54);
        assert_eq!(t.edge_at(BOUNDS, 50.5), Some(TrimEdge::Start));
        assert_eq!(t.edge_at(BOUNDS, 53.5), Some(TrimEdge::End));
    }

    #[test]
    fn no_edges_without_a_trim() {
        let t: Timeline<()> = Timeline::new(false, 0.0, 100, |_| ());
        assert_eq!(t.edge_at(BOUNDS, 25.0), None);
        assert_eq!(t.edge_x(BOUNDS, TrimEdge::Start), None);
    }

    #[test]
    fn grab_zone_has_limited_reach() {
        let t = timeline_with_trim(0.25, 0.75);
        let just_inside = 25.0 + HANDLE_GRAB_PX - 0.5;
        let just_outside = 25.0 + HANDLE_GRAB_PX + 0.5;
        assert_eq!(t.edge_at(BOUNDS, just_inside), Some(TrimEdge::Start));
        assert_eq!(t.edge_at(BOUNDS, just_outside), None);
    }
}
