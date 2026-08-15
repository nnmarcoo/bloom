//! GPU-side timing baselines for the modifier pipeline.
//!
//! The CPU bench in `export::bench` says nothing about shader quality -- the
//! preview path is entirely separate code. This measures the GPU pipeline
//! directly so decisions about shaders and cost bounds rest on data.
//!
//! ```text
//! cargo test --release gpu_bench -- --ignored --nocapture
//! ```
//!
//! Timing is wall clock around `prepare` plus a blocking readback, so it
//! includes CPU-side encoding and submission, not just GPU execution. That is
//! the number a user feels, but it means small differences are not meaningful;
//! treat these as coarse comparisons between chains, not precise shader
//! profiles.
//!
//! The zoomed-in case is the one that matters most. `quality_scale` is capped at
//! 1.0 and only drops below it when zoomed *out*, so at 100% zoom the pipeline
//! has no proxy relief and pays full cost -- which is where large-radius blurs
//! were historically slow enough to lag.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::modifiers::kinds::{ChromaticAberration, Exposure, GaussianBlur, MotionBlur};
    use crate::modifiers::{Modifier, ModifierKind};
    use crate::wgpu::media::image_data::ImageData;
    use crate::wgpu::modifier_pipeline::goldens::read_texture;
    use crate::wgpu::passes::display::DisplayPass;
    use crate::wgpu::test_device::{GPU_LOCK, try_device};
    use crate::wgpu::tiled_source::TiledSource;
    use iced::wgpu::TextureFormat;
    use std::time::{Duration, Instant};

    const W: u32 = 4096;
    const H: u32 = 2731;

    const RUNS: u32 = 5;

    fn pixels(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        let mut s = 0x12345678u32;
        for _ in 0..w * h {
            for _ in 0..3 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                v.push((s >> 24) as u8);
            }
            v.push(255);
        }
        v
    }

    fn make_source(device: &Device, queue: &Queue, image: &ImageData) -> TiledSource {
        let format = TextureFormat::Rgba8Unorm;
        let display = DisplayPass::new(device, format);
        let (blit_pipeline, blit_bgl) = gpu::blit_pipeline(device, format);
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor::default());
        TiledSource::new(
            device,
            queue,
            image,
            &display,
            &sampler,
            &sampler,
            &sampler,
            false,
            &blit_pipeline,
            &blit_bgl,
            None,
        )
        .expect("tiled source")
    }

    fn set_viewport(source: &mut TiledSource, frac: f32, physical_scale: f32) {
        source.physical_scale = physical_scale;
        let (fw, fh) = (source.full_width as f32, source.full_height as f32);
        let half_w = fw * frac * 0.5;
        let half_h = fh * frac * 0.5;
        let view = [
            fw * 0.5 - half_w,
            fh * 0.5 - half_h,
            fw * 0.5 + half_w,
            fh * 0.5 + half_h,
        ];
        for t in &mut source.tiles {
            let tl = t.x as f32;
            let tt = t.y as f32;
            let tr = tl + t.width as f32;
            let tb = tt + t.height as f32;
            let isect = [
                view[0].max(tl),
                view[1].max(tt),
                view[2].min(tr),
                view[3].min(tb),
            ];
            t.proc_rect_px = if isect[2] > isect[0] && isect[3] > isect[1] {
                Some(isect)
            } else {
                None
            };
        }
    }

    fn time_chain(
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        modifiers: &[Modifier],
    ) -> Option<Duration> {
        let mut best: Option<Duration> = None;
        for _ in 0..RUNS {
            let mut mp = ModifierPipeline::new(device, TextureFormat::Rgba8Unorm, W, H);
            let t = Instant::now();
            let mut dirty = true;
            let mut converged = false;
            for _ in 0..256 {
                mp.prepare(device, queue, source, modifiers, dirty);
                dirty = false;
                let all_valid = (0..source.tiles.len()).all(|ti| {
                    mp.tile_outputs
                        .get(ti)
                        .and_then(|o| o.as_ref())
                        .is_none_or(|o| o.valid)
                });
                if !mp.reprocess_pending() && all_valid {
                    converged = true;
                    break;
                }
            }
            if !converged {
                return None;
            }
            let ti = (0..source.tiles.len()).find(|&i| {
                mp.tile_outputs
                    .get(i)
                    .and_then(|o| o.as_ref())
                    .is_some_and(|o| o.valid)
            })?;
            let o = mp.tile_outputs[ti].as_ref()?;
            let _ = read_texture(device, queue, &o._tex, o.width.min(64), o.height.min(64));
            let elapsed = t.elapsed();
            best = Some(best.map_or(elapsed, |b: Duration| b.min(elapsed)));
        }
        best
    }

    fn frames_to_converge(
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        modifiers: &[Modifier],
        w: u32,
        h: u32,
    ) -> Option<u32> {
        let mut mp = ModifierPipeline::new(device, TextureFormat::Rgba8Unorm, w, h);
        let mut dirty = true;
        for n in 1..=256u32 {
            mp.prepare(device, queue, source, modifiers, dirty);
            dirty = false;
            let all_valid = (0..source.tiles.len()).all(|ti| {
                mp.tile_outputs
                    .get(ti)
                    .and_then(|o| o.as_ref())
                    .is_none_or(|o| o.valid)
            });
            if !mp.reprocess_pending() && all_valid {
                return Some(n);
            }
        }
        None
    }

    fn m(kind: ModifierKind) -> Modifier {
        Modifier::new(kind)
    }

    fn blur(radius: f32) -> Vec<Modifier> {
        vec![m(ModifierKind::GaussianBlur(GaussianBlur { radius }))]
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_large_images() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench_large_images: no adapter, skipping");
            return;
        };

        let limit = device.limits().max_texture_dimension_2d;
        println!("\nScaling with source size -- visible region held at ~1024x1024");
        println!("max_texture_dimension_2d = {limit}");
        println!("{:-<74}", "");
        println!(
            "  {:<14} {:>7} {:>9} {:>10} {:>10} {:>10}",
            "source", "tiles", "VRAM GB", "pointwise", "blur r=8", "blur r=64"
        );
        println!("{:-<74}", "");

        for dim in [2048u32, 4096, 8192, 16384] {
            let image = ImageData::new(pixels(dim, dim), dim, dim);
            let mut source = make_source(&device, &queue, &image);
            let n_tiles = source.tiles.len();
            let vram = (dim as f64 * dim as f64 * 4.0) / 1e9;

            let frac = (1024.0 / dim as f32).min(1.0);
            set_viewport(&mut source, frac, 1.0);

            let t = |mods: &[Modifier]| match time_chain(&device, &queue, &source, mods) {
                Some(d) => format!("{:.2}", d.as_secs_f64() * 1000.0),
                None => "n/c".to_string(),
            };
            let pw = t(&[m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))]);
            let b8 = t(&blur(8.0));
            let b64 = t(&blur(64.0));

            println!(
                "  {:<14} {:>7} {:>9.2} {:>10} {:>10} {:>10}",
                format!("{dim}x{dim}"),
                n_tiles,
                vram,
                pw,
                b8,
                b64
            );
        }
        println!("{:-<74}", "");
        println!(
            "\nNote: TiledSource uploads every tile to VRAM up front and never\n\
             evicts, so source residency grows with total image size regardless\n\
             of what is on screen.\n"
        );
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_pipeline() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench: no adapter, skipping");
            return;
        };

        let image = ImageData::new(pixels(W, H), W, H);
        let mut source = make_source(&device, &queue, &image);

        let cases: Vec<(&str, Vec<Modifier>)> = vec![
            (
                "pointwise x1",
                vec![m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))],
            ),
            ("blur r=8", blur(8.0)),
            ("blur r=32", blur(32.0)),
            ("blur r=64", blur(64.0)),
            ("blur r=128", blur(128.0)),
            ("blur r=200 (ks=0.5)", blur(200.0)),
            ("blur r=500 (ks=0.25)", blur(500.0)),
            (
                "motion blur d=64",
                vec![m(ModifierKind::MotionBlur(MotionBlur {
                    angle: 30.0,
                    distance: 64.0,
                }))],
            ),
            (
                "chromatic aberration",
                vec![m(ModifierKind::ChromaticAberration(ChromaticAberration {
                    amount: 20.0,
                }))],
            ),
        ];

        let views: [(&str, f32, f32); 3] = [
            ("fit (zoomed out)", 1.0, 0.25),
            ("100% zoom", 0.25, 1.0),
            ("400% zoom", 0.0625, 4.0),
        ];

        println!("\nGPU pipeline baseline -- {W}x{H}, best of {RUNS}");
        println!("(time to converge a full render, including readback sync)");

        for (view_label, frac, phys) in views {
            set_viewport(&mut source, frac, phys);
            println!("\n  {view_label}  (viewport={frac:.4} of image, physical_scale={phys})");
            println!("  {:-<46}", "");
            println!("  {:<28} {:>14}", "chain", "ms");
            println!("  {:-<46}", "");
            for (label, modifiers) in &cases {
                match time_chain(&device, &queue, &source, modifiers) {
                    Some(d) => println!("  {:<28} {:>14.2}", label, d.as_secs_f64() * 1000.0),
                    None => println!("  {:<28} {:>14}", label, "did not converge"),
                }
            }
            println!("  {:-<46}", "");
        }
        println!();
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_crop_stage() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench_crop_headroom: no adapter, skipping");
            return;
        };

        use crate::modifiers::kinds::Crop;

        const KEEP: [f32; 3] = [1.0, 0.5, 0.25];

        let cropped = |chain: &[Modifier], keep: f32| -> Vec<Modifier> {
            let mut v = vec![m(ModifierKind::Crop(Crop {
                x: 0.0,
                y: 0.0,
                width: (W as f32 * keep).max(1.0),
                height: (H as f32 * keep).max(1.0),
            }))];
            v.extend(chain.iter().cloned());
            v
        };

        let cases: Vec<(&str, Vec<Modifier>)> = vec![
            (
                "pointwise x1",
                vec![m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))],
            ),
            ("blur r=8", blur(8.0)),
            ("blur r=32", blur(32.0)),
            ("blur r=128", blur(128.0)),
            (
                "chromatic aberration",
                vec![m(ModifierKind::ChromaticAberration(ChromaticAberration {
                    amount: 20.0,
                }))],
            ),
        ];

        let full_image = ImageData::new(pixels(W, H), W, H);
        let mut full_source = make_source(&device, &queue, &full_image);
        set_viewport(&mut full_source, 1.0, 1.0);

        println!(
            "
Crop as a chain stage -- measured saving, best of {RUNS}, 100% zoom"
        );
        println!("  full frame is {W}x{H}");
        println!("  {:-<78}", "");
        println!(
            "  {:<22} {:>9} {:>15} {:>15} {:>12}",
            "chain", "no crop", "crop 50%", "crop 25%", "ceiling 25%"
        );
        println!("  {:-<78}", "");

        for (label, modifiers) in &cases {
            let base = time_chain(&device, &queue, &full_source, modifiers)
                .map(|d| d.as_secs_f64() * 1000.0);

            let mut cells: Vec<String> = Vec::new();
            for keep in KEEP.iter().skip(1) {
                let chain = cropped(modifiers, *keep);
                let ms = time_chain(&device, &queue, &full_source, &chain)
                    .map(|d| d.as_secs_f64() * 1000.0);
                cells.push(match (ms, base) {
                    (Some(v), Some(b)) => format!("{v:.2} ({:.1}x)", b / v),
                    (Some(v), None) => format!("{v:.2}"),
                    (None, _) => "n/c".into(),
                });
            }

            let keep = KEEP[KEEP.len() - 1];
            let (cw, ch) = (
                ((W as f32 * keep) as u32).max(1),
                ((H as f32 * keep) as u32).max(1),
            );
            let small = ImageData::new(pixels(cw, ch), cw, ch);
            let mut small_source = make_source(&device, &queue, &small);
            set_viewport(&mut small_source, 1.0, 1.0);
            let ceiling = time_chain(&device, &queue, &small_source, modifiers)
                .map(|d| d.as_secs_f64() * 1000.0);

            println!(
                "  {:<22} {:>9} {:>15} {:>15} {:>12}",
                label,
                base.map_or("n/c".into(), |v| format!("{v:.2}")),
                cells[0],
                cells[1],
                ceiling.map_or("n/c".into(), |v| format!("{v:.2}")),
            );
        }
        println!("  {:-<78}", "");
        println!("  crop N% keeps N% of each axis, so 50% is a quarter of the pixels");
        println!("  ceiling = same chain on an already-small source: the best a crop could do");
        println!();
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_resize() {
        use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench_resize: no adapter, skipping");
            return;
        };

        let image = ImageData::new(pixels(W, H), W, H);
        let mut source = make_source(&device, &queue, &image);

        let resize = |pct: f32, filter: ResizeFilter| -> Vec<Modifier> {
            vec![m(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: pct,
                height: pct,
                filter,
                lock_aspect: true,
            }))]
        };

        let cases: Vec<(&str, Vec<Modifier>)> = vec![
            ("resize 50% lanczos", resize(50.0, ResizeFilter::Lanczos)),
            ("resize 200% nearest", resize(200.0, ResizeFilter::Nearest)),
            (
                "resize 200% bilinear",
                resize(200.0, ResizeFilter::Bilinear),
            ),
            ("resize 200% lanczos", resize(200.0, ResizeFilter::Lanczos)),
            ("resize 400% lanczos", resize(400.0, ResizeFilter::Lanczos)),
            (
                "resize 200% + blur r=8",
                vec![
                    m(ModifierKind::Resize(Resize {
                        mode: ResizeMode::Percent,
                        width: 200.0,
                        height: 200.0,
                        filter: ResizeFilter::Lanczos,
                        lock_aspect: true,
                    })),
                    m(ModifierKind::GaussianBlur(GaussianBlur { radius: 8.0 })),
                ],
            ),
        ];

        let views: [(&str, f32, f32); 4] = [
            ("100% zoom", 0.25, 1.0),
            ("fit after 2x (~0.5)", 1.0, 0.5),
            ("zoomed out (0.25)", 1.0, 0.25),
            ("far out (0.1)", 1.0, 0.1),
        ];

        println!("\nGPU resize baseline -- {W}x{H} source, best of {RUNS}");
        println!("(the floor means an upscale's cost no longer falls as you zoom out)");

        for (view_label, frac, phys) in views {
            set_viewport(&mut source, frac, phys);
            println!("\n  {view_label}  (physical_scale={phys})");
            println!("  {:-<46}", "");
            println!("  {:<28} {:>14}", "chain", "ms");
            println!("  {:-<46}", "");
            for (label, modifiers) in &cases {
                match time_chain(&device, &queue, &source, modifiers) {
                    Some(d) => println!("  {:<28} {:>14.2}", label, d.as_secs_f64() * 1000.0),
                    None => println!("  {:<28} {:>14}", label, "did not converge"),
                }
            }
            println!("  {:-<46}", "");
        }
        println!();
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_small_image_resize() {
        use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench_small_image_resize: no adapter, skipping");
            return;
        };

        const SW: u32 = 1000;
        const SH: u32 = 1000;
        let image = ImageData::new(pixels(SW, SH), SW, SH);
        let mut source = make_source(&device, &queue, &image);

        let resize = |pct: f32| -> Vec<Modifier> {
            vec![m(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: pct,
                height: pct,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            }))]
        };

        let cases: Vec<(&str, Vec<Modifier>)> = vec![
            ("none", vec![]),
            (
                "exposure only",
                vec![m(ModifierKind::Exposure(Exposure { exposure: 0.3 }))],
            ),
            ("resize 100% (identity)", resize(100.0)),
            ("resize 50%", resize(50.0)),
            ("resize 200%", resize(200.0)),
            ("resize 400%", resize(400.0)),
            ("resize 800%", resize(800.0)),
        ];

        println!(
            "
GPU small-image baseline -- {SW}x{SH} source, best of {RUNS}"
        );
        println!("(frames = prepare() calls before the pipeline stops asking for more)");

        for (view_label, frac, phys) in [("100% zoom", 1.0, 1.0), ("fit (0.5)", 1.0, 0.5)] {
            set_viewport(&mut source, frac, phys);
            println!(
                "
  {view_label}  (physical_scale={phys})"
            );
            println!("  {:-<52}", "");
            println!("  {:<28} {:>9} {:>10}", "chain", "ms", "frames");
            println!("  {:-<52}", "");
            for (label, modifiers) in &cases {
                let ms = match time_chain(&device, &queue, &source, modifiers) {
                    Some(d) => format!("{:.2}", d.as_secs_f64() * 1000.0),
                    None => "n/c".to_string(),
                };
                let fr = match frames_to_converge(&device, &queue, &source, modifiers, SW, SH) {
                    Some(n) => n.to_string(),
                    None => ">256".to_string(),
                };
                println!("  {:<28} {:>9} {:>10}", label, ms, fr);
            }
            println!("  {:-<52}", "");
        }
        println!();
    }

    #[test]
    #[ignore = "GPU timing baseline; run with --release --ignored --nocapture"]
    fn gpu_bench_resize_slider_drag() {
        use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_bench_resize_slider_drag: no adapter, skipping");
            return;
        };

        for (sw, sh) in [(1000u32, 1000u32), (2000, 2000)] {
            let image = ImageData::new(pixels(sw, sh), sw, sh);
            let mut source = make_source(&device, &queue, &image);
            set_viewport(&mut source, 1.0, 0.5);

            println!(
                "
  slider drag -- {sw}x{sh} source"
            );
            println!("  {:-<58}", "");
            println!(
                "  {:<20} {:>12} {:>10} {:>10}",
                "range", "total ms", "ticks", "ms/tick"
            );
            println!("  {:-<58}", "");

            for (label, lo, hi, refit) in [
                ("100->150 no refit", 100.0f32, 150.0f32, false),
                ("100->150 refit", 100.0, 150.0, true),
                ("150->300 no refit", 150.0, 300.0, false),
                ("150->300 refit", 150.0, 300.0, true),
                ("300->400 no refit", 300.0, 400.0, false),
                ("300->400 refit", 300.0, 400.0, true),
            ] {
                let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, sw, sh);
                let ticks = 20u32;
                let t = Instant::now();
                for i in 0..ticks {
                    let pct = lo + (hi - lo) * (i as f32 / (ticks - 1) as f32);
                    let chain = vec![m(ModifierKind::Resize(Resize {
                        mode: ResizeMode::Percent,
                        width: pct,
                        height: pct,
                        filter: ResizeFilter::Lanczos,
                        lock_aspect: true,
                    }))];
                    if refit {
                        source.physical_scale = (1000.0 / (sw as f32 * pct / 100.0)).min(1.0);
                    }
                    mp.prepare(&device, &queue, &source, &chain, true);
                }
                let _ = device.poll(iced::wgpu::PollType::wait_indefinitely());
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                println!(
                    "  {:<20} {:>12.2} {:>10} {:>10.2}",
                    label,
                    ms,
                    ticks,
                    ms / ticks as f64
                );
            }
            println!("  {:-<58}", "");
        }
        println!();
    }
}
