//! Which source tiles should hold GPU memory.
//!
//! Tiles used to stay resident for the lifetime of an image, making VRAM use a
//! function of image size rather than of what is on screen. Past the card's
//! capacity the driver pages textures across PCIe instead of failing, and
//! sampling cost rises roughly 8x with nothing for the pipeline to observe
//! (`large_image_probe::probe_vram_spill`).
//!
//! The policy here is deliberately *not* a capacity check. Production code
//! cannot discover how much VRAM a machine has: iced hands us only a `Device`,
//! `AdapterInfo` carries no memory size, and `device.limits()` reports the
//! limits we *requested* — `max_texture_dimension_2d` reads 8192 on an RTX 3070
//! because that is `Limits::default()`, not the card's capability. So the budget
//! is a policy we impose.
//!
//! Visibility does the real work. A viewport shows a handful of tiles at any
//! zoom, so keeping exactly the visible set bounds residency near the working
//! set on every machine, whatever its VRAM. The byte ceiling is a backstop for
//! the case visibility cannot help with, described below.
//!
//! There is deliberately no prefetch margin. An earlier version kept a ring of
//! off-screen neighbours so panning would not stall, which was wrong three ways:
//! a ring's perimeter costs more than the visible set it surrounds (8 extra
//! tiles around a single visible one, 32 around a 7x7 view), it prefetches all
//! eight directions to cover movement in one, and under the default budget it
//! never got any tiles at all because the visible set had already spent it. If
//! panning stalls turn out to be a real, measured problem, the fix is a
//! directional prefetch driven by the pan delta — a few tiles along the axis of
//! travel — not a ring.
//!
//! # When visibility is not enough
//!
//! Zoomed out with `mipmap_zoom_out` disabled, every tile is on screen and there
//! is no reduced level to sample instead, so nothing is evictable: a
//! 50000x50000 image wants ~13GB and visibility offers no relief. The ceiling
//! then has to bind, and it degrades *detail* rather than dropping tiles —
//! keeping coverage and losing sharpness is far better than a hole in the image.

// The policy is complete and tested, but `TiledSource::apply_residency` is not
// yet called from the render loop, so nothing here has a production caller. That
// wiring is the next step; until then the dead-code lint is expected.
#![allow(dead_code)]

use crate::modifiers::plan::ImageSpec;
use crate::wgpu::tiled_source::tile_resident_bytes;

/// The resolution to load source tiles at, for a given zoom.
///
/// Zoomed out, a tile is drawn into far fewer pixels than it contains, so
/// loading it at full resolution spends VRAM on detail the display cannot
/// resolve. Fitting a 50000x50000 image to a 4K screen is a ~13x reduction: at
/// full resolution its tiles want 13.15GB, at 1/8 they want 0.21GB, and 1/8 is
/// still finer than the screen can show.
///
/// Deliberately the same formula the processing path uses for `proc_scale`,
/// including snapping *up* to a power of two. The snapping is what keeps
/// residency stable — a continuous zoom produces only five distinct levels
/// between 0.05 and 1.0, so scrolling does not thrash tiles in and out.
///
/// Capped at 1.0: there is no more detail in the source than the source has.
pub fn tile_scale_for_zoom(physical_scale: f32) -> f32 {
    if physical_scale <= 0.0 {
        return 1.0;
    }
    physical_scale.log2().ceil().exp2().min(1.0)
}

/// Default ceiling on source-tile VRAM.
///
/// Roughly one 4K viewport's worth of 8192px tiles. Deliberately conservative:
/// being too low costs some eviction churn, while being too high reintroduces
/// the silent PCIe thrash this exists to prevent. Visibility keeps most sessions
/// well under this, so it rarely binds.
pub const DEFAULT_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// What a tile needs from the residency policy this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileNeed {
    /// On screen; must hold GPU memory.
    Resident,
    /// Off screen; its memory can be reclaimed.
    Evictable,
}

/// One tile's inputs to the policy.
#[derive(Debug, Clone, Copy)]
pub struct TileFacts {
    /// False when the tile is outside the viewport (`tile_ndc_culled`).
    pub visible: bool,
    /// The tile's document geometry — its size in the source image, independent
    /// of what resolution we choose to load it at.
    pub spec: ImageSpec,
    /// Runtime quality factor from [`tile_scale_for_zoom`]. Combined with `spec`
    /// this gives the texture actually allocated.
    pub scale: f32,
    pub mip_count: u32,
}

impl TileFacts {
    /// The texture size this tile will actually occupy.
    pub fn device_spec(&self) -> ImageSpec {
        self.spec.scaled(self.scale)
    }

    fn bytes(&self) -> u64 {
        let d = self.device_spec();
        tile_resident_bytes(d.w, d.h, self.mip_count)
    }
}

/// The policy's decision for a whole source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Per tile, in input order.
    pub needs: Vec<TileNeed>,
    /// Bytes the resident set will occupy.
    pub resident_bytes: u64,
    /// True when the visible set alone exceeds the budget, so the ceiling had to
    /// bind. Callers should reduce detail rather than drop tiles.
    pub over_budget: bool,
}

/// Decides residency for one source.
///
/// Visible tiles are resident; everything else is evictable.
///
/// Visible tiles are never demoted, even over budget: a missing visible tile is
/// a hole in the image, and no byte ceiling is worth that. When the visible set
/// alone exceeds the budget, `over_budget` reports it so the caller can degrade
/// detail — a coarser mip level — while keeping coverage.
pub fn plan(tiles: &[TileFacts], budget_bytes: u64) -> Plan {
    let mut needs = vec![TileNeed::Evictable; tiles.len()];
    let mut resident_bytes = 0u64;

    for (i, t) in tiles.iter().enumerate() {
        if t.visible {
            needs[i] = TileNeed::Resident;
            resident_bytes += t.bytes();
        }
    }

    Plan {
        over_budget: resident_bytes > budget_bytes,
        needs,
        resident_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUGE: u64 = u64::MAX;

    fn tile(visible: bool) -> TileFacts {
        TileFacts {
            visible,
            spec: ImageSpec::new(1024, 1024),
            scale: 1.0,
            mip_count: 1,
        }
    }

    fn one_tile_bytes() -> u64 {
        tile(true).bytes()
    }

    #[test]
    fn visible_tiles_are_resident_and_others_are_not() {
        let p = plan(&[tile(true), tile(false)], HUGE);
        assert_eq!(p.needs, [TileNeed::Resident, TileNeed::Evictable]);
        assert_eq!(p.resident_bytes, one_tile_bytes());
    }

    #[test]
    fn an_empty_source_plans_nothing() {
        let p = plan(&[], DEFAULT_BUDGET_BYTES);
        assert!(p.needs.is_empty());
        assert_eq!(p.resident_bytes, 0);
        assert!(!p.over_budget);
    }

    /// Residency tracks what is on screen, not how large the image is. This is
    /// the property the whole design rests on: a 49-tile source with two tiles
    /// visible must cost two tiles, not 49.
    #[test]
    fn residency_follows_visibility_not_image_size() {
        let mut tiles: Vec<TileFacts> = (0..49).map(|_| tile(false)).collect();
        tiles[10] = tile(true);
        tiles[11] = tile(true);

        let p = plan(&tiles, HUGE);
        assert_eq!(p.resident_bytes, one_tile_bytes() * 2);
        assert!(!p.over_budget);
    }

    /// The case visibility cannot solve: zoomed out with mipmaps disabled, every
    /// tile is on screen and there is no coarser level to fall back to. The
    /// policy must report the overflow rather than silently dropping tiles,
    /// because a missing visible tile is a hole in the image.
    #[test]
    fn a_visible_set_over_budget_is_reported_not_dropped() {
        let tiles: Vec<TileFacts> = (0..49).map(|_| tile(true)).collect();
        let budget = one_tile_bytes() * 4;
        let p = plan(&tiles, budget);

        assert!(p.over_budget, "the caller needs to know detail must drop");
        assert!(
            p.needs.iter().all(|n| *n == TileNeed::Resident),
            "every visible tile stays wanted; coverage is not negotiable"
        );
        assert!(
            p.resident_bytes > budget,
            "resident_bytes should report the real demand, not a clamped figure"
        );
    }

    /// Zooming out must reduce what a tile costs, not just what is drawn. This
    /// is the mechanism that makes a gigapixel image viewable: fit-to-screen
    /// loads eighth-resolution tiles, which is still finer than the display.
    #[test]
    fn zooming_out_reduces_what_a_tile_costs() {
        let full = tile(true);
        let mut eighth = tile(true);
        eighth.scale = 0.125;

        assert_eq!(eighth.device_spec(), ImageSpec::new(128, 128));
        assert_eq!(
            eighth.bytes() * 64,
            full.bytes(),
            "an eighth in each axis is a 64th of the pixels"
        );
    }

    /// The scale never inflates a tile past its own resolution: there is no more
    /// detail in the source than the source has.
    #[test]
    fn tile_scale_is_capped_at_full_resolution() {
        assert_eq!(tile_scale_for_zoom(4.0), 1.0);
        assert_eq!(tile_scale_for_zoom(1.0), 1.0);
        assert_eq!(
            tile_scale_for_zoom(0.0),
            1.0,
            "degenerate zoom must be safe"
        );
    }

    /// Snapping to powers of two is what keeps residency stable while zooming;
    /// without it every scroll tick would resize every tile.
    #[test]
    fn tile_scale_snaps_up_to_powers_of_two() {
        assert_eq!(tile_scale_for_zoom(0.5), 0.5);
        assert_eq!(tile_scale_for_zoom(0.3), 0.5, "snaps up, never down");
        assert_eq!(tile_scale_for_zoom(0.26), 0.5);
        assert_eq!(tile_scale_for_zoom(0.25), 0.25);
        // 50000x50000 fit to a 4K screen.
        assert_eq!(tile_scale_for_zoom(3840.0 / 50000.0), 0.125);
    }

    /// The motivating case end to end: a gigapixel source zoomed to fit must
    /// cost a fraction of what it costs at full resolution.
    #[test]
    fn a_gigapixel_source_zoomed_to_fit_is_affordable() {
        let scale = tile_scale_for_zoom(3840.0 / 50000.0);
        let tiles: Vec<TileFacts> = (0..49)
            .map(|_| TileFacts {
                visible: true,
                spec: ImageSpec::new(8192, 8192),
                scale,
                mip_count: 1,
            })
            .collect();

        let p = plan(&tiles, DEFAULT_BUDGET_BYTES);
        assert!(
            !p.over_budget,
            "zoomed to fit, a 50000^2 source should fit the default budget;              got {} bytes",
            p.resident_bytes
        );
        assert!(
            p.resident_bytes < 1_000_000_000,
            "expected well under a gigabyte, got {}",
            p.resident_bytes
        );
    }

    /// Mipped tiles cost more, so the same budget holds fewer of them.
    #[test]
    fn mip_chains_are_charged_against_the_budget() {
        let mut mipped = tile(false);
        mipped.mip_count = 11;
        assert!(
            mipped.bytes() > tile(false).bytes(),
            "a mip chain must be accounted for, not ignored"
        );
    }

    /// Evicting a tile must never leave a processed output behind that is still
    /// marked valid, or the pipeline would display stale pixels rather than
    /// simply missing ones — a silent corruption instead of a visible gap.
    ///
    /// Nothing enforces this directly. It holds because the executor drops
    /// `tile_outputs[ti]` for every tile failing `tile_ndc_culled`, and this
    /// policy evicts exactly the tiles that are not visible — the two sets are
    /// identical. This test pins that equivalence so a change to either side has
    /// to confront it.
    #[test]
    fn the_evicted_set_is_exactly_the_set_the_executor_culls() {
        let tiles = [tile(true), tile(false), tile(true), tile(false)];
        // A budget of zero, so any tendency to evict under pressure shows up.
        let p = plan(&tiles, 0);

        for (i, t) in tiles.iter().enumerate() {
            let evicted = p.needs[i] == TileNeed::Evictable;
            assert_eq!(
                evicted, !t.visible,
                "tile {i}: eviction must track visibility exactly. Evicting a \
                 visible tile would leave its processed output marked valid, \
                 showing stale pixels instead of a gap."
            );
        }
    }

    #[test]
    fn a_zero_budget_still_keeps_visible_tiles() {
        let p = plan(&[tile(true), tile(false)], 0);
        assert_eq!(p.needs[0], TileNeed::Resident);
        assert_eq!(p.needs[1], TileNeed::Evictable);
        assert!(p.over_budget);
    }
}
