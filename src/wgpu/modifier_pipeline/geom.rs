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
//! to_doc maps a source rect into the document by translating away the crop's
//! origin and then scaling by doc/kept, where kept is the source region the
//! document was made from -- the crop's extent, before any resize. Deriving
//! that as "source minus the origin" leaves the crop's own extent in the
//! ratio, so a crop at the origin rescaled by doc/src and squeezed the texture
//! by exactly the crop's fraction. It showed up only on tiled images, since
//! this path runs for per-tile ROIs and a single-tile source never reaches it,
//! which is why dragging a crop slider on a large image stretched the picture
//! while every small-image test stayed green.
//!
//! device_dims exists for the same reason grid_edge does, one level down. A
//! tile's texture must be sized from its scaled *edges*, never from its span:
//! round(span * scale) and round(r * scale) - round(l * scale) differ by a
//! pixel for most scales, and since the quad is placed from the edges, sizing
//! from the span leaves a gap or an overlap at every seam. That is the dark
//! line visible between tiles on a large upscaled document.
//!
//! tile_roi and can_reuse are the two decisions the executor's per-tile loop
//! makes each frame, split out so they can be driven without a device. Between
//! them sits to_doc, and that is where this path keeps going wrong: the cached
//! proc_px is in document space while the tile's ROI starts in source space, so
//! anything comparing them without mapping first is reading two spaces as one.
//! The reuse tests replay whole frame sequences over those three functions
//! because no single one of them can show that class of bug.
//!
//! The reuse keys are deliberately three separate facts, because each catches
//! something the others cannot. DocId::size catches a document resize; its
//! origin catches a crop dragged without resizing, where the size is identical;
//! rect_contains catches a pan onto pixels never processed; quality_scale
//! catches a zoom that leaves the region identical but changes resolution.
//! Dropping any one leaves a real bug uncaught, and each test was verified to
//! detect exactly that by deleting the key it covers.
//!
//! A composition test spanning several keys has to prove the other keys did not
//! fire, or it silently tests something else. The crop-origin test first passed
//! with the origin check deleted: to_doc subtracts the offset, so a crop moved
//! away from zero pushes its tiles to lower document coordinates, outside the
//! cached region, and rect_contains refused the reuse before the origin was
//! ever consulted. It now moves the crop toward zero and asserts containment
//! still holds before asserting the reuse is refused.
//!
//! Above the three reuse keys sits a fourth mechanism the harness also drives:
//! the dirty flag, which clears every tile output outright, and the deferral
//! that postpones it. Deferring takes dirty out of the caller's holding field,
//! so a deferred frame must hand it back or the edit is lost for good -- the
//! pipeline keeps its last render and nothing marks it again once the view
//! settles. The harness calls the real view_pipeline::defer_decision and
//! view_pipeline::doc_changed rather than restating their rules, so the two
//! cannot drift apart -- modelling them instead hid a live bug, since breaking
//! the production rule left every test green. It replays whole drags: edit
//! mid-drag, several more drag frames, then release. Neither a resize nor a
//! crop drag is ever deferred, the first because full-resolution textures left
//! on shrunken quads read as flicker, the second because a deferred frame
//! redraws from a doc_offset it never updated.

use super::*;
use crate::modifiers::roi::{self, RegionPx};
use crate::wgpu::tiled_source::TileGeom;

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
    kept: (u32, u32),
    doc: (u32, u32),
    offset: (f32, f32),
) -> RegionPx {
    let shifted = [
        r[0] - offset.0,
        r[1] - offset.1,
        r[2] - offset.0,
        r[3] - offset.1,
    ];
    let mapped = tile_out_rect(shifted, kept.0.max(1), kept.1.max(1), doc.0, doc.1);
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
    tile: TileGeom,
    full_w: f32,
    full_h: f32,
    quality_scale: f32,
    downscale: bool,
    apron_px: f32,
    roi_enabled: bool,
) -> ProcRect {
    let fw = tile.right();
    let fh = tile.bottom();
    let tl = tile.left();
    let tt = tile.top();

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

pub(super) fn tile_roi(tile: TileGeom, visible_px: [f32; 4]) -> Option<[f32; 4]> {
    let roi = tile.proc_rect_px.unwrap_or_else(|| {
        let g = roi::dilate(visible_px, ROI_MARGIN_PX);
        [
            g[0].max(tile.left()),
            g[1].max(tile.top()),
            g[2].min(tile.right()),
            g[3].min(tile.bottom()),
        ]
    });
    (roi[2] > roi[0] && roi[3] > roi[1]).then_some(roi)
}

pub(super) fn can_reuse(
    have_doc: DocId,
    have_proc_px: Option<[f32; 4]>,
    have_quality: f32,
    want_doc: DocId,
    want_roi_doc: [f32; 4],
    want_quality: f32,
) -> bool {
    have_doc == want_doc
        && have_proc_px.is_some_and(|p| rect_contains(p, want_roi_doc))
        && (have_quality - want_quality).abs() < 1e-4
}

pub(super) fn proc_rect_from_px(
    proc_px: Option<[f32; 4]>,
    tile: TileGeom,
    full_w: f32,
    full_h: f32,
    w: u32,
    h: u32,
) -> ProcRect {
    let px = proc_px.unwrap_or([tile.left(), tile.top(), tile.right(), tile.bottom()]);
    let proc = UvRect {
        origin: [px[0] / full_w, px[1] / full_h],
        size: [(px[2] - px[0]) / full_w, (px[3] - px[1]) / full_h],
    };
    let src = UvRect {
        origin: [tile.left() / full_w, tile.top() / full_h],
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
pub(super) mod harness {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(in crate::wgpu::modifier_pipeline) struct Cached {
        pub doc: DocId,
        pub proc_px: Option<[f32; 4]>,
        pub quality: f32,
        pub w: u32,
        pub h: u32,
    }

    #[derive(Clone, Copy, Debug)]
    pub(in crate::wgpu::modifier_pipeline) struct Frame {
        pub visible_px: [f32; 4],
        pub quality: f32,
        pub doc: DocId,
        pub kept: (u32, u32),
        pub edited: bool,
        pub interacting: bool,
        pub has_expensive: bool,
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(in crate::wgpu::modifier_pipeline) enum Outcome {
        Culled,
        Deferred,
        Reused,
        Processed,
    }

    pub(in crate::wgpu::modifier_pipeline) struct TileState {
        pub geom: TileGeom,
        pub cached: Option<Cached>,
        pub pending_dirty: bool,
        pub stored_doc: Option<DocId>,
        pub stored_kept: Option<(u32, u32)>,
    }

    impl TileState {
        pub fn new(geom: TileGeom) -> Self {
            Self {
                geom,
                cached: None,
                pending_dirty: false,
                stored_doc: None,
                stored_kept: None,
            }
        }

        pub fn display_quad(&self) -> Option<[f32; 4]> {
            let doc = self.stored_doc?;
            let kept = self.stored_kept?;
            let px = self.cached?.proc_px?;
            Some(to_doc(px, kept, doc.size, doc.origin))
        }

        pub fn frame(&mut self, f: Frame) -> Outcome {
            let dirty = f.edited || self.pending_dirty;
            self.pending_dirty = false;
            let doc_changed = crate::wgpu::view_pipeline::doc_changed(
                self.stored_doc.map(|d| (d.size, d.origin)),
                f.doc.size,
                f.doc.origin,
            );
            let (defer, carry) = crate::wgpu::view_pipeline::defer_decision(
                f.has_expensive,
                doc_changed,
                f.interacting,
                dirty,
            );
            if defer && self.cached.is_some() {
                self.pending_dirty |= carry;
                return Outcome::Deferred;
            }
            self.stored_doc = Some(f.doc);
            self.stored_kept = Some(f.kept);

            let Some(roi) = tile_roi(self.geom, f.visible_px) else {
                self.cached = None;
                return Outcome::Culled;
            };
            if dirty {
                self.cached = None;
            }
            let roi_doc = to_doc(roi, f.kept, f.doc.size, f.doc.origin);
            let reuse = self
                .cached
                .is_some_and(|c| can_reuse(c.doc, c.proc_px, c.quality, f.doc, roi_doc, f.quality));
            if reuse {
                return Outcome::Reused;
            }
            let scale = f.quality;
            let downscale = scale < 1.0;
            let pr = tile_proc_rect(
                TileGeom {
                    proc_rect_px: Some(roi),
                    ..self.geom
                },
                f.kept.0 as f32,
                f.kept.1 as f32,
                scale,
                downscale,
                0.0,
                true,
            );
            let px = to_doc(pr.px, f.kept, f.doc.size, f.doc.origin);
            let (w, h) = device_dims(px, scale);
            self.cached = Some(Cached {
                doc: f.doc,
                proc_px: Some(px),
                quality: scale,
                w,
                h,
            });
            Outcome::Processed
        }
    }

    pub(in crate::wgpu::modifier_pipeline) fn doc_of(size: (u32, u32)) -> DocId {
        DocId {
            size,
            origin: (0.0, 0.0),
        }
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::harness::{Frame, Outcome, TileState, doc_of};
    use super::*;
    use crate::wgpu::tiled_source::TileGeom;

    const SRC: u32 = 30000;
    const TILE: u32 = 8192;

    fn tile_at(x: u32, y: u32) -> TileGeom {
        TileGeom {
            x,
            y,
            width: TILE,
            height: TILE,
            proc_rect_px: None,
        }
    }

    fn whole_source_frame(quality: f32) -> Frame {
        Frame {
            visible_px: [0.0, 0.0, SRC as f32, SRC as f32],
            quality,
            doc: doc_of((SRC, SRC)),
            kept: (SRC, SRC),
            edited: false,
            interacting: false,
            has_expensive: false,
        }
    }

    #[test]
    fn a_settled_view_stops_reprocessing() {
        let mut t = TileState::new(tile_at(0, 0));
        let f = whole_source_frame(1.0);
        assert_eq!(t.frame(f), Outcome::Processed);
        for i in 0..5 {
            assert_eq!(
                t.frame(f),
                Outcome::Reused,
                "frame {i} after the view settled reprocessed the tile"
            );
        }
    }

    #[test]
    fn a_quality_change_reprocesses_even_at_the_same_region() {
        let mut t = TileState::new(tile_at(0, 0));
        assert_eq!(t.frame(whole_source_frame(0.25)), Outcome::Processed);
        assert_eq!(
            t.frame(whole_source_frame(1.0)),
            Outcome::Processed,
            "the same region at a new quality was reused, so the tile keeps the \
             resolution it was rendered at and the view stays blurry"
        );
        assert_eq!(t.frame(whole_source_frame(1.0)), Outcome::Reused);
    }

    #[test]
    fn zooming_in_within_the_processed_region_reuses() {
        let mut t = TileState::new(tile_at(0, 0));
        let mut f = whole_source_frame(1.0);
        f.visible_px = [0.0, 0.0, TILE as f32, TILE as f32];
        assert_eq!(t.frame(f), Outcome::Processed);

        f.visible_px = [2000.0, 2000.0, 4000.0, 4000.0];
        assert_eq!(
            t.frame(f),
            Outcome::Reused,
            "zooming into an already-processed region reprocessed it"
        );
    }

    #[test]
    fn panning_to_new_pixels_reprocesses() {
        let mut t = TileState::new(tile_at(0, 0));
        let mut f = whole_source_frame(1.0);
        f.visible_px = [0.0, 0.0, 1000.0, 1000.0];
        assert_eq!(t.frame(f), Outcome::Processed);

        f.visible_px = [7000.0, 7000.0, 8000.0, 8000.0];
        assert_eq!(
            t.frame(f),
            Outcome::Processed,
            "panning onto pixels the cache never covered reused it anyway"
        );
    }

    #[test]
    fn a_document_resize_invalidates_every_tile() {
        for tile in [tile_at(0, 0), tile_at(TILE, 0), tile_at(TILE, TILE)] {
            let mut t = TileState::new(tile);
            assert_eq!(t.frame(whole_source_frame(1.0)), Outcome::Processed);

            let resized = Frame {
                doc: doc_of((SRC / 2, SRC / 2)),
                ..whole_source_frame(1.0)
            };
            assert_eq!(
                t.frame(resized),
                Outcome::Processed,
                "tile at ({}, {}) survived a document resize",
                tile.x,
                tile.y
            );
        }
    }

    #[test]
    fn moving_a_crop_by_its_origin_alone_invalidates() {
        let doc = (10000u32, 10000u32);
        let mut t = TileState::new(tile_at(TILE, TILE));

        let at = |ox: f32, oy: f32| Frame {
            doc: DocId {
                size: doc,
                origin: (ox, oy),
            },
            kept: doc,
            ..whole_source_frame(1.0)
        };

        assert_eq!(t.frame(at(5000.0, 5000.0)), Outcome::Processed);
        let cached = t.cached.unwrap().proc_px.unwrap();

        let moved = at(4000.0, 4000.0);
        let roi_doc = to_doc(
            tile_roi(t.geom, moved.visible_px).unwrap(),
            moved.kept,
            moved.doc.size,
            moved.doc.origin,
        );
        assert!(
            rect_contains(cached, roi_doc),
            "this test only proves the origin is checked if the moved ROI \
             {roi_doc:?} still sits inside the cached region {cached:?}; \
             otherwise rect_contains rejects the reuse by itself"
        );

        assert_eq!(
            t.frame(moved),
            Outcome::Processed,
            "the crop moved but the tile was reused, so the view keeps showing \
             the region the crop used to cover"
        );
    }

    #[test]
    fn a_culled_tile_drops_its_output_and_reprocesses_on_return() {
        let mut t = TileState::new(tile_at(TILE, TILE));
        let mut f = whole_source_frame(1.0);
        assert_eq!(t.frame(f), Outcome::Processed);

        f.visible_px = [0.0, 0.0, 100.0, 100.0];
        assert_eq!(t.frame(f), Outcome::Culled);

        f.visible_px = [0.0, 0.0, SRC as f32, SRC as f32];
        assert_eq!(
            t.frame(f),
            Outcome::Processed,
            "a tile that was culled came back as a reuse, so it draws from a \
             texture the cull dropped"
        );
    }

    #[test]
    fn a_cached_output_never_claims_pixels_outside_the_document() {
        for (ox, oy, dw, dh) in [
            (0.0f32, 0.0f32, SRC, SRC),
            (5000.0, 5000.0, 10000, 10000),
            (1000.0, 2000.0, 4000, 3000),
        ] {
            for quality in [1.0f32, 0.5, 0.25] {
                for tile in [tile_at(0, 0), tile_at(TILE, TILE), tile_at(TILE * 2, 0)] {
                    let mut t = TileState::new(tile);
                    let f = Frame {
                        doc: DocId {
                            size: (dw, dh),
                            origin: (ox, oy),
                        },
                        kept: (dw, dh),
                        ..whole_source_frame(quality)
                    };
                    if t.frame(f) == Outcome::Culled {
                        continue;
                    }
                    let px = t.cached.unwrap().proc_px.unwrap();
                    assert!(
                        px[0] >= 0.0 && px[1] >= 0.0,
                        "crop ({ox},{oy}) doc {dw}x{dh} q={quality} tile \
                         ({},{}): proc_px {px:?} starts before the document",
                        tile.x,
                        tile.y
                    );
                    assert!(
                        px[2] <= dw as f32 && px[3] <= dh as f32,
                        "crop ({ox},{oy}) doc {dw}x{dh} q={quality} tile \
                         ({},{}): proc_px {px:?} runs past the document",
                        tile.x,
                        tile.y
                    );
                }
            }
        }
    }

    #[test]
    fn a_reused_tile_keeps_the_texture_size_it_was_given() {
        let mut t = TileState::new(tile_at(0, 0));
        let mut f = whole_source_frame(0.5);
        f.visible_px = [0.0, 0.0, TILE as f32, TILE as f32];
        assert_eq!(t.frame(f), Outcome::Processed);
        let before = t.cached.unwrap();

        f.visible_px = [1000.0, 1000.0, 3000.0, 3000.0];
        assert_eq!(t.frame(f), Outcome::Reused);
        let after = t.cached.unwrap();
        assert_eq!(
            (before.w, before.h),
            (after.w, after.h),
            "a reused tile changed its recorded texture size"
        );

        let pr = proc_rect_from_px(
            after.proc_px,
            t.geom,
            f.doc.size.0 as f32,
            f.doc.size.1 as f32,
            after.w,
            after.h,
        );
        let (dw, dh) = device_dims(after.proc_px.unwrap(), after.quality);
        assert_eq!(
            (pr.w, pr.h),
            (dw, dh),
            "the rebuilt ProcRect disagrees with the region it covers, so the \
             reused texture is sampled at the wrong size"
        );
    }

    fn expensive_drag(quality: f32) -> Frame {
        Frame {
            interacting: true,
            has_expensive: true,
            ..whole_source_frame(quality)
        }
    }

    #[test]
    fn an_edit_invalidates_a_tile_that_would_otherwise_be_reused() {
        let mut t = TileState::new(tile_at(0, 0));
        assert_eq!(t.frame(whole_source_frame(1.0)), Outcome::Processed);
        assert_eq!(t.frame(whole_source_frame(1.0)), Outcome::Reused);

        let edited = Frame {
            edited: true,
            ..whole_source_frame(1.0)
        };
        assert_eq!(
            t.frame(edited),
            Outcome::Processed,
            "the modifier stack changed but the tile was reused, so the edit \
             never reaches the screen"
        );
    }

    #[test]
    fn an_edit_deferred_mid_drag_still_runs_once_the_view_settles() {
        let mut t = TileState::new(tile_at(0, 0));
        assert_eq!(t.frame(expensive_drag(1.0)), Outcome::Processed);

        let edited_mid_drag = Frame {
            edited: true,
            ..expensive_drag(1.0)
        };
        assert_eq!(
            t.frame(edited_mid_drag),
            Outcome::Deferred,
            "an expensive chain mid-drag should postpone the reprocess"
        );
        assert!(
            t.pending_dirty,
            "the deferred edit was not carried, so nothing will mark the tile \
             again and the pipeline keeps its last render forever"
        );

        assert_eq!(
            t.frame(whole_source_frame(1.0)),
            Outcome::Processed,
            "the view settled on a clean frame, but the edit deferred during \
             the drag was swallowed rather than replayed"
        );
        assert!(!t.pending_dirty, "the carry outlived the frame that ran it");
    }

    #[test]
    fn repeated_deferrals_keep_carrying_the_same_edit() {
        let mut t = TileState::new(tile_at(0, 0));
        assert_eq!(t.frame(expensive_drag(1.0)), Outcome::Processed);

        let edited_mid_drag = Frame {
            edited: true,
            ..expensive_drag(1.0)
        };
        assert_eq!(t.frame(edited_mid_drag), Outcome::Deferred);

        for i in 0..4 {
            assert_eq!(
                t.frame(expensive_drag(1.0)),
                Outcome::Deferred,
                "drag frame {i} should still be deferring"
            );
            assert!(
                t.pending_dirty,
                "drag frame {i} dropped the carried edit: a clean frame takes \
                 dirty out of the holding field and must put it back"
            );
        }

        assert_eq!(t.frame(whole_source_frame(1.0)), Outcome::Processed);
    }

    #[test]
    fn a_crop_dragged_without_resizing_is_not_deferred_onto_stale_quads() {
        let doc = (10000u32, 10000u32);
        let mut t = TileState::new(tile_at(TILE, TILE));

        let at = |ox: f32, oy: f32| Frame {
            doc: DocId {
                size: doc,
                origin: (ox, oy),
            },
            kept: doc,
            interacting: true,
            has_expensive: true,
            edited: true,
            ..whole_source_frame(1.0)
        };

        assert_eq!(t.frame(at(5000.0, 5000.0)), Outcome::Processed);
        let settled = t.display_quad().expect("the tile has a quad");

        let outcome = t.frame(at(4000.0, 4000.0));
        let dragged = t.display_quad().expect("the tile still has a quad");

        assert_ne!(
            dragged, settled,
            "the crop moved by 1000px but the tile is still drawn at {settled:?}. \
             doc_changed compares only DocId::size, so a crop dragged without \
             resizing lets the frame defer, and refresh_display_transforms \
             rebuilds the quad from the pipeline's stored doc_offset, which the \
             deferred frame never updated. outcome was {outcome:?}"
        );
    }

    #[test]
    fn a_document_resize_is_never_deferred() {
        let mut t = TileState::new(tile_at(0, 0));
        assert_eq!(t.frame(expensive_drag(1.0)), Outcome::Processed);

        let resized_mid_drag = Frame {
            doc: doc_of((SRC / 2, SRC / 2)),
            edited: true,
            ..expensive_drag(1.0)
        };
        assert_eq!(
            t.frame(resized_mid_drag),
            Outcome::Processed,
            "deferring across a size change leaves full-resolution textures on \
             shrunken quads, which reads as flicker"
        );
    }
}

#[cfg(test)]
mod inscribe_tests {
    use super::{inscribe_transform, to_doc};
    use glam::{Mat4, Vec4, vec3};

    fn corners(m: Mat4) -> [f32; 4] {
        let p: Vec<Vec4> = [
            Vec4::new(-1.0, -1.0, 0.0, 1.0),
            Vec4::new(1.0, 1.0, 0.0, 1.0),
        ]
        .iter()
        .map(|c| m * *c)
        .collect();
        [
            p[0].x.min(p[1].x),
            p[0].y.min(p[1].y),
            p[0].x.max(p[1].x),
            p[0].y.max(p[1].y),
        ]
    }

    #[test]
    fn a_full_coverage_sub_rect_reproduces_the_tiles_own_quad() {
        // pr.px covering the whole intersection means the processed texture is
        // the tile's whole visible part, so the quad must not move at all.
        let base = Mat4::from_scale(vec3(0.4, 0.7, 1.0));
        for isec in [
            [0.0f32, 0.0, 8192.0, 8192.0],
            [1000.0, 2000.0, 9192.0, 10192.0],
        ] {
            let got = corners(inscribe_transform(base, isec, isec));
            let want = corners(base);
            for i in 0..4 {
                assert!(
                    (got[i] - want[i]).abs() < 1e-4,
                    "isec {isec:?}: inscribing the full rect moved the quad from \
                     {want:?} to {got:?}"
                );
            }
        }
    }

    #[test]
    fn a_resized_document_inscribes_where_the_roi_actually_is() {
        // A 30000px source resized to 15000, tiled at 8192. The second tile
        // covers source 8192..16384, i.e. document 4096..8192. A ROI covering
        // the left half of that must occupy the left half of the tile's quad.
        let kept = (30000u32, 30000u32);
        let doc = (15000u32, 15000u32);
        let isec_src = [8192.0f32, 0.0, 16384.0, 8192.0];
        let isec = to_doc(isec_src, kept, doc, (0.0, 0.0));

        let half = [isec[0], isec[1], (isec[0] + isec[2]) * 0.5, isec[3]];
        let base = Mat4::IDENTITY;
        let got = corners(inscribe_transform(base, isec, half));

        assert!(
            (got[0] - -1.0).abs() < 1e-3 && (got[2] - 0.0).abs() < 1e-3,
            "a ROI over the left half of the tile should span NDC -1..0, got \
             {got:?}. The processed texture is placed at the wrong scale, so a \
             resized document draws zoomed in."
        );
    }
}

#[cfg(test)]
mod to_doc_tests {
    use super::to_doc;

    #[test]
    fn a_crop_maps_source_pixels_to_document_pixels_one_to_one() {
        // A crop selects pixels; it never resamples them. A source pixel inside
        // the crop must land on the document pixel at the same offset from the
        // crop's origin, whatever the crop's size or position.
        for (ox, oy, dw, dh) in [
            (0.0f32, 0.0f32, 10000u32, 10000u32),
            (5000.0, 5000.0, 10000, 10000),
            (0.0, 0.0, 25000, 20000),
            (1000.0, 2000.0, 4000, 3000),
        ] {
            let doc = (dw, dh);
            let offset = (ox, oy);
            let probe = [ox + 100.0, oy + 200.0, ox + 900.0, oy + 700.0];
            // A crop alone keeps exactly the document's extent.
            let mapped = to_doc(probe, doc, doc, offset);
            assert_eq!(
                mapped,
                [100.0, 200.0, 900.0, 700.0],
                "crop at ({ox},{oy}) sized {dw}x{dh}: a pixel {:?} from the \
                 crop origin landed at {mapped:?} instead. The crop is being \
                 scaled by src/doc rather than translated, so the texture is \
                 stretched across the quad.",
                [100.0, 200.0, 900.0, 700.0]
            );
        }
    }

    #[test]
    fn a_crop_translates_its_tiles_instead_of_scaling_them() {
        const SRC: u32 = 30000;
        let doc = (10000u32, 10000u32);
        let offset = (5000.0f32, 5000.0f32);
        // A crop alone: the kept region is the crop's extent, which is the
        // document's own size. The mapping is then a pure translation.
        let kept = (doc.0, doc.1);

        let tile = [8192.0, 8192.0, 16384.0, 16384.0];
        let mapped = to_doc(tile, kept, doc, offset);

        // Translated, then clamped to the document: the tile runs past the
        // crop's far edge and cannot claim pixels the document does not have.
        let expect = |v: f32| (v - offset.0).clamp(0.0, doc.0 as f32);
        assert_eq!(
            mapped,
            [
                expect(tile[0]),
                expect(tile[1]),
                expect(tile[2]),
                expect(tile[3])
            ],
            "a crop must translate its tiles, not rescale them"
        );

        let by_ratio = super::tile_out_rect(tile, SRC, SRC, doc.0, doc.1);
        assert_ne!(
            mapped, by_ratio,
            "mapping a crop by ratio must not agree with translating it, or \
             this test proves nothing"
        );
        assert!(
            (mapped[0] - by_ratio[0]).abs() > 100.0,
            "the drift on a 30000px source is large: {mapped:?} vs {by_ratio:?}"
        );
    }

    #[test]
    fn a_resize_without_a_crop_is_the_plain_ratio_mapping() {
        // No crop, so the whole source is kept and the document is a scaled
        // copy of it. This is the one case where to_doc *is* a ratio mapping.
        const SRC: u32 = 4096;
        let doc = (2048u32, 2048u32);
        let r = [1024.0, 512.0, 3072.0, 2048.0];
        assert_eq!(
            to_doc(r, (SRC, SRC), doc, (0.0, 0.0)),
            super::tile_out_rect(r, SRC, SRC, doc.0, doc.1),
            "without a crop the two must be the same mapping, or every resize \
             would shift"
        );
    }

    #[test]
    fn a_crop_then_resize_scales_by_the_resize_alone() {
        // Crop 30000 -> 10000, then resize to 5000. The kept region is the
        // crop's 10000, so the ratio is the resize's 5000/10000, not 5000/30000.
        let kept = (10000u32, 10000u32);
        let doc = (5000u32, 5000u32);
        let offset = (5000.0f32, 5000.0f32);

        let m = to_doc([5000.0, 5000.0, 15000.0, 15000.0], kept, doc, offset);
        assert_eq!(
            m,
            [0.0, 0.0, 5000.0, 5000.0],
            "the crop's full extent must fill the document exactly"
        );
    }

    #[test]
    fn edges_stay_integers_so_neighbours_still_meet() {
        const SRC: u32 = 1179;
        let doc = (590u32, 400u32);
        let offset = (37.0f32, 61.0f32);
        for x in [0.0f32, 512.0, 1024.0, 1179.0] {
            let m = to_doc([x, 0.0, x + 1.0, 1.0], (SRC, SRC), doc, offset);
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
