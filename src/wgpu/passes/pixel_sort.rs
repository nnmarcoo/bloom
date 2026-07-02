use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, CommandEncoder,
    ComputePassDescriptor, ComputePipeline, Device, Queue, ShaderStages,
};

use crate::modifiers::pixel_sort::{SortAxis, key_cutoff, reduced_step};
use crate::wgpu::gpu;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PixelSortUniforms {
    width: u32,
    height: u32,
    cutoff: u32,
    reverse: u32,
    vertical: u32,
    row_words: u32,
    _pad1: u32,
    _pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PixelSortDiagUniforms {
    width: u32,
    height: u32,
    row_words: u32,
    cutoff: u32,
    dx: u32,
    dy: u32,
    flip_x: u32,
    flip_y: u32,
    n_lines: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

pub struct PixelSortCompute {
    pipeline: ComputePipeline,
    bgl: BindGroupLayout,
    diag_pipeline: ComputePipeline,
    diag_bgl: BindGroupLayout,
}

impl PixelSortCompute {
    pub fn new(device: &Device) -> Self {
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("pixel-sort-bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = gpu::compute_pipeline(
            device,
            include_str!("../shaders/pixel_sort_compute.wgsl"),
            "main",
            Some("pixel-sort-pipeline"),
            &bgl,
        );

        let storage_entry = |binding: u32, read_only: bool| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let diag_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("pixel-sort-diag-bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1, true),
                storage_entry(2, false),
            ],
        });
        let diag_pipeline = gpu::compute_pipeline(
            device,
            include_str!("../shaders/pixel_sort_diagonal.wgsl"),
            "main",
            Some("pixel-sort-diag-pipeline"),
            &diag_bgl,
        );

        Self {
            pipeline,
            bgl,
            diag_pipeline,
            diag_bgl,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform: &Buffer,
        src: &Buffer,
        dst: &Buffer,
        width: u32,
        height: u32,
        row_words: u32,
        threshold: f32,
        angle: f32,
    ) {
        let axis = SortAxis::from_angle(angle);
        let (vertical, reverse) = match axis {
            SortAxis::Horizontal { reverse } => (0u32, reverse as u32),
            SortAxis::Vertical { reverse } => (1u32, reverse as u32),
        };
        gpu::write_uniform(
            queue,
            uniform,
            &PixelSortUniforms {
                width,
                height,
                cutoff: key_cutoff(threshold) as u32,
                reverse,
                vertical,
                row_words,
                _pad1: 0,
                _pad2: 0,
            },
        );
        let n_lines = if vertical == 0 { height } else { width };
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("pixel-sort-bg"),
            layout: &self.bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: src.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: dst.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("pixel-sort-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(n_lines.div_ceil(64), 1, 1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_diagonal(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform: &Buffer,
        src: &Buffer,
        dst: &Buffer,
        width: u32,
        height: u32,
        row_words: u32,
        threshold: f32,
        dx: i32,
        dy: i32,
    ) {
        let (dx, dy) = reduced_step(dx, dy);
        debug_assert!(dx != 0 && dy != 0, "diagonal sort requires nonzero steps");
        if dx == 0 || dy == 0 || width == 0 || height == 0 {
            return;
        }
        let bx = dx.unsigned_abs().min(width);
        let by = dy.unsigned_abs().min(height);
        let n_lines = bx * height + (width - bx) * by;
        gpu::write_uniform(
            queue,
            uniform,
            &PixelSortDiagUniforms {
                width,
                height,
                row_words,
                cutoff: key_cutoff(threshold) as u32,
                dx: dx.unsigned_abs(),
                dy: dy.unsigned_abs(),
                flip_x: (dx < 0) as u32,
                flip_y: (dy < 0) as u32,
                n_lines,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        );
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("pixel-sort-diag-bg"),
            layout: &self.diag_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: src.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: dst.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("pixel-sort-diag-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.diag_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(n_lines.div_ceil(64), 1, 1);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<PixelSortUniforms>(device, Some("pixel-sort-uniform"))
    }

    pub fn diag_uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<PixelSortDiagUniforms>(device, Some("pixel-sort-diag-uniform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::pixel_sort::{SortMode, pixel_sort_cpu};

    use iced::wgpu::{
        CommandEncoderDescriptor, DeviceDescriptor, Instance, PowerPreference,
        RequestAdapterOptions,
    };

    fn try_device() -> Option<(Device, Queue)> {
        let instance = Instance::default();
        let adapter =
            futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .ok()?;
        futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).ok()
    }

    fn pack(rgba: &[u8]) -> Vec<u32> {
        rgba.chunks_exact(4)
            .map(|p| {
                (p[0] as u32) | ((p[1] as u32) << 8) | ((p[2] as u32) << 16) | ((p[3] as u32) << 24)
            })
            .collect()
    }

    fn unpack(words: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(words.len() * 4);
        for &w in words {
            out.push((w & 0xff) as u8);
            out.push(((w >> 8) & 0xff) as u8);
            out.push(((w >> 16) & 0xff) as u8);
            out.push(((w >> 24) & 0xff) as u8);
        }
        out
    }

    fn gpu_pixel_sort(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        w: u32,
        h: u32,
        threshold: f32,
        angle: f32,
    ) -> Vec<u8> {
        let words = pack(src);
        let bytes = (words.len() * 4) as u64;
        let src_buf = gpu::storage_buffer(device, bytes, Some("ps-src"));
        let dst_buf = gpu::storage_buffer(device, bytes, Some("ps-dst"));
        let read = gpu::readback_buffer(device, bytes, Some("ps-read"));
        queue.write_buffer(&src_buf, 0, bytemuck::cast_slice(&words));

        let pass = PixelSortCompute::new(device);
        let uniform = pass.uniform_buffer(device);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        pass.record(
            device,
            queue,
            &mut encoder,
            &uniform,
            &src_buf,
            &dst_buf,
            w,
            h,
            w,
            threshold,
            angle,
        );
        encoder.copy_buffer_to_buffer(&dst_buf, 0, &read, 0, bytes);
        queue.submit([encoder.finish()]);

        let raw = gpu::read_buffer_blocking(device, &read);
        let out_words: Vec<u32> = bytemuck::cast_slice(&raw).to_vec();
        unpack(&out_words)
    }

    struct TestTile {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        tex: iced::wgpu::Texture,
    }

    #[allow(clippy::too_many_arguments)]
    fn cross_tile_sort(
        device: &Device,
        queue: &Queue,
        tiles: &[TestTile],
        full_w: u32,
        full_h: u32,
        vertical: bool,
        threshold: f32,
        angle: f32,
        band: u32,
    ) {
        use std::collections::BTreeMap;
        let pass = PixelSortCompute::new(device);
        let uniform = pass.uniform_buffer(device);

        let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, t) in tiles.iter().enumerate() {
            let key = if vertical { t.x } else { t.y };
            groups.entry(key).or_default().push(i);
        }

        for (_, group) in groups {
            let cross = if vertical {
                tiles[group[0]].w
            } else {
                tiles[group[0]].h
            };
            let band = band.clamp(1, cross);

            let mut c0 = 0u32;
            while c0 < cross {
                let c1 = (c0 + band).min(cross);
                let band_n = c1 - c0;
                let (sort_w, sort_h) = if vertical {
                    (band_n, full_h)
                } else {
                    (full_w, band_n)
                };
                let row_bytes = (sort_w * 4).div_ceil(256) * 256;
                let row_words = row_bytes / 4;
                let bytes = (row_bytes * sort_h) as u64;
                let src_buf = gpu::storage_buffer(device, bytes, Some("ct-src"));
                let dst_buf = gpu::storage_buffer(device, bytes, Some("ct-dst"));

                let origin = if vertical {
                    iced::wgpu::Origin3d { x: c0, y: 0, z: 0 }
                } else {
                    iced::wgpu::Origin3d { x: 0, y: c0, z: 0 }
                };
                let extent = |t: &TestTile| {
                    if vertical {
                        iced::wgpu::Extent3d {
                            width: band_n,
                            height: t.h,
                            depth_or_array_layers: 1,
                        }
                    } else {
                        iced::wgpu::Extent3d {
                            width: t.w,
                            height: band_n,
                            depth_or_array_layers: 1,
                        }
                    }
                };

                let mut enc =
                    device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                for &i in &group {
                    let t = &tiles[i];
                    let offset = if vertical {
                        (t.y as u64) * (row_bytes as u64)
                    } else {
                        (t.x as u64) * 4
                    };
                    enc.copy_texture_to_buffer(
                        iced::wgpu::TexelCopyTextureInfo {
                            texture: &t.tex,
                            mip_level: 0,
                            origin,
                            aspect: iced::wgpu::TextureAspect::All,
                        },
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: &src_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset,
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        extent(t),
                    );
                }
                pass.record(
                    device, queue, &mut enc, &uniform, &src_buf, &dst_buf, sort_w, sort_h,
                    row_words, threshold, angle,
                );
                for &i in &group {
                    let t = &tiles[i];
                    let offset = if vertical {
                        (t.y as u64) * (row_bytes as u64)
                    } else {
                        (t.x as u64) * 4
                    };
                    enc.copy_buffer_to_texture(
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: &dst_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset,
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(sort_h),
                            },
                        },
                        iced::wgpu::TexelCopyTextureInfo {
                            texture: &t.tex,
                            mip_level: 0,
                            origin,
                            aspect: iced::wgpu::TextureAspect::All,
                        },
                        extent(t),
                    );
                }
                queue.submit([enc.finish()]);
                c0 = c1;
            }
        }
    }

    fn make_tiles(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        full_w: u32,
        full_h: u32,
        tw: u32,
        th: u32,
    ) -> Vec<TestTile> {
        let mut tiles = Vec::new();
        let mut y = 0;
        while y < full_h {
            let h = th.min(full_h - y);
            let mut x = 0;
            while x < full_w {
                let w = tw.min(full_w - x);
                let tex = gpu::texture_2d(
                    device,
                    w,
                    h,
                    iced::wgpu::TextureFormat::Rgba8Unorm,
                    iced::wgpu::TextureUsages::COPY_SRC | iced::wgpu::TextureUsages::COPY_DST,
                    Some("test-tile"),
                );
                let rb = (w * 4).div_ceil(256) * 256;
                let mut padded = vec![0u8; (rb * h) as usize];
                for r in 0..h {
                    for c in 0..w {
                        let si = (((y + r) * full_w + (x + c)) * 4) as usize;
                        let di = (r * rb + c * 4) as usize;
                        padded[di..di + 4].copy_from_slice(&src[si..si + 4]);
                    }
                }
                queue.write_texture(
                    tex.as_image_copy(),
                    &padded,
                    iced::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(rb),
                        rows_per_image: Some(h),
                    },
                    iced::wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                tiles.push(TestTile { x, y, w, h, tex });
                x += tw;
            }
            y += th;
        }
        tiles
    }

    fn assemble_tiles(
        device: &Device,
        queue: &Queue,
        tiles: &[TestTile],
        full_w: u32,
        full_h: u32,
    ) -> Vec<u8> {
        let mut assembled = vec![0u8; (full_w * full_h * 4) as usize];
        for t in tiles {
            let data = read_tile(device, queue, t);
            for r in 0..t.h {
                for c in 0..t.w {
                    let si = ((r * t.w + c) * 4) as usize;
                    let di = (((t.y + r) * full_w + (t.x + c)) * 4) as usize;
                    assembled[di..di + 4].copy_from_slice(&data[si..si + 4]);
                }
            }
        }
        assembled
    }

    fn read_tile(device: &Device, queue: &Queue, t: &TestTile) -> Vec<u8> {
        let row_bytes = (t.w * 4).div_ceil(256) * 256;
        let bytes = (row_bytes * t.h) as u64;
        let read = gpu::readback_buffer(device, bytes, Some("tile-read"));
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            t.tex.as_image_copy(),
            iced::wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: iced::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(t.h),
                },
            },
            iced::wgpu::Extent3d {
                width: t.w,
                height: t.h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let raw = gpu::read_buffer_blocking(device, &read);
        let mut out = Vec::with_capacity((t.w * t.h * 4) as usize);
        for r in 0..t.h {
            let start = (r * row_bytes) as usize;
            out.extend_from_slice(&raw[start..start + (t.w * 4) as usize]);
        }
        out
    }

    #[test]
    fn cross_tile_pixel_sort_matches_cpu_reference() {
        let Some((device, queue)) = try_device() else {
            eprintln!("cross_tile_pixel_sort_matches_cpu_reference: no GPU adapter, skipping");
            return;
        };

        let (full_w, full_h) = (40u32, 24u32);
        let (tw, th) = (16u32, 10u32);

        let mut src = vec![0u8; (full_w * full_h * 4) as usize];
        let mut s: u32 = 0x9e37_79b9;
        for b in src.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (s >> 24) as u8;
        }
        for p in src.chunks_exact_mut(4) {
            p[3] = 255;
        }

        for &band in &[3u32, 7, 1000] {
            for &(vertical, angle) in
                &[(false, 0.0f32), (false, 180.0), (true, 90.0), (true, 270.0)]
            {
                for &threshold in &[0.0f32, 0.3, 0.6] {
                    let tiles = make_tiles(&device, &queue, &src, full_w, full_h, tw, th);

                    cross_tile_sort(
                        &device, &queue, &tiles, full_w, full_h, vertical, threshold, angle, band,
                    );

                    let assembled = assemble_tiles(&device, &queue, &tiles, full_w, full_h);

                    let cpu =
                        pixel_sort_cpu(&src, full_w as usize, full_h as usize, threshold, angle);
                    let mism = cpu.iter().zip(&assembled).filter(|(a, b)| a != b).count();
                    assert_eq!(
                        mism, 0,
                        "cross-tile != CPU at band={band} vertical={vertical} angle={angle} threshold={threshold}: {mism} bytes"
                    );
                }
            }
        }
    }

    #[test]
    fn gpu_pixel_sort_matches_cpu_reference() {
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_pixel_sort_matches_cpu_reference: no GPU adapter, skipping");
            return;
        };

        let (w, h) = (53usize, 29usize);
        let mut src = vec![0u8; w * h * 4];
        let mut s: u32 = 0x1234_5678;
        for b in src.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (s >> 24) as u8;
        }
        for p in src.chunks_exact_mut(4) {
            p[3] = 255;
        }

        for &angle in &[0.0f32, 90.0, 180.0, 270.0] {
            for &threshold in &[0.0f32, 0.25, 0.5, 0.75] {
                let cpu = pixel_sort_cpu(&src, w, h, threshold, angle);
                let gpu =
                    gpu_pixel_sort(&device, &queue, &src, w as u32, h as u32, threshold, angle);
                let mismatches = cpu.iter().zip(&gpu).filter(|(a, b)| a != b).count();
                assert_eq!(
                    mismatches, 0,
                    "GPU != CPU pixel sort at angle {angle} threshold {threshold}: {mismatches} bytes differ"
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gpu_pixel_sort_diagonal(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        w: u32,
        h: u32,
        threshold: f32,
        dx: i32,
        dy: i32,
        row_words: u32,
    ) -> Vec<u8> {
        let words = pack(src);
        let rw = row_words as usize;
        let mut padded = vec![0u32; rw * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                padded[y * rw + x] = words[y * w as usize + x];
            }
        }
        let bytes = (padded.len() * 4) as u64;
        let src_buf = gpu::storage_buffer(device, bytes, Some("psd-src"));
        let dst_buf = gpu::storage_buffer(device, bytes, Some("psd-dst"));
        let read = gpu::readback_buffer(device, bytes, Some("psd-read"));
        queue.write_buffer(&src_buf, 0, bytemuck::cast_slice(&padded));

        let pass = PixelSortCompute::new(device);
        let uniform = pass.diag_uniform_buffer(device);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        pass.record_diagonal(
            device,
            queue,
            &mut encoder,
            &uniform,
            &src_buf,
            &dst_buf,
            w,
            h,
            row_words,
            threshold,
            dx,
            dy,
        );
        encoder.copy_buffer_to_buffer(&dst_buf, 0, &read, 0, bytes);
        queue.submit([encoder.finish()]);

        let raw = gpu::read_buffer_blocking(device, &read);
        let out_words: Vec<u32> = bytemuck::cast_slice(&raw).to_vec();
        let mut tight = vec![0u32; w as usize * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                tight[y * w as usize + x] = out_words[y * rw + x];
            }
        }
        unpack(&tight)
    }

    #[test]
    fn gpu_diagonal_pixel_sort_matches_cpu_reference() {
        let Some((device, queue)) = try_device() else {
            eprintln!("gpu_diagonal_pixel_sort_matches_cpu_reference: no GPU adapter, skipping");
            return;
        };

        let (w, h) = (53usize, 29usize);
        let mut src = vec![0u8; w * h * 4];
        let mut s: u32 = 0x1234_5678;
        for b in src.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (s >> 24) as u8;
        }
        for p in src.chunks_exact_mut(4) {
            p[3] = 255;
        }

        let tight = w as u32;
        let padded = (w as u32 * 4).div_ceil(256) * 256 / 4;
        for &angle in &[
            45.0f32, 315.0, 26.565, 63.435, 71.565, 135.0, 206.565, 341.565,
        ] {
            let (dx, dy) = match SortMode::from_angle(angle) {
                SortMode::Diagonal { dx, dy } => (dx, dy),
                other => panic!("angle {angle} did not map to a diagonal: {other:?}"),
            };
            for &threshold in &[0.0f32, 0.25, 0.5, 0.75] {
                let cpu = pixel_sort_cpu(&src, w, h, threshold, angle);
                for &row_words in &[tight, padded] {
                    let gpu = gpu_pixel_sort_diagonal(
                        &device, &queue, &src, w as u32, h as u32, threshold, dx, dy, row_words,
                    );
                    let mismatches = cpu.iter().zip(&gpu).filter(|(a, b)| a != b).count();
                    assert_eq!(
                        mismatches, 0,
                        "GPU != CPU diagonal at angle {angle} ({dx},{dy}) threshold {threshold} row_words {row_words}: {mismatches} bytes"
                    );
                }
            }
        }
    }

    #[test]
    fn cross_tile_diagonal_pixel_sort_matches_cpu_reference() {
        let Some((device, queue)) = try_device() else {
            eprintln!(
                "cross_tile_diagonal_pixel_sort_matches_cpu_reference: no GPU adapter, skipping"
            );
            return;
        };

        let (full_w, full_h) = (40u32, 24u32);
        let (tw, th) = (16u32, 10u32);

        let mut src = vec![0u8; (full_w * full_h * 4) as usize];
        let mut s: u32 = 0x9e37_79b9;
        for b in src.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (s >> 24) as u8;
        }
        for p in src.chunks_exact_mut(4) {
            p[3] = 255;
        }

        let pass = PixelSortCompute::new(&device);
        let uniform = pass.diag_uniform_buffer(&device);
        let row_bytes = (full_w * 4).div_ceil(256) * 256;
        let row_words = row_bytes / 4;
        let bytes = (row_bytes * full_h) as u64;

        for &angle in &[45.0f32, 135.0, 315.0, 26.565] {
            let (dx, dy) = match SortMode::from_angle(angle) {
                SortMode::Diagonal { dx, dy } => (dx, dy),
                other => panic!("angle {angle} did not map to a diagonal: {other:?}"),
            };
            for &threshold in &[0.0f32, 0.3, 0.6] {
                let tiles = make_tiles(&device, &queue, &src, full_w, full_h, tw, th);
                let src_buf = gpu::storage_buffer(&device, bytes, Some("ctd-src"));
                let dst_buf = gpu::storage_buffer(&device, bytes, Some("ctd-dst"));

                let mut enc =
                    device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                for t in &tiles {
                    let offset = (t.y as u64) * (row_bytes as u64) + (t.x as u64) * 4;
                    enc.copy_texture_to_buffer(
                        t.tex.as_image_copy(),
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: &src_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset,
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(t.h),
                            },
                        },
                        iced::wgpu::Extent3d {
                            width: t.w,
                            height: t.h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                pass.record_diagonal(
                    &device, &queue, &mut enc, &uniform, &src_buf, &dst_buf, full_w, full_h,
                    row_words, threshold, dx, dy,
                );
                for t in &tiles {
                    let offset = (t.y as u64) * (row_bytes as u64) + (t.x as u64) * 4;
                    enc.copy_buffer_to_texture(
                        iced::wgpu::TexelCopyBufferInfo {
                            buffer: &dst_buf,
                            layout: iced::wgpu::TexelCopyBufferLayout {
                                offset,
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(t.h),
                            },
                        },
                        t.tex.as_image_copy(),
                        iced::wgpu::Extent3d {
                            width: t.w,
                            height: t.h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                queue.submit([enc.finish()]);

                let assembled = assemble_tiles(&device, &queue, &tiles, full_w, full_h);
                let cpu = pixel_sort_cpu(&src, full_w as usize, full_h as usize, threshold, angle);
                let mism = cpu.iter().zip(&assembled).filter(|(a, b)| a != b).count();
                assert_eq!(
                    mism, 0,
                    "cross-tile diagonal != CPU at angle {angle} ({dx},{dy}) threshold {threshold}: {mism} bytes"
                );
            }
        }
    }
}
