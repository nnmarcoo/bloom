use super::*;
use crate::modifiers::pixel_sort::SortMode as ExecSortMode;
use crate::modifiers::roi::{self, RegionPx, StepClass};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

const MAX_KERNEL_RADIUS_PX: f32 = 128.0;

struct Stage {
    tex: Texture,
    view: TextureView,
    rect: RegionPx,
}

fn snap_region(r: RegionPx, pitch: f32, w: f32, h: f32) -> RegionPx {
    roi::clamp_region(
        [
            (r[0] / pitch).floor() * pitch,
            (r[1] / pitch).floor() * pitch,
            (r[2] / pitch).ceil() * pitch,
            (r[3] / pitch).ceil() * pitch,
        ],
        w,
        h,
    )
}

fn rect_dims(r: RegionPx, scale: f32) -> (u32, u32) {
    (
        ((((r[2] - r[0]) * scale).round()) as u32).max(1),
        ((((r[3] - r[1]) * scale).round()) as u32).max(1),
    )
}

fn scale_rect(r: RegionPx, scale: f32) -> [u32; 4] {
    let x0 = (r[0] * scale).round() as u32;
    let y0 = (r[1] * scale).round() as u32;
    [
        x0,
        y0,
        ((r[2] * scale).round() as u32).max(x0 + 1),
        ((r[3] * scale).round() as u32).max(y0 + 1),
    ]
}

fn uv_of(r: RegionPx, full_w: f32, full_h: f32) -> UvRect {
    UvRect {
        origin: [r[0] / full_w, r[1] / full_h],
        size: [(r[2] - r[0]) / full_w, (r[3] - r[1]) / full_h],
    }
}

#[allow(clippy::too_many_arguments)]
fn pr_with_roi(
    tile: &crate::wgpu::tiled_source::Tile,
    full_w: f32,
    full_h: f32,
    scale: f32,
    downscale: bool,
    roi: RegionPx,
    pitch: f32,
) -> ProcRect {
    let tl = tile.x as f32;
    let tt = tile.y as f32;
    let fw = tl + tile.width as f32;
    let fh = tt + tile.height as f32;
    let margin = if downscale { 0.0 } else { ROI_MARGIN_PX };
    let s = snap_region(
        [
            roi[0] - margin,
            roi[1] - margin,
            roi[2] + margin,
            roi[3] + margin,
        ],
        pitch,
        full_w,
        full_h,
    );
    let px = [s[0].max(tl), s[1].max(tt), s[2].min(fw), s[3].min(fh)];
    let w = (((px[2] - px[0]).max(1.0) * scale).round() as u32).max(1);
    let h = (((px[3] - px[1]).max(1.0) * scale).round() as u32).max(1);
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

fn full_tile_info(source: &TiledSource) -> TileInfo {
    TileInfo {
        tile_x: 0,
        tile_y: 0,
        tile_w: source.full_width,
        tile_h: source.full_height,
        full_w: source.full_width,
        full_h: source.full_height,
    }
}

impl ModifierPipeline {
    pub(super) fn execute_pointwise(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        seg: &[&Modifier],
        proc_scale: f32,
        downscale: bool,
    ) {
        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;
        let cur_scale = if downscale { proc_scale } else { 1.0 };

        let mut encoder: Option<CommandEncoder> = None;
        let mut pool_used = 0usize;
        let mut scheduler = Scheduler::new();

        for ti in 0..source.tiles.len() {
            let tile = &source.tiles[ti];

            if tile_ndc_culled(tile.last_ndc_rect) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }

            let visible_roi = tile.proc_rect_px;
            let reuse = match (self.tile_outputs[ti].as_ref(), visible_roi) {
                (Some(o), Some(roi)) => {
                    o.proc_px.is_some_and(|p| rect_contains(p, roi))
                        && (o.proc_scale - cur_scale).abs() < 1e-4
                }
                (Some(o), None) => o.proc_px.is_none() && (o.proc_scale - cur_scale).abs() < 1e-4,
                _ => false,
            };

            let pr = if reuse {
                let o = self.tile_outputs[ti].as_ref().unwrap();
                proc_rect_from_px(o.proc_px, tile, full_w, full_h, o.width, o.height)
            } else {
                tile_proc_rect(tile, full_w, full_h, proc_scale, downscale, 0.0, true)
            };

            if !reuse {
                let tex = gpu::texture_2d(
                    device,
                    pr.w,
                    pr.h,
                    self.format,
                    TextureUsages::RENDER_ATTACHMENT
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC
                        | TextureUsages::COPY_DST,
                    Some(&format!("modifier-tile{ti}-output")),
                );
                let view = tex.create_view(&Default::default());
                self.tile_outputs[ti] = Some(TileOutput {
                    _tex: tex,
                    view,
                    valid: false,
                    width: pr.w,
                    height: pr.h,
                    proc_px: Some(pr.px),
                    proc_scale: cur_scale,
                });
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }

            let needs_reprocess = !self.tile_outputs[ti].as_ref().unwrap().valid;
            let roi_active = tile.isec_px.is_some();
            let needs_bg_rebuild =
                self.tile_display_bgs_linear[ti].is_none() || needs_reprocess || roi_active;

            if !needs_bg_rebuild {
                continue;
            }

            if needs_reprocess && !scheduler.admit() {
                continue;
            }

            if needs_reprocess {
                let tile_info = TileInfo {
                    tile_x: tile.x,
                    tile_y: tile.y,
                    tile_w: tile.width,
                    tile_h: tile.height,
                    full_w: source.full_width,
                    full_h: source.full_height,
                };
                let uniforms = build_segment_uniforms(seg, &tile_info, pr.proc, pr.src);
                if pool_used == self.uniform_pool.len() {
                    self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                        device,
                        Some("combined-modifiers-uniform"),
                    ));
                }
                let buffer = &self.uniform_pool[pool_used];
                pool_used += 1;
                gpu::write_uniform(queue, buffer, &uniforms);
                let bg = gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    &tile.source_view,
                    &self.trilinear_sampler,
                    Some("combined-modifiers-bg"),
                );
                let enc = encoder.get_or_insert_with(|| {
                    device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                        label: Some("pointwise-executor-encoder"),
                    })
                });
                let out = self.tile_outputs[ti].as_ref().unwrap().view.clone();
                self.combined.run(enc, &bg, &out);
                self.tile_outputs[ti].as_mut().unwrap().valid = true;
            }

            self.build_roi_display_bgs(device, queue, ti, tile, &pr, true);
        }

        self.reprocess_pending |= scheduler.pending();

        if let Some(encoder) = encoder {
            queue.submit([encoder.finish()]);
        }
    }

    pub(super) fn execute_kernel_chain(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        plan: &[PlanItem],
        proc_scale: f32,
        downscale: bool,
    ) {
        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let n_tiles = source.tiles.len();
        let mut visible: Vec<usize> = Vec::new();
        for ti in 0..n_tiles {
            if tile_ndc_culled(source.tiles[ti].last_ndc_rect) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            } else {
                visible.push(ti);
            }
        }
        if visible.is_empty() {
            return;
        }

        let mut u_disp: RegionPx = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut any_roi = false;
        for &ti in &visible {
            if let Some(r) = source.tiles[ti].proc_rect_px {
                any_roi = true;
                u_disp = [
                    u_disp[0].min(r[0]),
                    u_disp[1].min(r[1]),
                    u_disp[2].max(r[2]),
                    u_disp[3].max(r[3]),
                ];
            }
        }
        if !any_roi {
            for &ti in &visible {
                let t = &source.tiles[ti];
                u_disp = [
                    u_disp[0].min(t.x as f32),
                    u_disp[1].min(t.y as f32),
                    u_disp[2].max((t.x + t.width) as f32),
                    u_disp[3].max((t.y + t.height) as f32),
                ];
            }
        }

        let classes: Vec<StepClass> = plan
            .iter()
            .map(|p| match p {
                PlanItem::Fused(_) => StepClass::Pointwise,
                PlanItem::Step(_, m) => roi::step_class(&m.kind),
            })
            .collect();

        let limit_dim = device.limits().max_texture_dimension_2d;
        let mut fit_scale = if downscale { proc_scale } else { 1.0 };
        loop {
            let pitch = 1.0 / fit_scale;
            let fits = |r: RegionPx| -> bool {
                let (w, h) = rect_dims(r, fit_scale);
                w <= limit_dim && h <= limit_dim
            };
            let mut cur = snap_region(roi::dilate(u_disp, ROI_MARGIN_PX), pitch, full_w, full_h);
            let mut ok = fits(cur);
            for k in (0..classes.len()).rev() {
                cur = snap_region(
                    roi::input_needed(classes[k], cur, full_w, full_h),
                    pitch,
                    full_w,
                    full_h,
                );
                ok &= fits(cur);
                if matches!(classes[k], StepClass::Scanline { .. }) {
                    let (w, h) = rect_dims(cur, fit_scale);
                    let bytes = ((w * 4).div_ceil(256) * 256) as u64 * h as u64;
                    ok &= bytes * 2 <= sort_buffer_limit(device);
                }
                if !ok {
                    break;
                }
            }
            if ok || fit_scale <= 1.0 / 4096.0 {
                break;
            }
            fit_scale *= 0.5;
        }
        let scale = fit_scale;
        let downscale = scale < 1.0;
        let pitch = 1.0 / scale;

        let mut procs: Vec<usize> = Vec::new();
        let mut prs: Vec<Option<ProcRect>> = (0..n_tiles).map(|_| None).collect();
        for &ti in &visible {
            let tile = &source.tiles[ti];
            let roi = tile.proc_rect_px.unwrap_or_else(|| {
                let g = roi::dilate(u_disp, ROI_MARGIN_PX);
                [
                    g[0].max(tile.x as f32),
                    g[1].max(tile.y as f32),
                    g[2].min((tile.x + tile.width) as f32),
                    g[3].min((tile.y + tile.height) as f32),
                ]
            });
            if roi[2] <= roi[0] || roi[3] <= roi[1] {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }
            let reuse = self.tile_outputs[ti].as_ref().is_some_and(|o| {
                o.proc_px.is_some_and(|p| rect_contains(p, roi))
                    && (o.proc_scale - scale).abs() < 1e-4
            });
            let pr = if reuse {
                let o = self.tile_outputs[ti].as_ref().unwrap();
                proc_rect_from_px(o.proc_px, tile, full_w, full_h, o.width, o.height)
            } else {
                pr_with_roi(tile, full_w, full_h, scale, downscale, roi, pitch)
            };
            if !reuse {
                let tex = gpu::texture_2d(
                    device,
                    pr.w,
                    pr.h,
                    self.format,
                    TextureUsages::RENDER_ATTACHMENT
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC
                        | TextureUsages::COPY_DST,
                    Some(&format!("modifier-tile{ti}-output")),
                );
                let view = tex.create_view(&Default::default());
                self.tile_outputs[ti] = Some(TileOutput {
                    _tex: tex,
                    view,
                    valid: false,
                    width: pr.w,
                    height: pr.h,
                    proc_px: Some(pr.px),
                    proc_scale: scale,
                });
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }
            prs[ti] = Some(pr);
            procs.push(ti);
        }
        if procs.is_empty() {
            return;
        }

        let mut u_px: RegionPx = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for &ti in &procs {
            let p = prs[ti].as_ref().unwrap().px;
            u_px = [
                u_px[0].min(p[0]),
                u_px[1].min(p[1]),
                u_px[2].max(p[2]),
                u_px[3].max(p[3]),
            ];
        }
        let u_px = snap_region(u_px, pitch, full_w, full_h);

        let single_band = classes
            .iter()
            .any(|c| matches!(c, StepClass::Scanline { dir } if *dir != (1, 0)));

        fn mix(sig: u64, v: u64) -> u64 {
            (sig ^ v).wrapping_mul(0x100000001b3)
        }
        let mut sig = 0xcbf29ce484222325u64;
        for v in u_px {
            sig = mix(sig, v.to_bits() as u64);
        }
        sig = mix(sig, scale.to_bits() as u64);
        sig = mix(sig, plan.len() as u64);
        let mut ph = DefaultHasher::new();
        for p in plan {
            match p {
                PlanItem::Fused(seg) => {
                    for m in seg {
                        m.kind.hash_into(&mut ph);
                    }
                }
                PlanItem::Step(_, m) => m.kind.hash_into(&mut ph),
            }
        }
        sig = mix(sig, ph.finish());
        if sig != self.exec_sig {
            self.exec_sig = sig;
            self.exec_band_cursor = 0;
            for &ti in &procs {
                if let Some(o) = self.tile_outputs[ti].as_mut() {
                    o.valid = false;
                }
            }
        }

        let any_stale = procs
            .iter()
            .any(|&ti| !self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid));

        if any_stale {
            let (su_w, su_h) = rect_dims(u_px, scale);
            let max_r_px = classes
                .iter()
                .filter_map(|c| match c {
                    StepClass::Kernel { apron_px, .. } => {
                        Some((apron_px * scale).min(MAX_KERNEL_RADIUS_PX))
                    }
                    _ => None,
                })
                .fold(0.0f32, f32::max);
            let taps = (4.0 * max_r_px) as u32 + 2;
            let band_h = if single_band {
                su_h.max(1)
            } else {
                let budget_band = (BLUR_WORK_BUDGET / (su_w.max(1) * taps.max(1)))
                    .clamp(BLUR_MIN_BAND_H, BLUR_MAX_BAND_H);
                let frame_band = su_h.div_ceil(MAX_BLUR_FRAMES);
                budget_band.max(frame_band)
            };
            if self.exec_band_cursor >= su_h {
                self.exec_band_cursor = 0;
            }
            let by0 = self.exec_band_cursor.min(su_h);
            let by1 = (by0 + band_h).min(su_h);

            let band_img: RegionPx = [
                u_px[0],
                if by0 == 0 {
                    u_px[1]
                } else {
                    u_px[1] + by0 as f32 / scale
                },
                u_px[2],
                if by1 == su_h {
                    u_px[3]
                } else {
                    u_px[1] + by1 as f32 / scale
                },
            ];

            let n = plan.len();
            let mut out_rects = vec![[0.0f32; 4]; n];
            let mut cur = roi::clamp_region(band_img, full_w, full_h);
            for k in (0..n).rev() {
                if matches!(classes[k], StepClass::Scanline { .. }) {
                    cur = snap_region(
                        roi::input_needed(classes[k], cur, full_w, full_h),
                        pitch,
                        full_w,
                        full_h,
                    );
                    out_rects[k] = cur;
                } else {
                    out_rects[k] = cur;
                    cur = snap_region(
                        roi::input_needed(classes[k], cur, full_w, full_h),
                        pitch,
                        full_w,
                        full_h,
                    );
                }
            }
            let src_rect = cur;

            let mut encoder =
                device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                    label: Some("kernel-chain-executor"),
                });
            let mut pool_used = 0usize;
            let mut blur_pool_used = 0usize;
            let mut sort_pool_used = 0usize;
            let mut diag_pool_used = 0usize;
            let mut ca_pool_used = 0usize;
            let mut mb_pool_used = 0usize;
            let mut text_pool_used = 0usize;
            let mut drawing_pool_used = 0usize;
            let mut slab_slot = 0usize;

            let mut prev = self.gather_render(
                device,
                queue,
                &mut encoder,
                source,
                src_rect,
                scale,
                &mut pool_used,
                &mut slab_slot,
            );

            for (k, item) in plan.iter().enumerate() {
                let out_r = out_rects[k];
                match item {
                    PlanItem::Fused(seg) => {
                        let (w, h) = rect_dims(out_r, scale);
                        let stage = self.pooled_stage(device, &mut slab_slot, w, h, out_r);
                        let uniforms = build_segment_uniforms(
                            seg,
                            &full_tile_info(source),
                            uv_of(out_r, full_w, full_h),
                            uv_of(prev.rect, full_w, full_h),
                        );
                        if pool_used == self.uniform_pool.len() {
                            self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                                device,
                                Some("combined-modifiers-uniform"),
                            ));
                        }
                        let buffer = &self.uniform_pool[pool_used];
                        pool_used += 1;
                        gpu::write_uniform(queue, buffer, &uniforms);
                        let bg = gpu::standard_bind_group(
                            device,
                            &self.combined.bgl,
                            buffer,
                            &prev.view,
                            &self.trilinear_sampler,
                            Some("combined-modifiers-bg"),
                        );
                        self.combined.run(&mut encoder, &bg, &stage.view);
                        prev = stage;
                    }
                    PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline() => {
                        let ModifierKind::PixelSort(ps) = &m.kind else {
                            unreachable!("scanline class is only PixelSort")
                        };
                        let (sw, sh) = rect_dims(out_r, scale);
                        let row_bytes = (sw * 4).div_ceil(256) * 256;
                        let bytes = row_bytes as u64 * sh as u64;
                        if self
                            .sort_buffers
                            .as_ref()
                            .is_none_or(|(s, _)| s.size() < bytes)
                        {
                            self.sort_buffers = Some((
                                gpu::storage_buffer(device, bytes, Some("exec-sort-src")),
                                gpu::storage_buffer(device, bytes, Some("exec-sort-dst")),
                            ));
                        }
                        let outs = self.pooled_stage(device, &mut slab_slot, sw, sh, out_r);
                        match ExecSortMode::from_angle(ps.angle) {
                            ExecSortMode::Diagonal { .. } => {
                                while self.pixel_sort_diag_uniform_pool.len() <= diag_pool_used {
                                    self.pixel_sort_diag_uniform_pool
                                        .push(self.pixel_sort.diag_uniform_buffer(device));
                                }
                            }
                            ExecSortMode::Cardinal(_) => {
                                while self.pixel_sort_uniform_pool.len() <= sort_pool_used {
                                    self.pixel_sort_uniform_pool
                                        .push(self.pixel_sort.uniform_buffer(device));
                                }
                            }
                        }
                        let (src_buf, dst_buf) = self.sort_buffers.as_ref().unwrap();
                        let layout = iced::wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(row_bytes),
                            rows_per_image: Some(sh),
                        };
                        let extent = iced::wgpu::Extent3d {
                            width: sw,
                            height: sh,
                            depth_or_array_layers: 1,
                        };
                        encoder.copy_texture_to_buffer(
                            tex_copy_info(&prev.tex, iced::wgpu::Origin3d::ZERO),
                            iced::wgpu::TexelCopyBufferInfo {
                                buffer: src_buf,
                                layout,
                            },
                            extent,
                        );
                        match ExecSortMode::from_angle(ps.angle) {
                            ExecSortMode::Diagonal { dx, dy } => {
                                let u = &self.pixel_sort_diag_uniform_pool[diag_pool_used];
                                diag_pool_used += 1;
                                self.pixel_sort.record_diagonal(
                                    device,
                                    queue,
                                    &mut encoder,
                                    u,
                                    src_buf,
                                    dst_buf,
                                    sw,
                                    sh,
                                    row_bytes / 4,
                                    ps.threshold,
                                    dx,
                                    dy,
                                );
                            }
                            ExecSortMode::Cardinal(_) => {
                                let u = &self.pixel_sort_uniform_pool[sort_pool_used];
                                sort_pool_used += 1;
                                self.pixel_sort.record(
                                    device,
                                    queue,
                                    &mut encoder,
                                    u,
                                    src_buf,
                                    dst_buf,
                                    sw,
                                    sh,
                                    row_bytes / 4,
                                    ps.threshold,
                                    ps.angle,
                                );
                            }
                        }
                        encoder.copy_buffer_to_texture(
                            iced::wgpu::TexelCopyBufferInfo {
                                buffer: dst_buf,
                                layout,
                            },
                            tex_copy_info(&outs.tex, iced::wgpu::Origin3d::ZERO),
                            extent,
                        );
                        prev = outs;
                    }
                    PlanItem::Step(idx, m)
                        if matches!(m.kind, ModifierKind::Text(_) | ModifierKind::Drawing(_)) =>
                    {
                        let has_layer = match &m.kind {
                            ModifierKind::Text(_) => {
                                self.text_layers.get(*idx).is_some_and(|l| l.is_some())
                            }
                            _ => self.drawing_layers.get(*idx).is_some_and(|l| l.is_some()),
                        };
                        if !has_layer {
                            continue;
                        }
                        let (ow, oh) = rect_dims(out_r, scale);
                        let outs = self.pooled_stage(device, &mut slab_slot, ow, oh, out_r);
                        match &m.kind {
                            ModifierKind::Text(_) => {
                                if text_pool_used == self.text_uniform_pool.len() {
                                    self.text_uniform_pool
                                        .push(self.text.uniform_buffer(device));
                                }
                                let buffer = &self.text_uniform_pool[text_pool_used];
                                text_pool_used += 1;
                                let layer = self.text_layers[*idx].as_ref().unwrap();
                                self.text.record(
                                    device,
                                    queue,
                                    &mut encoder,
                                    buffer,
                                    layer,
                                    &full_tile_info(source),
                                    uv_of(out_r, full_w, full_h),
                                    uv_of(prev.rect, full_w, full_h),
                                    &prev.view,
                                    &outs.view,
                                );
                            }
                            _ => {
                                if drawing_pool_used == self.drawing_uniform_pool.len() {
                                    self.drawing_uniform_pool
                                        .push(self.drawing.uniform_buffer(device));
                                }
                                let buffer = &self.drawing_uniform_pool[drawing_pool_used];
                                drawing_pool_used += 1;
                                let layer = self.drawing_layers[*idx].as_ref().unwrap();
                                self.drawing.record(
                                    device,
                                    queue,
                                    &mut encoder,
                                    buffer,
                                    layer,
                                    uv_of(out_r, full_w, full_h),
                                    uv_of(prev.rect, full_w, full_h),
                                    &prev.view,
                                    &outs.view,
                                );
                            }
                        }
                        prev = outs;
                    }
                    PlanItem::Step(_, m)
                        if matches!(
                            m.kind,
                            ModifierKind::ChromaticAberration(_) | ModifierKind::MotionBlur(_)
                        ) =>
                    {
                        let (ow, oh) = rect_dims(out_r, scale);
                        let outs = self.pooled_stage(device, &mut slab_slot, ow, oh, out_r);
                        match &m.kind {
                            ModifierKind::ChromaticAberration(ca) => {
                                if ca_pool_used == self.ca_uniform_pool.len() {
                                    self.ca_uniform_pool
                                        .push(self.chromatic_aberration.uniform_buffer(device));
                                }
                                let buffer = &self.ca_uniform_pool[ca_pool_used];
                                ca_pool_used += 1;
                                self.chromatic_aberration.record(
                                    device,
                                    queue,
                                    &mut encoder,
                                    buffer,
                                    ca.amount,
                                    full_w,
                                    uv_of(out_r, full_w, full_h),
                                    uv_of(prev.rect, full_w, full_h),
                                    &prev.view,
                                    &outs.view,
                                );
                            }
                            ModifierKind::MotionBlur(mb) => {
                                if mb_pool_used == self.mb_uniform_pool.len() {
                                    self.mb_uniform_pool
                                        .push(self.motion_blur.uniform_buffer(device));
                                }
                                let buffer = &self.mb_uniform_pool[mb_pool_used];
                                mb_pool_used += 1;
                                self.motion_blur.record(
                                    device,
                                    queue,
                                    &mut encoder,
                                    buffer,
                                    mb.angle,
                                    mb.distance,
                                    full_w,
                                    full_h,
                                    uv_of(out_r, full_w, full_h),
                                    uv_of(prev.rect, full_w, full_h),
                                    &prev.view,
                                    &outs.view,
                                );
                            }
                            _ => unreachable!(),
                        }
                        prev = outs;
                    }
                    PlanItem::Step(_, m) => {
                        let radius = m.kind.effect_class().separable_apron().unwrap_or(0.0);
                        let apron_img = radius.ceil();
                        let r_px_full = (radius * scale).max(1e-3);
                        let ks = (MAX_KERNEL_RADIUS_PX / r_px_full)
                            .min(1.0)
                            .log2()
                            .floor()
                            .exp2();
                        let hmid_r = snap_region(
                            [
                                out_r[0],
                                out_r[1] - apron_img,
                                out_r[2],
                                out_r[3] + apron_img,
                            ],
                            pitch / ks,
                            full_w,
                            full_h,
                        );
                        let (hw, hh) = rect_dims(hmid_r, scale * ks);
                        let hmid = self.pooled_stage(device, &mut slab_slot, hw, hh, hmid_r);
                        let (ow, oh) = rect_dims(out_r, scale);
                        let outs = self.pooled_stage(device, &mut slab_slot, ow, oh, out_r);

                        while self.blur_uniform_pool.len() < blur_pool_used + 2 {
                            self.blur_uniform_pool
                                .push(self.gaussian_blur.uniform_buffer(device));
                        }
                        let (h_pool, v_pool) = self.blur_uniform_pool.split_at(blur_pool_used + 1);
                        let h_buffer = &h_pool[blur_pool_used];
                        let v_buffer = &v_pool[0];
                        blur_pool_used += 2;

                        let hmid_img_w = (hmid_r[2] - hmid_r[0]).max(1.0);
                        let radius_h = radius * hw as f32 / hmid_img_w;
                        let sigma_h = (radius_h / 3.0).max(0.5);
                        let step_h = hmid_img_w / hw as f32;
                        self.gaussian_blur.record(
                            device,
                            queue,
                            &mut encoder,
                            h_buffer,
                            [step_h / full_w, 0.0],
                            radius_h,
                            sigma_h,
                            TileRect {
                                origin: uv_of(hmid_r, full_w, full_h).origin,
                                size: uv_of(hmid_r, full_w, full_h).size,
                            },
                            TileRect {
                                origin: uv_of(prev.rect, full_w, full_h).origin,
                                size: uv_of(prev.rect, full_w, full_h).size,
                            },
                            None,
                            None,
                            &prev.view,
                            &hmid.view,
                            None,
                            0.0,
                        );

                        let hmid_texel_h = (hmid_r[3] - hmid_r[1]).max(1.0) / hh as f32;
                        let radius_v = radius / hmid_texel_h;
                        let sigma_v = (radius_v / 3.0).max(0.5);
                        let step_v = hmid_texel_h;
                        self.gaussian_blur.record(
                            device,
                            queue,
                            &mut encoder,
                            v_buffer,
                            [0.0, step_v / full_h],
                            radius_v,
                            sigma_v,
                            TileRect {
                                origin: uv_of(out_r, full_w, full_h).origin,
                                size: uv_of(out_r, full_w, full_h).size,
                            },
                            TileRect {
                                origin: uv_of(hmid_r, full_w, full_h).origin,
                                size: uv_of(hmid_r, full_w, full_h).size,
                            },
                            None,
                            None,
                            &hmid.view,
                            &outs.view,
                            None,
                            0.0,
                        );
                        prev = outs;
                    }
                }
            }

            let slab_r = scale_rect(prev.rect, scale);
            for &ti in &procs {
                let p = scale_rect(prs[ti].as_ref().unwrap().px, scale);
                let i = [
                    p[0].max(slab_r[0]),
                    p[1].max(slab_r[1]),
                    p[2].min(slab_r[2]),
                    p[3].min(slab_r[3]),
                ];
                if i[2] <= i[0] || i[3] <= i[1] {
                    continue;
                }
                let o = self.tile_outputs[ti].as_ref().unwrap();
                encoder.copy_texture_to_texture(
                    tex_copy_info(
                        &prev.tex,
                        iced::wgpu::Origin3d {
                            x: i[0] - slab_r[0],
                            y: i[1] - slab_r[1],
                            z: 0,
                        },
                    ),
                    tex_copy_info(
                        &o._tex,
                        iced::wgpu::Origin3d {
                            x: i[0] - p[0],
                            y: i[1] - p[1],
                            z: 0,
                        },
                    ),
                    iced::wgpu::Extent3d {
                        width: (i[2] - i[0]).min(o.width.saturating_sub(i[0] - p[0])),
                        height: (i[3] - i[1]).min(o.height.saturating_sub(i[1] - p[1])),
                        depth_or_array_layers: 1,
                    },
                );
            }

            self.exec_band_cursor = by1;
            if by1 >= su_h {
                for &ti in &procs {
                    self.tile_outputs[ti].as_mut().unwrap().valid = true;
                }
            } else {
                self.reprocess_pending = true;
            }
            queue.submit([encoder.finish()]);
        }

        for &ti in &procs {
            let pr = prs[ti].take().unwrap();
            self.build_roi_display_bgs(device, queue, ti, &source.tiles[ti], &pr, true);
        }
    }

    fn pooled_stage(
        &mut self,
        device: &Device,
        slot: &mut usize,
        w: u32,
        h: u32,
        rect: RegionPx,
    ) -> Stage {
        let idx = *slot;
        *slot += 1;
        if self.exec_slab_pool.len() <= idx {
            self.exec_slab_pool.resize_with(idx + 1, || None);
        }
        let entry = &mut self.exec_slab_pool[idx];
        if entry.as_ref().is_none_or(|t| t.width != w || t.height != h) {
            *entry = Some(ScratchTarget::new(device, self.format, w, h));
        }
        let t = entry.as_ref().unwrap();
        Stage {
            tex: t._tex.clone(),
            view: t.view.clone(),
            rect,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gather_render(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        rect: RegionPx,
        scale: f32,
        pool_used: &mut usize,
        slab_slot: &mut usize,
    ) -> Stage {
        let (w, h) = rect_dims(rect, scale);
        let stage = self.pooled_stage(device, slab_slot, w, h, rect);
        let proc_uv = uv_of(rect, source.full_width as f32, source.full_height as f32);

        let mut pieces: Vec<(usize, [u32; 4])> = Vec::new();
        for (ti, tile) in source.tiles.iter().enumerate() {
            let tr = [
                tile.x as f32,
                tile.y as f32,
                (tile.x + tile.width) as f32,
                (tile.y + tile.height) as f32,
            ];
            let ix = [
                tr[0].max(rect[0]),
                tr[1].max(rect[1]),
                tr[2].min(rect[2]),
                tr[3].min(rect[3]),
            ];
            if ix[2] <= ix[0] || ix[3] <= ix[1] {
                continue;
            }
            let sx0 = (((ix[0] - rect[0]) * scale).round() as u32).min(w);
            let sy0 = (((ix[1] - rect[1]) * scale).round() as u32).min(h);
            let sx1 = (((ix[2] - rect[0]) * scale).round() as u32).clamp(sx0, w);
            let sy1 = (((ix[3] - rect[1]) * scale).round() as u32).clamp(sy0, h);
            if sx1 <= sx0 || sy1 <= sy0 {
                continue;
            }
            pieces.push((ti, [sx0, sy0, sx1 - sx0, sy1 - sy0]));
        }

        while self.uniform_pool.len() < *pool_used + pieces.len() {
            self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                device,
                Some("combined-modifiers-uniform"),
            ));
        }

        let mut bgs: Vec<(BindGroup, Option<[u32; 4]>)> = Vec::new();
        for (ti, scissor) in &pieces {
            let tile = &source.tiles[*ti];
            let tr = [
                tile.x as f32,
                tile.y as f32,
                (tile.x + tile.width) as f32,
                (tile.y + tile.height) as f32,
            ];
            let uniforms = build_segment_uniforms(
                &[],
                &full_tile_info(source),
                proc_uv,
                uv_of(tr, source.full_width as f32, source.full_height as f32),
            );
            let buffer = &self.uniform_pool[*pool_used];
            *pool_used += 1;
            gpu::write_uniform(queue, buffer, &uniforms);
            bgs.push((
                gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    &tile.source_view,
                    &self.trilinear_sampler,
                    Some("slab-gather-bg"),
                ),
                Some(*scissor),
            ));
        }
        self.combined
            .run_pieces(encoder, &stage.view, bgs.iter().map(|(b, s)| (b, *s)));
        stage
    }
}
