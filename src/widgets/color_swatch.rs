use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Overlay, Shell, Widget};
use iced::keyboard::{self, key::Named};
use iced::mouse;
use iced::overlay;
use iced::widget::{column, container, row, text, text_input};
use iced::{
    Background, Border, Color, Element, Event, Gradient, Length, Point, Rectangle, Renderer, Size,
    Theme, Vector, gradient::Linear,
};

use crate::styles::radius;

const SWATCH_W: f32 = 70.0;
const SWATCH_H: f32 = 22.0;

const PAD: f32 = 10.0;
const SQUARE: f32 = 160.0;
const HUE_H: f32 = 12.0;
const PAD_GAP: f32 = 8.0;
const HEX_W: f32 = 62.0;
const FIELD_SIZE: f32 = 12.0;

const POPUP_W: f32 = PAD + SQUARE + PAD;

fn inner_radius() -> f32 {
    if radius() > 0.0 { 4.0 } else { 0.0 }
}

const FIELDS: [Field; 7] = [
    Field::Hex,
    Field::R,
    Field::G,
    Field::B,
    Field::H,
    Field::S,
    Field::V,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Hex,
    R,
    G,
    B,
    H,
    S,
    V,
}

impl Field {
    fn index(self) -> usize {
        self as usize
    }

    fn label(self) -> &'static str {
        ["#", "R", "G", "B", "H", "S", "V"][self as usize]
    }
}

#[derive(Clone)]
enum Op {
    SetRgb([f32; 3]),
    Edit(Field, String),
    Sync,
}

#[derive(Default)]
struct State {
    open: bool,
    buffers: [String; 7],
}

impl State {
    fn sync(&mut self, rgb: [f32; 3]) {
        self.sync_except(rgb, None);
    }

    fn sync_except(&mut self, rgb: [f32; 3], skip: Option<Field>) {
        let hsv = rgb_to_hsv(rgb);
        let to255 = |c: f32| ((c * 255.0).round() as i32).to_string();
        let pct = |c: f32| ((c * 100.0).round() as i32).to_string();
        let values = [
            hex_string(rgb),
            to255(rgb[0]),
            to255(rgb[1]),
            to255(rgb[2]),
            ((hsv[0] * 360.0).round() as i32).to_string(),
            pct(hsv[1]),
            pct(hsv[2]),
        ];
        for (f, value) in FIELDS.into_iter().zip(values) {
            if Some(f) != skip {
                self.buffers[f.index()] = value;
            }
        }
    }
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

fn hex_string(rgb: [f32; 3]) -> String {
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "{:02X}{:02X}{:02X}",
        to_u8(rgb[0]),
        to_u8(rgb[1]),
        to_u8(rgb[2])
    )
}

fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let s = s.trim().trim_start_matches('#');
    let s = match s.len() {
        3 => s.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => s.to_string(),
        _ => return None,
    };
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
}

fn apply_field(field: Field, buffer: &str, rgb: [f32; 3]) -> Option<[f32; 3]> {
    if field == Field::Hex {
        return parse_hex(buffer);
    }
    let n: f32 = buffer.trim().parse().ok()?;
    let mut out = rgb;
    let mut hsv = rgb_to_hsv(rgb);
    match field {
        Field::R => out[0] = (n / 255.0).clamp(0.0, 1.0),
        Field::G => out[1] = (n / 255.0).clamp(0.0, 1.0),
        Field::B => out[2] = (n / 255.0).clamp(0.0, 1.0),
        Field::H => {
            hsv[0] = (n / 360.0).rem_euclid(1.0);
            out = hsv_to_rgb(hsv);
        }
        Field::S => {
            hsv[1] = (n / 100.0).clamp(0.0, 1.0);
            out = hsv_to_rgb(hsv);
        }
        Field::V => {
            hsv[2] = (n / 100.0).clamp(0.0, 1.0);
            out = hsv_to_rgb(hsv);
        }
        Field::Hex => unreachable!(),
    }
    Some(out)
}

fn field_filter(field: Field, s: String) -> String {
    let max = if field == Field::Hex { 6 } else { 3 };
    s.chars()
        .filter(|c| {
            if field == Field::Hex {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            }
        })
        .take(max)
        .collect()
}

fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let text_color = palette.background.base.text;
    let border_color = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.strong.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: inner_radius().into(),
        },
        icon: text_color,
        placeholder: text_color.scale_alpha(0.4),
        value: text_color,
        selection: palette.primary.base.color.scale_alpha(0.35),
    }
}

fn build_content<'a>(rgb: [f32; 3], buffers: &[String; 7]) -> Element<'a, Op, Theme, Renderer> {
    let field = |f: Field, width: Length| -> Element<'a, Op, Theme, Renderer> {
        let input = text_input("", &buffers[f.index()])
            .on_input(move |s| Op::Edit(f, field_filter(f, s)))
            .on_submit(Op::Sync)
            .size(FIELD_SIZE)
            .padding([2, 5])
            .width(width)
            .style(input_style);
        row![
            text(f.label())
                .size(FIELD_SIZE)
                .width(Length::Fixed(10.0))
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.extended_palette().background.strong.text),
                }),
            input,
        ]
        .spacing(3)
        .align_y(iced::alignment::Vertical::Center)
        .into()
    };

    let preview = container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(SWATCH_H))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(color(rgb))),
            border: Border {
                color: theme.extended_palette().background.strong.color,
                width: 1.0,
                radius: inner_radius().into(),
            },
            ..container::Style::default()
        });

    column![
        SvHuePad::new(rgb),
        row![preview, field(Field::Hex, Length::Fixed(HEX_W))]
            .spacing(PAD_GAP)
            .align_y(iced::alignment::Vertical::Center),
        row![
            field(Field::R, Length::Fill),
            field(Field::G, Length::Fill),
            field(Field::B, Length::Fill),
        ]
        .spacing(PAD_GAP),
        row![
            field(Field::H, Length::Fill),
            field(Field::S, Length::Fill),
            field(Field::V, Length::Fill),
        ]
        .spacing(PAD_GAP),
    ]
    .spacing(PAD_GAP)
    .padding(PAD)
    .width(Length::Fixed(POPUP_W))
    .into()
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for ColorSwatch<Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let mut state = State::default();
        state.sync(self.rgb);
        tree::State::new(state)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(build_content(
            self.rgb,
            &<[String; 7]>::default(),
        ))]
    }

    fn diff(&self, tree: &mut Tree) {
        let buffers = tree.state.downcast_ref::<State>().buffers.clone();
        tree.diff_children(&[build_content(self.rgb, &buffers)]);
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
            if state.open {
                state.sync(self.rgb);
            }
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
        let state = tree.state.downcast_ref::<State>();
        if !state.open {
            return None;
        }
        let content = build_content(self.rgb, &state.buffers);
        let position = layout.position() + translation;
        let bounds = layout.bounds();
        Some(overlay::Element::new(Box::new(PickerOverlay {
            content,
            content_tree: &mut tree.children[0],
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

struct PickerOverlay<'a, 'b, Message> {
    content: Element<'a, Op, Theme, Renderer>,
    content_tree: &'b mut Tree,
    state: &'b mut tree::State,
    rgb: [f32; 3],
    on_change: &'b dyn Fn([f32; 3]) -> Message,
    anchor: Rectangle,
}

impl<Message: Clone> Overlay<Message, Theme, Renderer> for PickerOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.content.as_widget_mut().layout(
            self.content_tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = node.bounds().size();
        let x = self
            .anchor
            .x
            .clamp(0.0, (bounds.width - size.width).max(0.0));
        let y = (self.anchor.y + self.anchor.height + 4.0)
            .clamp(0.0, (bounds.height - size.height).max(0.0));
        node.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer as _;
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
        let viewport = layout.bounds();
        self.content.as_widget().draw(
            self.content_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &viewport,
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            self.state.downcast_mut::<State>().open = false;
            shell.request_redraw();
            return;
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            ..
        }) = event
        {
            self.state.downcast_mut::<State>().open = false;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        let mut ops: Vec<Op> = Vec::new();
        let mut local = Shell::new(&mut ops);
        let viewport = layout.bounds();
        self.content.as_widget_mut().update(
            self.content_tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            &viewport,
        );
        let captured = local.is_event_captured();
        if local.is_layout_invalid() {
            shell.invalidate_layout();
        }
        if local.are_widgets_invalid() {
            shell.invalidate_widgets();
        }
        shell.request_redraw_at(local.redraw_request());

        let mut rgb = self.rgb;
        let mut changed = false;
        let state = self.state.downcast_mut::<State>();
        for op in ops {
            match op {
                Op::SetRgb(new) => {
                    rgb = new;
                    changed = true;
                    state.sync(rgb);
                }
                Op::Edit(field, buffer) => {
                    if let Some(new) = apply_field(field, &buffer, rgb) {
                        rgb = new;
                        changed = true;
                        state.sync_except(rgb, Some(field));
                    }
                    state.buffers[field.index()] = buffer;
                }
                Op::Sync => state.sync(rgb),
            }
        }

        if changed {
            shell.publish((self.on_change)(rgb));
            shell.request_redraw();
        }
        if captured {
            shell.capture_event();
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(self.content_tree, layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        self.content.as_widget().mouse_interaction(
            self.content_tree,
            layout,
            cursor,
            &viewport,
            renderer,
        )
    }
}

struct SvHuePad {
    rgb: [f32; 3],
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Drag {
    #[default]
    None,
    Square,
    Hue,
}

impl SvHuePad {
    fn new(rgb: [f32; 3]) -> Self {
        Self { rgb }
    }

    fn square(bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: SQUARE,
        }
    }

    fn hue(bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x,
            y: bounds.y + SQUARE + PAD_GAP,
            width: bounds.width,
            height: HUE_H,
        }
    }
}

impl Widget<Op, Theme, Renderer> for SvHuePad {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Drag>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Drag::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(SQUARE + PAD_GAP + HUE_H),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        layout::Node::new(Size::new(width, SQUARE + PAD_GAP + HUE_H))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Op>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let square = Self::square(bounds);
        let hue = Self::hue(bounds);
        let drag = tree.state.downcast_mut::<Drag>();

        let emit = |drag: Drag, p: Point, shell: &mut Shell<Op>| {
            let mut hsv = rgb_to_hsv(self.rgb);
            match drag {
                Drag::Square => {
                    hsv[1] = ((p.x - square.x) / square.width).clamp(0.0, 1.0);
                    hsv[2] = 1.0 - ((p.y - square.y) / square.height).clamp(0.0, 1.0);
                }
                Drag::Hue => hsv[0] = ((p.x - hue.x) / hue.width).clamp(0.0, 1.0),
                Drag::None => return,
            }
            shell.publish(Op::SetRgb(hsv_to_rgb(hsv)));
            shell.request_redraw();
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position() else { return };
                if square.contains(p) {
                    *drag = Drag::Square;
                    emit(Drag::Square, p, shell);
                    shell.capture_event();
                } else if hue.contains(p) {
                    *drag = Drag::Hue;
                    emit(Drag::Hue, p, shell);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let d = *drag;
                if d != Drag::None
                    && let Some(p) = cursor.position()
                {
                    emit(d, p, shell);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if *drag != Drag::None =>
            {
                *drag = Drag::None;
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let square = Self::square(bounds);
        let hue = Self::hue(bounds);
        let hsv = rgb_to_hsv(self.rgb);
        let hue_color = color(hsv_to_rgb([hsv[0], 1.0, 1.0]));

        renderer.fill_quad(
            Quad {
                bounds: square,
                border: Border {
                    radius: inner_radius().into(),
                    ..Border::default()
                },
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
                border: Border {
                    radius: inner_radius().into(),
                    ..Border::default()
                },
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
                border: Border {
                    radius: inner_radius().into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Gradient(Gradient::Linear({
                let mut grad = Linear::new(std::f32::consts::FRAC_PI_2);
                for i in 0..=6 {
                    let t = i as f32 / 6.0;
                    grad = grad.add_stop(t, color(hsv_to_rgb([t, 1.0, 1.0])));
                }
                grad
            })),
        );
        let hx = hue.x + hsv[0] * hue.width;
        ring(renderer, hx, hue.y + hue.height * 0.5);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        if let Some(p) = cursor.position()
            && (Self::square(bounds).contains(p) || Self::hue(bounds).contains(p))
        {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a> From<SvHuePad> for Element<'a, Op, Theme, Renderer> {
    fn from(w: SvHuePad) -> Self {
        Self::new(w)
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
