use rayon::prelude::*;

use crate::modifiers::drawing_raster::LayerView;
use crate::modifiers::kinds::ResizeFilter;
use crate::modifiers::plan::{ImageSpec, PlanItem, infer_specs, plan_modifiers};
use crate::modifiers::roi::{StepClass, step_class};
use crate::modifiers::text_raster::TextRaster;
use crate::modifiers::{Modifier, ModifierKind, motion_blur_samples};

/// The rows of a stage's input needed to produce rows `y0..y1` of its output.
///
/// Vertical reach only -- horizontal reach never forces extra *rows*, so a band
/// is always full width. Returns `None` when the stage needs its entire input,
/// which is the honest answer for anything that reads across rows arbitrarily.
fn rows_needed(class: StepClass, y0: u32, y1: u32, in_h: u32, scale_num: u32, scale_den: u32) -> Option<(u32, u32)> {
    // Map the output span back through any geometry change first: a resize
    // means output row y came from input row y * in_h / out_h.
    let map = |y: u32| -> u32 {
        if scale_den == 0 {
            return y;
        }
        ((y as u64 * scale_num as u64) / scale_den as u64) as u32
    };
    let (my0, my1) = (map(y0), map(y1).min(in_h));

    match class {
        StepClass::Pointwise => Some((my0, my1)),
        StepClass::Kernel { apron_px, .. } => {
            let a = apron_px.ceil().max(0.0) as u32;
            Some((my0.saturating_sub(a), (my1 + a).min(in_h)))
        }
        // A row-major scanline reads whole rows but no *extra* rows, so it
        // bands cleanly. Anything else (columns, diagonals, whole frame) needs
        // everything.
        StepClass::Scanline { dir: (_, 0) } => Some((my0, my1)),
        StepClass::Scanline { .. } | StepClass::WholeFrame => None,
    }
}

/// The source rows a chain needs to produce output rows `y0..y1`, or `None` if
/// some stage forces the whole frame.
///
/// This is the CPU analogue of the backward ROI walk the GPU executor performs
/// in `execute_kernel_chain`, restricted to the vertical axis. Sharing the walk
/// exactly is the eventual goal; sharing `StepClass` is what keeps the two from
/// disagreeing about reach in the meantime.
pub(crate) fn source_rows_for_band(
    plan: &[PlanItem],
    specs: &[crate::modifiers::plan::StageSpec],
    y0: u32,
    y1: u32,
) -> Option<(u32, u32)> {
    let (mut lo, mut hi) = (y0, y1);
    for (item, spec) in plan.iter().zip(specs).rev() {
        let class = match item {
            PlanItem::Fused(_) => StepClass::Pointwise,
            PlanItem::Step(_, m) => step_class(&m.kind),
        };
        let (in_h, out_h) = (spec.input.h, spec.output.h);
        let (n_lo, n_hi) = rows_needed(class, lo, hi, in_h, in_h, out_h)?;
        lo = n_lo;
        hi = n_hi.max(n_lo);
    }
    Some((lo, hi))
}

/// Total vertical reach of a chain, in source rows.
///
/// The sum rather than the max: aprons stack, because each stage's apron is
/// applied to a region that the next stage's apron already widened. Callers use
/// this to size a band so the redundant work stays a bounded fraction.
pub(crate) fn chain_apron_rows(plan: &[PlanItem]) -> u32 {
    plan.iter()
        .map(|item| match item {
            PlanItem::Fused(_) => 0,
            PlanItem::Step(_, m) => match step_class(&m.kind) {
                StepClass::Kernel { apron_px, .. } => apron_px.ceil().max(0.0) as u32,
                _ => 0,
            },
        })
        .sum()
}

/// True when every stage in the plan can be rendered band-by-band.
///
/// A chain containing a column sort, a diagonal sort, or a full-frame stage
/// cannot be banded, and the caller must fall back to `render_full`.
pub(crate) fn plan_is_bandable(plan: &[PlanItem]) -> bool {
    plan.iter().all(|item| match item {
        PlanItem::Fused(_) => true,
        PlanItem::Step(_, m) => !matches!(
            step_class(&m.kind),
            StepClass::WholeFrame | StepClass::Scanline { dir: (_, 1..) } | StepClass::Scanline { dir: (_, i32::MIN..=-1) }
        ),
    })
}

/// Renders output rows `y0..y1` of the chain, reading only the source rows
/// those output rows depend on.
///
/// Returns the band's pixels, tightly packed at the chain's output width. The
/// caller is responsible for having checked [`plan_is_bandable`]; a chain with
/// a column sort or a full-frame stage will produce wrong pixels here because
/// the band does not contain the input those stages read.
///
/// Peak memory is proportional to the band height, not the image height, which
/// is the entire point: a 50000x50000 export runs in the same working set as a
/// 4K one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_band(
    modifiers: &[Modifier],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    y0: u32,
    y1: u32,
) -> Vec<u8> {
    let plan = plan_modifiers(modifiers);
    let specs = infer_specs(ImageSpec::new(img_w, img_h), &plan);
    let out_spec = specs.last().map_or(ImageSpec::new(img_w, img_h), |s| s.output);
    let row_bytes = out_spec.w as usize * 4;

    let Some((src_lo, src_hi)) = source_rows_for_band(&plan, &specs, y0, y1) else {
        // Not bandable: the caller should not have reached here.
        debug_assert!(false, "render_band called on a chain that cannot be banded");
        return vec![0u8; row_bytes * (y1 - y0) as usize];
    };
    let src_hi = src_hi.min(img_h).max(src_lo);
    let band_h = src_hi - src_lo;
    if band_h == 0 {
        return vec![0u8; row_bytes * (y1 - y0) as usize];
    }

    // Copy just the source rows this band needs.
    let stride = img_w as usize * 4;
    let start = src_lo as usize * stride;
    let end = (src_hi as usize * stride).min(pixels.len());
    let mut cur = vec![0u8; band_h as usize * stride];
    if start < end {
        cur[..end - start].copy_from_slice(&pixels[start..end]);
    }

    // Run the chain over the band. Stage geometry is the band's height rather
    // than the image's; `y_off` tracks where the band sits so stages that care
    // about absolute position (text, drawing, vignette) stay correct.
    let mut cur_h = band_h;
    let mut cur_w = img_w;
    let mut y_off = src_lo;
    for (item, spec) in plan.iter().zip(&specs) {
        let class = match item {
            PlanItem::Fused(_) => StepClass::Pointwise,
            PlanItem::Step(_, m) => step_class(&m.kind),
        };
        cur = apply_stage_banded(
            item,
            spec,
            class,
            cur,
            cur_w,
            cur_h,
            y_off,
            spec.input.h,
            text_layers,
            drawing_layers,
        );
        // A resize changes both dimensions and rescales the band's offset.
        if spec.input != spec.output {
            let num = spec.output.h as u64;
            let den = spec.input.h.max(1) as u64;
            y_off = ((y_off as u64 * num) / den) as u32;
            cur_h = (cur.len() / (spec.output.w as usize * 4)) as u32;
            cur_w = spec.output.w;
        }
    }

    // Slice the requested rows out of the rendered band.
    let mut out = vec![0u8; row_bytes * (y1 - y0) as usize];
    for (i, dst) in out.chunks_mut(row_bytes).enumerate() {
        let want = y0 + i as u32;
        if want < y_off {
            continue;
        }
        let local = (want - y_off) as usize;
        let s = local * row_bytes;
        if s + row_bytes <= cur.len() {
            dst.copy_from_slice(&cur[s..s + row_bytes]);
        }
    }
    out
}

pub(crate) fn render_full(
    modifiers: &[Modifier],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
) -> Vec<u8> {
    let n = img_w as usize * img_h as usize * 4;
    let mut cur = vec![0u8; n];
    let copy = n.min(pixels.len());
    cur[..copy].copy_from_slice(&pixels[..copy]);

    // Walk the shared plan rather than re-deriving the segmentation here: the
    // GPU pipeline consumes the same plan, so the two backends cannot drift
    // apart in how they group modifiers.
    let plan = plan_modifiers(modifiers);
    let specs = infer_specs(ImageSpec::new(img_w, img_h), &plan);

    // Geometry is a running value rather than a loop constant, so a stage that
    // changes dimensions only has to declare it in `infer_specs`. Every stage is
    // passthrough today; the debug assert below pins that.
    for (item, spec) in plan.iter().zip(&specs) {
        let ImageSpec { w: img_w, h: img_h } = spec.input;
        let w = img_w as usize;
        let h = img_h as usize;
        debug_assert_eq!(
            cur.len(),
            w * h * 4,
            "buffer does not match the stage's declared input spec"
        );

        match item {
            PlanItem::Fused(segment) => {
                apply_pointwise_segment(&mut cur, img_w, img_h, segment);
            }
            // `i` indexes the original stack, which is what the positionally
            // stored text and drawing rasters are keyed by.
            PlanItem::Step(i, m) => match &m.kind {
                ModifierKind::GaussianBlur(gb) => blur_full(&mut cur, w, h, gb.radius),
                ModifierKind::ChromaticAberration(ca) => {
                    cur = chromatic_aberration_full(&cur, img_w, img_h, ca.amount);
                }
                ModifierKind::MotionBlur(mb) => {
                    cur = motion_blur_full(&cur, img_w, img_h, mb.angle, mb.distance);
                }
                ModifierKind::Text(_) => {
                    if let Some(Some(raster)) = text_layers.get(*i) {
                        text_full(&mut cur, img_w, img_h, raster);
                    }
                }
                ModifierKind::Drawing(_) => {
                    if let Some(Some(raster)) = drawing_layers.get(*i) {
                        drawing_full(&mut cur, img_w, raster);
                    }
                }
                ModifierKind::PixelSort(ps) => {
                    cur = crate::modifiers::pixel_sort::pixel_sort_cpu(
                        &cur,
                        w,
                        h,
                        ps.threshold,
                        ps.angle,
                    );
                }
                // The only stage whose output geometry differs from its input.
                // `spec.output` is authoritative -- it is what every later
                // stage was sized against by `infer_specs`.
                ModifierKind::Resize(r) => {
                    let out = spec.output;
                    cur = resample(&cur, img_w, img_h, out.w, out.h, r.filter);
                }
                // `plan_modifiers` only emits a `Step` for modifiers the
                // planner classifies as non-pointwise, and
                // `planner_classification_covers_every_modifier_type` pins that
                // set to exactly the arms above. A kind arriving here means a
                // new non-pointwise modifier was added without a CPU
                // implementation, which would otherwise render as a silent
                // no-op.
                other => debug_assert!(
                    false,
                    "{} is planned as a standalone step but has no CPU implementation",
                    other.name()
                ),
            },
        }

        debug_assert_eq!(
            cur.len(),
            spec.output.w as usize * spec.output.h as usize * 4,
            "stage output does not match its declared output spec"
        );
    }
    cur
}

/// Runs one plan item over a band.
///
/// `y_off` is the band's first row in full-image coordinates and `full_h` the
/// full image height, because stages that evaluate a UV -- vignette, grain,
/// text, drawing -- must see their true position in the frame. Getting this
/// wrong produces a band that renders correctly in isolation and wrongly in
/// context, which is exactly the failure a naive banding would introduce.
#[allow(clippy::too_many_arguments)]
fn apply_stage_banded(
    item: &PlanItem,
    spec: &crate::modifiers::plan::StageSpec,
    class: StepClass,
    mut cur: Vec<u8>,
    w: u32,
    h: u32,
    y_off: u32,
    full_h: u32,
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
) -> Vec<u8> {
    let _ = class;
    let (wu, hu) = (w as usize, h as usize);
    match item {
        PlanItem::Fused(segment) => {
            apply_pointwise_band(&mut cur, w, full_h, y_off, segment);
        }
        PlanItem::Step(i, m) => match &m.kind {
            ModifierKind::GaussianBlur(gb) => blur_full(&mut cur, wu, hu, gb.radius),
            ModifierKind::ChromaticAberration(ca) => {
                cur = chromatic_aberration_full(&cur, w, h, ca.amount);
            }
            ModifierKind::MotionBlur(mb) => {
                cur = motion_blur_full(&cur, w, h, mb.angle, mb.distance);
            }
            ModifierKind::Text(_) => {
                if let Some(Some(raster)) = text_layers.get(*i) {
                    text_band(&mut cur, w, h, y_off, raster);
                }
            }
            ModifierKind::Drawing(_) => {
                if let Some(Some(raster)) = drawing_layers.get(*i) {
                    drawing_band(&mut cur, w, h, y_off, raster);
                }
            }
            ModifierKind::PixelSort(ps) => {
                cur = crate::modifiers::pixel_sort::pixel_sort_cpu(
                    &cur,
                    wu,
                    hu,
                    ps.threshold,
                    ps.angle,
                );
            }
            ModifierKind::Resize(r) => {
                // Scale the band by the same ratio the full image would use, so
                // adjacent bands tile without seams or drift.
                let num = spec.output.h as u64;
                let den = spec.input.h.max(1) as u64;
                let band_out_h = (((h as u64 * num) / den) as u32).max(1);
                cur = resample(&cur, w, h, spec.output.w, band_out_h, r.filter);
            }
            other => debug_assert!(
                false,
                "{} is planned as a standalone step but has no CPU implementation",
                other.name()
            ),
        },
    }
    cur
}

fn apply_pointwise_band(
    buf: &mut [u8],
    img_w: u32,
    full_h: u32,
    y_off: u32,
    segment: &[&Modifier],
) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let v = (y_off as f32 + y as f32 + 0.5) / full_h as f32;
        for x in 0..w {
            let o = x * 4;
            let u = (x as f32 + 0.5) / img_w as f32;
            let mut c = pixel_to_f32(&row[o..o + 4]);
            for m in segment {
                c = m.kind.apply_cpu(img_w, full_h, [u, v], c);
            }
            row[o..o + 4].copy_from_slice(&f32_to_pixel(c.map(|v| v.clamp(0.0, 1.0))));
        }
    });
}

fn apply_pointwise_segment(buf: &mut [u8], img_w: u32, img_h: u32, segment: &[&Modifier]) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let v = (y as f32 + 0.5) / img_h as f32;
        for x in 0..w {
            let o = x * 4;
            let u = (x as f32 + 0.5) / img_w as f32;
            let mut c = pixel_to_f32(&row[o..o + 4]);
            for m in segment {
                c = m.kind.apply_cpu(img_w, img_h, [u, v], c);
            }
            row[o..o + 4].copy_from_slice(&f32_to_pixel(c.map(|v| v.clamp(0.0, 1.0))));
        }
    });
}

/// Largest kernel radius, in pixels, evaluated directly.
///
/// Matches `MAX_KERNEL_RADIUS_PX` in the GPU executor, which caps its kernel
/// the same way. Keeping the two equal is what keeps preview and export
/// agreeing on large blurs -- `golden_blur_banded_tall_image` compares them at
/// radius 100 and catches any drift.
const MAX_DIRECT_RADIUS: f32 = 128.0;

/// Gaussian blur, evaluated at a resolution suited to its radius.
///
/// A direct convolution is O(radius) per pixel per axis, so a 500px blur on a
/// 50000x50000 image is ~5 trillion multiply-adds -- minutes of work. Blurring
/// at reduced scale removes almost all of it.
///
/// This is not an approximation of the filter. A Gaussian of sigma `s`
/// downsampled by `k` is exactly a Gaussian of sigma `s * k`, so blurring a
/// half-size image with half the radius yields the same Gaussian, sampled more
/// coarsely. The detail lost to the downsample is detail the blur itself was
/// about to remove; what changes is the sampling grid, not the filter. This is
/// the same trade the GPU path already makes, and the same one Photoshop and
/// Affinity make to keep large blurs interactive.
fn blur_full(buf: &mut [u8], w: usize, h: usize, radius: f32) {
    if radius <= 0.0 || w == 0 || h == 0 {
        return;
    }

    // Power-of-two scales only, mirroring the GPU's `ks`: they keep the
    // resample cheap and the radius exactly representable at every level.
    let ks = (MAX_DIRECT_RADIUS / radius).min(1.0).log2().floor().exp2();
    if ks >= 1.0 {
        blur_direct(buf, w, h, radius);
        return;
    }

    let sw = ((w as f32 * ks).round() as usize).max(1);
    let sh = ((h as f32 * ks).round() as usize).max(1);

    // Down, blur at the matching radius, back up. Lanczos on the way down to
    // avoid aliasing the detail that survives; bilinear on the way back up
    // because the result is already smooth and Lanczos would only ring.
    let small = resample(buf, w as u32, h as u32, sw as u32, sh as u32, ResizeFilter::Lanczos);
    let mut small = small;
    blur_direct(&mut small, sw, sh, radius * ks);
    let up = resample(
        &small,
        sw as u32,
        sh as u32,
        w as u32,
        h as u32,
        ResizeFilter::Bilinear,
    );
    buf.copy_from_slice(&up);
}

fn blur_direct(buf: &mut [u8], w: usize, h: usize, radius: f32) {
    let r = radius.ceil() as i32;
    if r <= 0 || w == 0 || h == 0 {
        return;
    }

    let sigma = (radius / 3.0).max(0.5);
    let inv = 1.0 / (2.0 * sigma * sigma);
    let kernel: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 * inv).exp()).collect();
    let wsum: f32 = kernel.iter().sum();
    let norm: Vec<f32> = kernel.iter().map(|k| k / wsum).collect();

    // The horizontal pass is row-local, so it runs in place against a one-row
    // copy per thread. A full-size scratch here doubles the working set for no
    // benefit -- at a 500px apron on a 50000px-wide band that is hundreds of MB.
    //
    // Shaped like the vertical pass below: accumulate the whole row once per
    // tap so the inner loop is a contiguous scaled add over `w * 4` floats.
    // Interior pixels (those at least `r` from either edge) need no clamping,
    // so that span is split out and runs branch-free -- the clamp in the inner
    // loop was what stopped this vectorizing.
    let stride_h = w * 4;
    buf.par_chunks_mut(stride_h).for_each_init(
        || (vec![0u8; stride_h], vec![0.0f32; stride_h]),
        |(row_copy, acc), row| {
            row_copy.copy_from_slice(row);
            acc.iter_mut().for_each(|v| *v = 0.0);

            let ru = r as usize;
            for (ki, &k) in norm.iter().enumerate() {
                // Offset of this tap in pixels: negative reads to the left.
                let off = ki as isize - r as isize;
                let (lo, hi) = (ru.min(w), w.saturating_sub(ru));

                // Left edge: source index clamps to 0.
                for x in 0..lo.min(w) {
                    let sx = (x as isize + off).clamp(0, w as isize - 1) as usize;
                    for c in 0..4 {
                        acc[x * 4 + c] += row_copy[sx * 4 + c] as f32 * k;
                    }
                }
                // Interior: no clamping, so both slices are contiguous and the
                // compiler can use wide loads and FMA.
                if hi > lo {
                    let src_start = (lo as isize + off) as usize * 4;
                    let dst = &mut acc[lo * 4..hi * 4];
                    let src = &row_copy[src_start..src_start + (hi - lo) * 4];
                    for (a, &p) in dst.iter_mut().zip(src.iter()) {
                        *a += p as f32 * k;
                    }
                }
                // Right edge: source index clamps to w - 1.
                for x in hi.max(lo)..w {
                    let sx = (x as isize + off).clamp(0, w as isize - 1) as usize;
                    for c in 0..4 {
                        acc[x * 4 + c] += row_copy[sx * 4 + c] as f32 * k;
                    }
                }
            }

            for (o, &a) in row.iter_mut().zip(acc.iter()) {
                *o = (a + 0.5).clamp(0.0, 255.0) as u8;
            }
        },
    );

    // The vertical pass reads across rows, so it does need a copy of the
    // horizontally-blurred intermediate.
    //
    // Accumulate whole rows rather than walking a column per output pixel: for
    // each tap, the source row and the accumulator are both contiguous, so the
    // inner loop is a simple scaled add that the compiler vectorizes. The
    // previous form strided by `w * 4` bytes per tap, which defeated both the
    // vectorizer and the cache.
    let scratch = buf.to_vec();
    let stride = w * 4;
    buf.par_chunks_mut(stride)
        .enumerate()
        .for_each_init(
            || vec![0.0f32; stride],
            |acc_row, (y, out_row)| {
                acc_row.iter_mut().for_each(|v| *v = 0.0);
                for (ki, &k) in norm.iter().enumerate() {
                    let sy = (y as i32 - r + ki as i32).clamp(0, h as i32 - 1) as usize;
                    let src_row = &scratch[sy * stride..sy * stride + stride];
                    for (a, &p) in acc_row.iter_mut().zip(src_row.iter()) {
                        *a += p as f32 * k;
                    }
                }
                for (o, &a) in out_row.iter_mut().zip(acc_row.iter()) {
                    *o = (a + 0.5).clamp(0.0, 255.0) as u8;
                }
            },
        );
}

fn chromatic_aberration_full(src: &[u8], img_w: u32, img_h: u32, amount: f32) -> Vec<u8> {
    let w = img_w as usize;
    let scale = amount / img_w as f32;
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let v = (y as f32 + 0.5) / img_h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / img_w as f32;
            let r_uv = [
                (u + (u - 0.5) * scale).clamp(0.0, 1.0),
                (v + (v - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let b_uv = [
                (u - (u - 0.5) * scale).clamp(0.0, 1.0),
                (v - (v - 0.5) * scale).clamp(0.0, 1.0),
            ];
            let cr = sample_bilinear(
                src,
                img_w,
                img_h,
                r_uv[0] * img_w as f32,
                r_uv[1] * img_h as f32,
            );
            let cg = sample_pixel(src, img_w, img_h, u, v);
            let cb = sample_bilinear(
                src,
                img_w,
                img_h,
                b_uv[0] * img_w as f32,
                b_uv[1] * img_h as f32,
            );
            let o = x * 4;
            row[o..o + 4].copy_from_slice(&f32_to_pixel([cr[0], cg[1], cb[2], cg[3]]));
        }
    });
    out
}

fn motion_blur_full(src: &[u8], img_w: u32, img_h: u32, angle: f32, distance: f32) -> Vec<u8> {
    let w = img_w as usize;
    let rad = angle.to_radians();
    let du = rad.cos() * distance;
    let dv = rad.sin() * distance;
    let n = motion_blur_samples(distance) as i32;
    let mut out = vec![0u8; src.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let cy = y as f32 + 0.5;
        for x in 0..w {
            let cx = x as f32 + 0.5;
            let mut acc = [0.0f32; 4];
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32 - 0.5;
                let s = sample_bilinear(src, img_w, img_h, cx + du * t, cy + dv * t);
                acc[0] += s[0];
                acc[1] += s[1];
                acc[2] += s[2];
                acc[3] += s[3];
            }
            let inv = 1.0 / n as f32;
            let o = x * 4;
            row[o..o + 4].copy_from_slice(&f32_to_pixel([
                acc[0] * inv,
                acc[1] * inv,
                acc[2] * inv,
                acc[3] * inv,
            ]));
        }
    });
    out
}

fn sample_bilinear(pixels: &[u8], w: u32, h: u32, fx: f32, fy: f32) -> [f32; 4] {
    let px = fx - 0.5;
    let py = fy - 0.5;
    let x0 = px.floor();
    let y0 = py.floor();
    let tx = px - x0;
    let ty = py - y0;
    let load = |x: f32, y: f32| -> [f32; 4] {
        let cx = (x.max(0.0) as u32).min(w - 1);
        let cy = (y.max(0.0) as u32).min(h - 1);
        let base = (cy as usize * w as usize + cx as usize) * 4;
        match pixels.get(base..base + 4) {
            Some(p) => [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
                p[3] as f32 / 255.0,
            ],
            None => [0.0; 4],
        }
    };
    let c00 = load(x0, y0);
    let c10 = load(x0 + 1.0, y0);
    let c01 = load(x0, y0 + 1.0);
    let c11 = load(x0 + 1.0, y0 + 1.0);
    let mut o = [0.0f32; 4];
    for i in 0..4 {
        let top = c00[i] + (c10[i] - c00[i]) * tx;
        let bot = c01[i] + (c11[i] - c01[i]) * tx;
        o[i] = top + (bot - top) * ty;
    }
    o
}

fn drawing_full(buf: &mut [u8], img_w: u32, raster: &LayerView<'_>) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = y as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

/// Band variants: the raster is sampled in absolute image coordinates, so the
/// only difference from the full-frame versions is offsetting the row index.
fn drawing_band(buf: &mut [u8], img_w: u32, _h: u32, y_off: u32, raster: &LayerView<'_>) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = (y_off + y as u32) as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

fn text_band(buf: &mut [u8], img_w: u32, _h: u32, y_off: u32, raster: &TextRaster) {
    let w = img_w as usize;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = (y_off + y as u32) as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

fn text_full(buf: &mut [u8], img_w: u32, img_h: u32, raster: &TextRaster) {
    let w = img_w as usize;
    let _ = img_h;
    buf.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = y as f32 + 0.5;
        for x in 0..w {
            if let Some(src) = raster.sample(x as f32 + 0.5, fy) {
                let o = x * 4;
                let dst = pixel_to_f32(&row[o..o + 4]);
                row[o..o + 4].copy_from_slice(&f32_to_pixel(blend_over(dst, src)));
            }
        }
    });
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn blend_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let sa = src[3];
    let da = dst[3];
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let blend = |s: f32, d: f32| (s * sa + d * da * (1.0 - sa)) / out_a;
    [
        blend(src[0], dst[0]),
        blend(src[1], dst[1]),
        blend(src[2], dst[2]),
        out_a,
    ]
}

pub(crate) fn pixel_to_f32(p: &[u8]) -> [f32; 4] {
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}

fn lanczos3(x: f32) -> f32 {
    const A: f32 = 3.0;
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= A {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    (px.sin() / px) * ((px / A).sin() / (px / A))
}

/// Resamples `src` to `dst_w` x `dst_h`.
///
/// Separable: one horizontal pass into a scratch buffer, then one vertical
/// pass. The filter radius is scaled by the *reciprocal* of the scale factor
/// when minifying, which is what makes a downscale average its input rather
/// than point-sample it -- a fixed-radius kernel would alias badly at large
/// reductions.
///
/// `Nearest` deliberately ignores that: its blockiness is the reason to pick
/// it, so it stays a true point sample in both directions.
pub(crate) fn resample(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    filter: ResizeFilter,
) -> Vec<u8> {
    if (src_w, src_h) == (dst_w, dst_h) {
        return src.to_vec();
    }

    // Per-axis kernel: `support` is the half-width in *source* pixels, and
    // `weight` is evaluated in kernel space.
    let axis = |dst: u32, src: u32| -> (f32, f32) {
        let scale = dst as f32 / src as f32;
        // Minifying (scale < 1) widens the footprint in source space.
        let inv = if scale < 1.0 { 1.0 / scale } else { 1.0 };
        (scale, inv)
    };
    let radius = |f: ResizeFilter| -> f32 {
        match f {
            ResizeFilter::Nearest => 0.0,
            ResizeFilter::Bilinear => 1.0,
            ResizeFilter::Lanczos => 3.0,
        }
    };
    let weight = |f: ResizeFilter, x: f32| -> f32 {
        match f {
            ResizeFilter::Nearest => 1.0,
            ResizeFilter::Bilinear => (1.0 - x.abs()).max(0.0),
            ResizeFilter::Lanczos => lanczos3(x),
        }
    };

    // One entry per output position: which source samples it reads, and with
    // what weights. Building this once per axis rather than per output pixel is
    // the difference between calling `weight` O(out_w * out_h * taps) times and
    // O(out_len * taps) -- for a Lanczos downscale that removes billions of
    // sin() calls, which dominated the resample.
    struct Tap {
        start: u32,
        weights: Vec<f32>,
        norm: f32,
    }

    let build_taps = |out_len: u32, src_len: u32| -> Vec<Tap> {
        let (scale, inv) = axis(out_len, src_len);
        let r = radius(filter) * inv;
        (0..out_len)
            .map(|o| {
                let center = (o as f32 + 0.5) / scale;
                if matches!(filter, ResizeFilter::Nearest) {
                    let s = (center.floor().max(0.0) as u32).min(src_len - 1);
                    return Tap {
                        start: s,
                        weights: vec![1.0],
                        norm: 1.0,
                    };
                }
                let lo = (center - r).floor().max(0.0) as u32;
                let hi = ((center + r).ceil() as u32).min(src_len).max(lo + 1);
                let weights: Vec<f32> = (lo..hi)
                    .map(|s| weight(filter, (s as f32 + 0.5 - center) / inv))
                    .collect();
                // A Lanczos kernel has negative lobes, so normalise by the
                // actual weight sum rather than assuming it is 1.0.
                let sum: f32 = weights.iter().sum();
                Tap {
                    start: lo,
                    weights,
                    norm: if sum.abs() < 1e-6 { 1.0 } else { sum },
                }
            })
            .collect()
    };

    let one_axis = |input: &[u8], in_w: u32, in_h: u32, out_len: u32, horizontal: bool| -> Vec<u8> {
        let (out_w, out_h) = if horizontal {
            (out_len, in_h)
        } else {
            (in_w, out_len)
        };
        let src_len = if horizontal { in_w } else { in_h };
        let taps = build_taps(out_len, src_len);
        let in_stride = in_w as usize * 4;

        let mut out = vec![0u8; out_w as usize * out_h as usize * 4];
        if horizontal {
            // Each output row reads only its own input row, so both are
            // contiguous and the tap table is shared across rows.
            out.par_chunks_mut(out_w as usize * 4)
                .enumerate()
                .for_each(|(row, out_row)| {
                    let in_row = &input[row * in_stride..(row + 1) * in_stride];
                    for (col, tap) in taps.iter().enumerate() {
                        let mut acc = [0.0f32; 4];
                        for (i, &wt) in tap.weights.iter().enumerate() {
                            let base = (tap.start as usize + i).min(src_len as usize - 1) * 4;
                            for c in 0..4 {
                                acc[c] += in_row[base + c] as f32 * wt;
                            }
                        }
                        for c in 0..4 {
                            out_row[col * 4 + c] =
                                (acc[c] / tap.norm).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                });
        } else {
            // Each output row combines a fixed set of input rows with fixed
            // weights, so accumulate row-at-a-time: the inner loop then walks
            // two contiguous rows, which vectorizes far better than striding
            // down a column per output pixel.
            let row_floats = out_w as usize * 4;
            out.par_chunks_mut(row_floats)
                .zip(taps.par_iter())
                .for_each_init(
                    || vec![0.0f32; row_floats],
                    |acc_row, (out_row, tap)| {
                        acc_row.iter_mut().for_each(|v| *v = 0.0);
                        for (i, &wt) in tap.weights.iter().enumerate() {
                            let sy = (tap.start as usize + i).min(src_len as usize - 1);
                            let in_row = &input[sy * in_stride..sy * in_stride + row_floats];
                            for (a, &p) in acc_row.iter_mut().zip(in_row.iter()) {
                                *a += p as f32 * wt;
                            }
                        }
                        for (o, &a) in out_row.iter_mut().zip(acc_row.iter()) {
                            *o = (a / tap.norm).round().clamp(0.0, 255.0) as u8;
                        }
                    },
                );
        }
        out
    };

    let mid = one_axis(src, src_w, src_h, dst_w, true);
    one_axis(&mid, dst_w, src_h, dst_h, false)
}

pub(crate) fn f32_to_pixel(c: [f32; 4]) -> [u8; 4] {
    [
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        (c[3] * 255.0).round() as u8,
    ]
}

pub(crate) fn sample_pixel(pixels: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
    let x = (u * w as f32).clamp(0.0, w as f32 - 1.0) as usize;
    let y = (v * h as f32).clamp(0.0, h as f32 - 1.0) as usize;
    let base = (y * w as usize + x) * 4;
    match pixels.get(base..base + 4) {
        Some(p) => [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            p[3] as f32 / 255.0,
        ],
        None => [0.0; 4],
    }
}

fn hash_u(v: u32) -> u32 {
    let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let s = ((s >> ((s >> 28).wrapping_add(4))) ^ s).wrapping_mul(277803737);
    (s >> 22) ^ s
}

pub(crate) fn hash21(ix: i32, iy: i32, seed: i32) -> f32 {
    let h = hash_u(
        (ix as u32) ^ (iy as u32).wrapping_mul(1664525) ^ (seed as u32).wrapping_mul(22695477),
    );
    h as f32 / 4294967295.0
}

pub(crate) fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let max_c = rgb[0].max(rgb[1]).max(rgb[2]);
    let min_c = rgb[0].min(rgb[1]).min(rgb[2]);
    let l = (max_c + min_c) * 0.5;
    if max_c == min_c {
        return [0.0, 0.0, l];
    }
    let d = max_c - min_c;
    let s = if l < 0.5 {
        d / (max_c + min_c)
    } else {
        d / (2.0 - max_c - min_c)
    };
    let h = if max_c == rgb[0] {
        (rgb[1] - rgb[2]) / d + if rgb[1] >= rgb[2] { 0.0 } else { 6.0 }
    } else if max_c == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.0
    } else {
        (rgb[0] - rgb[1]) / d + 4.0
    };
    [h / 6.0, s, l]
}

pub(crate) fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    if hsl[1] == 0.0 {
        return [hsl[2]; 3];
    }
    let q = if hsl[2] < 0.5 {
        hsl[2] * (1.0 + hsl[1])
    } else {
        hsl[2] + hsl[1] - hsl[2] * hsl[1]
    };
    let p = 2.0 * hsl[2] - q;
    [
        hue_to_rgb(p, q, hsl[0] + 1.0 / 3.0),
        hue_to_rgb(p, q, hsl[0]),
        hue_to_rgb(p, q, hsl[0] - 1.0 / 3.0),
    ]
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    let mut t = t_in;
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

#[cfg(test)]
mod pointwise_tests {
    use crate::modifiers::kinds::{Duotone, Grayscale, Invert, Sepia, Solarize, Temperature};
    use crate::modifiers::{Modifier, ModifierKind};

    fn apply(kind: ModifierKind, c: [f32; 4]) -> [f32; 4] {
        Modifier::new(kind).kind.apply_cpu(1, 1, [0.5, 0.5], c)
    }

    #[test]
    fn invert_flips_channels_and_scales_by_amount() {
        let full = apply(
            ModifierKind::Invert(Invert { amount: 1.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        assert!((full[0] - 0.8).abs() < 1e-5);
        assert!((full[1] - 0.4).abs() < 1e-5);
        assert!((full[2] - 0.1).abs() < 1e-5);
        assert_eq!(full[3], 1.0, "alpha untouched");

        let half = apply(
            ModifierKind::Invert(Invert { amount: 0.5 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        assert!(
            (half[0] - 0.5).abs() < 1e-5,
            "amount 0.5 is halfway to inverse"
        );

        let none = apply(
            ModifierKind::Invert(Invert { amount: 0.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        assert!((none[0] - 0.2).abs() < 1e-5, "amount 0 is identity");
    }

    #[test]
    fn grayscale_collapses_to_luma() {
        let g = apply(
            ModifierKind::Grayscale(Grayscale { amount: 1.0 }),
            [0.2, 0.6, 0.9, 1.0],
        );
        let luma = 0.2 * 0.2126 + 0.6 * 0.7152 + 0.9 * 0.0722;
        assert!((g[0] - luma).abs() < 1e-5);
        assert!(
            (g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6,
            "all channels equal"
        );
        assert_eq!(g[3], 1.0);
    }

    #[test]
    fn temperature_warms_red_cools_blue() {
        let t = apply(
            ModifierKind::Temperature(Temperature {
                temperature: 0.1,
                tint: 0.05,
            }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!((t[0] - 0.6).abs() < 1e-5, "temp raises red");
        assert!((t[1] - 0.55).abs() < 1e-5, "tint raises green");
        assert!((t[2] - 0.4).abs() < 1e-5, "temp lowers blue");
    }

    #[test]
    fn sepia_tints_toward_warm_and_desaturates() {
        let s = apply(
            ModifierKind::Sepia(Sepia { intensity: 1.0 }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!(s[0] > s[1] && s[1] > s[2], "sepia is R>G>B warm tint");
        assert_eq!(s[3], 1.0);

        let none = apply(
            ModifierKind::Sepia(Sepia { intensity: 0.0 }),
            [0.5, 0.5, 0.5, 1.0],
        );
        assert!((none[0] - 0.5).abs() < 1e-5, "intensity 0 is identity");
    }

    #[test]
    fn solarize_inverts_above_threshold_only() {
        let s = apply(
            ModifierKind::Solarize(Solarize { threshold: 0.5 }),
            [0.2, 0.8, 0.5, 1.0],
        );
        assert!((s[0] - 0.2).abs() < 1e-5, "0.2 < 0.5 stays");
        assert!((s[1] - 0.2).abs() < 1e-5, "0.8 >= 0.5 inverts to 0.2");
        assert!((s[2] - 0.5).abs() < 1e-5, "0.5 >= 0.5 inverts to 0.5");
        assert_eq!(s[3], 1.0);
    }

    #[test]
    fn duotone_maps_luma_endpoints_to_colors() {
        let shadow = [0.1, 0.15, 0.4];
        let highlight = [1.0, 0.95, 0.8];
        let d = |c| {
            apply(
                ModifierKind::Duotone(Duotone {
                    shadow,
                    highlight,
                    amount: 1.0,
                }),
                c,
            )
        };

        let black = d([0.0, 0.0, 0.0, 1.0]);
        for i in 0..3 {
            assert!((black[i] - shadow[i]).abs() < 1e-5, "black -> shadow color");
        }

        let white = d([1.0, 1.0, 1.0, 1.0]);
        for i in 0..3 {
            assert!(
                (white[i] - highlight[i]).abs() < 1e-5,
                "white -> highlight color"
            );
        }
        assert_eq!(white[3], 1.0);

        let neutral = apply(
            ModifierKind::Duotone(Duotone {
                shadow,
                highlight,
                amount: 0.0,
            }),
            [0.3, 0.7, 0.2, 1.0],
        );
        assert!((neutral[0] - 0.3).abs() < 1e-5, "amount 0 is identity");
    }
}

#[cfg(test)]
mod scaled_blur_tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut v = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                v[o] = ((x * 255) / w.max(1)) as u8;
                v[o + 1] = ((y * 255) / h.max(1)) as u8;
                v[o + 2] = (((x + y) * 255) / (w + h).max(1)) as u8;
                v[o + 3] = 255;
            }
        }
        v
    }

    /// Radii at or below the cap must be byte-identical to the direct kernel:
    /// the scale path must not perturb the blurs the goldens already pin.
    #[test]
    fn radii_within_the_cap_are_unchanged() {
        let (w, h) = (64usize, 48usize);
        for radius in [1.0f32, 16.0, 64.0, 128.0] {
            let src = gradient(w, h);
            let mut a = src.clone();
            blur_full(&mut a, w, h, radius);
            let mut b = src.clone();
            blur_direct(&mut b, w, h, radius);
            assert_eq!(a, b, "radius {radius} must take the direct path unchanged");
        }
    }

    /// Above the cap the result is the same Gaussian sampled more coarsely, so
    /// it should stay close to the direct evaluation -- not merely "some blur".
    #[test]
    fn scaled_blur_tracks_the_direct_gaussian() {
        let (w, h) = (512usize, 384usize);
        for radius in [200.0f32, 400.0] {
            let src = gradient(w, h);
            let mut scaled = src.clone();
            blur_full(&mut scaled, w, h, radius);
            let mut direct = src.clone();
            blur_direct(&mut direct, w, h, radius);

            let mean: f64 = scaled
                .iter()
                .zip(&direct)
                .map(|(a, b)| a.abs_diff(*b) as f64)
                .sum::<f64>()
                / scaled.len() as f64;
            assert!(
                mean <= 2.0,
                "radius {radius}: scaled blur drifts from the direct Gaussian \
                 (mean {mean:.2})"
            );
        }
    }

    /// The scale factor must match the GPU's, or preview and export disagree.
    #[test]
    fn scale_factor_matches_the_gpu_rule() {
        for (radius, want) in [(128.0f32, 1.0f32), (256.0, 0.5), (500.0, 0.25), (1000.0, 0.125)] {
            let ks = (MAX_DIRECT_RADIUS / radius).min(1.0).log2().floor().exp2();
            assert_eq!(ks, want, "radius {radius} should blur at scale {want}");
        }
    }

    #[test]
    fn large_blur_preserves_a_constant_image() {
        let (w, h) = (256usize, 192usize);
        let mut buf = vec![200u8; w * h * 4];
        blur_full(&mut buf, w, h, 400.0);
        assert!(
            buf.iter().all(|&b| (b as i32 - 200).abs() <= 2),
            "a constant image must survive a large blur unchanged"
        );
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use crate::modifiers::kinds::{
        ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort, Vignette,
    };
    use crate::modifiers::{Modifier, ModifierKind};

    fn noise(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        let mut s = 0x2545F491u32;
        for _ in 0..w * h {
            for _ in 0..3 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                v.push((s >> 24) as u8);
            }
            v.push(255);
        }
        v
    }

    fn m(kind: ModifierKind) -> Modifier {
        Modifier::new(kind)
    }

    /// THE oracle for layer 2: assembling bands must reproduce `render_full`
    /// byte for byte. Any apron miscalculation shows up as a seam at a band
    /// boundary, which this catches exactly.
    fn assert_bands_match_full(label: &str, chain: &[Modifier], w: u32, h: u32, band: u32) {
        let src = noise(w, h);
        let full = render_full(chain, &[], &[], &src, w, h);

        let plan = plan_modifiers(chain);
        assert!(
            plan_is_bandable(&plan),
            "{label}: chain is not bandable, test would prove nothing"
        );
        let specs = infer_specs(ImageSpec::new(w, h), &plan);
        let out = specs.last().map_or(ImageSpec::new(w, h), |s| s.output);
        let row_bytes = out.w as usize * 4;

        let mut assembled = Vec::with_capacity(full.len());
        let mut y = 0u32;
        while y < out.h {
            let y1 = (y + band).min(out.h);
            assembled.extend_from_slice(&render_band(chain, &[], &[], &src, w, h, y, y1));
            y = y1;
        }

        assert_eq!(
            assembled.len(),
            full.len(),
            "{label}: assembled band output has the wrong size"
        );
        if assembled != full {
            let bad = assembled
                .chunks(row_bytes)
                .zip(full.chunks(row_bytes))
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let diff = assembled
                .iter()
                .zip(&full)
                .filter(|(a, b)| a != b)
                .count();
            panic!(
                "{label}: banded render differs from full render; first bad row {bad} \
                 (band height {band}), {diff} bytes differ of {}",
                full.len()
            );
        }
    }

    #[test]
    fn bands_match_full_pointwise() {
        let chain = vec![m(ModifierKind::Exposure(Exposure { exposure: 0.4 }))];
        assert_bands_match_full("pointwise", &chain, 61, 47, 8);
    }

    /// Vignette reads its own UV, so a band that does not know its absolute
    /// position renders a vignette per band instead of one per image.
    #[test]
    fn bands_match_full_position_dependent_pointwise() {
        let chain = vec![m(ModifierKind::Vignette(Vignette::default()))];
        assert_bands_match_full("vignette", &chain, 48, 64, 7);
    }

    /// The apron case: a blur needs `radius` rows beyond the band on each side.
    #[test]
    fn bands_match_full_blur() {
        let chain = vec![m(ModifierKind::GaussianBlur(GaussianBlur { radius: 5.0 }))];
        assert_bands_match_full("blur", &chain, 40, 70, 9);
    }

    #[test]
    fn bands_match_full_motion_blur() {
        let chain = vec![m(ModifierKind::MotionBlur(MotionBlur {
            angle: 65.0,
            distance: 9.0,
        }))];
        assert_bands_match_full("motion-blur", &chain, 40, 70, 11);
    }

    /// A horizontal sort reads whole rows but no extra rows, so it bands.
    #[test]
    fn bands_match_full_horizontal_sort() {
        let chain = vec![m(ModifierKind::PixelSort(PixelSort {
            threshold: 0.35,
            angle: 0.0,
        }))];
        assert_bands_match_full("h-sort", &chain, 55, 40, 6);
    }

    #[test]
    fn bands_match_full_mixed_chain() {
        let chain = vec![
            m(ModifierKind::Exposure(Exposure { exposure: 0.2 })),
            m(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
            m(ModifierKind::Vignette(Vignette::default())),
        ];
        assert_bands_match_full("mixed", &chain, 50, 66, 10);
    }

    /// Two aprons stack: the first stage must fetch enough for what the second
    /// stage's apron needs, not just its own.
    #[test]
    fn bands_match_full_stacked_aprons() {
        let chain = vec![
            m(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
            m(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
        ];
        assert_bands_match_full("blur+blur", &chain, 36, 80, 8);
    }

    /// A band height that does not divide the image evenly leaves a short final
    /// band, which is where off-by-one errors surface.
    #[test]
    fn bands_match_full_with_ragged_final_band() {
        let chain = vec![m(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 }))];
        assert_bands_match_full("ragged", &chain, 32, 53, 10);
    }

    /// A single band covering the whole image must equal the full render; if
    /// this fails the band path is wrong independently of any seam logic.
    #[test]
    fn one_band_covering_everything_matches_full() {
        let chain = vec![
            m(ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 })),
            m(ModifierKind::Exposure(Exposure { exposure: -0.3 })),
        ];
        assert_bands_match_full("one-band", &chain, 40, 40, 40);
    }

    #[test]
    fn column_and_diagonal_sorts_are_not_bandable() {
        for angle in [90.0, 45.0, 30.0] {
            let chain = vec![m(ModifierKind::PixelSort(PixelSort {
                threshold: 0.4,
                angle,
            }))];
            let plan = plan_modifiers(&chain);
            assert!(
                !plan_is_bandable(&plan),
                "pixel sort at {angle} deg must be rejected for banding"
            );
        }
    }

    #[test]
    fn chromatic_aberration_is_not_bandable() {
        let chain = vec![m(ModifierKind::ChromaticAberration(ChromaticAberration {
            amount: 5.0,
        }))];
        let plan = plan_modifiers(&chain);
        assert!(
            !plan_is_bandable(&plan),
            "CA reads across the whole frame and must not be banded"
        );
    }
}

#[cfg(test)]
mod resample_tests {
    use super::{render_full, resample};
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::{Modifier, ModifierKind};

    /// Left half black, right half white -- a single vertical edge, which is
    /// what makes filter differences legible.
    fn split(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                let c = if x < w / 2 { 0u8 } else { 255u8 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    }

    fn px(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let b = ((y * w + x) * 4) as usize;
        [buf[b], buf[b + 1], buf[b + 2], buf[b + 3]]
    }

    #[test]
    fn resample_produces_the_requested_dimensions() {
        for f in ResizeFilter::ALL {
            let out = resample(&split(64, 32), 64, 32, 21, 47, f);
            assert_eq!(out.len(), 21 * 47 * 4, "{} produced a wrong-sized buffer", f.label());
        }
    }

    #[test]
    fn identity_resample_is_byte_exact() {
        let src = split(32, 16);
        for f in ResizeFilter::ALL {
            assert_eq!(resample(&src, 32, 16, 32, 16, f), src, "{}", f.label());
        }
    }

    /// Nearest must not invent intermediate values -- that blockiness is the
    /// only reason to choose it.
    #[test]
    fn nearest_preserves_the_source_palette() {
        let out = resample(&split(64, 8), 64, 8, 27, 8, ResizeFilter::Nearest);
        for i in (0..out.len()).step_by(4) {
            assert!(
                out[i] == 0 || out[i] == 255,
                "nearest produced a blended value {} at byte {i}",
                out[i]
            );
        }
    }

    /// Bilinear and Lanczos must blend across the edge; if they did not, they
    /// would be point sampling under another name.
    #[test]
    fn smooth_filters_blend_across_an_edge() {
        for f in [ResizeFilter::Bilinear, ResizeFilter::Lanczos] {
            let out = resample(&split(64, 8), 64, 8, 32, 8, f);
            let blended = (0..32).any(|x| {
                let v = px(&out, 32, x, 4)[0];
                v > 8 && v < 247
            });
            assert!(blended, "{} produced no intermediate values", f.label());
        }
    }

    /// A large downscale must average its input rather than point-sample it.
    /// With a fixed-radius kernel this aliases: the result depends on which
    /// source column each output pixel happens to land on. Averaging makes the
    /// two halves come out near the extremes with a smooth transition.
    #[test]
    fn large_downscale_averages_rather_than_aliases() {
        // 512 -> 8 is a 64x reduction; each output pixel covers 64 source px.
        let out = resample(&split(512, 8), 512, 8, 8, 8, ResizeFilter::Lanczos);
        let left = px(&out, 8, 0, 4)[0];
        let right = px(&out, 8, 7, 4)[0];
        assert!(left < 16, "left edge should stay dark, got {left}");
        assert!(right > 239, "right edge should stay light, got {right}");
    }

    #[test]
    fn alpha_survives_resampling() {
        let src = vec![255u8; 16 * 16 * 4];
        for f in ResizeFilter::ALL {
            let out = resample(&src, 16, 16, 9, 9, f);
            assert!(
                out.chunks(4).all(|p| p[3] == 255),
                "{} did not preserve opaque alpha",
                f.label()
            );
        }
    }

    /// End-to-end through the plan: `render_full` must return a buffer at the
    /// chain's output size, not the source size.
    #[test]
    fn render_full_returns_the_resized_buffer() {
        let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Pixels,
            width: 20.0,
            height: 10.0,
            filter: ResizeFilter::Bilinear,
            lock_aspect: false,
        }))];
        let out = render_full(&chain, &[], &[], &split(64, 32), 64, 32);
        assert_eq!(out.len(), 20 * 10 * 4);
    }

    /// A resize mid-chain must leave later stages operating at the new size.
    /// Before `infer_specs` was wired up this would panic on a buffer/geometry
    /// mismatch in the debug assert.
    #[test]
    fn a_stage_after_a_resize_runs_at_the_new_size() {
        use crate::modifiers::kinds::GaussianBlur;
        let chain = vec![
            Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 50.0,
                height: 50.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            })),
            Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 2.0 })),
        ];
        let out = render_full(&chain, &[], &[], &split(64, 32), 64, 32);
        assert_eq!(out.len(), 32 * 16 * 4);
    }
}

#[cfg(test)]
mod motion_blur_tests {
    use super::render_full;
    use crate::modifiers::kinds::MotionBlur;
    use crate::modifiers::{Modifier, ModifierKind};

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let on = (x + y) % 2 == 0;
                let v = if on { 255 } else { 0 };
                px[o] = v;
                px[o + 1] = v;
                px[o + 2] = v;
                px[o + 3] = 255;
            }
        }
        px
    }

    fn gpu_math_reference(src: &[u8], w: u32, h: u32, angle: f32, distance: f32) -> Vec<u8> {
        let (wi, hi) = (w as i32, h as i32);
        let rad = angle.to_radians();
        let du = rad.cos() * distance;
        let dv = rad.sin() * distance;
        let n = crate::modifiers::motion_blur_samples(distance) as i32;
        let load = |x: i32, y: i32| -> [f32; 4] {
            let cx = x.clamp(0, wi - 1);
            let cy = y.clamp(0, hi - 1);
            let o = ((cy * wi + cx) * 4) as usize;
            [
                src[o] as f32 / 255.0,
                src[o + 1] as f32 / 255.0,
                src[o + 2] as f32 / 255.0,
                src[o + 3] as f32 / 255.0,
            ]
        };
        let bilinear = |fx: f32, fy: f32| -> [f32; 4] {
            let px = fx - 0.5;
            let py = fy - 0.5;
            let x0 = px.floor() as i32;
            let y0 = py.floor() as i32;
            let tx = px - x0 as f32;
            let ty = py - y0 as f32;
            let c00 = load(x0, y0);
            let c10 = load(x0 + 1, y0);
            let c01 = load(x0, y0 + 1);
            let c11 = load(x0 + 1, y0 + 1);
            let mut o = [0.0f32; 4];
            for i in 0..4 {
                let top = c00[i] + (c10[i] - c00[i]) * tx;
                let bot = c01[i] + (c11[i] - c01[i]) * tx;
                o[i] = top + (bot - top) * ty;
            }
            o
        };
        let mut out = vec![0u8; src.len()];
        for y in 0..hi {
            for x in 0..wi {
                let cx = x as f32 + 0.5;
                let cy = y as f32 + 0.5;
                let mut acc = [0.0f32; 4];
                for i in 0..n {
                    let t = i as f32 / (n - 1) as f32 - 0.5;
                    let s = bilinear(cx + du * t, cy + dv * t);
                    for c in 0..4 {
                        acc[c] += s[c];
                    }
                }
                let o = ((y * wi + x) * 4) as usize;
                for c in 0..4 {
                    out[o + c] = ((acc[c] / n as f32).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
        out
    }

    #[test]
    fn export_matches_gpu_sample_math() {
        let (w, h) = (24u32, 18u32);
        let src = checker(w, h);
        for &(angle, dist) in &[(0.0, 20.0), (90.0, 16.0), (33.0, 25.0)] {
            let mods = vec![Modifier::new(ModifierKind::MotionBlur(MotionBlur {
                angle,
                distance: dist,
            }))];
            let bulk = render_full(&mods, &[], &[], &src, w, h);
            let reference = gpu_math_reference(&src, w, h, angle, dist);
            for (a, b) in bulk.iter().zip(&reference) {
                assert!(
                    (*a as i32 - *b as i32).abs() <= 1,
                    "angle {angle} dist {dist}: export vs gpu-math differ"
                );
            }
        }
    }

    #[test]
    fn horizontal_blur_smears_across_vertical_edge() {
        let (w, h) = (9u32, 1u32);
        let mut src = vec![0u8; (w * h * 4) as usize];
        for x in 0..w {
            let o = (x * 4) as usize;
            let v = if x < w / 2 { 0 } else { 255 };
            src[o] = v;
            src[o + 1] = v;
            src[o + 2] = v;
            src[o + 3] = 255;
        }
        let mods = vec![Modifier::new(ModifierKind::MotionBlur(MotionBlur {
            angle: 0.0,
            distance: 40.0,
        }))];
        let out = render_full(&mods, &[], &[], &src, w, h);
        let mid = ((w / 2) * 4) as usize;
        assert!(
            out[mid] > 10 && out[mid] < 245,
            "pixel at the edge should be a blend, got {}",
            out[mid]
        );
    }
}



#[cfg(test)]
mod blur_edge_tests {
    use super::*;

    /// Radii comparable to or larger than the image exercise the edge/interior
    /// split in `blur_direct`'s horizontal pass, where a bad slice bound would
    /// panic or read the wrong pixels.
    #[test]
    fn narrow_images_and_large_radii_do_not_panic() {
        for (w, h) in [(1usize, 1usize), (1, 9), (9, 1), (3, 3), (5, 40), (40, 5)] {
            for radius in [0.5f32, 1.0, 4.0, 16.0, 64.0, 200.0] {
                let mut buf = vec![128u8; w * h * 4];
                blur_direct(&mut buf, w, h, radius);
                assert_eq!(buf.len(), w * h * 4, "{w}x{h} r{radius} changed length");
                assert!(
                    buf.iter().all(|&b| (b as i32 - 128).abs() <= 1),
                    "{w}x{h} r{radius}: constant image was not preserved"
                );
            }
        }
    }

    /// The interior fast path must produce exactly what the clamped path would.
    ///
    /// Uses a single row so the vertical pass is an identity (every tap clamps
    /// to row 0 and the weights sum to 1), leaving the horizontal pass as the
    /// only thing under test.
    #[test]
    fn edge_and_interior_spans_agree() {
        let (w, h) = (64usize, 1usize);
        let mut src = vec![0u8; w * h * 4];
        let mut s = 0x1234u32;
        for b in src.chunks_mut(4) {
            for c in b.iter_mut().take(3) {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *c = (s >> 24) as u8;
            }
            b[3] = 255;
        }
        let reference = |radius: f32| -> Vec<u8> {
            let r = radius.ceil() as i32;
            let sigma = (radius / 3.0).max(0.5);
            let inv = 1.0 / (2.0 * sigma * sigma);
            let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 * inv).exp()).collect();
            let sum: f32 = k.iter().sum();
            let norm: Vec<f32> = k.iter().map(|v| v / sum).collect();
            let mut out = src.clone();
            for y in 0..h {
                for x in 0..w {
                    let mut acc = [0.0f32; 4];
                    for (ki, &kk) in norm.iter().enumerate() {
                        let sx = (x as i32 - r + ki as i32).clamp(0, w as i32 - 1) as usize;
                        for c in 0..4 {
                            acc[c] += src[(y * w + sx) * 4 + c] as f32 * kk;
                        }
                    }
                    for c in 0..4 {
                        out[(y * w + x) * 4 + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            }
            out
        };
        for radius in [2.0f32, 7.0, 20.0] {
            let mut got = src.clone();
            blur_direct(&mut got, w, h, radius);
            let want = reference(radius);
            let max = got
                .iter()
                .zip(want.iter())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max <= 1,
                "radius {radius}: the interior fast path disagrees with the \
                 clamped reference (max {max})"
            );
        }
    }
}
