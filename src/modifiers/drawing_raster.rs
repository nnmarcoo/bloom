use crate::modifiers::cpu::{blend_over, f32_to_pixel, pixel_to_f32, smoothstep};
use crate::modifiers::kinds::{Drawing, Stroke};

pub const MAX_LAYER_SIDE: u32 = 8192;

pub fn layer_dims(full_w: u32, full_h: u32) -> (u32, u32) {
    let side = full_w.max(full_h).max(1) as f32;
    let scale = (MAX_LAYER_SIDE as f32 / side).min(1.0);
    (
        ((full_w as f32 * scale).round() as u32).max(1),
        ((full_h as f32 * scale).round() as u32).max(1),
    )
}

type Rect = [u32; 4];

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (r, None) | (None, r) => r,
    }
}

struct Brush {
    outer: f32,
    inner: f32,
    gain: f32,
    spacing: f32,
}

fn brush_for(stroke: &Stroke, scale: f32) -> Brush {
    let r_true = (stroke.size * 0.5 * scale).max(0.05);
    let radius = r_true.max(0.5);
    let outer = radius + 0.5;
    let inner = (radius * stroke.hardness.clamp(0.0, 1.0))
        .min(outer - 1.0)
        .max(0.0);
    let gain = (r_true / radius).clamp(0.35, 1.0);
    let spacing = (outer * 0.25).max(0.35);
    Brush {
        outer,
        inner,
        gain,
        spacing,
    }
}

#[derive(Clone, Copy)]
struct Walk {
    last: Option<[f32; 2]>,
    since: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct StrokeId {
    brush: u64,
    n_points: usize,
    points: u64,
}

impl StrokeId {
    fn of(s: &Stroke) -> Self {
        Self {
            brush: s.brush_sig(),
            n_points: s.points.len(),
            points: s.points_sig(s.points.len()),
        }
    }
}

struct ActiveStroke {
    id: StrokeId,
    color: [f32; 3],
    opacity: f32,
    walk: Walk,
    bbox: Option<Rect>,
}

pub struct DrawingLayerCache {
    pub w: u32,
    pub h: u32,
    sx: f32,
    sy: f32,
    base: Vec<u8>,
    mask: Vec<u8>,
    layer: Vec<u8>,
    completed: Vec<StrokeId>,
    active: Option<ActiveStroke>,
}

impl DrawingLayerCache {
    pub fn new(full_w: u32, full_h: u32) -> Self {
        let (w, h) = layer_dims(full_w, full_h);
        let n = w as usize * h as usize;
        Self {
            w,
            h,
            sx: w as f32 / full_w.max(1) as f32,
            sy: h as f32 / full_h.max(1) as f32,
            base: vec![0; n * 4],
            mask: vec![0; n],
            layer: vec![0; n * 4],
            completed: Vec::new(),
            active: None,
        }
    }

    pub fn layer_data(&self) -> &[u8] {
        &self.layer
    }

    pub fn view(&self) -> LayerView<'_> {
        LayerView {
            data: &self.layer,
            w: self.w,
            h: self.h,
            sx: self.sx,
            sy: self.sy,
        }
    }

    pub fn sync(&mut self, d: &Drawing) -> Option<Rect> {
        let n = d.strokes.len();
        let nb = self.completed.len();
        let prefix_ok = n >= nb
            && self
                .completed
                .iter()
                .zip(&d.strokes)
                .all(|(id, s)| *id == StrokeId::of(s));
        if !prefix_ok {
            return self.rebuild(d);
        }

        let mut dirty: Option<Rect> = None;
        loop {
            let idx = self.completed.len();
            if idx >= n {
                if self.active.is_some() {
                    return self.rebuild(d);
                }
                return dirty;
            }
            let s = &d.strokes[idx];
            let last = idx == n - 1;
            match &self.active {
                Some(act) => {
                    let same_brush = act.id.brush == s.brush_sig();
                    let extends = same_brush
                        && s.points.len() >= act.id.n_points
                        && s.points_sig(act.id.n_points) == act.id.points;
                    if !extends {
                        return self.rebuild(d);
                    }
                    if s.points.len() > act.id.n_points {
                        dirty = union(dirty, self.extend_active(s));
                    }
                    if last {
                        return dirty;
                    }
                    self.flatten();
                }
                None => {
                    dirty = union(dirty, self.start_active(s));
                    if last {
                        return dirty;
                    }
                    self.flatten();
                }
            }
        }
    }

    fn rebuild(&mut self, d: &Drawing) -> Option<Rect> {
        self.base.fill(0);
        self.mask.fill(0);
        self.layer.fill(0);
        self.completed.clear();
        self.active = None;
        let n = d.strokes.len();
        for (i, s) in d.strokes.iter().enumerate() {
            self.start_active(s);
            if i != n - 1 {
                self.flatten();
            }
        }
        Some([0, 0, self.w, self.h])
    }

    fn flatten(&mut self) {
        let Some(act) = self.active.take() else {
            return;
        };
        if let Some([x0, y0, x1, y1]) = act.bbox {
            for y in y0..y1 {
                let row = (y * self.w + x0) as usize;
                self.base[row * 4..(row + (x1 - x0) as usize) * 4]
                    .copy_from_slice(&self.layer[row * 4..(row + (x1 - x0) as usize) * 4]);
                self.mask[row..row + (x1 - x0) as usize].fill(0);
            }
        }
        self.completed.push(act.id);
    }

    fn start_active(&mut self, s: &Stroke) -> Option<Rect> {
        let mut act = ActiveStroke {
            id: StrokeId::of(s),
            color: s.color,
            opacity: s.opacity,
            walk: Walk {
                last: None,
                since: 0.0,
            },
            bbox: None,
        };
        let stamped = self.stamp_points(&mut act, s, 0);
        self.composite(&act, stamped);
        act.bbox = union(act.bbox, stamped);
        self.active = Some(act);
        stamped
    }

    fn extend_active(&mut self, s: &Stroke) -> Option<Rect> {
        let mut act = self.active.take().unwrap();
        let from = act.id.n_points;
        let stamped = self.stamp_points(&mut act, s, from);
        self.composite(&act, stamped);
        act.bbox = union(act.bbox, stamped);
        act.id.n_points = s.points.len();
        act.id.points = s.points_sig(s.points.len());
        self.active = Some(act);
        stamped
    }

    fn stamp_points(&mut self, act: &mut ActiveStroke, s: &Stroke, from: usize) -> Option<Rect> {
        let brush = brush_for(s, self.sx.min(self.sy));
        let mut stamped: Option<Rect> = None;
        for p in &s.points[from..] {
            let p = [p[0] * self.w as f32, p[1] * self.h as f32];
            match act.walk.last {
                None => {
                    stamped = union(stamped, self.stamp_dab(p, &brush));
                    act.walk.since = 0.0;
                }
                Some(a) => {
                    let dx = p[0] - a[0];
                    let dy = p[1] - a[1];
                    let len = (dx * dx + dy * dy).sqrt();
                    if len <= 0.0 {
                        continue;
                    }
                    let offset = brush.spacing - act.walk.since;
                    if offset <= len {
                        let mut t = offset;
                        let mut last_t = 0.0;
                        while t <= len {
                            let q = [a[0] + dx * t / len, a[1] + dy * t / len];
                            stamped = union(stamped, self.stamp_dab(q, &brush));
                            last_t = t;
                            t += brush.spacing;
                        }
                        act.walk.since = len - last_t;
                    } else {
                        act.walk.since += len;
                    }
                }
            }
            act.walk.last = Some(p);
        }
        stamped
    }

    fn stamp_dab(&mut self, c: [f32; 2], brush: &Brush) -> Option<Rect> {
        let r = brush.outer;
        let x0 = (c[0] - r).floor().max(0.0) as u32;
        let y0 = (c[1] - r).floor().max(0.0) as u32;
        let x1 = (((c[0] + r).ceil()).max(0.0) as u32).min(self.w);
        let y1 = (((c[1] + r).ceil()).max(0.0) as u32).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        for y in y0..y1 {
            let py = y as f32 + 0.5 - c[1];
            for x in x0..x1 {
                let px = x as f32 + 0.5 - c[0];
                let d = (px * px + py * py).sqrt();
                if d >= r {
                    continue;
                }
                let cov = (1.0 - smoothstep(brush.inner, r, d)) * brush.gain;
                let m = (cov * 255.0 + 0.5) as u8;
                let i = (y * self.w + x) as usize;
                self.mask[i] = self.mask[i].max(m);
            }
        }
        Some([x0, y0, x1, y1])
    }

    fn composite(&mut self, act: &ActiveStroke, rect: Option<Rect>) {
        let Some([x0, y0, x1, y1]) = rect else {
            return;
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * self.w + x) as usize;
                let o = i * 4;
                let a = self.mask[i] as f32 / 255.0 * act.opacity;
                if a <= 0.0 {
                    self.layer[o..o + 4].copy_from_slice(&self.base[o..o + 4]);
                } else {
                    let dst = pixel_to_f32(&self.base[o..o + 4]);
                    let src = [act.color[0], act.color[1], act.color[2], a];
                    self.layer[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct LayerView<'a> {
    data: &'a [u8],
    w: u32,
    h: u32,
    sx: f32,
    sy: f32,
}

impl LayerView<'_> {
    pub fn sample(&self, fx: f32, fy: f32) -> Option<[f32; 4]> {
        let gx = (fx * self.sx - 0.5).clamp(0.0, self.w as f32 - 1.0);
        let gy = (fy * self.sy - 0.5).clamp(0.0, self.h as f32 - 1.0);
        let x0 = gx.floor() as usize;
        let y0 = gy.floor() as usize;
        let x1 = (x0 + 1).min(self.w as usize - 1);
        let y1 = (y0 + 1).min(self.h as usize - 1);
        let tx = gx - x0 as f32;
        let ty = gy - y0 as f32;

        let w = self.w as usize;
        let at = |x: usize, y: usize, k: usize| self.data[(y * w + x) * 4 + k] as f32 / 255.0;
        let mut c = [0.0f32; 4];
        if tx == 0.0 && ty == 0.0 {
            for (k, v) in c.iter_mut().enumerate() {
                *v = at(x0, y0, k);
            }
        } else {
            for (k, v) in c.iter_mut().enumerate() {
                let top = at(x0, y0, k) * (1.0 - tx) + at(x1, y0, k) * tx;
                let bot = at(x0, y1, k) * (1.0 - tx) + at(x1, y1, k) * tx;
                *v = top * (1.0 - ty) + bot * ty;
            }
        }
        if c[3] <= 0.0 { None } else { Some(c) }
    }
}

pub struct DrawingRaster {
    data: Vec<u8>,
    w: u32,
    h: u32,
    sx: f32,
    sy: f32,
}

impl DrawingRaster {
    pub fn build(d: &Drawing, full_w: u32, full_h: u32) -> Option<Self> {
        if !d
            .strokes
            .iter()
            .any(|s| !s.points.is_empty() && s.opacity > 0.0)
        {
            return None;
        }
        let mut cache = DrawingLayerCache::new(full_w, full_h);
        cache.sync(d);
        Some(Self {
            data: cache.layer,
            w: cache.w,
            h: cache.h,
            sx: cache.sx,
            sy: cache.sy,
        })
    }

    pub fn view(&self) -> LayerView<'_> {
        LayerView {
            data: &self.data,
            w: self.w,
            h: self.h,
            sx: self.sx,
            sy: self.sy,
        }
    }
}

pub fn build_layers(
    modifiers: &[crate::modifiers::Modifier],
    full_w: u32,
    full_h: u32,
) -> Vec<Option<DrawingRaster>> {
    use crate::modifiers::ModifierKind;

    let needs = modifiers
        .iter()
        .any(|m| m.has_visible_effect() && matches!(m.kind, ModifierKind::Drawing(_)));
    if !needs {
        return Vec::new();
    }

    modifiers
        .iter()
        .map(|m| {
            if m.has_visible_effect()
                && let ModifierKind::Drawing(d) = &m.kind
            {
                DrawingRaster::build(d, full_w, full_h)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: Vec<[f32; 2]>, size: f32, opacity: f32, color: [f32; 3]) -> Stroke {
        Stroke {
            points,
            size,
            hardness: 0.7,
            opacity,
            color,
        }
    }

    #[test]
    fn single_dab_covers_center_not_corner() {
        let d = Drawing {
            strokes: vec![stroke(vec![[0.5, 0.5]], 20.0, 1.0, [1.0, 0.0, 0.0])],
            ..Drawing::default()
        };
        let r = DrawingRaster::build(&d, 100, 100).expect("raster");
        let center = r.view().sample(50.0, 50.0).expect("coverage at center");
        assert!(center[3] > 0.9);
        assert!(center[0] > 0.9 && center[1] < 0.1);
        assert!(r.view().sample(2.0, 2.0).is_none());
    }

    #[test]
    fn tiny_brush_line_has_no_gaps() {
        for (img_w, img_h, size) in [(200u32, 100u32, 1.0f32), (200, 100, 2.0), (400, 200, 4.0)] {
            let d = Drawing {
                strokes: vec![stroke(
                    vec![[0.1, 0.5], [0.9, 0.5]],
                    size,
                    1.0,
                    [1.0, 1.0, 1.0],
                )],
                ..Drawing::default()
            };
            let r = DrawingRaster::build(&d, img_w, img_h).expect("raster");
            let y = img_h as f32 * 0.5 + 0.5;
            let x0 = (img_w as f32 * 0.15) as u32;
            let x1 = (img_w as f32 * 0.85) as u32;
            for x in x0..x1 {
                let a = r
                    .view()
                    .sample(x as f32 + 0.5, y)
                    .map(|c| c[3])
                    .unwrap_or(0.0);
                assert!(
                    a > 0.25,
                    "gap at x={x} for size {size} on {img_w}x{img_h} (alpha {a})"
                );
            }
        }
    }

    #[test]
    fn tiny_brush_survives_layer_downscale() {
        let d = Drawing {
            strokes: vec![stroke(
                vec![[0.2, 0.5], [0.8, 0.5]],
                6.0,
                1.0,
                [1.0, 1.0, 1.0],
            )],
            ..Drawing::default()
        };
        let (img_w, img_h) = (40000u32, 20000u32);
        let r = DrawingRaster::build(&d, img_w, img_h).expect("raster");
        let y = img_h as f32 * 0.5;
        let step = (img_w as f32 * 0.6 / 200.0) as u32;
        for i in 0..200u32 {
            let x = (img_w as f32 * 0.2) as u32 + i * step;
            let a = r
                .view()
                .sample(x as f32 + 0.5, y + 0.5)
                .map(|c| c[3])
                .unwrap_or(0.0);
            assert!(a > 0.1, "gap at x={x} on downscaled layer (alpha {a})");
        }
    }

    #[test]
    fn opacity_scales_alpha() {
        let mk = |op: f32| Drawing {
            strokes: vec![stroke(vec![[0.5, 0.5]], 20.0, op, [1.0, 1.0, 1.0])],
            ..Drawing::default()
        };
        let full = DrawingRaster::build(&mk(1.0), 100, 100).unwrap();
        let half = DrawingRaster::build(&mk(0.5), 100, 100).unwrap();
        let fa = full.view().sample(50.0, 50.0).unwrap()[3];
        let ha = half.view().sample(50.0, 50.0).unwrap()[3];
        assert!(fa > 0.9);
        assert!((ha - fa * 0.5).abs() < 0.02);
    }

    #[test]
    fn incremental_sync_matches_fresh_build() {
        let pts_a: Vec<[f32; 2]> = (0..40)
            .map(|i| {
                let t = i as f32 / 39.0;
                [0.1 + 0.8 * t, 0.3 + 0.35 * (t * 9.0).sin() * 0.4]
            })
            .collect();
        let pts_b: Vec<[f32; 2]> = (0..25)
            .map(|i| {
                let t = i as f32 / 24.0;
                [0.8 - 0.6 * t, 0.8 - 0.5 * t]
            })
            .collect();

        let mut cache = DrawingLayerCache::new(300, 200);
        let mut d = Drawing {
            opacity: 0.6,
            size: 14.0,
            hardness: 0.5,
            color: [0.2, 0.9, 0.4],
            strokes: Vec::new(),
        };

        for (pts, color) in [(&pts_a, [0.2, 0.9, 0.4]), (&pts_b, [0.9, 0.3, 0.1])] {
            d.strokes.push(Stroke {
                points: vec![pts[0]],
                size: d.size,
                hardness: d.hardness,
                opacity: d.opacity,
                color,
            });
            cache.sync(&d);
            for p in &pts[1..] {
                d.strokes.last_mut().unwrap().points.push(*p);
                cache.sync(&d);
            }
        }

        let fresh = DrawingRaster::build(&d, 300, 200).expect("raster");
        assert_eq!(cache.layer_data(), &fresh.data[..]);
    }

    #[test]
    fn undo_falls_back_to_rebuild() {
        let mut cache = DrawingLayerCache::new(200, 200);
        let s1 = stroke(vec![[0.3, 0.3], [0.5, 0.5]], 16.0, 1.0, [1.0, 0.0, 0.0]);
        let s2 = stroke(vec![[0.7, 0.7], [0.6, 0.2]], 16.0, 0.5, [0.0, 0.0, 1.0]);
        let mut d = Drawing {
            strokes: vec![s1.clone()],
            ..Drawing::default()
        };
        cache.sync(&d);
        d.strokes.push(s2);
        cache.sync(&d);
        d.strokes.pop();
        cache.sync(&d);

        let fresh = DrawingRaster::build(&d, 200, 200).expect("raster");
        assert_eq!(cache.layer_data(), &fresh.data[..]);
    }
}
