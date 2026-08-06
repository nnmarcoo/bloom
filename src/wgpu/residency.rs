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
//! zoom, so keeping the visible set plus a small margin bounds residency near
//! the working set on every machine, whatever its VRAM. The byte ceiling is a
//! backstop for the case visibility cannot help with, described below.
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

use crate::wgpu::tiled_source::tile_resident_bytes;

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
    /// On screen, or close enough that panning will reach it shortly.
    Resident,
    /// Off screen; its memory can be reclaimed.
    Evictable,
}

/// One tile's inputs to the policy.
#[derive(Debug, Clone, Copy)]
pub struct TileFacts {
    /// False when the tile is outside the viewport (`tile_ndc_culled`).
    pub visible: bool,
    /// Rough distance from the viewport in tile widths; 0 when visible. Used to
    /// keep the nearest off-screen tiles resident so panning does not stall.
    pub rings_away: u32,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
}

impl TileFacts {
    fn bytes(&self) -> u64 {
        tile_resident_bytes(self.width, self.height, self.mip_count)
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

/// How many rings of off-screen tiles to keep for panning headroom.
///
/// One ring is a good trade: it absorbs a pan of up to a tile without a stall,
/// while costing far less than the visible set itself on a large image. Larger
/// values erode the point of evicting at all.
pub const MARGIN_RINGS: u32 = 1;

/// Decides residency for one source.
///
/// Visible tiles are always wanted. Tiles within [`MARGIN_RINGS`] are wanted
/// next, nearest first. Anything further out is evictable.
///
/// The budget is applied only to the margin: visible tiles are never demoted,
/// because a missing visible tile is a hole in the image, whereas a missing
/// margin tile is only a stall when panning reaches it. If the visible set alone
/// exceeds the budget, `over_budget` says so and the caller decides how to
/// degrade.
pub fn plan(tiles: &[TileFacts], budget_bytes: u64) -> Plan {
    let mut needs = vec![TileNeed::Evictable; tiles.len()];
    let mut resident_bytes = 0u64;

    for (i, t) in tiles.iter().enumerate() {
        if t.visible {
            needs[i] = TileNeed::Resident;
            resident_bytes += t.bytes();
        }
    }

    let over_budget = resident_bytes > budget_bytes;

    // Spend whatever is left on the margin, nearest rings first.
    if !over_budget {
        let mut margin: Vec<(usize, u32)> = tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.visible && t.rings_away <= MARGIN_RINGS)
            .map(|(i, t)| (i, t.rings_away))
            .collect();
        margin.sort_by_key(|(_, rings)| *rings);

        for (i, _) in margin {
            let bytes = tiles[i].bytes();
            if resident_bytes + bytes > budget_bytes {
                break;
            }
            needs[i] = TileNeed::Resident;
            resident_bytes += bytes;
        }
    }

    Plan {
        needs,
        resident_bytes,
        over_budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUGE: u64 = u64::MAX;

    fn tile(visible: bool, rings_away: u32) -> TileFacts {
        TileFacts {
            visible,
            rings_away,
            width: 1024,
            height: 1024,
            mip_count: 1,
        }
    }

    fn one_tile_bytes() -> u64 {
        tile(true, 0).bytes()
    }

    #[test]
    fn visible_tiles_are_resident() {
        let p = plan(&[tile(true, 0), tile(false, 9)], HUGE);
        assert_eq!(p.needs, [TileNeed::Resident, TileNeed::Evictable]);
    }

    #[test]
    fn distant_tiles_are_evictable() {
        let p = plan(&[tile(false, MARGIN_RINGS + 1)], HUGE);
        assert_eq!(p.needs, [TileNeed::Evictable]);
    }

    #[test]
    fn the_margin_ring_is_kept_for_panning() {
        let p = plan(&[tile(true, 0), tile(false, 1)], HUGE);
        assert_eq!(
            p.needs,
            [TileNeed::Resident, TileNeed::Resident],
            "a tile one ring out should stay resident so panning does not stall"
        );
    }

    #[test]
    fn an_empty_source_plans_nothing() {
        let p = plan(&[], DEFAULT_BUDGET_BYTES);
        assert!(p.needs.is_empty());
        assert_eq!(p.resident_bytes, 0);
        assert!(!p.over_budget);
    }

    #[test]
    fn the_budget_trims_the_margin_before_anything_visible() {
        // Room for the visible tile and one more, but two margin tiles want in.
        let budget = one_tile_bytes() * 2;
        let p = plan(&[tile(true, 0), tile(false, 1), tile(false, 1)], budget);
        assert_eq!(p.needs[0], TileNeed::Resident, "visible must survive");
        let kept = p.needs.iter().filter(|n| **n == TileNeed::Resident).count();
        assert_eq!(
            kept, 2,
            "the budget should cap the margin at one extra tile"
        );
        assert!(p.resident_bytes <= budget);
        assert!(!p.over_budget);
    }

    #[test]
    fn nearer_margin_tiles_win_the_remaining_budget() {
        let budget = one_tile_bytes() * 2;
        let p = plan(&[tile(true, 0), tile(false, 2), tile(false, 1)], budget);
        assert_eq!(
            p.needs[2],
            TileNeed::Resident,
            "the closer margin tile should be preferred"
        );
        assert_eq!(p.needs[1], TileNeed::Evictable);
    }

    /// The case visibility cannot solve: zoomed out with mipmaps disabled, every
    /// tile is on screen and there is no coarser level to fall back to. The
    /// policy must report the overflow rather than silently dropping tiles,
    /// because a missing visible tile is a hole in the image.
    #[test]
    fn a_visible_set_over_budget_is_reported_not_dropped() {
        let tiles: Vec<TileFacts> = (0..49).map(|_| tile(true, 0)).collect();
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

    /// Mipped tiles cost more, so the same budget holds fewer of them.
    #[test]
    fn mip_chains_are_charged_against_the_budget() {
        let mut mipped = tile(false, 1);
        mipped.mip_count = 11;
        assert!(
            mipped.bytes() > tile(false, 1).bytes(),
            "a mip chain must be accounted for, not ignored"
        );
    }

    /// Evicting a tile must never leave a processed output behind that is still
    /// marked valid, or the pipeline would display stale pixels rather than
    /// simply missing ones — a silent corruption instead of a visible gap.
    ///
    /// Nothing enforces this directly. It holds because the executor drops
    /// `tile_outputs[ti]` for every tile failing `tile_ndc_culled`, and this
    /// policy only ever evicts tiles that are not visible. The evicted set is
    /// therefore a subset of the culled set. That relationship is the invariant;
    /// this test pins it so a change to either side has to confront it.
    #[test]
    fn evicted_tiles_are_always_ones_the_executor_has_already_culled() {
        let tiles = [
            tile(true, 0),                 // visible
            tile(false, 1),                // culled, inside the margin
            tile(false, MARGIN_RINGS + 5), // culled, outside the margin
        ];
        // A budget too small for the margin, forcing the tightest eviction the
        // policy can produce.
        let p = plan(&tiles, one_tile_bytes());

        for (i, t) in tiles.iter().enumerate() {
            if p.needs[i] == TileNeed::Evictable {
                assert!(
                    !t.visible,
                    "tile {i} was evicted while visible; the executor would keep \
                     its processed output marked valid and show stale pixels"
                );
            }
        }
    }

    #[test]
    fn a_zero_budget_still_keeps_visible_tiles() {
        let p = plan(&[tile(true, 0), tile(false, 1)], 0);
        assert_eq!(p.needs[0], TileNeed::Resident);
        assert_eq!(p.needs[1], TileNeed::Evictable);
        assert!(p.over_budget);
    }
}
