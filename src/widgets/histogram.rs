use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Background, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

const DEFAULT_HEIGHT: f32 = 140.0;

pub struct Histogram {
    r: [u32; 256],
    g: [u32; 256],
    b: [u32; 256],
    height: f32,
    max: u32,
}

impl Histogram {
    pub fn new(r: [u32; 256], g: [u32; 256], b: [u32; 256]) -> Self {
        let max = r.iter().chain(g.iter()).chain(b.iter()).copied().max().unwrap_or(1);
        Self { r, g, b, height: DEFAULT_HEIGHT, max }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Histogram {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn state(&self) -> tree::State {
        tree::State::None
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
        layout::atomic(limits, Length::Fill, self.height)
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        _event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        // purely visual widget
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
        use advanced::Renderer as _;

        let bounds = layout.bounds();
        let max = self.max as f32;

        if max <= 0.0 {
            return;
        }

        let bin_width = bounds.width / 256.0;

        let palette = theme.extended_palette();
        let bg = palette.background.weak.color;

        // background
        renderer.fill_quad(
            Quad {
                bounds,
                ..Default::default()
            },
            Background::Color(bg),
        );

        for i in 0..256 {
            let x = bounds.x + i as f32 * bin_width;

            let r_h = (self.r[i] as f32 / max) * bounds.height;
            let g_h = (self.g[i] as f32 / max) * bounds.height;
            let b_h = (self.b[i] as f32 / max) * bounds.height;

            draw_bar(renderer, x, bounds.y + bounds.height - r_h, bin_width, r_h, Color::from_rgba(1.0, 0.0, 0.0, 0.5));
            draw_bar(renderer, x, bounds.y + bounds.height - g_h, bin_width, g_h, Color::from_rgba(0.0, 1.0, 0.0, 0.5));
            draw_bar(renderer, x, bounds.y + bounds.height - b_h, bin_width, b_h, Color::from_rgba(0.0, 0.4, 1.0, 0.5));
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

fn draw_bar(renderer: &mut Renderer, x: f32, y: f32, width: f32, height: f32, color: Color) {
    if height <= 0.0 {
        return;
    }

    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x,
                y,
                width,
                height,
            },
            ..Default::default()
        },
        Background::Color(color),
    );
}

impl<'a, Message> From<Histogram> for Element<'a, Message> {
    fn from(hist: Histogram) -> Self {
        Element::new(hist)
    }
}
