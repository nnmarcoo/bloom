use super::*;
use crate::modifiers::kinds::{
    ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort, Posterize,
};
use crate::wgpu::media::image_data::ImageData;
use crate::wgpu::passes::display::DisplayPass;
use iced::wgpu::CommandEncoderDescriptor;

const GOLDEN_W: u32 = 96;
const GOLDEN_H: u32 = 64;
const FORCED_TILE_DIM: u32 = 48;

pub(super) use crate::wgpu::test_device::try_device;

fn test_pixels(w: u32, h: u32) -> Vec<u8> {
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

fn make_source(
    device: &Device,
    queue: &Queue,
    image: &ImageData,
    tile_dim: Option<u32>,
) -> TiledSource {
    let format = TextureFormat::Rgba8Unorm;
    let display = DisplayPass::new(device, format);
    let (blit_pipeline, blit_bgl) = gpu::blit_pipeline(device, format);
    let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor::default());
    let mut source = TiledSource::new(
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
        tile_dim,
    )
    .expect("tiled source");
    for t in &mut source.tiles {
        t.proc_rect_px = Some([
            t.x as f32,
            t.y as f32,
            (t.x + t.width) as f32,
            (t.y + t.height) as f32,
        ]);
    }
    source
}

pub(super) fn read_texture(
    device: &Device,
    queue: &Queue,
    tex: &Texture,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let row_bytes = (w * 4).div_ceil(256) * 256;
    let buf = gpu::readback_buffer(device, row_bytes as u64 * h as u64, Some("golden-readback"));
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("golden-readback"),
    });
    enc.copy_texture_to_buffer(
        tex_copy_info(tex, iced::wgpu::Origin3d::ZERO),
        iced::wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: iced::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(h),
            },
        },
        iced::wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    let raw = gpu::read_buffer_blocking(device, &buf);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let s = (y * row_bytes) as usize;
        out.extend_from_slice(&raw[s..s + (w * 4) as usize]);
    }
    out
}

fn assemble(
    device: &Device,
    queue: &Queue,
    mp: &ModifierPipeline,
    source: &TiledSource,
) -> Vec<u8> {
    let fw = source.full_width;
    let mut full = vec![0u8; (fw * source.full_height * 4) as usize];
    for (ti, tile) in source.tiles.iter().enumerate() {
        let o = mp.tile_outputs[ti]
            .as_ref()
            .unwrap_or_else(|| panic!("tile {ti} has no output"));
        assert_eq!(
            (o.width, o.height),
            (tile.width, tile.height),
            "tile {ti} output not at native scale"
        );
        let px = read_texture(device, queue, &o._tex, o.width, o.height);
        for r in 0..tile.height {
            let d = (((tile.y + r) * fw + tile.x) * 4) as usize;
            let s = (r * tile.width * 4) as usize;
            let n = (tile.width * 4) as usize;
            full[d..d + n].copy_from_slice(&px[s..s + n]);
        }
    }
    full
}

fn assemble_scaled(
    device: &Device,
    queue: &Queue,
    mp: &ModifierPipeline,
    source: &TiledSource,
    s: f32,
) -> Vec<u8> {
    let fw = ((source.full_width as f32 * s).round() as u32).max(1);
    let fh = ((source.full_height as f32 * s).round() as u32).max(1);
    let mut full = vec![0u8; (fw * fh * 4) as usize];
    for ti in 0..source.tiles.len() {
        let Some(o) = mp.tile_outputs[ti].as_ref() else {
            continue;
        };
        let px = o.proc_px.expect("executor outputs always carry proc_px");
        let x0 = (px[0] * s).round() as u32;
        let y0 = (px[1] * s).round() as u32;
        let data = read_texture(device, queue, &o._tex, o.width, o.height);
        for r in 0..o.height.min(fh.saturating_sub(y0)) {
            let cols = o.width.min(fw.saturating_sub(x0));
            let d = (((y0 + r) * fw + x0) * 4) as usize;
            let src = (r * o.width * 4) as usize;
            full[d..d + (cols * 4) as usize].copy_from_slice(&data[src..src + (cols * 4) as usize]);
        }
    }
    full
}

/// Restricts every tile's ROI to its intersection with a centred window
/// covering `frac` of the image, mimicking a zoomed-in viewport.
///
/// This is the piece the rest of the golden suite lacks: `make_source` gives
/// every tile a *full-bounds* `proc_rect_px`, so the region the executor
/// derives is always the whole frame and no apron or reach value can change
/// the result. With a partial ROI a stage must actually fetch beyond what it
/// writes, which is what makes under-fetch observable.
///
/// Tiles falling entirely outside the window get `None`, matching what the
/// view pipeline does when a tile is scrolled off screen.
fn set_partial_roi(source: &mut TiledSource, frac: f32) {
    let (fw, fh) = (source.full_width as f32, source.full_height as f32);
    let (half_w, half_h) = (fw * frac * 0.5, fh * frac * 0.5);
    let view = [
        fw * 0.5 - half_w,
        fh * 0.5 - half_h,
        fw * 0.5 + half_w,
        fh * 0.5 + half_h,
    ];
    for t in &mut source.tiles {
        let (tl, tt) = (t.x as f32, t.y as f32);
        let (tr, tb) = (tl + t.width as f32, tt + t.height as f32);
        let isect = [
            view[0].max(tl),
            view[1].max(tt),
            view[2].min(tr),
            view[3].min(tb),
        ];
        t.proc_rect_px = (isect[2] > isect[0] && isect[3] > isect[1]).then_some(isect);
    }
}

/// Compares GPU output against the CPU oracle *only where the GPU actually
/// rendered*, which is what a partial ROI requires: outside the ROI the tile
/// textures hold nothing meaningful, so a whole-image diff would drown the
/// signal in regions neither path claims to have produced.
///
/// Returns `(max_diff, pct_over, compared)`. `compared` guards against the
/// degenerate pass where the ROI collapsed and nothing was checked at all.
fn diff_within_roi(
    device: &Device,
    queue: &Queue,
    mp: &ModifierPipeline,
    source: &TiledSource,
    cpu_full: &[u8],
    tol: u8,
) -> (u8, f64, usize) {
    let fw = source.full_width;
    let mut max_d = 0u8;
    let mut over = 0usize;
    let mut compared = 0usize;

    for (ti, tile) in source.tiles.iter().enumerate() {
        let (Some(o), Some(roi)) = (mp.tile_outputs[ti].as_ref(), tile.proc_rect_px) else {
            continue;
        };
        let px = o.proc_px.expect("executor outputs always carry proc_px");
        let data = read_texture(device, queue, &o._tex, o.width, o.height);

        // Walk the ROI in image space and map into the tile's own texture,
        // which covers `px` -- a superset of the ROI once the apron is added.
        let x0 = roi[0].ceil() as u32;
        let y0 = roi[1].ceil() as u32;
        let x1 = (roi[2].floor() as u32).min(fw);
        let y1 = (roi[3].floor() as u32).min(source.full_height);
        for y in y0..y1 {
            for x in x0..x1 {
                let lx = x as f32 - px[0];
                let ly = y as f32 - px[1];
                if lx < 0.0 || ly < 0.0 || lx >= o.width as f32 || ly >= o.height as f32 {
                    continue;
                }
                let s = ((ly as u32 * o.width + lx as u32) * 4) as usize;
                let d = ((y * fw + x) * 4) as usize;
                for c in 0..4 {
                    let diff = data[s + c].abs_diff(cpu_full[d + c]);
                    max_d = max_d.max(diff);
                    if diff > tol {
                        over += 1;
                    }
                    compared += 1;
                }
            }
        }
    }
    (max_d, over as f64 * 100.0 / compared.max(1) as f64, compared)
}

/// GPU-vs-oracle agreement with a partial ROI, at a size where a stage's
/// reach matters. `frac` shrinks the visible window; `tol` is the per-channel
/// tolerance already used by the other goldens.
fn run_roi_golden(
    label: &str,
    modifiers: &[Modifier],
    tile_dim: u32,
    frac: f32,
    tol: u8,
    w: u32,
    h: u32,
) {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels.clone(), w, h);
    let mut source = make_source(&device, &queue, &image, Some(tile_dim));
    set_partial_roi(&mut source, frac);

    let partial = source
        .tiles
        .iter()
        .filter(|t| {
            t.proc_rect_px.is_some_and(|r| {
                r[0] > t.x as f32
                    || r[1] > t.y as f32
                    || r[2] < (t.x + t.width) as f32
                    || r[3] < (t.y + t.height) as f32
            })
        })
        .count();
    assert!(
        partial > 0,
        "{label}: no tile got a strictly-partial ROI, so this test would not \
         exercise anything the full-bounds goldens miss"
    );

    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, modifiers, label);

    // The apron is only observable when the rendered region is a strict subset
    // of the tile. `ROI_MARGIN_PX` (256) is added to every ROI before clamping,
    // so with tiles at or below that size the clamp always restores full
    // bounds and no apron value can matter. Fail loudly rather than pass
    // vacuously if the geometry ever stops satisfying that.
    let strict = source
        .tiles
        .iter()
        .enumerate()
        .filter(|(ti, tile)| {
            mp.tile_outputs[*ti]
                .as_ref()
                .and_then(|o| o.proc_px)
                .is_some_and(|p| {
                    p[0] > tile.x as f32
                        || p[1] > tile.y as f32
                        || p[2] < (tile.x + tile.width) as f32
                        || p[3] < (tile.y + tile.height) as f32
                })
        })
        .count();
    assert!(
        strict > 0,
        "{label}: every tile rendered its full bounds, so the apron is not \
         observable here. Tiles must exceed ROI_MARGIN_PX ({ROI_MARGIN_PX}) \
         for a partial region to survive clamping."
    );

    let cpu_full = crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, w, h);
    let (max_d, pct, compared) = diff_within_roi(&device, &queue, &mp, &source, &cpu_full, tol);
    assert!(
        compared > 0,
        "{label}: compared no pixels; the ROI collapsed and the test proved nothing"
    );
    assert!(
        max_d <= tol,
        "{label}: GPU diverges from oracle inside the ROI: max channel diff \
         {max_d} > tol {tol} ({pct:.3}% of {compared} channels over). A stage \
         is reading input the ROI did not fetch."
    );
}

#[test]
fn tiling_invisible_blur_at_downscale() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let chain = blur_chain();
    let mut outs: Vec<Vec<u8>> = Vec::new();
    for tile_dim in [None, Some(FORCED_TILE_DIM)] {
        let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
        let image = ImageData::new(pixels, GOLDEN_W, GOLDEN_H);
        let mut source = make_source(&device, &queue, &image, tile_dim);
        source.physical_scale = 0.4;
        let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);
        converge(&mut mp, &device, &queue, &source, &chain, "downscale");
        outs.push(assemble_scaled(&device, &queue, &mp, &source, 0.5));
    }
    let (max_d, pct) = diff_stats(&outs[0], &outs[1], 1);
    assert!(
        max_d <= 1,
        "tiled downscaled blur diverges from single-tile: max diff {max_d} ({pct:.3}% over)"
    );
}

#[test]
fn oversized_sort_lines_reduce_scale_instead_of_failing() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (w, h) = (20000u32, 256u32);
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels, w, h);
    let source = make_source(&device, &queue, &image, None);
    assert!(source.tiles.len() > 1);
    let chain = sort_cardinal_chain_angle(0.0);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, &chain, "oversized-sort");
    let o = mp.tile_outputs[0].as_ref().expect("output");
    assert!(o.valid);
    assert!(
        o.quality_scale < 1.0,
        "expected reduced processing scale, got {}",
        o.quality_scale
    );
}

fn sort_cardinal_chain_angle(angle: f32) -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::PixelSort(PixelSort {
        threshold: 0.4,
        angle,
    }))]
}

#[test]
fn kernel_chain_handles_missing_tile_roi() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
    let image = ImageData::new(pixels.clone(), GOLDEN_W, GOLDEN_H);
    let mut source = make_source(&device, &queue, &image, Some(FORCED_TILE_DIM));
    source.tiles[3].proc_rect_px = None;
    let chain = blur_chain();
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);
    converge(&mut mp, &device, &queue, &source, &chain, "missing-roi");
    let gpu_img = assemble(&device, &queue, &mp, &source);
    let cpu_img = crate::modifiers::cpu::render_full(&chain, &[], &[], &pixels, GOLDEN_W, GOLDEN_H);
    let (max_d, pct) = diff_stats(&gpu_img, &cpu_img, 4);
    assert!(
        max_d <= 4,
        "missing-roi tile diverges: max diff {max_d} ({pct:.3}% over)"
    );
}

fn diff_stats(a: &[u8], b: &[u8], tol: u8) -> (u8, f64) {
    assert_eq!(a.len(), b.len());
    let mut max_d = 0u8;
    let mut over = 0usize;
    for (&x, &y) in a.iter().zip(b) {
        let d = x.abs_diff(y);
        max_d = max_d.max(d);
        if d > tol {
            over += 1;
        }
    }
    (max_d, over as f64 * 100.0 / a.len() as f64)
}

pub(super) use crate::wgpu::test_device::GPU_LOCK;

fn converge(
    mp: &mut ModifierPipeline,
    device: &Device,
    queue: &Queue,
    source: &TiledSource,
    modifiers: &[Modifier],
    label: &str,
) {
    let mut dirty = true;
    for _ in 0..64 {
        mp.prepare(device, queue, source, modifiers, dirty);
        dirty = false;
        let all_valid = (0..source.tiles.len()).all(|ti| {
            mp.tile_outputs
                .get(ti)
                .and_then(|o| o.as_ref())
                .is_some_and(|o| o.valid)
        });
        if !mp.reprocess_pending() && all_valid {
            return;
        }
    }
    panic!("{label}: pipeline did not converge in 64 frames");
}

fn run_golden(label: &str, modifiers: &[Modifier], tile_dim: Option<u32>, tol: u8) {
    run_golden_dims(label, modifiers, tile_dim, tol, GOLDEN_W, GOLDEN_H);
}

fn run_golden_dims(
    label: &str,
    modifiers: &[Modifier],
    tile_dim: Option<u32>,
    tol: u8,
    w: u32,
    h: u32,
) {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels.clone(), w, h);
    let source = make_source(&device, &queue, &image, tile_dim);
    if tile_dim.is_some() {
        assert!(
            source.tiles.len() > 1,
            "{label}: expected forced multi-tiling, got {} tiles",
            source.tiles.len()
        );
    }

    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, modifiers, label);

    let gpu_img = assemble(&device, &queue, &mp, &source);
    let cpu_img = crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, w, h);
    let (max_d, pct_over) = diff_stats(&gpu_img, &cpu_img, tol);
    assert!(
        max_d <= tol,
        "{label}: GPU vs CPU oracle diverges: max channel diff {max_d} > tol {tol} ({pct_over:.3}% of channels over)"
    );
}

fn pointwise_chain() -> Vec<Modifier> {
    vec![
        Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.5 })),
        Modifier::new(ModifierKind::Posterize(Posterize { levels: 6 })),
    ]
}

fn blur_chain() -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
        radius: 4.0,
    }))]
}

fn sort_cardinal_chain() -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::PixelSort(PixelSort {
        threshold: 0.4,
        angle: 90.0,
    }))]
}

fn sort_diag_chain() -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::PixelSort(PixelSort {
        threshold: 0.4,
        angle: 45.0,
    }))]
}

fn motion_blur_chain() -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::MotionBlur(MotionBlur {
        angle: 30.0,
        distance: 10.0,
    }))]
}

fn ca_chain() -> Vec<Modifier> {
    vec![Modifier::new(ModifierKind::ChromaticAberration(
        ChromaticAberration { amount: 8.0 },
    ))]
}

#[test]
fn golden_pointwise_single_tile() {
    run_golden("pointwise/1-tile", &pointwise_chain(), None, 2);
}

#[test]
fn golden_pointwise_multi_tile() {
    run_golden(
        "pointwise/2x2",
        &pointwise_chain(),
        Some(FORCED_TILE_DIM),
        2,
    );
}

#[test]
fn golden_blur_single_tile() {
    run_golden("blur/1-tile", &blur_chain(), None, 4);
}

#[test]
fn golden_blur_multi_tile() {
    run_golden("blur/2x2", &blur_chain(), Some(FORCED_TILE_DIM), 4);
}

fn mixed_chain() -> Vec<Modifier> {
    use crate::modifiers::kinds::Invert;
    vec![
        Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.5 })),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
        Modifier::new(ModifierKind::Invert(Invert { amount: 1.0 })),
    ]
}

#[test]
fn blur_extreme_radius_converges_capped() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (w, h) = (256u32, 256u32);
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels, w, h);
    let source = make_source(&device, &queue, &image, None);
    let chain = vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
        radius: 500.0,
    }))];
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, &chain, "blur-500");
    let out = assemble(&device, &queue, &mp, &source);
    assert!(out.chunks_exact(4).any(|p| p[0] > 0 && p[3] > 0));
}

#[test]
fn golden_blur_banded_tall_image() {
    let chain = vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
        radius: 100.0,
    }))];
    run_golden_dims("blur-banded/96x3000", &chain, None, 4, 96, 3000);
}

#[test]
fn golden_mixed_pointwise_blur_single_tile() {
    run_golden("pointwise+blur/1-tile", &mixed_chain(), None, 4);
}

#[test]
fn golden_mixed_pointwise_blur_multi_tile() {
    run_golden(
        "pointwise+blur/2x2",
        &mixed_chain(),
        Some(FORCED_TILE_DIM),
        4,
    );
}

#[test]
fn golden_sort_then_blur_multi_tile() {
    let chain = vec![
        Modifier::new(ModifierKind::PixelSort(PixelSort {
            threshold: 0.4,
            angle: 0.0,
        })),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
    ];
    run_golden("sort+blur/2x2", &chain, Some(FORCED_TILE_DIM), 4);
}

#[test]
fn golden_drawing_multi_tile() {
    use crate::export::{ExportData, ExportFrame, ExportSource, render_still_rgba};
    use crate::modifiers::kinds::{Drawing, Stroke};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
    let image = ImageData::new(pixels.clone(), GOLDEN_W, GOLDEN_H);
    let source = make_source(&device, &queue, &image, Some(FORCED_TILE_DIM));
    let mut d = Drawing::default();
    d.strokes.push(Stroke {
        points: vec![[0.1, 0.15], [0.5, 0.5], [0.85, 0.75]],
        size: 12.0,
        hardness: 0.6,
        opacity: 0.9,
        color: [0.9, 0.2, 0.1],
    });
    let chain = vec![Modifier::new(ModifierKind::Drawing(d))];

    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);
    converge(&mut mp, &device, &queue, &source, &chain, "drawing/2x2");
    let gpu_img = assemble(&device, &queue, &mp, &source);

    let data = ExportData {
        source: ExportSource::Frames {
            frames: vec![ExportFrame {
                pixels: std::sync::Arc::new(pixels),
                delay: std::time::Duration::ZERO,
            }],
            still_index: 0,
        },
        width: GOLDEN_W,
        height: GOLDEN_H,
        modifiers: chain,
        crop: None,
        rotation: 0,
        trim: None,
    };
    let (_, _, cpu_img) = render_still_rgba(&data).expect("render");
    let (max_d, pct) = diff_stats(&gpu_img, &cpu_img, 4);
    assert!(
        max_d <= 4,
        "drawing/2x2: preview diverges from export: max diff {max_d} ({pct:.3}% over)"
    );
}

#[test]
fn golden_sort_cardinal_single_tile() {
    run_golden("sort-cardinal/1-tile", &sort_cardinal_chain(), None, 0);
}

#[test]
fn golden_sort_cardinal_multi_tile() {
    run_golden(
        "sort-cardinal/2x2",
        &sort_cardinal_chain(),
        Some(FORCED_TILE_DIM),
        0,
    );
}

#[test]
fn golden_sort_diag_single_tile() {
    run_golden("sort-diag/1-tile", &sort_diag_chain(), None, 0);
}

#[test]
fn golden_sort_diag_multi_tile() {
    run_golden(
        "sort-diag/2x2",
        &sort_diag_chain(),
        Some(FORCED_TILE_DIM),
        0,
    );
}

#[test]
fn golden_motion_blur_single_tile() {
    run_golden("motion-blur/1-tile", &motion_blur_chain(), None, 4);
}

#[test]
fn golden_motion_blur_multi_tile() {
    run_golden(
        "motion-blur/2x2",
        &motion_blur_chain(),
        Some(FORCED_TILE_DIM),
        4,
    );
}

// ---- Partial-ROI goldens ------------------------------------------------
//
// These are the only tests that can observe a stage's input reach. See
// `set_partial_roi` for why the full-bounds goldens above cannot.

#[test]
fn roi_blur_partial_viewport() {
    run_roi_golden("roi/blur", &blur_chain(), 1024, 0.42, 4, 2048, 2048);
}

/// Chromatic aberration under a partial ROI.
///
/// Unlike the kernel cases above, this test **cannot** detect an under-fetch,
/// and that is a property of the shader rather than a gap in the harness:
/// `chromatic_aberration.wgsl` clamps its sample coordinates into `src_size`
/// (lines 40-41), so a region that was never fetched reads as the edge of the
/// region that was. The output is wrong but smoothly wrong, and stays inside
/// the tolerance. Verified empirically -- reclassifying CA from `WholeFrame`
/// to an 8px kernel keeps this green.
///
/// Kept because it still pins GPU/oracle agreement for CA on a partial ROI,
/// but do not treat it as protection for CA's reach. Making that testable
/// needs the shader to stop clamping, or a separate assertion on the gathered
/// source rect rather than on pixels.
#[test]
fn roi_chromatic_aberration_partial_viewport() {
    run_roi_golden("roi/ca", &ca_chain(), 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_motion_blur_partial_viewport() {
    run_roi_golden("roi/motion-blur", &motion_blur_chain(), 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_pointwise_then_blur_partial_viewport() {
    let chain = vec![
        Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.3 })),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 })),
    ];
    run_roi_golden("roi/pointwise+blur", &chain, 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn golden_ca_single_tile() {
    run_golden("ca/1-tile", &ca_chain(), None, 4);
}

#[test]
fn golden_ca_multi_tile() {
    run_golden("ca/2x2", &ca_chain(), Some(FORCED_TILE_DIM), 4);
}
