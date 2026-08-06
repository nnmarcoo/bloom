//! Where large-image support runs out of memory, and why.
//!
//! Very large images (tens of thousands of pixels per side) crash on load. This
//! module pins down the cause rather than inferring it, because the fix differs
//! sharply depending on which allocation is the fatal one.
//!
//! Two probes:
//!
//! * [`tests::probe_memory_model`] — a pure accounting of every full-image
//!   allocation the current load path makes, from source dimensions alone. No
//!   allocation, so it can report on sizes far past what this machine can hold.
//! * [`tests::probe_actual_ceiling`] — allocates progressively larger sources
//!   for real and reports the largest one that succeeds, so the model can be
//!   checked against reality.
//!
//! ```text
//! cargo test --release large_image -- --ignored --nocapture
//! ```
//!
//! The load path holds up to three full-size host copies plus one in VRAM (see
//! `probe_memory_model`).
//! `ImageData::release_pixels` exists but is only called when selecting the
//! *next* image (`app::mod`), so it never lowers the peak for the image being
//! viewed. `TiledSource` uploads every tile up front and never evicts, so VRAM
//! residency tracks total image size rather than what is visible.

#[cfg(test)]
mod tests {
    /// One full-image allocation the load path makes.
    struct Allocation {
        what: &'static str,
        where_: &'static str,
        transient: bool,
    }

    const ALLOCATIONS: &[Allocation] = &[
        Allocation {
            what: "decoder DynamicImage buffer",
            where_: "ImageData::load — reader.no_limits() removes the guard",
            transient: true,
        },
        // `into_rgba8()` and `into_raw()` both consume, so an already-RGBA8
        // source moves rather than copies. Any other input format (RGB8, 16-bit,
        // float) converts into a second full-size buffer that briefly coexists
        // with the decoder's.
        Allocation {
            what: "into_rgba8() conversion copy",
            where_: "ImageData::load — only for non-RGBA8 sources",
            transient: true,
        },
        Allocation {
            what: "ImageData pixels (Arc<Vec<u8>>)",
            where_: "held for the image's lifetime",
            transient: false,
        },
        Allocation {
            what: "VRAM tile textures",
            where_: "TiledSource::new — all tiles resident, no eviction",
            transient: false,
        },
    ];

    fn gb(bytes: f64) -> f64 {
        bytes / 1e9
    }

    fn full_size_bytes(w: u64, h: u64) -> f64 {
        (w * h * 4) as f64
    }

    #[test]
    #[ignore = "diagnostic; run with --release --ignored --nocapture"]
    fn probe_memory_model() {
        println!("\nFull-image allocations in the current load path:");
        for a in ALLOCATIONS {
            let kind = if a.transient {
                "transient"
            } else {
                "retained "
            };
            println!("  [{kind}] {:<38} {}", a.what, a.where_);
        }

        let retained_host = ALLOCATIONS
            .iter()
            .filter(|a| !a.transient && !a.what.contains("VRAM"))
            .count();
        let peak_host = ALLOCATIONS
            .iter()
            .filter(|a| !a.what.contains("VRAM"))
            .count();

        println!(
            "\n  host peak during load: up to {peak_host} full copies\n  \
             host retained after load: {retained_host} full copy\n  \
             VRAM retained: 1 full copy"
        );

        println!("\n{:-<78}", "");
        println!(
            "  {:>15} {:>10} {:>14} {:>14} {:>14}",
            "source", "Gpx", "one copy GB", "host peak GB", "VRAM GB"
        );
        println!("{:-<78}", "");
        for (w, h) in [
            (4096u64, 4096u64),
            (16384, 16384),
            (30000, 30000),
            (50000, 50000),
            (100000, 100000),
        ] {
            let one = full_size_bytes(w, h);
            println!(
                "  {:>15} {:>10.2} {:>14.1} {:>14.1} {:>14.1}",
                format!("{w}x{h}"),
                (w * h) as f64 / 1e9,
                gb(one),
                gb(one * peak_host as f64),
                gb(one),
            );
        }
        println!("{:-<78}", "");
        println!(
            "\n  Host peak assumes 4 bytes/px throughout. A 16-bit source decodes\n  \
             into an 8-byte/px buffer first, so its peak is roughly 1.5x the\n  \
             figures above; a 32-bit float source, 2.5x."
        );
        println!(
            "\n  The crash is residency, not throughput: no amount of shader\n  \
             tuning changes these numbers. Note also that a single full-size\n  \
             allocation usually succeeds — it is the several coexisting copies\n  \
             that exhaust memory, which is why the failure looks intermittent.\n"
        );
    }

    /// Finds the largest source this machine can actually allocate on the host.
    ///
    /// Only allocates the retained `ImageData`-sized buffer, not the transient
    /// decode copies, so the true ceiling for loading a file is lower than what
    /// this reports.
    #[test]
    #[ignore = "diagnostic; allocates aggressively; run with --release --ignored --nocapture"]
    fn probe_actual_ceiling() {
        println!("\nLargest single full-image buffer this machine can allocate:");
        let mut last_ok: Option<(u64, f64)> = None;
        for dim in [4096u64, 8192, 16384, 24000, 32000, 40000, 50000] {
            let bytes = full_size_bytes(dim, dim);
            // try_reserve reports failure instead of aborting, unlike vec![].
            let mut v: Vec<u8> = Vec::new();
            match v.try_reserve_exact(bytes as usize) {
                Ok(()) => {
                    // Commit the reservation: extend to full length, then touch
                    // pages across the range so they are really backed rather
                    // than a lazy virtual mapping.
                    v.extend(std::iter::repeat_n(0u8, bytes as usize));
                    let step = (bytes as usize / 64).max(1);
                    let mut i = 0;
                    while i < v.len() {
                        v[i] = 1;
                        i += step;
                    }
                    println!("  {dim}x{dim}  {:>8.1} GB   ok", gb(bytes));
                    last_ok = Some((dim, gb(bytes)));
                    drop(v);
                }
                Err(_) => {
                    println!("  {dim}x{dim}  {:>8.1} GB   FAILED to allocate", gb(bytes));
                    break;
                }
            }
        }
        match last_ok {
            Some((dim, g)) => println!(
                "\n  Ceiling for ONE copy: {dim}x{dim} ({g:.1} GB).\n  \
                 The load path needs up to 3 host copies, so the practical\n  \
                 load ceiling is roughly a third of this.\n"
            ),
            None => println!("\n  Could not allocate even the smallest probe.\n"),
        }
    }
}
