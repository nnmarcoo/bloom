use super::geom::*;
use super::*;

impl ModifierPipeline {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_tiled(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        plan: &[PlanItem],
        proc_scale: f32,
        downscale: bool,
        dirty: bool,
        has_pixel_sort: bool,
    ) {
        if has_pixel_sort {
            self.prepare_tiled_scanlines(device, queue, source, plan, proc_scale, dirty);
            return;
        }

        let n_tiles = source.tiles.len();
        self.bank_a.resize_with(n_tiles, || None);
        self.bank_b.resize_with(n_tiles, || None);
        self.blur_hmid.resize_with(n_tiles, || None);
        self.blur_vstrip_top.resize_with(n_tiles, || None);
        self.blur_vstrip_bot.resize_with(n_tiles, || None);

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let mut apron_px: f32 = 0.0;
        for item in plan {
            if let PlanItem::Step(_, m) = item
                && let Some(radius) = m.kind.effect_class().separable_apron()
            {
                apron_px = apron_px.max(radius);
            }
        }

        let blur_is_sole_step = plan.len() == 1
            && matches!(
                plan.first(),
                Some(PlanItem::Step(_, m)) if m.kind.effect_class().separable_apron().is_some()
            );

        let roi_active = !downscale
            && blur_is_sole_step
            && source
                .tiles
                .iter()
                .any(|t| !tile_ndc_culled(t.last_ndc_rect))
            && source
                .tiles
                .iter()
                .all(|t| tile_ndc_culled(t.last_ndc_rect) || t.proc_rect_px.is_some());

        let (mut proc_scale, mut downscale) = (proc_scale, downscale);
        if !roi_active {
            let mut in_set = vec![false; n_tiles];
            for ti in 0..n_tiles {
                if tile_ndc_culled(source.tiles[ti].last_ndc_rect) {
                    continue;
                }
                in_set[ti] = true;
                let nb = tile_neighbors(&source.tiles, ti);
                for nn in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    in_set[nn] = true;
                }
            }
            let mut n_proc = 0u64;
            let mut tw = 1u32;
            let mut th = 1u32;
            for (ti, &keep) in in_set.iter().enumerate() {
                if keep {
                    n_proc += 1;
                    tw = tw.max(source.tiles[ti].width);
                    th = th.max(source.tiles[ti].height);
                }
            }
            let fit = fit_process_scale(tw, th, n_proc, 3, process_vram_budget(device), proc_scale);
            if fit < proc_scale {
                proc_scale = fit;
                downscale = true;
            }
        }

        let proc_rect_for =
            |ti: usize, reuse: bool, prev_px: Option<[f32; 4]>, prev_wh: (u32, u32)| -> ProcRect {
                if reuse {
                    proc_rect_from_px(
                        prev_px,
                        &source.tiles[ti],
                        full_w,
                        full_h,
                        prev_wh.0,
                        prev_wh.1,
                    )
                } else {
                    tile_proc_rect(
                        &source.tiles[ti],
                        full_w,
                        full_h,
                        proc_scale,
                        downscale,
                        apron_px,
                        roi_active,
                    )
                }
            };

        let cur_scale = if downscale { proc_scale } else { 1.0 };

        let mut proc_rects: Vec<Option<ProcRect>> = (0..n_tiles).map(|_| None).collect();
        let mut stale: Vec<usize> = Vec::new();
        #[allow(clippy::needless_range_loop)]
        for ti in 0..n_tiles {
            let tile = &source.tiles[ti];
            if tile_ndc_culled(tile.last_ndc_rect) {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                continue;
            }
            let roi = if roi_active { tile.proc_rect_px } else { None };
            let (prev_px, prev_scale, prev_wh) = self.tile_outputs[ti]
                .as_ref()
                .map(|o| (o.proc_px, o.proc_scale, (o.width, o.height)))
                .unwrap_or((None, 0.0, (0, 0)));
            let same_scale = (prev_scale - cur_scale).abs() < 1e-4;
            let reuse_valid = !dirty
                && match (prev_px, roi) {
                    (Some(p), Some(r)) => rect_contains(p, r) && same_scale,
                    (None, None) => same_scale,
                    _ => false,
                };
            let pr = proc_rect_for(ti, reuse_valid, prev_px, prev_wh);
            self.ensure_tile_output(
                device,
                ti,
                pr.w,
                pr.h,
                if roi_active { Some(pr.px) } else { None },
            );
            if !reuse_valid {
                let o = self.tile_outputs[ti].as_mut().unwrap();
                o.valid = false;
                o.proc_px = if roi_active { Some(pr.px) } else { None };
                o.band_y = 0;
            }
            self.tile_outputs[ti].as_mut().unwrap().proc_scale = cur_scale;

            let needs = !self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid)
                || self.tile_display_bgs_linear[ti].is_none()
                || !reuse_valid
                || (roi_active && tile.isec_px.is_some());
            if needs {
                stale.push(ti);
            }
            proc_rects[ti] = Some(pr);
        }

        if stale.is_empty() {
            return;
        }

        let mut scheduler = Scheduler::new();
        let mut reprocess: Vec<usize> = Vec::new();
        let mut display_set: Vec<usize> = Vec::new();
        for &ti in &stale {
            let needs_reprocess = !self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid);
            if needs_reprocess {
                if !scheduler.admit() {
                    continue;
                }
                reprocess.push(ti);
            }
            display_set.push(ti);
        }
        self.reprocess_pending = scheduler.pending();

        let mut in_process = vec![false; n_tiles];
        let mut blur_set: Vec<usize> = Vec::new();
        for &ti in &reprocess {
            if !in_process[ti] {
                in_process[ti] = true;
                blur_set.push(ti);
            }
            if !roi_active {
                let nb = tile_neighbors(&source.tiles, ti);
                for n in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    if !in_process[n] {
                        in_process[n] = true;
                        blur_set.push(n);
                        if proc_rects[n].is_none() {
                            proc_rects[n] = Some(tile_proc_rect(
                                &source.tiles[n],
                                full_w,
                                full_h,
                                proc_scale,
                                downscale,
                                apron_px,
                                false,
                            ));
                        }
                    }
                }
            }
        }

        let ensure_bank = |bank: &mut Vec<Option<ScratchTarget>>,
                           ti: usize,
                           w: u32,
                           h: u32,
                           fmt: TextureFormat| {
            let stale = bank[ti]
                .as_ref()
                .is_none_or(|t| t.width != w || t.height != h);
            if stale {
                bank[ti] = Some(ScratchTarget::new(device, fmt, w, h));
            }
        };
        for &ti in &blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            self.ensure_tile_output(
                device,
                ti,
                pr.w,
                pr.h,
                if roi_active { Some(pr.px) } else { None },
            );
            ensure_bank(&mut self.bank_a, ti, pr.w, pr.h, self.format);
            ensure_bank(&mut self.bank_b, ti, pr.w, pr.h, self.format);
            ensure_bank(&mut self.blur_hmid, ti, pr.w, pr.h, self.format);
        }

        let plan_steps = plan.len();
        let mut encoder = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("modifier-step-major-encoder"),
        });
        let mut pools = StepPools::default();
        let mut blur_pool_used = 0usize;

        let mut prev_in_a = true;

        for (k, item) in plan.iter().enumerate() {
            let last = k == plan_steps - 1;
            let is_blur = matches!(item, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some());
            if !is_blur {
                for &ti in &blur_set {
                    let tile = &source.tiles[ti];
                    let pr = proc_rects[ti].as_ref().unwrap();
                    let tile_info = TileInfo {
                        tile_x: tile.x,
                        tile_y: tile.y,
                        tile_w: tile.width,
                        tile_h: tile.height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    };

                    let prev: TextureView = if k == 0 {
                        tile.source_view.clone()
                    } else if prev_in_a {
                        self.bank_a[ti].as_ref().unwrap().view.clone()
                    } else {
                        self.bank_b[ti].as_ref().unwrap().view.clone()
                    };

                    let dst_in_a = !prev_in_a;
                    let out: TextureView = if last {
                        self.tile_outputs[ti].as_ref().unwrap().view.clone()
                    } else if dst_in_a {
                        self.bank_a[ti].as_ref().unwrap().view.clone()
                    } else {
                        self.bank_b[ti].as_ref().unwrap().view.clone()
                    };

                    let src_rect = if k == 0 { pr.src } else { pr.proc };
                    let proc = pr.proc;
                    self.run_ordinary_step(
                        device,
                        queue,
                        &mut encoder,
                        item,
                        &tile_info,
                        source.full_width as f32,
                        proc,
                        src_rect,
                        &prev,
                        &out,
                        &mut pools,
                    );
                }
            }

            if let PlanItem::Step(_, m) = item
                && let Some(radius) = m.kind.effect_class().separable_apron()
            {
                let deferred = self.run_separable_step(
                    device,
                    queue,
                    &mut encoder,
                    source,
                    &blur_set,
                    &proc_rects,
                    roi_active,
                    radius,
                    k == 0,
                    prev_in_a,
                    last,
                    &mut blur_pool_used,
                );
                self.reprocess_pending |= deferred;
            }

            if !last {
                prev_in_a = !prev_in_a;
            }
        }

        for &ti in &reprocess {
            let o = self.tile_outputs[ti].as_mut().unwrap();
            let complete = o.band_y == 0 || o.band_y >= o.height;
            o.valid = complete;
        }
        for &ti in &display_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            self.build_roi_display_bgs(device, queue, ti, &source.tiles[ti], pr, roi_active);
        }
        queue.submit([encoder.finish()]);
    }

    fn prepare_tiled_scanlines(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        plan: &[PlanItem],
        proc_scale: f32,
        dirty: bool,
    ) {
        let n_tiles = source.tiles.len();
        self.bank_a.resize_with(n_tiles, || None);
        self.bank_b.resize_with(n_tiles, || None);
        self.blur_hmid.resize_with(n_tiles, || None);

        let mut has_h = false;
        let mut has_v = false;
        let mut has_diag = false;
        for p in plan {
            if let PlanItem::Step(_, m) = p
                && let ModifierKind::PixelSort(ps) = &m.kind
                && m.kind.effect_class().is_compute_scanline()
            {
                match SortMode::from_angle(ps.angle) {
                    SortMode::Cardinal(SortAxis::Horizontal { .. }) => has_h = true,
                    SortMode::Cardinal(SortAxis::Vertical { .. }) => has_v = true,
                    SortMode::Diagonal { .. } => has_diag = true,
                }
            }
        }

        let mut scale = fit_process_scale(
            source.full_width,
            source.full_height,
            1,
            3,
            process_vram_budget(device),
            proc_scale,
        );
        if has_diag {
            let max_bytes = sort_buffer_limit(device);
            while scale > 1.0 / 4096.0 {
                let sw = scaled(source.full_width, scale).max(1) as u64;
                let sh = scaled(source.full_height, scale).max(1) as u64;
                if (sw * 4).div_ceil(256) * 256 * sh <= max_bytes {
                    break;
                }
                scale *= 0.5;
            }
        }
        let scale = scale;
        let stile = |ti: usize, source: &TiledSource| -> (u32, u32, u32, u32) {
            let t = &source.tiles[ti];
            let sx0 = scaled(t.x, scale);
            let sy0 = scaled(t.y, scale);
            let sx1 = scaled(t.x + t.width, scale);
            let sy1 = scaled(t.y + t.height, scale);
            (sx0, sy0, (sx1 - sx0).max(1), (sy1 - sy0).max(1))
        };

        let visible: Vec<usize> = (0..n_tiles)
            .filter(|&ti| !tile_ndc_culled(source.tiles[ti].last_ndc_rect))
            .collect();
        if visible.is_empty() {
            for ti in 0..n_tiles {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
            }
            return;
        }

        let plan_has_blur = plan
            .iter()
            .any(|p| matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().separable_apron().is_some()));

        let mut in_set = vec![has_diag; n_tiles];
        for &v in &visible {
            let (vx, vy) = (source.tiles[v].x, source.tiles[v].y);
            for (ti, slot) in in_set.iter_mut().enumerate() {
                let same_row = source.tiles[ti].y == vy;
                let same_col = source.tiles[ti].x == vx;
                if (has_h && same_row) || (has_v && same_col) {
                    *slot = true;
                }
            }
        }
        if plan_has_blur {
            let strip: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();
            for ti in strip {
                let nb = tile_neighbors(&source.tiles, ti);
                for n in [nb.left, nb.right, nb.up, nb.down].into_iter().flatten() {
                    in_set[n] = true;
                }
            }
            let expanded: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();
            for ei in expanded {
                let (ex, ey) = (source.tiles[ei].x, source.tiles[ei].y);
                for (ti, slot) in in_set.iter_mut().enumerate() {
                    let same_row = source.tiles[ti].y == ey;
                    let same_col = source.tiles[ti].x == ex;
                    if (has_h && same_row) || (has_v && same_col) {
                        *slot = true;
                    }
                }
            }
        }
        let proc_set: Vec<usize> = (0..n_tiles).filter(|&ti| in_set[ti]).collect();

        for (ti, &keep) in in_set.iter().enumerate() {
            if !keep {
                self.tile_outputs[ti] = None;
                self.tile_display_bgs_linear[ti] = None;
                self.tile_display_bgs_nearest[ti] = None;
                self.bank_a[ti] = None;
                self.bank_b[ti] = None;
            }
        }

        if dirty {
            for &ti in &proc_set {
                if let Some(o) = self.tile_outputs[ti].as_mut() {
                    o.valid = false;
                }
            }
        }

        let ensure_bank = |bank: &mut Vec<Option<ScratchTarget>>,
                           ti: usize,
                           w: u32,
                           h: u32,
                           fmt: TextureFormat| {
            let stale = bank[ti]
                .as_ref()
                .is_none_or(|t| t.width != w || t.height != h);
            if stale {
                bank[ti] = Some(ScratchTarget::new(device, fmt, w, h));
            }
        };

        let plan_steps = plan.len();

        let mut proc_rects: Vec<Option<ProcRect>> = (0..n_tiles).map(|_| None).collect();
        for &ti in &proc_set {
            let (_, _, sw, sh) = stile(ti, source);
            self.ensure_tile_output(device, ti, sw, sh, None);
            ensure_bank(&mut self.bank_a, ti, sw, sh, self.format);
            ensure_bank(&mut self.bank_b, ti, sw, sh, self.format);
            if plan_has_blur {
                ensure_bank(&mut self.blur_hmid, ti, sw, sh, self.format);
                let t = &source.tiles[ti];
                let fw = source.full_width as f32;
                let fh = source.full_height as f32;
                proc_rects[ti] = Some(ProcRect {
                    px: [
                        t.x as f32,
                        t.y as f32,
                        (t.x + t.width) as f32,
                        (t.y + t.height) as f32,
                    ],
                    proc: UvRect {
                        origin: [t.x as f32 / fw, t.y as f32 / fh],
                        size: [t.width as f32 / fw, t.height as f32 / fh],
                    },
                    src: UvRect {
                        origin: [t.x as f32 / fw, t.y as f32 / fh],
                        size: [t.width as f32 / fw, t.height as f32 / fh],
                    },
                    w: sw,
                    h: sh,
                });
            }
        }

        let all_valid = proc_set
            .iter()
            .all(|&ti| self.tile_outputs[ti].as_ref().is_some_and(|o| o.valid));
        let needs_bg_only = all_valid && !dirty;

        let mut sig: u64 = 1469598103934665603;
        for &ti in &proc_set {
            sig = (sig ^ ti as u64).wrapping_mul(1099511628211);
        }
        sig = (sig ^ scale.to_bits() as u64).wrapping_mul(1099511628211);
        if dirty || sig != self.sort_progress_sig {
            self.sort_band_cursor = 0;
            self.sort_progress_sig = sig;
        }

        let last_sort_k = plan.iter().enumerate().rev().find_map(|(k, p)| {
            matches!(p, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline())
                .then_some(k)
        });

        let continuation = self.sort_band_cursor > 0;

        if !needs_bg_only {
            let mut encoder =
                device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                    label: Some("pixel-sort-multitile-encoder"),
                });

            if self.uniform_pool.is_empty() {
                self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                    device,
                    Some("combined-modifiers-uniform"),
                ));
            }
            for &ti in &proc_set {
                if continuation {
                    break;
                }
                let src: TextureView = source.tiles[ti].source_view.clone();
                let dst: TextureView = self.bank_a[ti].as_ref().unwrap().view.clone();
                let uniforms = build_segment_uniforms(
                    &[],
                    &TileInfo {
                        tile_x: source.tiles[ti].x,
                        tile_y: source.tiles[ti].y,
                        tile_w: source.tiles[ti].width,
                        tile_h: source.tiles[ti].height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    },
                    UvRect {
                        origin: [0.0, 0.0],
                        size: [1.0, 1.0],
                    },
                    UvRect {
                        origin: [0.0, 0.0],
                        size: [1.0, 1.0],
                    },
                );
                let buffer = &self.uniform_pool[0];
                gpu::write_uniform(queue, buffer, &uniforms);
                let bg = gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    &src,
                    &self.trilinear_sampler,
                    Some("psort-downscale-bg"),
                );
                self.combined.run(&mut encoder, &bg, &dst);
            }

            let mut pools = StepPools {
                fused: 1,
                ..StepPools::default()
            };
            let mut blur_pool_used = 0usize;
            let mut prev_in_a = true;
            let mut sort_complete = true;

            for (k, item) in plan.iter().enumerate() {
                let last = k == plan_steps - 1;
                let is_sort = matches!(item, PlanItem::Step(_, m) if m.kind.effect_class().is_compute_scanline());
                let blur_radius = match item {
                    PlanItem::Step(_, m) => m.kind.effect_class().separable_apron(),
                    _ => None,
                };

                let post_sort = last_sort_k.is_some_and(|s| k > s);

                if !is_sort && ((continuation && !post_sort) || (post_sort && !sort_complete)) {
                    if !last {
                        prev_in_a = !prev_in_a;
                    }
                    continue;
                }

                if let Some(radius) = blur_radius {
                    self.run_separable_step(
                        device,
                        queue,
                        &mut encoder,
                        source,
                        &proc_set,
                        &proc_rects,
                        false,
                        radius,
                        false,
                        prev_in_a,
                        last,
                        &mut blur_pool_used,
                    );
                } else if !is_sort {
                    for &ti in &proc_set {
                        let tile = &source.tiles[ti];
                        let tile_info = TileInfo {
                            tile_x: tile.x,
                            tile_y: tile.y,
                            tile_w: tile.width,
                            tile_h: tile.height,
                            full_w: source.full_width,
                            full_h: source.full_height,
                        };
                        let whole = tile_proc_rect(
                            tile,
                            source.full_width as f32,
                            source.full_height as f32,
                            1.0,
                            false,
                            0.0,
                            false,
                        );
                        let prev: TextureView = self.step_input_view(ti, prev_in_a).clone();
                        let out: TextureView = self.step_output_view(ti, last, prev_in_a).clone();

                        self.run_ordinary_step(
                            device,
                            queue,
                            &mut encoder,
                            item,
                            &tile_info,
                            source.full_width as f32,
                            whole.proc,
                            whole.src,
                            &prev,
                            &out,
                            &mut pools,
                        );
                    }
                } else if let PlanItem::Step(_, m) = item
                    && let ModifierKind::PixelSort(ps) = &m.kind
                {
                    queue.submit([encoder.finish()]);
                    let budgeted = Some(k) == last_sort_k;
                    let done = match SortMode::from_angle(ps.angle) {
                        SortMode::Diagonal { dx, dy } => {
                            self.sort_diag_full(
                                device,
                                queue,
                                source,
                                &proc_set,
                                last,
                                prev_in_a,
                                ps.threshold,
                                dx,
                                dy,
                                scale,
                            );
                            true
                        }
                        SortMode::Cardinal(axis) => {
                            let step_vertical = matches!(axis, SortAxis::Vertical { .. });
                            let groups = pixel_sort_groups(source, &proc_set, step_vertical);
                            self.sort_cross_tile(
                                device,
                                queue,
                                source,
                                &groups,
                                last,
                                prev_in_a,
                                step_vertical,
                                ps.threshold,
                                ps.angle,
                                budgeted,
                                scale,
                            )
                        }
                    };
                    if budgeted {
                        sort_complete = done;
                    }
                    encoder =
                        device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                            label: Some("pixel-sort-multitile-encoder"),
                        });
                }

                if !last {
                    prev_in_a = !prev_in_a;
                }
            }

            queue.submit([encoder.finish()]);

            if sort_complete {
                self.sort_band_cursor = 0;
                for &ti in &proc_set {
                    if let Some(o) = self.tile_outputs[ti].as_mut() {
                        o.valid = true;
                    }
                }
            } else {
                self.reprocess_pending = true;
            }
        }

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;
        for &ti in &visible {
            let tile = &source.tiles[ti];
            let pr = proc_rect_from_px(None, tile, full_w, full_h, tile.width, tile.height);
            self.build_roi_display_bgs(device, queue, ti, tile, &pr, false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_ordinary_step(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        item: &PlanItem,
        tile_info: &TileInfo,
        full_w: f32,
        proc: UvRect,
        src: UvRect,
        prev: &TextureView,
        out: &TextureView,
        pools: &mut StepPools,
    ) {
        match item {
            PlanItem::Fused(seg) => {
                let uniforms = build_segment_uniforms(seg, tile_info, proc, src);
                if pools.fused == self.uniform_pool.len() {
                    self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                        device,
                        Some("combined-modifiers-uniform"),
                    ));
                }
                let buffer = &self.uniform_pool[pools.fused];
                pools.fused += 1;
                gpu::write_uniform(queue, buffer, &uniforms);
                let bg = gpu::standard_bind_group(
                    device,
                    &self.combined.bgl,
                    buffer,
                    prev,
                    &self.trilinear_sampler,
                    Some("combined-modifiers-bg"),
                );
                self.combined.run(encoder, &bg, out);
            }
            PlanItem::Step(idx, m) => match &m.kind {
                ModifierKind::ChromaticAberration(ca) => {
                    if pools.ca == self.ca_uniform_pool.len() {
                        self.ca_uniform_pool
                            .push(self.chromatic_aberration.uniform_buffer(device));
                    }
                    let buffer = &self.ca_uniform_pool[pools.ca];
                    pools.ca += 1;
                    self.chromatic_aberration.record(
                        device, queue, encoder, buffer, ca.amount, full_w, proc, src, prev, out,
                    );
                }
                ModifierKind::Text(_) => {
                    if let Some(layer) = self.text_layers.get(*idx).and_then(|l| l.as_ref()) {
                        if pools.text == self.text_uniform_pool.len() {
                            self.text_uniform_pool
                                .push(self.text.uniform_buffer(device));
                        }
                        let buffer = &self.text_uniform_pool[pools.text];
                        pools.text += 1;
                        self.text.record(
                            device, queue, encoder, buffer, layer, tile_info, proc, src, prev, out,
                        );
                    }
                }
                ModifierKind::Drawing(_) => {
                    if let Some(layer) = self.drawing_layers.get(*idx).and_then(|l| l.as_ref()) {
                        if pools.drawing == self.drawing_uniform_pool.len() {
                            self.drawing_uniform_pool
                                .push(self.drawing.uniform_buffer(device));
                        }
                        let buffer = &self.drawing_uniform_pool[pools.drawing];
                        pools.drawing += 1;
                        self.drawing
                            .record(device, queue, encoder, buffer, layer, proc, src, prev, out);
                    }
                }
                _ => {
                    let uniforms = build_segment_uniforms(&[], tile_info, proc, src);
                    if pools.fused == self.uniform_pool.len() {
                        self.uniform_pool.push(gpu::uniform_buffer::<ModUniforms>(
                            device,
                            Some("combined-modifiers-uniform"),
                        ));
                    }
                    let buffer = &self.uniform_pool[pools.fused];
                    pools.fused += 1;
                    gpu::write_uniform(queue, buffer, &uniforms);
                    let bg = gpu::standard_bind_group(
                        device,
                        &self.combined.bgl,
                        buffer,
                        prev,
                        &self.trilinear_sampler,
                        Some("passthrough-bg"),
                    );
                    self.combined.run(encoder, &bg, out);
                }
            },
        }
    }

    fn step_input_view(&self, ti: usize, prev_in_a: bool) -> &TextureView {
        if prev_in_a {
            &self.bank_a[ti].as_ref().unwrap().view
        } else {
            &self.bank_b[ti].as_ref().unwrap().view
        }
    }

    fn step_output_view(&self, ti: usize, last: bool, prev_in_a: bool) -> &TextureView {
        if last {
            &self.tile_outputs[ti].as_ref().unwrap().view
        } else if prev_in_a {
            &self.bank_b[ti].as_ref().unwrap().view
        } else {
            &self.bank_a[ti].as_ref().unwrap().view
        }
    }
}
