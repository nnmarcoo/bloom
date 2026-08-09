use super::*;

pub(super) fn process_vram_budget(device: &Device) -> u64 {
    device
        .limits()
        .max_buffer_size
        .clamp(PROCESS_VRAM_BUDGET_MIN, PROCESS_VRAM_BUDGET_MAX)
}

pub(super) fn sort_buffer_limit(device: &Device) -> u64 {
    let limits = device.limits();
    limits
        .max_buffer_size
        .min(limits.max_storage_buffer_binding_size as u64)
}

pub(super) fn fit_process_scale(
    unit_w: u32,
    unit_h: u32,
    n_units: u64,
    banks: u64,
    budget: u64,
    base: f32,
) -> f32 {
    let mut scale = base.clamp(1.0 / 4096.0, 1.0).log2().floor().exp2();
    loop {
        let w = ((unit_w as f32 * scale).round() as u64).max(1);
        let h = ((unit_h as f32 * scale).round() as u64).max(1);
        let total = w * h * 4 * banks * n_units.max(1);
        if total <= budget || scale <= 1.0 / 4096.0 {
            break;
        }
        scale *= 0.5;
    }
    scale
}

/// Map a source-space edge onto the output grid.
///
/// Consumed by the executor once tiles are carved in output space; the geometry
/// and its tests land first so the property is pinned before anything uses it.
///
/// Every boundary is computed from the source coordinate alone, so two tiles
/// sharing an edge in the source land on the same output edge by construction.
/// Scaling each tile's width independently does not have that property: two
/// tiles that met exactly can round apart, leaving a seam, or round together,
/// producing an overlap.
///
/// The far edge needs no special case: `src_len * out_len / src_len` is exactly
/// `out_len`, so the last row and column reach the document edge on their own.
/// The previous design missed it because it scaled each tile's *width* rather
/// than its edges, and widths accumulate error where edges do not.
#[allow(dead_code, reason = "wired into the executor next")]
pub(super) fn grid_edge(src: f32, src_len: u32, out_len: u32) -> f32 {
    if src_len == 0 {
        return 0.0;
    }
    (src * out_len as f32 / src_len as f32).round()
}

/// A tile's region in the output document, on integer boundaries.
#[allow(dead_code, reason = "wired into the executor next")]
pub(super) fn tile_out_rect(
    px: [f32; 4],
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
) -> [f32; 4] {
    [
        grid_edge(px[0], src_w, out_w),
        grid_edge(px[1], src_h, out_h),
        grid_edge(px[2], src_w, out_w),
        grid_edge(px[3], src_h, out_h),
    ]
}

pub(super) fn tile_proc_rect(
    tile: &crate::wgpu::tiled_source::Tile,
    full_w: f32,
    full_h: f32,
    quality_scale: f32,
    downscale: bool,
    apron_px: f32,
    roi_enabled: bool,
) -> ProcRect {
    let fw = tile.x as f32 + tile.width as f32;
    let fh = tile.y as f32 + tile.height as f32;
    let tl = tile.x as f32;
    let tt = tile.y as f32;

    let margin_px = if downscale { 0.0 } else { ROI_MARGIN_PX };
    let roi = if roi_enabled { tile.proc_rect_px } else { None };
    let px = match roi {
        Some([l, t, r, b]) => {
            let grow = apron_px + margin_px;
            [
                (l - grow).floor().max(tl),
                (t - grow).floor().max(tt),
                (r + grow).ceil().min(fw),
                (b + grow).ceil().min(fh),
            ]
        }
        None => [tl, tt, fw, fh],
    };

    let pw_px = (px[2] - px[0]).max(1.0);
    let ph_px = (px[3] - px[1]).max(1.0);
    let scale = if downscale { quality_scale } else { 1.0 };
    let w = ((pw_px * scale).ceil() as u32).max(1);
    let h = ((ph_px * scale).ceil() as u32).max(1);

    let proc = UvRect {
        origin: [px[0] / full_w, px[1] / full_h],
        size: [(px[2] - px[0]) / full_w, (px[3] - px[1]) / full_h],
    };
    let src = UvRect {
        origin: [tl / full_w, tt / full_h],
        size: [tile.width as f32 / full_w, tile.height as f32 / full_h],
    };
    ProcRect {
        px,
        proc,
        src,
        w,
        h,
    }
}

pub(super) fn inscribe_transform(base: glam::Mat4, isec: [f32; 4], sub: [f32; 4]) -> glam::Mat4 {
    let [il, it, ir, ib] = isec;
    let iw = (ir - il).max(1e-6);
    let ih = (ib - it).max(1e-6);
    let qx = |x: f32| -1.0 + 2.0 * (x - il) / iw;
    let qy = |y: f32| 1.0 - 2.0 * (y - it) / ih;
    let qx0 = qx(sub[0]);
    let qx1 = qx(sub[2]);
    let qy_top = qy(sub[1]);
    let qy_bot = qy(sub[3]);
    let cx = (qx0 + qx1) * 0.5;
    let cy = (qy_top + qy_bot) * 0.5;
    let hx = (qx1 - qx0) * 0.5;
    let hy = (qy_top - qy_bot) * 0.5;
    base * glam::Mat4::from_translation(glam::vec3(cx, cy, 0.0))
        * glam::Mat4::from_scale(glam::vec3(hx, hy, 1.0))
}

pub(super) fn rect_contains(outer: [f32; 4], inner: [f32; 4]) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[2] <= outer[2] + 0.5
        && inner[3] <= outer[3] + 0.5
}

pub(super) fn proc_rect_from_px(
    proc_px: Option<[f32; 4]>,
    tile: &crate::wgpu::tiled_source::Tile,
    full_w: f32,
    full_h: f32,
    w: u32,
    h: u32,
) -> ProcRect {
    let px = proc_px.unwrap_or([
        tile.x as f32,
        tile.y as f32,
        tile.x as f32 + tile.width as f32,
        tile.y as f32 + tile.height as f32,
    ]);
    let proc = UvRect {
        origin: [px[0] / full_w, px[1] / full_h],
        size: [(px[2] - px[0]) / full_w, (px[3] - px[1]) / full_h],
    };
    let src = UvRect {
        origin: [tile.x as f32 / full_w, tile.y as f32 / full_h],
        size: [tile.width as f32 / full_w, tile.height as f32 / full_h],
    };
    ProcRect {
        px,
        proc,
        src,
        w,
        h,
    }
}

pub(super) fn tex_copy_info(
    tex: &Texture,
    origin: iced::wgpu::Origin3d,
) -> iced::wgpu::TexelCopyTextureInfo<'_> {
    iced::wgpu::TexelCopyTextureInfo {
        texture: tex,
        mip_level: 0,
        origin,
        aspect: iced::wgpu::TextureAspect::All,
    }
}

#[cfg(test)]
mod grid_tests {
    use super::{grid_edge, tile_out_rect};

    /// The geometry that broke the previous attempt: 1179x1159 in 512px tiles,
    /// halved. Neither axis divides evenly and the edge tiles are partial.
    const SRC_W: u32 = 1179;
    const SRC_H: u32 = 1159;
    const OUT_W: u32 = 590;
    const OUT_H: u32 = 580;

    /// Source tile boundaries for a 3-column, 3-row grid at 512px.
    fn src_edges(len: u32, tile: u32) -> Vec<f32> {
        let mut v = vec![0.0f32];
        let mut x = tile;
        while x < len {
            v.push(x as f32);
            x += tile;
        }
        v.push(len as f32);
        v
    }

    #[test]
    fn edges_are_integers() {
        for &e in &src_edges(SRC_W, 512) {
            let g = grid_edge(e, SRC_W, OUT_W);
            assert_eq!(g, g.round(), "edge {e} mapped to {g}, not an integer");
        }
    }

    /// The property that makes tiles meet: an edge depends only on its source
    /// coordinate, never on which tile is asking.
    ///
    /// The previous design scaled each tile's width independently, so the same
    /// boundary could come out differently for the tile on its left than for
    /// the one on its right. Here the right edge of one tile and the left edge
    /// of the next are the same call, so they cannot disagree.
    #[test]
    fn neighbors_agree_on_their_shared_edge() {
        let xs = src_edges(SRC_W, 512);
        for w in xs.windows(2) {
            let left_tile = tile_out_rect([w[0], 0.0, w[1], 1.0], SRC_W, SRC_H, OUT_W, OUT_H);
            let right_tile =
                tile_out_rect([w[1], 0.0, w[1] + 1.0, 1.0], SRC_W, SRC_H, OUT_W, OUT_H);
            assert_eq!(
                left_tile[2], right_tile[0],
                "source edge {} became {} on the left tile and {} on the right",
                w[1], left_tile[2], right_tile[0]
            );
        }
    }

    #[test]
    fn the_far_edge_lands_exactly_on_the_document() {
        assert_eq!(grid_edge(SRC_W as f32, SRC_W, OUT_W), OUT_W as f32);
        assert_eq!(grid_edge(SRC_H as f32, SRC_H, OUT_H), OUT_H as f32);
    }

    #[test]
    fn the_origin_stays_at_zero() {
        assert_eq!(grid_edge(0.0, SRC_W, OUT_W), 0.0);
    }

    /// The whole grid must tile the document exactly. This is the same property
    /// the executor golden checks, at the level where it is decided.
    #[test]
    fn the_grid_covers_the_document_without_gaps_or_overlaps() {
        let xs = src_edges(SRC_W, 512);
        let ys = src_edges(SRC_H, 512);
        let mut covered = vec![0u8; (OUT_W * OUT_H) as usize];

        for wy in ys.windows(2) {
            for wx in xs.windows(2) {
                let r = tile_out_rect([wx[0], wy[0], wx[1], wy[1]], SRC_W, SRC_H, OUT_W, OUT_H);
                for y in (r[1] as u32)..(r[3] as u32) {
                    for x in (r[0] as u32)..(r[2] as u32) {
                        covered[(y * OUT_W + x) as usize] += 1;
                    }
                }
            }
        }

        let gaps = covered.iter().filter(|&&c| c == 0).count();
        let overlaps = covered.iter().filter(|&&c| c > 1).count();
        assert_eq!(gaps, 0, "{gaps} output pixels covered by no tile");
        assert_eq!(overlaps, 0, "{overlaps} output pixels covered twice");
    }

    /// Upscaling has the same requirement, and a naive floor would leave the
    /// far edge short.
    #[test]
    fn the_grid_covers_an_upscaled_document() {
        let xs = src_edges(SRC_W, 512);
        let ys = src_edges(SRC_H, 512);
        let (uw, uh) = (SRC_W * 2 + 7, SRC_H * 2 + 3);
        let mut covered = vec![0u8; (uw * uh) as usize];

        for wy in ys.windows(2) {
            for wx in xs.windows(2) {
                let r = tile_out_rect([wx[0], wy[0], wx[1], wy[1]], SRC_W, SRC_H, uw, uh);
                for y in (r[1] as u32)..(r[3] as u32) {
                    for x in (r[0] as u32)..(r[2] as u32) {
                        covered[(y * uw + x) as usize] += 1;
                    }
                }
            }
        }

        assert_eq!(covered.iter().filter(|&&c| c == 0).count(), 0);
        assert_eq!(covered.iter().filter(|&&c| c > 1).count(), 0);
    }

    /// A resize to a single pixel is the extreme case, and it must still be a
    /// valid partition rather than nine tiles each claiming the same pixel.
    #[test]
    fn a_one_pixel_document_is_still_a_partition() {
        let xs = src_edges(SRC_W, 512);
        let ys = src_edges(SRC_H, 512);
        let mut covered = [0u8; 1];
        for wy in ys.windows(2) {
            for wx in xs.windows(2) {
                let r = tile_out_rect([wx[0], wy[0], wx[1], wy[1]], SRC_W, SRC_H, 1, 1);
                for y in (r[1] as u32)..(r[3] as u32) {
                    for x in (r[0] as u32)..(r[2] as u32) {
                        covered[(y + x) as usize] += 1;
                    }
                }
            }
        }
        assert_eq!(
            covered[0], 1,
            "the single output pixel is claimed {} times, not once",
            covered[0]
        );
    }
}
