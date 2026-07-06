use super::geom::*;
use super::*;

impl ModifierPipeline {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sort_cross_tile(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        groups: &[Vec<usize>],
        last: bool,
        prev_in_a: bool,
        vertical: bool,
        threshold: f32,
        angle: f32,
        budgeted: bool,
        scale: f32,
    ) -> bool {
        let budget = sort_buffer_limit(device).min(PIXEL_SORT_BUF_BUDGET);
        let line_len = if vertical {
            scaled(source.full_height, scale)
        } else {
            scaled(source.full_width, scale)
        };
        let stile = |ti: usize| -> (u32, u32, u32, u32) {
            let t = &source.tiles[ti];
            let sx0 = scaled(t.x, scale);
            let sy0 = scaled(t.y, scale);
            (
                sx0,
                sy0,
                (scaled(t.x + t.width, scale) - sx0).max(1),
                (scaled(t.y + t.height, scale) - sy0).max(1),
            )
        };

        let mut units: Vec<(usize, u32, u32)> = Vec::new();
        for (gi, group) in groups.iter().enumerate() {
            let (_, _, sw0, sh0) = stile(group[0]);
            let cross = if vertical { sw0 } else { sh0 };
            let per_line_bytes = if vertical {
                (line_len as u64) * 4
            } else {
                ((line_len * 4).div_ceil(256) * 256) as u64
            };
            let mem_band = (budget / per_line_bytes.max(1)).max(1) as u32;
            let band = mem_band.min(PIXEL_SORT_LINES_PER_BAND).min(cross).max(1);
            let mut c0 = 0u32;
            while c0 < cross {
                let c1 = (c0 + band).min(cross);
                units.push((gi, c0, c1));
                c0 = c1;
            }
        }

        let total = units.len() as u32;
        let (start, end) = if budgeted {
            let start = self.sort_band_cursor.min(total);
            let end = (start + PIXEL_SORT_BANDS_PER_FRAME).min(total);
            self.sort_band_cursor = end;
            (start, end)
        } else {
            (0, total)
        };

        for &(gi, c0, c1) in &units[start as usize..end as usize] {
            let group = &groups[gi];
            {
                let band_n = c1 - c0;

                let (sort_w, sort_h) = if vertical {
                    (band_n, line_len)
                } else {
                    (line_len, band_n)
                };
                let row_bytes = (sort_w * 4).div_ceil(256) * 256;
                let row_words = row_bytes / 4;
                let bytes = (row_bytes * sort_h) as u64;
                self.ensure_sort_buffers(device, bytes);
                if self.pixel_sort_uniform_pool.is_empty() {
                    self.pixel_sort_uniform_pool
                        .push(self.pixel_sort.uniform_buffer(device));
                }

                let mut enc =
                    device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                        label: Some("pixel-sort-cross-tile"),
                    });
                let (src_buf, dst_buf) = self.sort_buffers.as_ref().unwrap();
                let uniform = &self.pixel_sort_uniform_pool[0];

                let copy_origin = if vertical {
                    iced::wgpu::Origin3d { x: c0, y: 0, z: 0 }
                } else {
                    iced::wgpu::Origin3d { x: 0, y: c0, z: 0 }
                };
                let copy_extent = |sw: u32, sh: u32| {
                    if vertical {
                        iced::wgpu::Extent3d {
                            width: band_n,
                            height: sh,
                            depth_or_array_layers: 1,
                        }
                    } else {
                        iced::wgpu::Extent3d {
                            width: sw,
                            height: band_n,
                            depth_or_array_layers: 1,
                        }
                    }
                };
                let tile_offset = |sx0: u32, sy0: u32| -> u64 {
                    if vertical {
                        (sy0 as u64) * (row_bytes as u64)
                    } else {
                        (sx0 as u64) * 4
                    }
                };
                for &ti in group {
                    let (sx0, sy0, sw, sh) = stile(ti);
                    let tex = self.bank_in_tex(ti, prev_in_a);
                    enc.copy_texture_to_buffer(
                        tex_copy_info(tex, copy_origin),
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: src_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset: tile_offset(sx0, sy0),
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        copy_extent(sw, sh),
                    );
                }

                self.pixel_sort.record(
                    device, queue, &mut enc, uniform, src_buf, dst_buf, sort_w, sort_h, row_words,
                    threshold, angle,
                );

                for &ti in group {
                    let (sx0, sy0, sw, sh) = stile(ti);
                    let tex = self.bank_out_tex(ti, last, prev_in_a);
                    enc.copy_buffer_to_texture(
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: dst_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset: tile_offset(sx0, sy0),
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        tex_copy_info(tex, copy_origin),
                        copy_extent(sw, sh),
                    );
                }

                queue.submit([enc.finish()]);
            }
        }

        end >= total
    }

    pub(super) fn bank_in_tex(&self, ti: usize, prev_in_a: bool) -> &Texture {
        if prev_in_a {
            &self.bank_a[ti].as_ref().unwrap()._tex
        } else {
            &self.bank_b[ti].as_ref().unwrap()._tex
        }
    }

    pub(super) fn bank_out_tex(&self, ti: usize, last: bool, prev_in_a: bool) -> &Texture {
        if last {
            &self.tile_outputs[ti].as_ref().unwrap()._tex
        } else if prev_in_a {
            &self.bank_b[ti].as_ref().unwrap()._tex
        } else {
            &self.bank_a[ti].as_ref().unwrap()._tex
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn sort_diag_full(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &TiledSource,
        proc_set: &[usize],
        last: bool,
        prev_in_a: bool,
        threshold: f32,
        dx: i32,
        dy: i32,
        scale: f32,
    ) {
        let full_sw = scaled(source.full_width, scale).max(1);
        let full_sh = scaled(source.full_height, scale).max(1);
        let row_bytes = (full_sw * 4).div_ceil(256) * 256;
        let row_words = row_bytes / 4;
        let bytes = row_bytes as u64 * full_sh as u64;
        if bytes > sort_buffer_limit(device) {
            return;
        }
        self.ensure_sort_buffers(device, bytes);
        if self.pixel_sort_diag_uniform_pool.is_empty() {
            self.pixel_sort_diag_uniform_pool
                .push(self.pixel_sort.diag_uniform_buffer(device));
        }

        let stile = |ti: usize| -> (u32, u32, u32, u32) {
            let t = &source.tiles[ti];
            let sx0 = scaled(t.x, scale);
            let sy0 = scaled(t.y, scale);
            (
                sx0,
                sy0,
                (scaled(t.x + t.width, scale) - sx0).max(1),
                (scaled(t.y + t.height, scale) - sy0).max(1),
            )
        };
        let tile_layout = |sx0: u32, sy0: u32, sh: u32| iced::wgpu::TexelCopyBufferLayout {
            offset: sy0 as u64 * row_bytes as u64 + sx0 as u64 * 4,
            bytes_per_row: Some(row_bytes),
            rows_per_image: Some(sh),
        };

        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("pixel-sort-diag-full"),
        });
        let (src_buf, dst_buf) = self.sort_buffers.as_ref().unwrap();
        let uniform = &self.pixel_sort_diag_uniform_pool[0];

        for &ti in proc_set {
            let (sx0, sy0, sw, sh) = stile(ti);
            let tex = self.bank_in_tex(ti, prev_in_a);
            enc.copy_texture_to_buffer(
                tex_copy_info(tex, iced::wgpu::Origin3d::ZERO),
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: src_buf,
                    layout: tile_layout(sx0, sy0, sh),
                },
                iced::wgpu::Extent3d {
                    width: sw,
                    height: sh,
                    depth_or_array_layers: 1,
                },
            );
        }

        self.pixel_sort.record_diagonal(
            device, queue, &mut enc, uniform, src_buf, dst_buf, full_sw, full_sh, row_words,
            threshold, dx, dy,
        );

        for &ti in proc_set {
            let (sx0, sy0, sw, sh) = stile(ti);
            let tex = self.bank_out_tex(ti, last, prev_in_a);
            enc.copy_buffer_to_texture(
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: dst_buf,
                    layout: tile_layout(sx0, sy0, sh),
                },
                tex_copy_info(tex, iced::wgpu::Origin3d::ZERO),
                iced::wgpu::Extent3d {
                    width: sw,
                    height: sh,
                    depth_or_array_layers: 1,
                },
            );
        }

        queue.submit([enc.finish()]);
    }

    fn ensure_sort_buffers(&mut self, device: &Device, bytes: u64) {
        let need_new = self
            .sort_buffers
            .as_ref()
            .is_none_or(|(s, _)| s.size() < bytes);
        if need_new {
            self.sort_buffers = Some((
                gpu::storage_buffer(device, bytes, Some("pixel-sort-src")),
                gpu::storage_buffer(device, bytes, Some("pixel-sort-dst")),
            ));
        }
    }

    fn target_tex(&self, ti: usize, target: SortTarget) -> &Texture {
        match target {
            SortTarget::ScratchA => &self.scratch_a.as_ref().unwrap()._tex,
            SortTarget::ScratchB => &self.scratch_b.as_ref().unwrap()._tex,
            SortTarget::Output => &self.tile_outputs[ti].as_ref().unwrap()._tex,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn sort_target(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        target: SortTarget,
        w: u32,
        h: u32,
        threshold: f32,
        angle: f32,
    ) {
        match SortMode::from_angle(angle) {
            SortMode::Diagonal { dx, dy } => {
                self.sort_target_diagonal(device, queue, ti, target, w, h, threshold, dx, dy);
            }
            SortMode::Cardinal(axis) => {
                let vertical = matches!(axis, SortAxis::Vertical { .. });
                self.sort_target_cardinal(
                    device, queue, ti, target, w, h, threshold, angle, vertical,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sort_target_diagonal(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        target: SortTarget,
        w: u32,
        h: u32,
        threshold: f32,
        dx: i32,
        dy: i32,
    ) {
        let row_bytes = (w * 4).div_ceil(256) * 256;
        let row_words = row_bytes / 4;
        let bytes = row_bytes as u64 * h as u64;
        if bytes > sort_buffer_limit(device) {
            return;
        }
        self.ensure_sort_buffers(device, bytes);
        if self.pixel_sort_diag_uniform_pool.is_empty() {
            self.pixel_sort_diag_uniform_pool
                .push(self.pixel_sort.diag_uniform_buffer(device));
        }
        let (src, dst) = self.sort_buffers.as_ref().unwrap();
        let uniform = &self.pixel_sort_diag_uniform_pool[0];
        let tex = self.target_tex(ti, target);
        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("pixel-sort-bridge"),
        });
        let copy_layout = iced::wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row_bytes),
            rows_per_image: Some(h),
        };
        let extent = iced::wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            iced::wgpu::TexelCopyBufferInfo {
                buffer: src,
                layout: copy_layout,
            },
            extent,
        );
        self.pixel_sort.record_diagonal(
            device, queue, &mut enc, uniform, src, dst, w, h, row_words, threshold, dx, dy,
        );
        enc.copy_buffer_to_texture(
            iced::wgpu::TexelCopyBufferInfo {
                buffer: dst,
                layout: copy_layout,
            },
            tex.as_image_copy(),
            extent,
        );
        queue.submit([enc.finish()]);
    }

    #[allow(clippy::too_many_arguments)]
    fn sort_target_cardinal(
        &mut self,
        device: &Device,
        queue: &Queue,
        ti: usize,
        target: SortTarget,
        w: u32,
        h: u32,
        threshold: f32,
        angle: f32,
        vertical: bool,
    ) {
        let budget = sort_buffer_limit(device).min(PIXEL_SORT_BUF_BUDGET);
        let cross = if vertical { w } else { h };
        let line_px = if vertical { h as u64 } else { w as u64 };
        let band = (budget / (line_px * 4).max(1))
            .saturating_sub(64)
            .clamp(1, cross as u64) as u32;

        let (max_sw, max_sh) = if vertical { (band, h) } else { (w, band) };
        let max_bytes = (((max_sw * 4).div_ceil(256) * 256) as u64) * max_sh as u64;
        self.ensure_sort_buffers(device, max_bytes);
        if self.pixel_sort_uniform_pool.is_empty() {
            self.pixel_sort_uniform_pool
                .push(self.pixel_sort.uniform_buffer(device));
        }

        let mut c0 = 0u32;
        while c0 < cross {
            let c1 = (c0 + band).min(cross);
            let band_n = c1 - c0;
            let (sw, sh) = if vertical { (band_n, h) } else { (w, band_n) };
            let row_bytes = (sw * 4).div_ceil(256) * 256;
            let row_words = row_bytes / 4;
            let origin = if vertical {
                iced::wgpu::Origin3d { x: c0, y: 0, z: 0 }
            } else {
                iced::wgpu::Origin3d { x: 0, y: c0, z: 0 }
            };
            let copy_layout = iced::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(sh),
            };
            let extent = iced::wgpu::Extent3d {
                width: sw,
                height: sh,
                depth_or_array_layers: 1,
            };
            let (src, dst) = self.sort_buffers.as_ref().unwrap();
            let uniform = &self.pixel_sort_uniform_pool[0];
            let tex = self.target_tex(ti, target);
            let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                label: Some("pixel-sort-bridge"),
            });
            enc.copy_texture_to_buffer(
                tex_copy_info(tex, origin),
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: src,
                    layout: copy_layout,
                },
                extent,
            );
            self.pixel_sort.record(
                device, queue, &mut enc, uniform, src, dst, sw, sh, row_words, threshold, angle,
            );
            enc.copy_buffer_to_texture(
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: dst,
                    layout: copy_layout,
                },
                tex_copy_info(tex, origin),
                extent,
            );
            queue.submit([enc.finish()]);
            c0 = c1;
        }
    }
}
