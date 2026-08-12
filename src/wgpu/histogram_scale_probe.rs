//! Diagnostic: what the info panel costs on a very large image.
//!
//! The GPU benches hold the visible region constant, so they cannot see any of
//! this: these paths run on every modifier change, independent of zoom and
//! viewport.
//!
//! Two separate costs live here, and only the first has been fixed.
//!
//! The histogram used to render the whole document through cpu::render_full.
//! It now shrinks the source first, and measures 24.6 ms at 24000px -- flat
//! enough not to be the lag.
//!
//! The eyedropper is the remaining one. cursor_info and cursor_pixels are
//! called from the info panel's *view* function, so they run on the UI thread
//! every frame, and both go through with_staged -> cpu::render_full over the
//! whole document. It is keyed on the exact hash_modifiers, so every tick of a
//! resize slider invalidates it and re-renders the full document to fill a
//! small pixel-preview grid.
//!
//! Run with:
//!   cargo test --release histogram_scale -- --ignored --nocapture

#[cfg(test)]
mod tests {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::{Modifier, ModifierKind};
    use crate::wgpu::view_program::compute_subsampled_histogram;
    use std::time::Instant;

    fn resize(pct: f32) -> Modifier {
        Modifier::new(ModifierKind::Resize(Resize {
            mode: ResizeMode::Percent,
            width: pct,
            height: pct,
            filter: ResizeFilter::Lanczos,
            lock_aspect: true,
        }))
    }

    #[test]
    #[ignore = "diagnostic; allocates aggressively; run with --release --ignored --nocapture"]
    fn histogram_scale_with_source_size() {
        println!("\nHistogram cost vs source size (full-document CPU render)");
        println!("{:-<62}", "");
        println!(
            "  {:<14} {:>10} {:>12} {:>12}",
            "source", "GB", "no resize ms", "resize 50% ms"
        );
        println!("{:-<62}", "");

        for dim in [4096u32, 8192, 16384, 24000] {
            let n = dim as usize * dim as usize * 4;
            let gb = n as f64 / 1e9;
            let pixels: Vec<u8> = (0..n)
                .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
                .collect();

            let t = Instant::now();
            let _ = compute_subsampled_histogram(&pixels, dim, dim, &[]);
            let none_ms = t.elapsed().as_secs_f64() * 1000.0;

            let t = Instant::now();
            let _ = compute_subsampled_histogram(&pixels, dim, dim, &[resize(50.0)]);
            let down_ms = t.elapsed().as_secs_f64() * 1000.0;

            println!("  {dim:<14} {gb:>10.2} {none_ms:>12.1} {down_ms:>12.1}");
        }
        println!("{:-<62}", "");
    }
}
