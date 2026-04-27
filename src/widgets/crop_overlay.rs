use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::app::Message;
use crate::wgpu::view_program::ViewProgram;

const OVERLAY_ALPHA: f32 = 0.55;
const BORDER_W: f32 = 1.5;
const THIRDS_ALPHA: f32 = 0.45;
const HANDLE_SIZE: f32 = 9.0;
const HANDLE_HIT: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    TL,
    TC,
    TR,
    ML,
    MR,
    BL,
    BC,
    BR,
    Inside,
    Outside,
}

struct DragState {
    handle: Handle,
    start_cursor_uv: Vec2,
    start_rect: [f32; 4],
}

#[derive(Default)]
struct State {
    drag: Option<DragState>,
}

pub struct CropOverlay {
    program: ViewProgram,
    modifier_idx: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    img_w: f32,
    img_h: f32,
}

impl CropOverlay {
    pub fn new(
        program: ViewProgram,
        modifier_idx: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        img_w: f32,
        img_h: f32,
    ) -> Self {
        Self { program, modifier_idx, x, y, w, h, img_w, img_h }
    }

    fn crop_screen(&self) -> Option<(Vec2, Vec2)> {
        let iw = self.img_w.max(1.0);
        let ih = self.img_h.max(1.0);
        let corners = [
            self.program.image_uv_to_screen(vec2(self.x / iw, self.y / ih))?,
            self.program.image_uv_to_screen(vec2((self.x + self.w) / iw, self.y / ih))?,
            self.program.image_uv_to_screen(vec2(self.x / iw, (self.y + self.h) / ih))?,
            self.program.image_uv_to_screen(vec2((self.x + self.w) / iw, (self.y + self.h) / ih))?,
        ];
        let min = corners.iter().copied().fold(corners[0], Vec2::min);
        let max = corners.iter().copied().fold(corners[0], Vec2::max);
        Some((min, max))
    }

    fn remap_handle(h: Handle, rotation: u8) -> Handle {
        match rotation % 4 {
            0 => h,
            1 => match h {
                Handle::TL => Handle::BL,
                Handle::TC => Handle::ML,
                Handle::TR => Handle::TL,
                Handle::ML => Handle::BC,
                Handle::MR => Handle::TC,
                Handle::BL => Handle::BR,
                Handle::BC => Handle::MR,
                Handle::BR => Handle::TR,
                other => other,
            },
            2 => match h {
                Handle::TL => Handle::BR,
                Handle::TC => Handle::BC,
                Handle::TR => Handle::BL,
                Handle::ML => Handle::MR,
                Handle::MR => Handle::ML,
                Handle::BL => Handle::TR,
                Handle::BC => Handle::TC,
                Handle::BR => Handle::TL,
                other => other,
            },
            3 => match h {
                Handle::TL => Handle::TR,
                Handle::TC => Handle::MR,
                Handle::TR => Handle::BR,
                Handle::ML => Handle::TC,
                Handle::MR => Handle::BC,
                Handle::BL => Handle::TL,
                Handle::BC => Handle::ML,
                Handle::BR => Handle::BL,
                other => other,
            },
            _ => h,
        }
    }

    fn hit_handle(&self, local: Vec2) -> Handle {
        let Some((tl, br)) = self.crop_screen() else {
            return Handle::Outside;
        };
        let mx = (tl.x + br.x) * 0.5;
        let my = (tl.y + br.y) * 0.5;

        let candidates = [
            (Handle::TL, tl.x, tl.y),
            (Handle::TC, mx, tl.y),
            (Handle::TR, br.x, tl.y),
            (Handle::ML, tl.x, my),
            (Handle::MR, br.x, my),
            (Handle::BL, tl.x, br.y),
            (Handle::BC, mx, br.y),
            (Handle::BR, br.x, br.y),
        ];
        for (h, hx, hy) in candidates {
            if (local - vec2(hx, hy)).length() <= HANDLE_HIT {
                return Self::remap_handle(h, self.program.rotation());
            }
        }
        if local.x >= tl.x && local.x <= br.x && local.y >= tl.y && local.y <= br.y {
            return Handle::Inside;
        }
        Handle::Outside
    }

    fn apply_drag(
        handle: Handle,
        start_rect: [f32; 4],
        start_uv: Vec2,
        current_uv: Vec2,
        img_w: f32,
        img_h: f32,
    ) -> [f32; 4] {
        let img = vec2(img_w, img_h);
        let d = (current_uv - start_uv) * img;
        let [sx, sy, sw, sh] = start_rect;
        let (mut nx, mut ny, mut nw, mut nh) = (sx, sy, sw, sh);

        match handle {
            Handle::Inside => {
                nx += d.x;
                ny += d.y;
            }
            Handle::TL => {
                nx += d.x;
                ny += d.y;
                nw -= d.x;
                nh -= d.y;
            }
            Handle::TC => {
                ny += d.y;
                nh -= d.y;
            }
            Handle::TR => {
                nw += d.x;
                ny += d.y;
                nh -= d.y;
            }
            Handle::ML => {
                nx += d.x;
                nw -= d.x;
            }
            Handle::MR => nw += d.x,
            Handle::BL => {
                nx += d.x;
                nw -= d.x;
                nh += d.y;
            }
            Handle::BC => nh += d.y,
            Handle::BR => {
                nw += d.x;
                nh += d.y;
            }
            Handle::Outside => {
                let start_px = start_uv * img;
                let cur_px = current_uv * img;
                nx = start_px.x.min(cur_px.x);
                ny = start_px.y.min(cur_px.y);
                nw = (cur_px.x - start_px.x).abs();
                nh = (cur_px.y - start_px.y).abs();
            }
        }

        const MIN: f32 = 1.0;
        nw = nw.clamp(MIN, img_w);
        nh = nh.clamp(MIN, img_h);
        nx = nx.clamp(0.0, img_w - nw);
        ny = ny.clamp(0.0, img_h - nh);
        nw = nw.min(img_w - nx).max(MIN);
        nh = nh.min(img_h - ny).max(MIN);
        [nx.round(), ny.round(), nw.round(), nh.round()]
    }
}

fn fill(renderer: &mut Renderer, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    renderer.fill_quad(
        Quad { bounds: Rectangle { x, y, width: w, height: h }, ..Default::default() },
        Background::Color(color),
    );
}

fn fill_circle(renderer: &mut Renderer, cx: f32, cy: f32, r: f32, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: cx - r,
                y: cy - r,
                width: r * 2.0,
                height: r * 2.0,
            },
            border: Border { radius: r.into(), ..Default::default() },
            ..Default::default()
        },
        Background::Color(color),
    );
}

impl Widget<Message, Theme, Renderer> for CropOverlay {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Fill, height: Length::Fill }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
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
        let Event::Mouse(mouse_event) = event else {
            return;
        };
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let local = cursor.position_in(bounds).map(|p| vec2(p.x, p.y));

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(local) = local else { return };
                let handle = self.hit_handle(local);
                let iw = self.img_w.max(1.0);
                let ih = self.img_h.max(1.0);
                let start_uv = self
                    .program
                    .screen_to_image_uv(local)
                    .unwrap_or(vec2(self.x / iw, self.y / ih));
                state.drag = Some(DragState {
                    handle,
                    start_cursor_uv: start_uv,
                    start_rect: [self.x, self.y, self.w, self.h],
                });
                shell.capture_event();
                shell.request_redraw();
            }
            mouse::Event::CursorMoved { .. } => {
                if let Some(drag) = &state.drag {
                    let Some(local) = local else { return };
                    let iw = self.img_w.max(1.0);
                    let ih = self.img_h.max(1.0);
                    let cur_uv = self
                        .program
                        .screen_to_image_uv(local)
                        .unwrap_or(vec2((self.x + self.w) / iw, (self.y + self.h) / ih));
                    let [nx, ny, nw, nh] = Self::apply_drag(
                        drag.handle,
                        drag.start_rect,
                        drag.start_cursor_uv,
                        cur_uv,
                        iw,
                        ih,
                    );
                    shell.publish(Message::SetCropRect(self.modifier_idx, nx, ny, nw, nh));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if state.drag.take().is_some() {
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let Some((tl, br)) = self.crop_screen() else {
            return;
        };

        let widget_bounds = layout.bounds();
        renderer.with_layer(widget_bounds, |renderer| {
            let bx = widget_bounds.x;
            let by = widget_bounds.y;
            let bw = widget_bounds.width;
            let bh = widget_bounds.height;

            let cx0 = tl.x.clamp(0.0, bw);
            let cy0 = tl.y.clamp(0.0, bh);
            let cx1 = br.x.clamp(0.0, bw);
            let cy1 = br.y.clamp(0.0, bh);
            let strip_h = cy1 - cy0;

            let dark = Color { r: 0.0, g: 0.0, b: 0.0, a: OVERLAY_ALPHA };

            fill(renderer, bx, by, bw, cy0, dark);
            fill(renderer, bx, by + cy1, bw, bh - cy1, dark);
            fill(renderer, bx, by + cy0, cx0, strip_h, dark);
            fill(renderer, bx + cx1, by + cy0, bw - cx1, strip_h, dark);

            let ax = bx + tl.x;
            let ay = by + tl.y;
            let aw = br.x - tl.x;
            let ah = br.y - tl.y;

            let white = Color::WHITE;

            fill(renderer, ax, ay, aw, BORDER_W, white);
            fill(renderer, ax, ay + ah - BORDER_W, aw, BORDER_W, white);
            fill(renderer, ax, ay + BORDER_W, BORDER_W, ah - 2.0 * BORDER_W, white);
            fill(renderer, ax + aw - BORDER_W, ay + BORDER_W, BORDER_W, ah - 2.0 * BORDER_W, white);

            let thirds = Color { r: 1.0, g: 1.0, b: 1.0, a: THIRDS_ALPHA };
            for i in 1..3i32 {
                let vx = ax + aw * i as f32 / 3.0;
                fill(renderer, vx, ay + BORDER_W, 1.0, ah - 2.0 * BORDER_W, thirds);
                let hy = ay + ah * i as f32 / 3.0;
                fill(renderer, ax + BORDER_W, hy, aw - 2.0 * BORDER_W, 1.0, thirds);
            }

            let state = tree.state.downcast_ref::<State>();
            let show_handles =
                state.drag.is_some() || cursor.position_in(layout.bounds()).is_some();

            if show_handles {
                let mx = tl.x + aw * 0.5;
                let my = tl.y + ah * 0.5;
                for (lx, ly) in [
                    (tl.x, tl.y),
                    (mx, tl.y),
                    (br.x, tl.y),
                    (tl.x, my),
                    (br.x, my),
                    (tl.x, br.y),
                    (mx, br.y),
                    (br.x, br.y),
                ] {
                    fill_circle(renderer, bx + lx, by + ly, HANDLE_SIZE / 2.0, white);
                }
            }
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        match &state.drag {
            Some(drag) => match drag.handle {
                Handle::Inside => mouse::Interaction::Grabbing,
                _ => mouse::Interaction::Crosshair,
            },
            None => mouse::Interaction::None,
        }
    }
}

impl<'a> From<CropOverlay> for Element<'a, Message> {
    fn from(overlay: CropOverlay) -> Self {
        Element::new(overlay)
    }
}
