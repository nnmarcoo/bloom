use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget};
use iced::mouse;
use iced::overlay;
use iced::{
    Background, Border, Color, Element, Event, Gradient, Length, Point, Rectangle, Renderer, Size,
    Theme, Vector, gradient::Linear,
};

use crate::styles::radius;

const SWATCH_W: f32 = 70.0;
const SWATCH_H: f32 = 22.0;
const POPUP_W: f32 = 180.0;
const SQUARE: f32 = 160.0;
const HUE_H: f32 = 14.0;
const GAP: f32 = 8.0;
const PAD: f32 = 8.0;

fn popup_size() -> Size {
    Size::new(POPUP_W, PAD + SQUARE + GAP + HUE_H + PAD)
}

#[derive(Default)]
struct State {
    open: bool,
    drag: Drag,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Drag {
    #[default]
    None,
    Square,
    Hue,
}

pub struct ColorSwatch<Message> {
    rgb: [f32; 3],
    on_change: Box<dyn Fn([f32; 3]) -> Message>,
}

impl<Message> ColorSwatch<Message> {
    pub fn new(r: f32, g: f32, b: f32, on_change: impl Fn([f32; 3]) -> Message + 'static) -> Self {
        Self {
            rgb: [r, g, b],
            on_change: Box::new(on_change),
        }
    }
}

fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if max == rgb[0] {
        (((rgb[1] - rgb[2]) / d) % 6.0) / 6.0
    } else if max == rgb[1] {
        ((rgb[2] - rgb[0]) / d + 2.0) / 6.0
    } else {
        ((rgb[0] - rgb[1]) / d + 4.0) / 6.0
    };
    [h.rem_euclid(1.0), s, v]
}

fn hsv_to_rgb(hsv: [f32; 3]) -> [f32; 3] {
    let [h, s, v] = hsv;
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (i as i32).rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn color(rgb: [f32; 3]) -> Color {
    Color::from_rgb(rgb[0], rgb[1], rgb[2])
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for ColorSwatch<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(SWATCH_W),
            height: Length::Fixed(SWATCH_H),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, SWATCH_W, SWATCH_H)
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.is_over(layout.bounds())
        {
            let state = tree.state.downcast_mut::<State>();
            state.open = !state.open;
            shell.capture_event();
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let palette = theme.extended_palette();
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: radius().into(),
                },
                ..Quad::default()
            },
            Background::Color(color(self.rgb)),
        );
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

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if !tree.state.downcast_ref::<State>().open {
            return None;
        }
        let position = layout.position() + translation;
        let bounds = layout.bounds();
        Some(overlay::Element::new(Box::new(PickerOverlay {
            state: &mut tree.state,
            rgb: self.rgb,
            on_change: &self.on_change,
            anchor: Rectangle {
                x: position.x,
                y: position.y,
                width: bounds.width,
                height: bounds.height,
            },
        })))
    }
}

struct PickerOverlay<'b, Message> {
    state: &'b mut tree::State,
    rgb: [f32; 3],
    on_change: &'b dyn Fn([f32; 3]) -> Message,
    anchor: Rectangle,
}

impl<Message> PickerOverlay<'_, Message> {
    fn square_rect(&self, origin: Point) -> Rectangle {
        Rectangle {
            x: origin.x + PAD,
            y: origin.y + PAD,
            width: SQUARE,
            height: SQUARE,
        }
    }
    fn hue_rect(&self, origin: Point) -> Rectangle {
        Rectangle {
            x: origin.x + PAD,
            y: origin.y + PAD + SQUARE + GAP,
            width: POPUP_W - 2.0 * PAD,
            height: HUE_H,
        }
    }
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for PickerOverlay<'_, Message> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        let size = popup_size();
        let x = self.anchor.x;
        let y = self.anchor.y + self.anchor.height + 4.0;
        let x = x.clamp(0.0, (bounds.width - size.width).max(0.0));
        let y = y.clamp(0.0, (bounds.height - size.height).max(0.0));
        layout::Node::new(size).move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer as _;
        let origin = layout.bounds().position();
        let palette = theme.extended_palette();

        renderer.fill_quad(
            Quad {
                bounds: layout.bounds(),
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: radius().into(),
                },
                ..Quad::default()
            },
            Background::Color(palette.background.weak.color),
        );

        let hsv = rgb_to_hsv(self.rgb);
        let square = self.square_rect(origin);
        let hue = self.hue_rect(origin);

        let hue_color = color(hsv_to_rgb([hsv[0], 1.0, 1.0]));
        renderer.fill_quad(
            Quad {
                bounds: square,
                ..Quad::default()
            },
            Background::Gradient(Gradient::Linear(
                Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(1.0, hue_color),
            )),
        );
        renderer.fill_quad(
            Quad {
                bounds: square,
                ..Quad::default()
            },
            Background::Gradient(Gradient::Linear(
                Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, Color::TRANSPARENT)
                    .add_stop(1.0, Color::BLACK),
            )),
        );
        let cx = square.x + hsv[1] * square.width;
        let cy = square.y + (1.0 - hsv[2]) * square.height;
        ring(renderer, cx, cy);

        renderer.fill_quad(
            Quad {
                bounds: hue,
                ..Quad::default()
            },
            Background::Gradient(Gradient::Linear({
                let mut l = Linear::new(std::f32::consts::FRAC_PI_2);
                for i in 0..=6 {
                    let t = i as f32 / 6.0;
                    l = l.add_stop(t, color(hsv_to_rgb([t, 1.0, 1.0])));
                }
                l
            })),
        );
        let hx = hue.x + hsv[0] * hue.width;
        ring(renderer, hx, hue.y + hue.height * 0.5);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
    ) {
        let origin = layout.bounds().position();
        let square = self.square_rect(origin);
        let hue = self.hue_rect(origin);
        let pos = cursor.position();

        let emit = |drag: Drag, p: Point, shell: &mut Shell<Message>| {
            let mut hsv = rgb_to_hsv(self.rgb);
            match drag {
                Drag::Square => {
                    hsv[1] = ((p.x - square.x) / square.width).clamp(0.0, 1.0);
                    hsv[2] = 1.0 - ((p.y - square.y) / square.height).clamp(0.0, 1.0);
                }
                Drag::Hue => {
                    hsv[0] = ((p.x - hue.x) / hue.width).clamp(0.0, 1.0);
                }
                Drag::None => return,
            }
            shell.publish((self.on_change)(hsv_to_rgb(hsv)));
            shell.request_redraw();
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = pos else { return };
                if square.contains(p) {
                    self.state.downcast_mut::<State>().drag = Drag::Square;
                    emit(Drag::Square, p, shell);
                    shell.capture_event();
                } else if hue.contains(p) {
                    self.state.downcast_mut::<State>().drag = Drag::Hue;
                    emit(Drag::Hue, p, shell);
                    shell.capture_event();
                } else if !cursor.is_over(layout.bounds()) && !cursor.is_over(self.anchor) {
                    let st = self.state.downcast_mut::<State>();
                    st.open = false;
                    st.drag = Drag::None;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let drag = self.state.downcast_ref::<State>().drag;
                if drag != Drag::None
                    && let Some(p) = pos
                {
                    emit(drag, p, shell);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let st = self.state.downcast_mut::<State>();
                if st.drag != Drag::None {
                    st.drag = Drag::None;
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

fn ring(renderer: &mut Renderer, cx: f32, cy: f32) {
    use iced::advanced::Renderer as _;
    let r = 5.0;
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: cx - r,
                y: cy - r,
                width: r * 2.0,
                height: r * 2.0,
            },
            border: Border {
                color: Color::WHITE,
                width: 2.0,
                radius: r.into(),
            },
            ..Quad::default()
        },
        Background::Color(Color::TRANSPARENT),
    );
}

impl<'a, Message: Clone + 'a> From<ColorSwatch<Message>> for Element<'a, Message, Theme, Renderer> {
    fn from(w: ColorSwatch<Message>) -> Self {
        Self::new(w)
    }
}
