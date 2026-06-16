use glam::{Vec2, vec2};
use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::app::{EditMsg, Message};
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
    modifiers: keyboard::Modifiers,
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
    #[allow(clippy::too_many_arguments)]
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
        Self {
            program,
            modifier_idx,
            x,
            y,
            w,
            h,
            img_w,
            img_h,
        }
    }

    fn crop_screen(&self) -> Option<(Vec2, Vec2)> {
        let iw = self.img_w.max(1.0);
        let ih = self.img_h.max(1.0);
        let corners = [
            self.program
                .image_uv_to_screen(vec2(self.x / iw, self.y / ih))?,
            self.program
                .image_uv_to_screen(vec2((self.x + self.w) / iw, self.y / ih))?,
            self.program
                .image_uv_to_screen(vec2(self.x / iw, (self.y + self.h) / ih))?,
            self.program
                .image_uv_to_screen(vec2((self.x + self.w) / iw, (self.y + self.h) / ih))?,
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

    fn current_rect(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }

    fn drag_rect(&self, drag: &DragState, local: Vec2, center: bool, aspect: bool) -> [f32; 4] {
        let iw = self.img_w.max(1.0);
        let ih = self.img_h.max(1.0);
        let cur_uv = self
            .program
            .screen_to_image_uv(local)
            .unwrap_or(vec2((self.x + self.w) / iw, (self.y + self.h) / ih));
        Self::apply_drag(
            drag.handle,
            drag.start_rect,
            drag.start_cursor_uv,
            cur_uv,
            iw,
            ih,
            center,
            aspect,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_drag(
        handle: Handle,
        start_rect: [f32; 4],
        start_uv: Vec2,
        current_uv: Vec2,
        img_w: f32,
        img_h: f32,
        center: bool,
        aspect: bool,
    ) -> [f32; 4] {
        let img = vec2(img_w, img_h);
        let d = (current_uv - start_uv) * img;
        let [sx, sy, sw, sh] = start_rect;

        match handle {
            Handle::Inside => {
                return clamp_rect(sx + d.x, sy + d.y, sw, sh, img_w, img_h);
            }
            Handle::Outside => {
                let start_px = start_uv * img;
                let cur_px = current_uv * img;
                let nx = start_px.x.min(cur_px.x);
                let ny = start_px.y.min(cur_px.y);
                let nw = (cur_px.x - start_px.x).abs();
                let nh = (cur_px.y - start_px.y).abs();
                return clamp_rect(nx, ny, nw, nh, img_w, img_h);
            }
            _ => {}
        }

        let moves_l = matches!(handle, Handle::TL | Handle::BL | Handle::ML);
        let moves_r = matches!(handle, Handle::TR | Handle::BR | Handle::MR);
        let moves_t = matches!(handle, Handle::TL | Handle::TR | Handle::TC);
        let moves_b = matches!(handle, Handle::BL | Handle::BR | Handle::BC);
        let is_corner = (moves_l || moves_r) && (moves_t || moves_b);

        let cx = sx + sw * 0.5;
        let cy = sy + sh * 0.5;
        let ratio = (sw / sh).max(0.0001);

        let mut dw = if moves_l {
            -d.x
        } else if moves_r {
            d.x
        } else {
            0.0
        };
        let mut dh = if moves_t {
            -d.y
        } else if moves_b {
            d.y
        } else {
            0.0
        };
        if center {
            dw *= 2.0;
            dh *= 2.0;
        }

        let mut fw = sw + dw;
        let mut fh = sh + dh;

        if aspect {
            if is_corner {
                if fw >= fh * ratio {
                    fh = fw / ratio;
                } else {
                    fw = fh * ratio;
                }
            } else if moves_l || moves_r {
                fh = fw / ratio;
            } else {
                fw = fh * ratio;
            }
        }

        let span_x = 2.0 * cx.min(img_w - cx);
        let span_y = 2.0 * cy.min(img_h - cy);
        let max_w = if center || !(moves_l || moves_r) {
            span_x
        } else if moves_l {
            sx + sw
        } else {
            img_w - sx
        };
        let max_h = if center || !(moves_t || moves_b) {
            span_y
        } else if moves_t {
            sy + sh
        } else {
            img_h - sy
        };

        fw = fw.max(1.0);
        fh = fh.max(1.0);
        if aspect {
            let s = (max_w / fw).min(max_h / fh).min(1.0);
            fw *= s;
            fh *= s;
        } else {
            fw = fw.min(max_w);
            fh = fh.min(max_h);
        }

        let nx = if center || !(moves_l || moves_r) {
            cx - fw * 0.5
        } else if moves_l {
            (sx + sw) - fw
        } else {
            sx
        };
        let ny = if center || !(moves_t || moves_b) {
            cy - fh * 0.5
        } else if moves_t {
            (sy + sh) - fh
        } else {
            sy
        };

        clamp_rect(nx, ny, fw, fh, img_w, img_h)
    }
}

fn clamp_rect(nx: f32, ny: f32, nw: f32, nh: f32, img_w: f32, img_h: f32) -> [f32; 4] {
    const MIN: f32 = 1.0;
    let nw = nw.round().clamp(MIN, img_w);
    let nh = nh.round().clamp(MIN, img_h);
    let nx = nx.round().clamp(0.0, img_w - nw);
    let ny = ny.round().clamp(0.0, img_h - nh);
    let nw = nw.min(img_w - nx).max(MIN);
    let nh = nh.min(img_h - ny).max(MIN);
    [nx, ny, nw, nh]
}

fn fill(renderer: &mut Renderer, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x,
                y,
                width: w,
                height: h,
            },
            ..Default::default()
        },
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
            border: Border {
                radius: r.into(),
                ..Default::default()
            },
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
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
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
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let local = cursor.position_in(bounds).map(|p| vec2(p.x, p.y));

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
            if let (Some(drag), Some(local)) = (&state.drag, local) {
                let rect = self.drag_rect(drag, local, modifiers.control(), modifiers.shift());
                if rect != self.current_rect() {
                    let [nx, ny, nw, nh] = rect;
                    shell.publish(EditMsg::SetCropRect(self.modifier_idx, nx, ny, nw, nh).into());
                    shell.request_redraw();
                }
            }
            return;
        }

        let Event::Mouse(mouse_event) = event else {
            return;
        };

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
                    let rect = self.drag_rect(
                        drag,
                        local,
                        state.modifiers.control(),
                        state.modifiers.shift(),
                    );
                    if rect != self.current_rect() {
                        let [nx, ny, nw, nh] = rect;
                        shell.publish(
                            EditMsg::SetCropRect(self.modifier_idx, nx, ny, nw, nh).into(),
                        );
                        shell.request_redraw();
                    }
                    shell.capture_event();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if state.drag.take().is_some() => {
                shell.capture_event();
                shell.request_redraw();
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

            let dark = Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: OVERLAY_ALPHA,
            };

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
            fill(
                renderer,
                ax,
                ay + BORDER_W,
                BORDER_W,
                ah - 2.0 * BORDER_W,
                white,
            );
            fill(
                renderer,
                ax + aw - BORDER_W,
                ay + BORDER_W,
                BORDER_W,
                ah - 2.0 * BORDER_W,
                white,
            );

            let thirds = Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: THIRDS_ALPHA,
            };
            for i in 1..3i32 {
                let vx = ax + aw * i as f32 / 3.0;
                fill(
                    renderer,
                    vx,
                    ay + BORDER_W,
                    1.0,
                    ah - 2.0 * BORDER_W,
                    thirds,
                );
                let hy = ay + ah * i as f32 / 3.0;
                fill(
                    renderer,
                    ax + BORDER_W,
                    hy,
                    aw - 2.0 * BORDER_W,
                    1.0,
                    thirds,
                );
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
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if let Some(drag) = &state.drag {
            return match drag.handle {
                Handle::Inside => mouse::Interaction::Grabbing,
                _ => mouse::Interaction::Crosshair,
            };
        }
        let Some(local) = cursor.position_in(layout.bounds()).map(|p| vec2(p.x, p.y)) else {
            return mouse::Interaction::None;
        };
        match self.hit_handle(local) {
            Handle::Inside => mouse::Interaction::Grab,
            Handle::Outside => mouse::Interaction::None,
            _ => mouse::Interaction::Crosshair,
        }
    }
}

impl<'a> From<CropOverlay> for Element<'a, Message> {
    fn from(overlay: CropOverlay) -> Self {
        Element::new(overlay)
    }
}
