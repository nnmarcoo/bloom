use super::geom::*;
use super::*;

impl ModifierPipeline {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_separable_step(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        blur_set: &[usize],
        proc_rects: &[Option<ProcRect>],
        roi_active: bool,
        radius: f32,
        is_first: bool,
        prev_in_a: bool,
        last: bool,
        blur_pool_used: &mut usize,
    ) -> bool {
        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;

        let bank_rect = |i: usize| -> Option<TileRect> {
            proc_rects[i].as_ref().map(|p| TileRect {
                origin: p.proc.origin,
                size: p.proc.size,
            })
        };

        if roi_active && last && is_first {
            let mut deferred = false;
            for &ti in blur_set {
                if self.blur_tile_banded(
                    device,
                    queue,
                    encoder,
                    source,
                    proc_rects,
                    ti,
                    radius,
                    full_w,
                    full_h,
                    blur_pool_used,
                ) {
                    deferred = true;
                }
            }
            return deferred;
        }

        for &ti in blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            let proc_img_w = (pr.px[2] - pr.px[0]).max(1.0);
            let radius_px = radius * pr.w as f32 / proc_img_w;
            let sigma = (radius_px / 3.0).max(0.5);
            let step = proc_img_w / pr.w as f32;
            let rect = bank_rect(ti).unwrap();
            let src = if is_first {
                TileRect {
                    origin: pr.src.origin,
                    size: pr.src.size,
                }
            } else {
                rect
            };
            let nb = if roi_active {
                Neighbors::default()
            } else {
                tile_neighbors(&source.tiles, ti)
            };

            while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let h_buffer = &self.blur_uniform_pool[*blur_pool_used];
            *blur_pool_used += 1;

            let p_view = |i: usize| -> Option<&TextureView> {
                if is_first {
                    Some(&source.tiles[i].source_view)
                } else if prev_in_a {
                    self.bank_a[i].as_ref().map(|t| &t.view)
                } else {
                    self.bank_b[i].as_ref().map(|t| &t.view)
                }
            };
            let nbr_rect = |i: usize| -> Option<TileRect> {
                if is_first {
                    Some(full_rect(&TileInfo {
                        tile_x: source.tiles[i].x,
                        tile_y: source.tiles[i].y,
                        tile_w: source.tiles[i].width,
                        tile_h: source.tiles[i].height,
                        full_w: source.full_width,
                        full_h: source.full_height,
                    }))
                } else {
                    bank_rect(i)
                }
            };

            let input = p_view(ti).unwrap();
            let lo = nb.left.and_then(|i| Some((p_view(i)?, nbr_rect(i)?)));
            let hi = nb.right.and_then(|i| Some((p_view(i)?, nbr_rect(i)?)));
            let h_out = &self.blur_hmid[ti].as_ref().unwrap().view;

            let h_lod = if is_first { step.max(1.0).log2() } else { 0.0 };
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                h_buffer,
                [step / full_w, 0.0],
                radius_px,
                sigma,
                rect,
                src,
                lo,
                hi,
                input,
                h_out,
                None,
                h_lod,
            );
        }

        for &ti in blur_set {
            let pr = proc_rects[ti].as_ref().unwrap();
            let proc_img_h = (pr.px[3] - pr.px[1]).max(1.0);
            let radius_px = radius * pr.h as f32 / proc_img_h;
            let sigma = (radius_px / 3.0).max(0.5);
            let step = proc_img_h / pr.h as f32;
            let rect = bank_rect(ti).unwrap();
            let nb = if roi_active {
                Neighbors::default()
            } else {
                tile_neighbors(&source.tiles, ti)
            };

            while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let v_buffer = &self.blur_uniform_pool[*blur_pool_used];
            *blur_pool_used += 1;

            let h_view =
                |i: usize| -> Option<&TextureView> { self.blur_hmid[i].as_ref().map(|t| &t.view) };

            let input = h_view(ti).unwrap();
            let lo = nb.up.and_then(|i| Some((h_view(i)?, bank_rect(i)?)));
            let hi = nb.down.and_then(|i| Some((h_view(i)?, bank_rect(i)?)));

            let v_out: &TextureView = if last {
                &self.tile_outputs[ti].as_ref().unwrap().view
            } else if prev_in_a {
                &self.bank_b[ti].as_ref().unwrap().view
            } else {
                &self.bank_a[ti].as_ref().unwrap().view
            };

            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                v_buffer,
                [0.0, step / full_h],
                radius_px,
                sigma,
                rect,
                rect,
                lo,
                hi,
                input,
                v_out,
                None,
                0.0,
            );
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn blur_tile_banded(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        proc_rects: &[Option<ProcRect>],
        ti: usize,
        radius: f32,
        full_w: f32,
        full_h: f32,
        blur_pool_used: &mut usize,
    ) -> bool {
        let pr = proc_rects[ti].as_ref().unwrap();
        let w = pr.w;
        let h = pr.h;
        let proc_img_w = (pr.px[2] - pr.px[0]).max(1.0);
        let proc_img_h = (pr.px[3] - pr.px[1]).max(1.0);
        let radius_h = radius * w as f32 / proc_img_w;
        let radius_v = radius * h as f32 / proc_img_h;
        let sigma_h = (radius_h / 3.0).max(0.5);
        let sigma_v = (radius_v / 3.0).max(0.5);
        let step_h = proc_img_w / w as f32;
        let step_v = proc_img_h / h as f32;
        let apron = radius_v.ceil() as u32;

        let rect = TileRect {
            origin: pr.proc.origin,
            size: pr.proc.size,
        };
        let src = TileRect {
            origin: pr.src.origin,
            size: pr.src.size,
        };

        let tile_top = source.tiles[ti].y as f32;
        let tile_bot = (source.tiles[ti].y + source.tiles[ti].height) as f32;
        let apron_img = radius.ceil();
        let nb = tile_neighbors(&source.tiles, ti);
        let top_strip = match nb.up {
            Some(up) if pr.px[1] <= tile_top + 0.5 => self.record_v_neighbor_strip(
                device,
                queue,
                encoder,
                source,
                ti,
                up,
                pr,
                apron,
                apron_img,
                w,
                tile_top - apron_img,
                tile_top,
                full_w,
                full_h,
                step_h,
                radius_h,
                sigma_h,
                blur_pool_used,
                true,
            ),
            _ => None,
        };
        let bot_strip = match nb.down {
            Some(down) if pr.px[3] >= tile_bot - 0.5 => self.record_v_neighbor_strip(
                device,
                queue,
                encoder,
                source,
                ti,
                down,
                pr,
                apron,
                apron_img,
                w,
                tile_bot,
                tile_bot + apron_img,
                full_w,
                full_h,
                step_h,
                radius_h,
                sigma_h,
                blur_pool_used,
                false,
            ),
            _ => None,
        };

        let taps = 2 * (radius_h.ceil() as u32 + radius_v.ceil() as u32) + 2;
        let per_row = (w.max(1)) * taps.max(1);
        let band_h = (BLUR_WORK_BUDGET / per_row).clamp(BLUR_MIN_BAND_H, BLUR_MAX_BAND_H);

        let start = self.tile_outputs[ti].as_ref().unwrap().band_y;
        let mut by = start;
        let mut bands_done = 0u32;
        while by < h && bands_done < 1 {
            let by1 = (by + band_h).min(h);
            let h0 = by.saturating_sub(apron);
            let h1 = (by1 + apron).min(h);
            while self.blur_uniform_pool.len() < *blur_pool_used + 2 {
                self.blur_uniform_pool
                    .push(self.gaussian_blur.uniform_buffer(device));
            }
            let (h_pool, v_pool) = self.blur_uniform_pool.split_at(*blur_pool_used + 1);
            let h_buffer = &h_pool[*blur_pool_used];
            let v_buffer = &v_pool[0];
            *blur_pool_used += 2;

            let input = &source.tiles[ti].source_view;
            let h_out = &self.blur_hmid[ti].as_ref().unwrap().view;
            let h_lod = step_h.max(1.0).log2();
            let nb = tile_neighbors(&source.tiles, ti);
            let tile_rect = |i: usize| -> TileRect {
                full_rect(&TileInfo {
                    tile_x: source.tiles[i].x,
                    tile_y: source.tiles[i].y,
                    tile_w: source.tiles[i].width,
                    tile_h: source.tiles[i].height,
                    full_w: source.full_width,
                    full_h: source.full_height,
                })
            };
            let lo = nb
                .left
                .map(|i| (&source.tiles[i].source_view, tile_rect(i)));
            let hi = nb
                .right
                .map(|i| (&source.tiles[i].source_view, tile_rect(i)));
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                h_buffer,
                [step_h / full_w, 0.0],
                radius_h,
                sigma_h,
                rect,
                src,
                lo,
                hi,
                input,
                h_out,
                Some([0, h0, w, h1 - h0]),
                h_lod,
            );

            let hmid = &self.blur_hmid[ti].as_ref().unwrap().view;
            let out = &self.tile_outputs[ti].as_ref().unwrap().view;
            let lo_v =
                top_strip.and_then(|r| self.blur_vstrip_top[ti].as_ref().map(|s| (&s.view, r)));
            let hi_v =
                bot_strip.and_then(|r| self.blur_vstrip_bot[ti].as_ref().map(|s| (&s.view, r)));
            self.gaussian_blur.record(
                device,
                queue,
                encoder,
                v_buffer,
                [0.0, step_v / full_h],
                radius_v,
                sigma_v,
                rect,
                rect,
                lo_v,
                hi_v,
                hmid,
                out,
                Some([0, by, w, by1 - by]),
                0.0,
            );

            by = by1;
            bands_done += 1;
        }

        let o = self.tile_outputs[ti].as_mut().unwrap();
        o.band_y = by;
        by < h
    }

    #[allow(clippy::too_many_arguments)]
    fn record_v_neighbor_strip(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        source: &TiledSource,
        ti: usize,
        nbr: usize,
        pr: &ProcRect,
        apron: u32,
        apron_img: f32,
        w: u32,
        y0_img: f32,
        y1_img: f32,
        full_w: f32,
        full_h: f32,
        step_h: f32,
        radius_h: f32,
        sigma_h: f32,
        blur_pool_used: &mut usize,
        top: bool,
    ) -> Option<TileRect> {
        let strip_h = apron.max(1);
        if y1_img <= y0_img + 0.5 || apron_img < 0.5 || w == 0 {
            return None;
        }

        {
            let slot = if top {
                &mut self.blur_vstrip_top[ti]
            } else {
                &mut self.blur_vstrip_bot[ti]
            };
            if slot
                .as_ref()
                .is_none_or(|s| s.width != w || s.height != strip_h)
            {
                *slot = Some(ScratchTarget::new(device, self.format, w, strip_h));
            }
        }

        let strip_rect = TileRect {
            origin: [pr.proc.origin[0], y0_img / full_h],
            size: [pr.proc.size[0], (y1_img - y0_img) / full_h],
        };
        let nsrc = full_rect(&TileInfo {
            tile_x: source.tiles[nbr].x,
            tile_y: source.tiles[nbr].y,
            tile_w: source.tiles[nbr].width,
            tile_h: source.tiles[nbr].height,
            full_w: source.full_width,
            full_h: source.full_height,
        });

        while self.blur_uniform_pool.len() < *blur_pool_used + 1 {
            self.blur_uniform_pool
                .push(self.gaussian_blur.uniform_buffer(device));
        }
        let h_buffer = &self.blur_uniform_pool[*blur_pool_used];
        *blur_pool_used += 1;

        let strip_view = if top {
            &self.blur_vstrip_top[ti].as_ref().unwrap().view
        } else {
            &self.blur_vstrip_bot[ti].as_ref().unwrap().view
        };
        let input = &source.tiles[nbr].source_view;
        let h_lod = step_h.max(1.0).log2();
        self.gaussian_blur.record(
            device,
            queue,
            encoder,
            h_buffer,
            [step_h / full_w, 0.0],
            radius_h,
            sigma_h,
            strip_rect,
            nsrc,
            None,
            None,
            input,
            strip_view,
            None,
            h_lod,
        );
        Some(strip_rect)
    }
}
