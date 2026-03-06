use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{
    Background, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer, Size,
    Theme,
};

use crate::styles::radius;

const DEFAULT_HEIGHT: f32 = 140.0;
const LABEL_HEIGHT: f32 = 18.0;
const LABEL_PAD: f32 = 4.0;
const CHIP_GAP: f32 = 2.0;
const TEXT_SIZE: f32 = 10.0;
const BAR_ALPHA: f32 = 0.55;
const LABELS: [&str; 4] = ["R", "G", "B", "L"];

struct ChannelPair {
    dark: Color,
    light: Color,
}

const CHANNEL_COLORS: [ChannelPair; 4] = [
    ChannelPair {
        dark: Color {
            r: 1.0,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        },
        light: Color {
            r: 0.85,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    },
    ChannelPair {
        dark: Color {
            r: 0.2,
            g: 0.9,
            b: 0.2,
            a: 1.0,
        },
        light: Color {
            r: 0.0,
            g: 0.6,
            b: 0.0,
            a: 1.0,
        },
    },
    ChannelPair {
        dark: Color {
            r: 0.3,
            g: 0.5,
            b: 1.0,
            a: 1.0,
        },
        light: Color {
            r: 0.1,
            g: 0.3,
            b: 0.9,
            a: 1.0,
        },
    },
    ChannelPair {
        dark: Color {
            r: 0.85,
            g: 0.85,
            b: 0.85,
            a: 1.0,
        },
        light: Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 1.0,
        },
    },
];

#[derive(Debug, Clone)]
struct State {
    channels: [bool; 4],
}

impl Default for State {
    fn default() -> Self {
        Self {
            channels: [true, true, true, true],
        }
    }
}

pub struct Histogram {
    data: [[u32; 256]; 4],
    height: f32,
    max: u32,
}

impl Histogram {
    pub fn new(r: [u32; 256], g: [u32; 256], b: [u32; 256]) -> Self {
        let mut l = [0u32; 256];
        for i in 0..256 {
            l[i] = (0.299 * r[i] as f64 + 0.587 * g[i] as f64 + 0.114 * b[i] as f64) as u32;
        }
        let max = r
            .iter()
            .chain(g.iter())
            .chain(b.iter())
            .copied()
            .max()
            .unwrap_or(1);
        Self {
            data: [r, g, b, l],
            height: DEFAULT_HEIGHT,
            max,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    fn label_rects(bounds: &Rectangle) -> [Rectangle; 4] {
        let chip_w = (bounds.width / 4.0).floor();
        std::array::from_fn(|i| Rectangle {
            x: bounds.x + i as f32 * chip_w,
            y: bounds.y,
            width: chip_w,
            height: LABEL_HEIGHT,
        })
    }

    fn bar_area(bounds: &Rectangle) -> Rectangle {
        Rectangle {
            y: bounds.y + LABEL_HEIGHT + LABEL_PAD,
            height: bounds.height - LABEL_HEIGHT - LABEL_PAD,
            ..*bounds
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Histogram {
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
        layout::atomic(limits, Length::Fill, self.height)
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position() {
                let bounds = layout.bounds();
                for (i, rect) in Self::label_rects(&bounds).iter().enumerate() {
                    if rect.contains(pos) {
                        tree.state.downcast_mut::<State>().channels[i] ^= true;
                        shell.request_redraw();
                        return;
                    }
                }
            }
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
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let bar_area = Self::bar_area(&bounds);
        let label_rects = Self::label_rects(&bounds);

        let palette = theme.extended_palette();
        let bg = palette.background.weak.color;
        let is_dark = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b < 0.5;

        renderer.fill_quad(
            Quad {
                bounds: bar_area,
                ..Default::default()
            },
            Background::Color(bg),
        );

        let max = self.max as f32;
        if max > 0.0 {
            let bin_width = bar_area.width / 256.0;
            for ch in 0..4 {
                if !state.channels[ch] {
                    continue;
                }
                let mut c = if is_dark {
                    CHANNEL_COLORS[ch].dark
                } else {
                    CHANNEL_COLORS[ch].light
                };
                c.a = BAR_ALPHA;
                for i in 0..256usize {
                    let h = self.data[ch][i] as f32 / max * bar_area.height;
                    if h <= 0.0 {
                        continue;
                    }
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x: bar_area.x + i as f32 * bin_width,
                                y: bar_area.y + bar_area.height - h,
                                width: bin_width,
                                height: h,
                            },
                            ..Default::default()
                        },
                        Background::Color(c),
                    );
                }
            }
        }

        let chip_bg_active = palette.background.base.color;
        let chip_border = palette.background.strong.color;
        let r = radius();

        for (i, rect) in label_rects.iter().enumerate() {
            let active = state.channels[i];
            let chip = Rectangle {
                x: rect.x + CHIP_GAP * 0.5,
                y: rect.y,
                width: rect.width - CHIP_GAP,
                height: rect.height,
            };
            let channel_color = if is_dark {
                CHANNEL_COLORS[i].dark
            } else {
                CHANNEL_COLORS[i].light
            };

            renderer.fill_quad(
                Quad {
                    bounds: chip,
                    border: iced::Border {
                        color: if active { channel_color } else { chip_border },
                        width: 1.0,
                        radius: r.into(),
                    },
                    ..Default::default()
                },
                Background::Color(if active { chip_bg_active } else { bg }),
            );

            renderer.fill_text(
                Text {
                    content: LABELS[i].to_string(),
                    bounds: Size::new(chip.width, chip.height),
                    size: Pixels(TEXT_SIZE),
                    line_height: text::LineHeight::default(),
                    font: Font::MONOSPACE,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center.into(),
                    shaping: text::Shaping::Basic,
                    wrapping: text::Wrapping::None,
                },
                Point::new(chip.center_x(), chip.center_y()),
                if active {
                    channel_color
                } else {
                    palette.background.base.text.scale_alpha(0.4)
                },
                chip,
            );
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
        if let Some(pos) = cursor.position() {
            let bounds = layout.bounds();
            if Self::label_rects(&bounds).iter().any(|r| r.contains(pos)) {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}

impl<'a, Message> From<Histogram> for Element<'a, Message> {
    fn from(hist: Histogram) -> Self {
        Element::new(hist)
    }
}
