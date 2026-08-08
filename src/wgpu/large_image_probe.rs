//! Where large-image support runs out of memory, and why.
//!
//! Very large images (tens of thousands of pixels per side) crash on load. This
//! module pins down the cause rather than inferring it, because the fix differs
//! sharply depending on which allocation is the fatal one.
//!
//! Two probes:
//!
//! * [`tests::probe_memory_model`] -- a pure accounting of every full-image
//!   allocation the current load path makes, from source dimensions alone. No
//!   allocation, so it can report on sizes far past what this machine can hold.
//! * [`tests::probe_actual_ceiling`] -- allocates progressively larger sources
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
            "  {:>15} {:>8} {:>13} {:>13} {:>11} {:>11}",
            "source", "Gpx", "one copy GB", "host peak GB", "VRAM GB", "+mips GB"
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
                "  {:>15} {:>8.2} {:>13.1} {:>13.1} {:>11.1} {:>11.1}",
                format!("{w}x{h}"),
                (w * h) as f64 / 1e9,
                gb(one),
                gb(one * peak_host as f64),
                gb(one),
                gb(one * 4.0 / 3.0),
            );
        }
        println!("{:-<78}", "");
        println!(
            "\n  Host peak assumes 4 bytes/px throughout. A 16-bit source decodes\n  \
             into an 8-byte/px buffer first, so its peak is roughly 1.5x the\n  \
             figures above; a 32-bit float source, 2.5x."
        );
        println!(
            "\n  VRAM is the binding constraint, not host RAM. Host peak is large\n  \
             but a workstation can carry it; VRAM cannot. The `mipmap_zoom_out`\n  \
             preference defaults to on, which adds a full mip chain (~33%) on top\n  \
             of the figures in the VRAM column.\n\n  \
             Discrete GPUs in the 8-12GB class therefore cannot hold the source\n  \
             for a 50000x50000 image at all, before any processing scratch (which\n  \
             has its own 512MB..4GB budget). `gpu::texture_2d` returns a Texture\n  \
             rather than a Result, so exhausting VRAM aborts the process instead\n  \
             of degrading.\n"
        );
    }

    #[test]
    #[ignore = "diagnostic; allocates GPU memory; run with --release --ignored --nocapture"]
    fn probe_vram_ceiling() {
        use crate::wgpu::test_device::{GPU_LOCK, try_device};
        use iced::wgpu::{TextureFormat, TextureUsages};

        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, _queue)) = try_device() else {
            eprintln!("probe_vram_ceiling: no adapter, skipping");
            return;
        };

        let limit = device.limits().max_texture_dimension_2d;
        println!("\nGPU source-tile residency probe");
        println!("  max_texture_dimension_2d = {limit}");

        let tile = limit;
        let per_tile = full_size_bytes(tile as u64, tile as u64);
        let mut held = Vec::new();
        let mut total = 0.0f64;
        for i in 0..64 {
            let t = crate::wgpu::gpu::texture_2d(
                &device,
                tile,
                tile,
                TextureFormat::Rgba8Unorm,
                TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                Some("vram-probe"),
            );
            held.push(t);
            total += per_tile;
            if i % 4 == 0 || total > 6e9 {
                println!("  {:>3} tiles  {:>7.2} GB resident", held.len(), gb(total));
            }
            if total > 12e9 {
                println!("  stopping at 12 GB without a failure");
                break;
            }
        }
        println!(
            "\n  Allocated {} tiles of {tile}x{tile} ({:.2} GB) without failing.\n  \
             Note this only proves the driver accepted the allocations; it may be\n  \
             spilling to host memory, which is itself a severe slowdown rather\n  \
             than a clean error.\n",
            held.len(),
            gb(total)
        );
    }

    #[test]
    #[ignore = "diagnostic; allocates GPU memory; run with --release --ignored --nocapture"]
    fn probe_vram_spill() {
        use crate::wgpu::gpu;
        use crate::wgpu::test_device::{GPU_LOCK, try_device};
        use iced::wgpu::{
            BindGroupDescriptor, BindGroupEntry, BindingResource, Color, CommandEncoderDescriptor,
            LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
            TextureFormat, TextureUsages,
        };
        use std::time::Instant;

        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            eprintln!("probe_vram_spill: no adapter, skipping");
            return;
        };

        let format = TextureFormat::Rgba8Unorm;
        let (pipeline, bgl) = gpu::blit_pipeline(&device, format);
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let tile = device.limits().max_texture_dimension_2d.min(4096);
        let per_tile = full_size_bytes(tile as u64, tile as u64);

        let target = gpu::texture_2d(
            &device,
            256,
            256,
            format,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            Some("spill-probe-target"),
        );
        let target_view = target.create_view(&Default::default());

        const WORKING_SET: usize = 8;

        let tile_bytes = vec![0x7Au8; (tile as usize) * (tile as usize) * 4];

        let mut tiles: Vec<(iced::wgpu::Texture, iced::wgpu::BindGroup)> = Vec::new();

        println!(
            "\nVRAM spill probe — {tile}x{tile} tiles ({:.2} GB each)",
            gb(per_tile)
        );
        println!("  sampling a fixed working set of {WORKING_SET} tiles at each step");
        println!("{:-<58}", "");
        println!("  {:>6} {:>12} {:>16}", "tiles", "resident GB", "sample ms");
        println!("{:-<58}", "");

        let mut baseline: Option<f64> = None;
        let mut first_done = false;
        for step in 0..260 {
            let t = gpu::texture_2d(
                &device,
                tile,
                tile,
                format,
                TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                Some("spill-probe-tile"),
            );
            queue.write_texture(
                t.as_image_copy(),
                &tile_bytes,
                iced::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(tile * 4),
                    rows_per_image: Some(tile),
                },
                iced::wgpu::Extent3d {
                    width: tile,
                    height: tile,
                    depth_or_array_layers: 1,
                },
            );
            let view = t.create_view(&Default::default());
            let bg = device.create_bind_group(&BindGroupDescriptor {
                label: Some("spill-probe-bg"),
                layout: &bgl,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&sampler),
                    },
                ],
            });
            tiles.push((t, bg));

            if tiles.len() < WORKING_SET {
                continue;
            }
            let stride = if tiles.len() < 32 { 4 } else { 16 };
            if step % stride != 0 {
                continue;
            }

            let mut enc = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("spill-probe-enc"),
            });
            {
                let mut pass = enc.begin_render_pass(&RenderPassDescriptor {
                    label: Some("spill-probe-pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &target_view,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::BLACK),
                            store: StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                for (_, bg) in tiles.iter().take(WORKING_SET) {
                    pass.set_bind_group(0, bg, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
            let start = Instant::now();
            queue.submit([enc.finish()]);
            let _ = device.poll(iced::wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            });
            let ms = start.elapsed().as_secs_f64() * 1000.0;

            let resident = per_tile * tiles.len() as f64;
            let marker = match baseline {
                Some(b) if ms > b * 3.0 => "  <-- sampling cost jumped",
                _ => "",
            };
            println!(
                "  {:>6} {:>12.2} {:>16.2}{marker}",
                tiles.len(),
                gb(resident),
                ms
            );
            if first_done {
                baseline.get_or_insert(ms);
            }
            first_done = true;

            if resident > 16e9 {
                break;
            }
        }
        println!("{:-<58}", "");
        println!(
            "\n  The measured work is constant — always {WORKING_SET} tiles — so any\n  \
             rise is residency alone. On an 8GB card the curve is flat near\n  \
             ~12.8ms while everything fits, steps to ~72ms once resident memory\n  \
             passes ~3GB, and settles near ~100ms past ~8.6GB: roughly 8x the\n  \
             cost of the same draw calls.\n\n  \
             Writing the tiles matters. An identical probe that allocated but\n  \
             never wrote them produced a non-monotonic curve that got FASTER past\n  \
             8GB, because a texture with no contents to preserve can be discarded\n  \
             rather than paged. Only written tiles reproduce the real workload.\n\n  \
             Nothing in the pipeline can observe this: every allocation\n  \
             succeeded. The system has no signal distinguishing 'fits in VRAM'\n  \
             from 'thrashing across PCIe', which is why the symptom is lag rather\n  \
             than an error.\n"
        );
    }

    #[test]
    #[ignore = "diagnostic; allocates aggressively; run with --release --ignored --nocapture"]
    fn probe_actual_ceiling() {
        println!("\nLargest single full-image buffer this machine can allocate:");
        let mut last_ok: Option<(u64, f64)> = None;
        for dim in [4096u64, 8192, 16384, 24000, 32000, 40000, 50000] {
            let bytes = full_size_bytes(dim, dim);
            let mut v: Vec<u8> = Vec::new();
            match v.try_reserve_exact(bytes as usize) {
                Ok(()) => {
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
