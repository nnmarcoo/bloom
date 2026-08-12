//! Streaming raster export: encode an image band by band rather than
//! materializing the whole frame.
//!
//! Band height comes from the chain's accumulated apron, at least 4x it so the
//! overlap stays a small fraction of the work, clamped to a sane range. Peak
//! memory is one band instead of three full frames.

use rayon::prelude::*;

use super::Geom;

const STRIP_HEIGHT: u32 = 64;

fn band_height(apron_rows: u32, out_h: u32) -> u32 {
    const MIN: u32 = 64;
    const MAX: u32 = 4096;
    let want = apron_rows.saturating_mul(4).max(MIN);
    want.clamp(MIN, MAX).min(out_h.max(1))
}

#[derive(Clone, Copy)]
pub(super) struct ExportCtx<'a> {
    pub geom: Geom,
    pub processed: &'a [u8],
}

impl ExportCtx<'_> {
    pub fn out_w(&self) -> u32 {
        self.geom.out_w
    }

    pub fn out_h(&self) -> u32 {
        self.geom.out_h
    }
}

pub(super) fn render_strips(
    ctx: &ExportCtx,
    mut sink: impl FnMut(&[u8]) -> Result<(), String>,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let row_bytes = ctx.out_w() as usize * 4;
    let mut strip = vec![0u8; row_bytes * STRIP_HEIGHT as usize];

    let mut oy = 0u32;
    while oy < ctx.out_h() {
        let strip_h = (ctx.out_h() - oy).min(STRIP_HEIGHT);
        let buf = &mut strip[..row_bytes * strip_h as usize];

        buf.par_chunks_mut(row_bytes)
            .enumerate()
            .for_each(|(i, row)| {
                fill_row(row, oy + i as u32, ctx);
            });

        sink(buf)?;
        oy += strip_h;
        progress(oy as f32 / ctx.out_h() as f32);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn stream_bands(
    geom: &Geom,
    data: &super::ExportData,
    text_layers: &[Option<crate::modifiers::text_raster::TextRaster>],
    drawing_layers: &[Option<crate::modifiers::drawing_raster::LayerView<'_>>],
    pixels: &[u8],
    mut sink: impl FnMut(&[u8]) -> Result<(), String>,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let row_bytes = geom.out_w as usize * 4;

    let apron = crate::modifiers::cpu::chain_apron_rows(
        crate::modifiers::plan::ImageSpec::new(data.width, data.height),
        &crate::modifiers::plan::plan_modifiers(&data.modifiers),
    );
    let strip_rows = band_height(apron, geom.out_h);
    let mut strip = vec![0u8; row_bytes * strip_rows as usize];

    let mut oy = 0u32;
    while oy < geom.out_h {
        let strip_h = (geom.out_h - oy).min(strip_rows);

        let (a, b) = if geom.rotation == 2 {
            (
                geom.cy0 + geom.ch.saturating_sub(oy + strip_h),
                geom.cy0 + geom.ch.saturating_sub(oy),
            )
        } else {
            (geom.cy0 + oy, geom.cy0 + oy + strip_h)
        };
        let (py0, py1) = (a.min(geom.img_h), b.min(geom.img_h));

        let band = if py1 > py0 {
            crate::modifiers::cpu::render_band(
                &data.modifiers,
                text_layers,
                drawing_layers,
                pixels,
                data.width,
                data.height,
                py0,
                py1,
            )
        } else {
            Vec::new()
        };

        let buf = &mut strip[..row_bytes * strip_h as usize];
        let band_ctx = BandCtx {
            geom: *geom,
            band: &band,
            band_y0: py0,
            band_h: py1.saturating_sub(py0),
        };
        buf.par_chunks_mut(row_bytes)
            .enumerate()
            .for_each(|(i, row)| fill_row_from_band(row, oy + i as u32, &band_ctx));

        sink(buf)?;
        oy += strip_h;
        progress(oy as f32 / geom.out_h as f32);
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct BandCtx<'a> {
    geom: Geom,
    band: &'a [u8],
    band_y0: u32,
    band_h: u32,
}

fn fill_row_from_band(row: &mut [u8], oy: u32, ctx: &BandCtx) {
    let g = &ctx.geom;
    let stride = g.img_w as usize * 4;
    for ox in 0..g.out_w {
        let (cx, cy) = match g.rotation {
            0 => (ox, oy),
            2 => (g.cw - 1 - ox, g.ch - 1 - oy),
            _ => (ox, oy),
        };

        let fx = g.cx0 + cx;
        let fy = g.cy0 + cy;
        let out = ox as usize * 4;

        if fx >= g.img_w || fy >= g.img_h || fy < ctx.band_y0 || fy >= ctx.band_y0 + ctx.band_h {
            row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }

        let local = (fy - ctx.band_y0) as usize;
        let src = local * stride + fx as usize * 4;
        match ctx.band.get(src..src + 4) {
            Some(p) => row[out..out + 4].copy_from_slice(p),
            None => row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]),
        }
    }
}

pub(super) fn render_into(buf: &mut [u8], ctx: &ExportCtx) {
    let row_bytes = ctx.out_w() as usize * 4;
    buf.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(oy, row)| fill_row(row, oy as u32, ctx));
}

fn fill_row(row: &mut [u8], oy: u32, ctx: &ExportCtx) {
    let g = &ctx.geom;
    for ox in 0..g.out_w {
        let (cx, cy) = match g.rotation {
            0 => (ox, oy),
            1 => (oy, g.ch - 1 - ox),
            2 => (g.cw - 1 - ox, g.ch - 1 - oy),
            3 => (g.cw - 1 - oy, ox),
            _ => unreachable!(),
        };

        let fx = g.cx0 + cx;
        let fy = g.cy0 + cy;

        let out = ox as usize * 4;
        if fx >= g.img_w || fy >= g.img_h {
            row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }

        let src = (fy as usize * g.img_w as usize + fx as usize) * 4;
        match ctx.processed.get(src..src + 4) {
            Some(p) => row[out..out + 4].copy_from_slice(p),
            None => row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]),
        }
    }
}
