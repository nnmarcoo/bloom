use super::*;
use iced::wgpu::Buffer;

pub(super) struct SlabPiece<'a> {
    pub tex: &'a Texture,
    pub rect: [u32; 4],
}

pub(super) struct TexSlab {
    pub tex: Texture,
    pub view: TextureView,
    pub rect: [u32; 4],
}

pub(super) struct BufSlab {
    pub buf: Buffer,
    pub rect: [u32; 4],
    pub row_words: u32,
}

fn isect(a: [u32; 4], b: [u32; 4]) -> Option<[u32; 4]> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}

fn extent(r: [u32; 4]) -> iced::wgpu::Extent3d {
    iced::wgpu::Extent3d {
        width: r[2] - r[0],
        height: r[3] - r[1],
        depth_or_array_layers: 1,
    }
}

fn origin(r: [u32; 4], base: [u32; 4]) -> iced::wgpu::Origin3d {
    iced::wgpu::Origin3d {
        x: r[0] - base[0],
        y: r[1] - base[1],
        z: 0,
    }
}

pub(super) fn gather_tex(
    device: &Device,
    encoder: &mut CommandEncoder,
    format: TextureFormat,
    pieces: &[SlabPiece],
    rect: [u32; 4],
) -> TexSlab {
    let tex = gpu::texture_2d(
        device,
        (rect[2] - rect[0]).max(1),
        (rect[3] - rect[1]).max(1),
        format,
        TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST,
        Some("slab-tex"),
    );
    for p in pieces {
        if let Some(i) = isect(p.rect, rect) {
            encoder.copy_texture_to_texture(
                tex_copy_info(p.tex, origin(i, p.rect)),
                tex_copy_info(&tex, origin(i, rect)),
                extent(i),
            );
        }
    }
    let view = tex.create_view(&Default::default());
    TexSlab { tex, view, rect }
}

pub(super) fn scatter_tex(encoder: &mut CommandEncoder, slab: &TexSlab, pieces: &[SlabPiece]) {
    for p in pieces {
        if let Some(i) = isect(p.rect, slab.rect) {
            encoder.copy_texture_to_texture(
                tex_copy_info(&slab.tex, origin(i, slab.rect)),
                tex_copy_info(p.tex, origin(i, p.rect)),
                extent(i),
            );
        }
    }
}

fn buf_layout(slab_rect: [u32; 4], i: [u32; 4], row_bytes: u32) -> iced::wgpu::TexelCopyBufferLayout {
    iced::wgpu::TexelCopyBufferLayout {
        offset: (i[1] - slab_rect[1]) as u64 * row_bytes as u64 + (i[0] - slab_rect[0]) as u64 * 4,
        bytes_per_row: Some(row_bytes),
        rows_per_image: Some(i[3] - i[1]),
    }
}

pub(super) fn gather_buf(
    device: &Device,
    encoder: &mut CommandEncoder,
    pieces: &[SlabPiece],
    rect: [u32; 4],
) -> BufSlab {
    let w = (rect[2] - rect[0]).max(1);
    let h = (rect[3] - rect[1]).max(1);
    let row_bytes = (w * 4).div_ceil(256) * 256;
    let buf = gpu::storage_buffer(device, row_bytes as u64 * h as u64, Some("slab-buf"));
    for p in pieces {
        if let Some(i) = isect(p.rect, rect) {
            encoder.copy_texture_to_buffer(
                tex_copy_info(p.tex, origin(i, p.rect)),
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: buf_layout(rect, i, row_bytes),
                },
                extent(i),
            );
        }
    }
    BufSlab {
        buf,
        rect,
        row_words: row_bytes / 4,
    }
}

pub(super) fn scatter_buf(encoder: &mut CommandEncoder, slab: &BufSlab, pieces: &[SlabPiece]) {
    let row_bytes = slab.row_words * 4;
    for p in pieces {
        if let Some(i) = isect(p.rect, slab.rect) {
            encoder.copy_buffer_to_texture(
                iced::wgpu::TexelCopyBufferInfo {
                    buffer: &slab.buf,
                    layout: buf_layout(slab.rect, i, row_bytes),
                },
                tex_copy_info(p.tex, origin(i, p.rect)),
                extent(i),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::goldens::{GPU_LOCK, read_texture, try_device};
    use super::*;
    use iced::wgpu::CommandEncoderDescriptor;

    const FULL_W: u32 = 80;
    const FULL_H: u32 = 60;
    const GRID: [[u32; 4]; 4] = [
        [0, 0, 40, 30],
        [40, 0, 80, 30],
        [0, 30, 40, 60],
        [40, 30, 80, 60],
    ];
    const RECT: [u32; 4] = [10, 5, 70, 55];

    fn full_pixels() -> Vec<u8> {
        let mut v = Vec::with_capacity((FULL_W * FULL_H * 4) as usize);
        let mut s = 0xdeadbeefu32;
        for _ in 0..FULL_W * FULL_H * 4 {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            v.push((s >> 24) as u8);
        }
        v
    }

    fn make_grid_textures(device: &Device, queue: &Queue, full: &[u8], zeroed: bool) -> Vec<Texture> {
        GRID.iter()
            .map(|r| {
                let (w, h) = (r[2] - r[0], r[3] - r[1]);
                let tex = gpu::texture_2d(
                    device,
                    w,
                    h,
                    TextureFormat::Rgba8Unorm,
                    TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC
                        | TextureUsages::COPY_DST,
                    Some("slab-test-tile"),
                );
                let mut data = vec![0u8; (w * h * 4) as usize];
                if !zeroed {
                    for y in 0..h {
                        let s = (((r[1] + y) * FULL_W + r[0]) * 4) as usize;
                        let d = (y * w * 4) as usize;
                        data[d..d + (w * 4) as usize]
                            .copy_from_slice(&full[s..s + (w * 4) as usize]);
                    }
                }
                queue.write_texture(
                    tex.as_image_copy(),
                    &data,
                    iced::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: None,
                    },
                    extent([0, 0, w, h]),
                );
                tex
            })
            .collect()
    }

    fn compose_full(device: &Device, queue: &Queue, tiles: &[Texture]) -> Vec<u8> {
        let mut full = vec![0u8; (FULL_W * FULL_H * 4) as usize];
        for (tex, r) in tiles.iter().zip(GRID.iter()) {
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            let px = read_texture(device, queue, tex, w, h);
            for y in 0..h {
                let d = (((r[1] + y) * FULL_W + r[0]) * 4) as usize;
                let s = (y * w * 4) as usize;
                full[d..d + (w * 4) as usize].copy_from_slice(&px[s..s + (w * 4) as usize]);
            }
        }
        full
    }

    fn expected_after_scatter(full: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; full.len()];
        for y in RECT[1]..RECT[3] {
            let s = ((y * FULL_W + RECT[0]) * 4) as usize;
            let n = ((RECT[2] - RECT[0]) * 4) as usize;
            out[s..s + n].copy_from_slice(&full[s..s + n]);
        }
        out
    }

    fn pieces<'a>(tiles: &'a [Texture]) -> Vec<SlabPiece<'a>> {
        tiles
            .iter()
            .zip(GRID.iter())
            .map(|(tex, &rect)| SlabPiece { tex, rect })
            .collect()
    }

    #[test]
    fn tex_slab_gather_scatter_round_trip() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            return;
        };
        let full = full_pixels();
        let sources = make_grid_textures(&device, &queue, &full, false);
        let targets = make_grid_textures(&device, &queue, &full, true);

        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("slab-test"),
        });
        let slab = gather_tex(
            &device,
            &mut enc,
            TextureFormat::Rgba8Unorm,
            &pieces(&sources),
            RECT,
        );
        scatter_tex(&mut enc, &slab, &pieces(&targets));
        queue.submit([enc.finish()]);

        let slab_px = read_texture(
            &device,
            &queue,
            &slab.tex,
            RECT[2] - RECT[0],
            RECT[3] - RECT[1],
        );
        for y in 0..RECT[3] - RECT[1] {
            let s = (((RECT[1] + y) * FULL_W + RECT[0]) * 4) as usize;
            let d = (y * (RECT[2] - RECT[0]) * 4) as usize;
            let n = ((RECT[2] - RECT[0]) * 4) as usize;
            assert_eq!(&slab_px[d..d + n], &full[s..s + n], "gather row {y}");
        }

        let after = compose_full(&device, &queue, &targets);
        assert_eq!(after, expected_after_scatter(&full));
    }

    #[test]
    fn buf_slab_gather_scatter_round_trip() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            return;
        };
        let full = full_pixels();
        let sources = make_grid_textures(&device, &queue, &full, false);
        let targets = make_grid_textures(&device, &queue, &full, true);

        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("slab-test"),
        });
        let slab = gather_buf(&device, &mut enc, &pieces(&sources), RECT);
        assert!(slab.row_words * 4 >= (RECT[2] - RECT[0]) * 4);
        scatter_buf(&mut enc, &slab, &pieces(&targets));
        queue.submit([enc.finish()]);

        let after = compose_full(&device, &queue, &targets);
        assert_eq!(after, expected_after_scatter(&full));
    }
}
