use rayon::prelude::*;

use super::Geom;

const STRIP_HEIGHT: u32 = 64;

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
