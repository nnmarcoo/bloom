//! Geometry for tiled execution: VRAM budgeting, the output grid, and the
//! rects a tile is drawn with.
//!
//! grid_edge maps a source edge onto the output document from that coordinate
//! alone, so two tiles sharing an edge in the source land on the same output
//! edge by construction. Scaling each tile's width independently does not have
//! that property: tiles that met exactly can round apart into a seam or round
//! together into an overlap. The far edge needs no special case, because
//! src_len * out_len / src_len is exactly out_len.
//!
//! The grid tests pin the partition at a real image's awkward dimensions
//! (1179x1159 in 512px tiles, neither axis dividing evenly), covering
//! downscale, upscale, and the degenerate one-pixel document.
//!
//! device_dims exists for the same reason grid_edge does, one level down. A
//! tile's texture must be sized from its scaled *edges*, never from its span:
//! round(span * scale) and round(r * scale) - round(l * scale) differ by a
//! pixel for most scales, and since the quad is placed from the edges, sizing
//! from the span leaves a gap or an overlap at every seam. That is the dark
//! line visible between tiles on a large upscaled document.

use super::*;
use crate::modifiers::roi::{self, RegionPx};

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

pub(super) fn grid_edge(src: f32, src_len: u32, out_len: u32) -> f32 {
    if src_len == 0 {
        return 0.0;
    }
    (src * out_len as f32 / src_len as f32).round()
}

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

pub(super) fn to_doc(
    r: RegionPx,
    src_w: u32,
    src_h: u32,
    doc: (u32, u32),
    offset: (f32, f32),
) -> RegionPx {
    let kept_w = ((src_w as f32 - offset.0).max(1.0)).round() as u32;
    let kept_h = ((src_h as f32 - offset.1).max(1.0)).round() as u32;
    let shifted = [
        r[0] - offset.0,
        r[1] - offset.1,
        r[2] - offset.0,
        r[3] - offset.1,
    ];
    let mapped = tile_out_rect(shifted, kept_w, kept_h, doc.0, doc.1);
    roi::clamp_region(mapped, doc.0 as f32, doc.1 as f32)
}

pub(super) fn device_dims(r: [f32; 4], scale: f32) -> (u32, u32) {
    let x0 = (r[0] * scale).round() as i64;
    let y0 = (r[1] * scale).round() as i64;
    let x1 = (r[2] * scale).round() as i64;
    let y1 = (r[3] * scale).round() as i64;
    (((x1 - x0).max(1)) as u32, ((y1 - y0).max(1)) as u32)
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
mod to_doc_tests {
    use super::to_doc;

    #[test]
    fn a_crop_translates_its_tiles_instead_of_scaling_them() {
        const SRC: u32 = 30000;
        let doc = (10000u32, 10000u32);
        let offset = (5000.0f32, 5000.0f32);

        let tile = [8192.0, 8192.0, 16384.0, 16384.0];
        let mapped = to_doc(tile, SRC, SRC, doc, offset);

        let kept = (SRC as f32 - offset.0) as u32;
        let expect = |v: f32| ((v - offset.0) * doc.0 as f32 / kept as f32).round();
        assert_eq!(
            mapped,
            [
                expect(tile[0]),
                expect(tile[1]),
                expect(tile[2]),
                expect(tile[3])
            ]
        );

        let by_ratio = super::tile_out_rect(tile, SRC, SRC, doc.0, doc.1);
        assert_ne!(
            mapped, by_ratio,
            "mapping a crop by ratio must not agree with translating it, or \
             this test proves nothing"
        );
        assert!(
            (mapped[0] - by_ratio[0]).abs() > 1000.0,
            "the drift on a 30000px source is large: {mapped:?} vs {by_ratio:?}"
        );
    }

    #[test]
    fn no_offset_is_the_plain_ratio_mapping() {
        const SRC: u32 = 4096;
        let doc = (2048u32, 2048u32);
        let r = [1024.0, 512.0, 3072.0, 2048.0];
        assert_eq!(
            to_doc(r, SRC, SRC, doc, (0.0, 0.0)),
            super::tile_out_rect(r, SRC, SRC, doc.0, doc.1),
            "without a crop the two must be the same mapping, or every resize \
             would shift"
        );
    }

    #[test]
    fn edges_stay_integers_so_neighbours_still_meet() {
        const SRC: u32 = 1179;
        let doc = (590u32, 400u32);
        let offset = (37.0f32, 61.0f32);
        for x in [0.0f32, 512.0, 1024.0, 1179.0] {
            let m = to_doc([x, 0.0, x + 1.0, 1.0], SRC, SRC, doc, offset);
            for v in m {
                assert_eq!(v, v.round(), "edge {v} is fractional, so tiles seam");
            }
        }
    }
}

#[cfg(test)]
mod grid_tests {
    use super::{grid_edge, tile_out_rect};

    const SRC_W: u32 = 1179;
    const SRC_H: u32 = 1159;
    const OUT_W: u32 = 590;
    const OUT_H: u32 = 580;

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
    fn a_tiles_texture_is_exactly_what_its_device_footprint_needs() {
        const SRC: u32 = 30000;
        let edges = [0u32, 8192, 16384, 24576, SRC];

        for doc in [300u32, 600, 1500, 45000, 60000] {
            for scale in [1.0f32, 0.5, 0.25, 0.125, 0.0625] {
                for i in 0..4 {
                    let l = grid_edge(edges[i] as f32, SRC, doc);
                    let r = grid_edge(edges[i + 1] as f32, SRC, doc);

                    let tex_w = super::device_dims([l, 0.0, r, 1.0], scale).0 as i64;
                    let dev_l = (l * scale).round() as i64;
                    let dev_r = (r * scale).round() as i64;
                    let footprint = (dev_r - dev_l).max(1);

                    assert_eq!(
                        tex_w, footprint,
                        "doc={doc} scale={scale} tile={i}: the tile's texture is \
                         {tex_w} device px wide but it is drawn across \
                         {footprint}. Sizing from the span while placing from the \
                         edges leaves a gap or an overlap at the seam, which \
                         shows as a dark line between tiles."
                    );
                }
            }
        }
    }

    #[test]
    fn edges_are_integers() {
        for &e in &src_edges(SRC_W, 512) {
            let g = grid_edge(e, SRC_W, OUT_W);
            assert_eq!(g, g.round(), "edge {e} mapped to {g}, not an integer");
        }
    }

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
