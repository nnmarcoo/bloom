//! Golden tests: the tiled GPU executor against the CPU oracle.
//!
//! The two backends must produce the same bytes, and they can diverge silently,
//! so these compare rendered output rather than checking either in isolation.
//!
//! Most goldens give every tile a full-bounds ROI, which means no apron or
//! reach value can change the result. The partial-ROI harness restricts tiles
//! to a centered window so a stage must actually fetch beyond what it writes,
//! which is what makes under-fetch observable.
//!
//! The resize goldens compare against the oracle at the chain's *output* size,
//! via assemble_output. They cover a trailing resize, a mid-chain one, and the
//! multi-tile case, in both directions: every resize golden written before them
//! downscaled, so the suite proved one direction of the parameter and said
//! nothing about the other. That is how a resample ignoring its region's origin
//! passed everything, since at 50% the executor produces a single band starting
//! at row 0, where treating the region as the whole image is accidentally
//! right.
//!
//! An earlier test asserted that a trailing resize left the preview unchanged.
//! That encoded the bug as the specification -- the executor dropped the resize,
//! so of course nothing moved -- and was deleted rather than adapted.

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

/// Assemble the tile outputs into a document of `dw` x `dh`.
///
/// assemble maps each tile back to its position in the *source*, which only
/// works while the chain keeps the source's size. A crop changes both the
/// document's size and where each tile's output lands in it, so the placement
/// has to come from proc_px -- which the executor writes in the chain's output
/// space for exactly this reason.
fn assemble_doc(
    device: &Device,
    queue: &Queue,
    mp: &ModifierPipeline,
    source: &TiledSource,
    dw: u32,
    dh: u32,
) -> Vec<u8> {
    let mut full = vec![0u8; (dw * dh * 4) as usize];
    for ti in 0..source.tiles.len() {
        let Some(o) = mp.tile_outputs[ti].as_ref() else {
            continue;
        };
        let px = o.proc_px.expect("executor outputs always carry proc_px");
        let (x0, y0) = (px[0].round() as u32, px[1].round() as u32);
        if x0 >= dw || y0 >= dh {
            continue;
        }
        let data = read_texture(device, queue, &o._tex, o.width, o.height);
        let cols = o.width.min(dw - x0);
        for r in 0..o.height.min(dh - y0) {
            let d = (((y0 + r) * dw + x0) * 4) as usize;
            let s = (r * o.width * 4) as usize;
            full[d..d + (cols * 4) as usize].copy_from_slice(&data[s..s + (cols * 4) as usize]);
        }
    }
    full
}

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
    (
        max_d,
        over as f64 * 100.0 / compared.max(1) as f64,
        compared,
    )
}

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

    // A chain that changes the document's size cannot be assembled back into
    // the source's grid; place the tiles by proc_px in the output document.
    let doc = crate::modifiers::plan::chain_output_spec(
        crate::modifiers::plan::ImageSpec::new(w, h),
        &crate::modifiers::plan::plan_modifiers(modifiers),
    );
    let gpu_img = if (doc.w, doc.h) == (w, h) {
        assemble(&device, &queue, &mp, &source)
    } else {
        assemble_doc(&device, &queue, &mp, &source, doc.w, doc.h)
    };
    let cpu_img = crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, w, h);
    assert_eq!(
        cpu_img.len(),
        (doc.w * doc.h * 4) as usize,
        "{label}: the CPU oracle did not produce the planned document size"
    );
    let (max_d, pct_over) = diff_stats(&gpu_img, &cpu_img, tol);
    assert!(
        max_d <= tol,
        "{label}: GPU vs CPU oracle diverges: max channel diff {max_d} > tol {tol} ({pct_over:.3}% of channels over)"
    );
}

pub(super) enum ParityOutcome {
    NoDevice,
    Checked { max_diff: u8, pct_over: f64 },
}

fn gradient_pixels(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let ty = y as f32 / (h - 1).max(1) as f32;
        for x in 0..w {
            let tx = x as f32 / (w - 1).max(1) as f32;
            let hue = tx * 6.0;
            let sector = hue as u32 % 6;
            let f = hue - hue.floor();
            let (r, g, b) = match sector {
                0 => (1.0, f, 0.0),
                1 => (1.0 - f, 1.0, 0.0),
                2 => (0.0, 1.0, f),
                3 => (0.0, 1.0 - f, 1.0),
                4 => (f, 0.0, 1.0),
                _ => (1.0, 0.0, 1.0 - f),
            };
            let l = 1.0 - ty;
            v.push((r * l * 255.0).round() as u8);
            v.push((g * l * 255.0).round() as u8);
            v.push((b * l * 255.0).round() as u8);
            v.push(if y == 0 {
                (tx * 255.0).round() as u8
            } else {
                255
            });
        }
    }
    v
}

pub(super) fn parity_probe(modifiers: &[Modifier], tol: u8) -> ParityOutcome {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return ParityOutcome::NoDevice;
    };
    let (w, h) = (GOLDEN_W, GOLDEN_H);
    let pixels = gradient_pixels(w, h);
    let image = ImageData::new(pixels.clone(), w, h);
    let source = make_source(&device, &queue, &image, None);

    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, modifiers, "parity");

    let gpu = assemble(&device, &queue, &mp, &source);
    let cpu = crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, w, h);
    let (max_diff, pct_over) = diff_stats(&gpu, &cpu, tol);
    ParityOutcome::Checked { max_diff, pct_over }
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

fn crop_chain() -> Vec<Modifier> {
    use crate::modifiers::kinds::Crop;
    vec![Modifier::new(ModifierKind::Crop(Crop {
        x: 13.0,
        y: 9.0,
        width: 51.0,
        height: 37.0,
    }))]
}

#[test]
fn golden_crop_single_tile() {
    run_golden("crop/1-tile", &crop_chain(), None, 2);
}

#[test]
fn golden_crop_multi_tile() {
    run_golden("crop/2x2", &crop_chain(), Some(FORCED_TILE_DIM), 2);
}

/// The workflow the crop stage exists for: reframe, blur what is left, trim.
/// The blur reads its own frame, so the second crop cannot be folded into the
/// first and the chain really does need both stages.
fn crop_blur_crop_chain() -> Vec<Modifier> {
    use crate::modifiers::kinds::Crop;
    vec![
        Modifier::new(ModifierKind::Crop(Crop {
            x: 8.0,
            y: 6.0,
            width: 70.0,
            height: 50.0,
        })),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
        Modifier::new(ModifierKind::Crop(Crop {
            x: 11.0,
            y: 7.0,
            width: 40.0,
            height: 30.0,
        })),
    ]
}

#[test]
fn golden_crop_blur_crop_single_tile() {
    run_golden("crop-blur-crop/1-tile", &crop_blur_crop_chain(), None, 4);
}

#[test]
fn golden_crop_blur_crop_multi_tile() {
    run_golden(
        "crop-blur-crop/2x2",
        &crop_blur_crop_chain(),
        Some(FORCED_TILE_DIM),
        4,
    );
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
        radius: 40.0,
    }))];
    run_golden_dims("blur-banded/96x3000", &chain, None, 4, 96, 3000);
}

#[test]
fn golden_blur_banded_above_the_cap() {
    let chain = vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
        radius: 100.0,
    }))];
    run_golden_dims("blur-banded-scaled/96x3000", &chain, None, 32, 96, 3000);
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

#[test]
fn roi_blur_partial_viewport() {
    run_roi_golden("roi/blur", &blur_chain(), 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_chromatic_aberration_partial_viewport() {
    run_roi_golden("roi/ca", &ca_chain(), 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_motion_blur_partial_viewport() {
    run_roi_golden(
        "roi/motion-blur",
        &motion_blur_chain(),
        1024,
        0.42,
        4,
        2048,
        2048,
    );
}

#[test]
fn roi_pointwise_then_blur_partial_viewport() {
    let chain = vec![
        Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.3 })),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 6.0 })),
    ];
    run_roi_golden("roi/pointwise+blur", &chain, 1024, 0.42, 4, 2048, 2048);
}

fn run_resize_roi_golden(
    label: &str,
    modifiers: &[Modifier],
    tile_dim: u32,
    frac: f32,
    tol: u8,
    w: u32,
    h: u32,
) {
    use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

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
        "{label}: no tile got a strictly-partial ROI, so this proves nothing \
         the full-bounds resize goldens do not already cover"
    );

    let out = chain_output_spec(ImageSpec::new(w, h), &plan_modifiers(modifiers));
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);
    converge(&mut mp, &device, &queue, &source, modifiers, label);

    let cpu_full = crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, w, h);

    let mut max_d = 0u8;
    let mut over = 0usize;
    let mut compared = 0usize;
    for ti in 0..source.tiles.len() {
        let Some(o) = mp.tile_outputs[ti].as_ref() else {
            continue;
        };
        let Some(px) = o.proc_px else { continue };
        let data = read_texture(&device, &queue, &o._tex, o.width, o.height);
        let x0 = px[0].round() as u32;
        let y0 = px[1].round() as u32;
        for r in 0..o.height {
            let oy = y0 + r;
            if oy >= out.h {
                break;
            }
            for c in 0..o.width {
                let ox = x0 + c;
                if ox >= out.w {
                    break;
                }
                let s = ((r * o.width + c) * 4) as usize;
                let d = ((oy * out.w + ox) * 4) as usize;
                for ch in 0..3 {
                    let a = data[s + ch];
                    let b = cpu_full[d + ch];
                    let diff = a.abs_diff(b);
                    max_d = max_d.max(diff);
                    compared += 1;
                    if diff > tol {
                        over += 1;
                    }
                }
            }
        }
    }

    assert!(
        compared > 0,
        "{label}: compared no pixels; the ROI collapsed and the test proved nothing"
    );
    assert!(
        max_d <= tol,
        "{label}: GPU diverges from the oracle inside a partial ROI: max channel \
         diff {max_d} > tol {tol} ({:.3}% of {compared} channels over). A resize \
         under a partial viewport is reading or placing input the ROI did not \
         account for.",
        over as f64 * 100.0 / compared.max(1) as f64
    );
}

#[test]
fn roi_resize_trailing_partial_viewport() {
    let mut chain = blur_chain();
    chain.push(resize_half());
    run_resize_roi_golden("roi/resize-trailing", &chain, 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_resize_mid_chain_partial_viewport() {
    let chain = vec![
        resize_half(),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
    ];
    run_resize_roi_golden("roi/resize-mid-chain", &chain, 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn roi_upscale_trailing_partial_viewport() {
    let mut chain = blur_chain();
    chain.push(resize_double());
    run_resize_roi_golden("roi/upscale-trailing", &chain, 1024, 0.42, 4, 2048, 2048);
}

#[test]
fn doc_size_is_reported_even_when_every_tile_is_culled() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
    let image = ImageData::new(pixels, GOLDEN_W, GOLDEN_H);
    let mut source = make_source(&device, &queue, &image, None);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);

    let resize = vec![Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 25.0,
        height: 25.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))];

    for t in &mut source.tiles {
        t.last_ndc_rect = Some((glam::vec2(50.0, 50.0), glam::vec2(60.0, 60.0)));
    }
    mp.prepare(&device, &queue, &source, &resize, true);

    let want = (GOLDEN_W / 4, GOLDEN_H / 4);
    assert_eq!(
        mp.doc_size(),
        want,
        "every tile culled, so the executor returned before recording the \
         document it planned. view_pipeline compares doc_size to decide whether \
         deferring is safe, so a stale value leaves quads placed for the old \
         document and the view never refits."
    );
}

#[test]
fn resize_only_stack_renders_through_the_pipeline() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
    let image = ImageData::new(pixels, GOLDEN_W, GOLDEN_H);
    let source = make_source(&device, &queue, &image, None);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);

    let resize = vec![Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 25.0,
        height: 25.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))];
    converge(&mut mp, &device, &queue, &source, &resize, "resize-only");

    assert!(
        mp.tile_outputs.iter().any(|o| o.is_some()),
        "a resize-only stack produced no tile outputs, so the view falls back \
         to drawing the unresized source"
    );
    for i in 0..source.tiles.len() {
        if mp.tile_outputs[i].is_none() {
            continue;
        }
        assert!(
            mp.tile_display_bg(i, true).is_some(),
            "tile {i} rendered but has no display bind group, so the draw loop \
             skips it"
        );
    }
}

fn run_resize_golden(label: &str, modifiers: &[Modifier], tile_dim: Option<u32>, tol: u8) {
    use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let pixels = test_pixels(GOLDEN_W, GOLDEN_H);
    let image = ImageData::new(pixels.clone(), GOLDEN_W, GOLDEN_H);
    let source = make_source(&device, &queue, &image, tile_dim);

    let out = chain_output_spec(
        ImageSpec::new(GOLDEN_W, GOLDEN_H),
        &plan_modifiers(modifiers),
    );
    assert_ne!(
        (out.w, out.h),
        (GOLDEN_W, GOLDEN_H),
        "{label}: this harness is for chains that change size"
    );

    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, GOLDEN_W, GOLDEN_H);
    converge(&mut mp, &device, &queue, &source, modifiers, label);

    let gpu_img = assemble_output(&device, &queue, &mp, &source, out.w, out.h);
    let cpu_img =
        crate::modifiers::cpu::render_full(modifiers, &[], &[], &pixels, GOLDEN_W, GOLDEN_H);
    assert_eq!(
        gpu_img.len(),
        cpu_img.len(),
        "{label}: GPU produced {} bytes, oracle {} -- the preview is not at the \
         chain's output size ({}x{})",
        gpu_img.len(),
        cpu_img.len(),
        out.w,
        out.h
    );
    let (max_d, pct_over) = diff_stats(&gpu_img, &cpu_img, tol);
    assert!(
        max_d <= tol,
        "{label}: GPU vs CPU oracle diverges: max channel diff {max_d} > tol {tol} \
         ({pct_over:.3}% of channels over)"
    );
}

fn assemble_output(
    device: &Device,
    queue: &Queue,
    mp: &ModifierPipeline,
    source: &TiledSource,
    out_w: u32,
    out_h: u32,
) -> Vec<u8> {
    let mut full = vec![0u8; (out_w * out_h * 4) as usize];
    for ti in 0..source.tiles.len() {
        let Some(o) = mp.tile_outputs[ti].as_ref() else {
            continue;
        };
        let tile = &source.tiles[ti];
        let px = o.proc_px.unwrap_or([
            tile.x as f32,
            tile.y as f32,
            (tile.x + tile.width) as f32,
            (tile.y + tile.height) as f32,
        ]);
        let x0 = px[0].round() as u32;
        let y0 = px[1].round() as u32;
        let data = read_texture(device, queue, &o._tex, o.width, o.height);
        for r in 0..o.height.min(out_h.saturating_sub(y0)) {
            let cols = o.width.min(out_w.saturating_sub(x0));
            if cols == 0 {
                break;
            }
            let d = (((y0 + r) * out_w + x0) * 4) as usize;
            let s = (r * o.width * 4) as usize;
            full[d..d + (cols * 4) as usize].copy_from_slice(&data[s..s + (cols * 4) as usize]);
        }
    }
    full
}

fn resize_half() -> Modifier {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 50.0,
        height: 50.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))
}

#[test]
fn golden_resize_trailing_matches_the_oracle() {
    let mut chain = blur_chain();
    chain.push(resize_half());
    run_resize_golden("resize/trailing", &chain, None, 4);
}

#[test]
fn golden_resize_mid_chain_matches_the_oracle() {
    let chain = vec![
        resize_half(),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
    ];
    run_resize_golden("resize/mid-chain", &chain, None, 4);
}

#[test]
fn golden_resize_mid_chain_multi_tile() {
    let chain = vec![
        resize_half(),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
    ];
    run_resize_golden("resize/mid-chain-2x2", &chain, Some(FORCED_TILE_DIM), 4);
}

fn resize_double() -> Modifier {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 200.0,
        height: 200.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))
}

#[test]
fn golden_upscale_trailing_matches_the_oracle() {
    let mut chain = blur_chain();
    chain.push(resize_double());
    run_resize_golden("upscale/trailing", &chain, None, 4);
}

#[test]
fn golden_upscale_mid_chain_matches_the_oracle() {
    let chain = vec![
        resize_double(),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
    ];
    run_resize_golden("upscale/mid-chain", &chain, None, 4);
}

#[test]
fn golden_upscale_mid_chain_multi_tile() {
    let chain = vec![
        resize_double(),
        Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 4.0 })),
    ];
    run_resize_golden("upscale/mid-chain-2x2", &chain, Some(FORCED_TILE_DIM), 4);
}

#[test]
fn golden_ca_single_tile() {
    run_golden("ca/1-tile", &ca_chain(), None, 4);
}

#[test]
fn golden_ca_multi_tile() {
    run_golden("ca/2x2", &ca_chain(), Some(FORCED_TILE_DIM), 4);
}

fn assert_tiles_cover_document(
    mp: &ModifierPipeline,
    source: &TiledSource,
    doc_w: u32,
    doc_h: u32,
    label: &str,
) {
    let mut covered = vec![0u8; (doc_w * doc_h) as usize];

    for (ti, tile) in source.tiles.iter().enumerate() {
        let Some(o) = mp.tile_outputs[ti].as_ref() else {
            continue;
        };
        let px = o.proc_px.unwrap_or([
            tile.x as f32,
            tile.y as f32,
            (tile.x + tile.width) as f32,
            (tile.y + tile.height) as f32,
        ]);

        for (i, v) in px.iter().enumerate() {
            assert!(
                (v - v.round()).abs() < 1e-3,
                "{label}: tile {ti} proc_px[{i}] is {v}, not an integer. \
                 Fractional boundaries mean adjacent tiles no longer meet, \
                 which shows as seams between tiles."
            );
        }

        let (x0, y0) = (px[0].round() as i64, px[1].round() as i64);
        let (x1, y1) = (px[2].round() as i64, px[3].round() as i64);
        assert!(
            x0 >= 0 && y0 >= 0 && x1 <= doc_w as i64 && y1 <= doc_h as i64,
            "{label}: tile {ti} covers [{x0},{y0}..{x1},{y1}] but the document \
             is {doc_w}x{doc_h}. A tile reaching past the edge leaves the strip \
             beyond it unpainted."
        );

        for y in y0..y1 {
            for x in x0..x1 {
                let idx = (y as u32 * doc_w + x as u32) as usize;
                covered[idx] += 1;
            }
        }
    }

    let gaps = covered.iter().filter(|&&c| c == 0).count();
    let overlaps = covered.iter().filter(|&&c| c > 1).count();
    assert_eq!(
        gaps,
        0,
        "{label}: {gaps} of {} document pixels are covered by no tile, so they \
         are never painted",
        covered.len()
    );
    assert_eq!(
        overlaps, 0,
        "{label}: {overlaps} document pixels are covered by more than one tile, \
         so tiles draw over each other",
    );
}

type TileGeometry = Option<(u32, u32, Option<[f32; 4]>)>;

fn assert_geometry_is_stable(
    mp: &mut ModifierPipeline,
    device: &Device,
    queue: &Queue,
    source: &TiledSource,
    modifiers: &[Modifier],
    label: &str,
) {
    let snapshot = |mp: &ModifierPipeline| -> Vec<TileGeometry> {
        mp.tile_outputs
            .iter()
            .map(|o| o.as_ref().map(|o| (o.width, o.height, o.proc_px)))
            .collect()
    };

    converge(mp, device, queue, source, modifiers, label);
    let first = snapshot(mp);

    for frame in 1..4 {
        mp.prepare(device, queue, source, modifiers, false);
        let now = snapshot(mp);
        assert_eq!(
            now, first,
            "{label}: tile geometry changed on frame {frame} without the stack \
             changing. An alternating size is what the viewport shows as \
             flicker."
        );
    }
}

const REAL_W: u32 = 1179;
const REAL_H: u32 = 1159;
const REAL_TILE: u32 = 512;

fn real_source(device: &Device, queue: &Queue) -> (TiledSource, ImageData) {
    let pixels = test_pixels(REAL_W, REAL_H);
    let image = ImageData::new(pixels, REAL_W, REAL_H);
    let source = make_source(device, queue, &image, Some(REAL_TILE));
    assert_eq!(
        source.tiles.len(),
        9,
        "expected a 3x3 grid with partial edge tiles"
    );
    (source, image)
}

#[test]
fn tiles_cover_an_odd_sized_document_without_a_resize() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (source, _image) = real_source(&device, &queue);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, REAL_W, REAL_H);
    let chain = pointwise_chain();
    converge(&mut mp, &device, &queue, &source, &chain, "cover/plain");
    assert_tiles_cover_document(&mp, &source, REAL_W, REAL_H, "cover/plain");
}

#[test]
fn tile_geometry_is_stable_across_frames() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (source, _image) = real_source(&device, &queue);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, REAL_W, REAL_H);
    let chain = pointwise_chain();
    assert_geometry_is_stable(
        &mut mp,
        &device,
        &queue,
        &source,
        &chain,
        "stable/pointwise",
    );
}

#[test]
fn tile_geometry_is_stable_with_a_blur() {
    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (source, _image) = real_source(&device, &queue);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, REAL_W, REAL_H);
    assert_geometry_is_stable(
        &mut mp,
        &device,
        &queue,
        &source,
        &blur_chain(),
        "stable/blur",
    );
}

#[test]
fn tiles_cover_a_resized_odd_sized_document() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (source, _image) = real_source(&device, &queue);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, REAL_W, REAL_H);

    let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 50.0,
        height: 50.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))];

    let out = chain_output_spec(ImageSpec::new(REAL_W, REAL_H), &plan_modifiers(&chain));
    assert_geometry_is_stable(&mut mp, &device, &queue, &source, &chain, "cover/resized");
    assert_tiles_cover_document(&mp, &source, out.w, out.h, "cover/resized");
}

#[test]
fn tiles_cover_an_upscaled_odd_sized_document() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (source, _image) = real_source(&device, &queue);
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, REAL_W, REAL_H);

    let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 200.0,
        height: 200.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))];

    let out = chain_output_spec(ImageSpec::new(REAL_W, REAL_H), &plan_modifiers(&chain));
    assert_geometry_is_stable(&mut mp, &device, &queue, &source, &chain, "cover/upscaled");
    assert_tiles_cover_document(&mp, &source, out.w, out.h, "cover/upscaled");
}

#[test]
fn changing_the_resize_does_not_reuse_stale_outputs() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (w, h) = (1024u32, 1024u32);
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels, w, h);
    let source = make_source(&device, &queue, &image, Some(512));
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);

    for pct in [90.0f32, 70.0, 55.0, 40.0, 25.0, 60.0, 95.0] {
        let chain = vec![
            Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 24.0 })),
            Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: pct,
                height: pct,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            })),
        ];
        converge(&mut mp, &device, &queue, &source, &chain, "drag");

        let out = crate::modifiers::plan::chain_output_spec(
            crate::modifiers::plan::ImageSpec::new(w, h),
            &crate::modifiers::plan::plan_modifiers(&chain),
        );
        for (ti, _tile) in source.tiles.iter().enumerate() {
            let Some(o) = mp.tile_outputs[ti].as_ref() else {
                continue;
            };
            let px = o.proc_px.expect("outputs carry proc_px");
            assert!(
                px[2] <= out.w as f32 + 0.5 && px[3] <= out.h as f32 + 0.5,
                "at {pct}%, tile {ti} claims {px:?} but the document is \
                 {}x{}; a region from an earlier size was reused",
                out.w,
                out.h
            );
        }
    }
}

#[test]
fn rows_across_a_band_boundary_match_the_oracle() {
    use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};
    use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};

    let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((device, queue)) = try_device() else {
        return;
    };
    let (w, h) = (1179u32, 1159u32);
    let pixels = test_pixels(w, h);
    let image = ImageData::new(pixels.clone(), w, h);
    let source = make_source(&device, &queue, &image, Some(512));
    let mut mp = ModifierPipeline::new(&device, TextureFormat::Rgba8Unorm, w, h);

    let chain = vec![Modifier::new(ModifierKind::Resize(Resize {
        mode: ResizeMode::Percent,
        width: 200.0,
        height: 200.0,
        filter: ResizeFilter::Lanczos,
        lock_aspect: true,
    }))];
    let out = chain_output_spec(ImageSpec::new(w, h), &plan_modifiers(&chain));
    converge(&mut mp, &device, &queue, &source, &chain, "band-seam");

    let gpu = assemble_output(&device, &queue, &mp, &source, out.w, out.h);
    let cpu = crate::modifiers::cpu::render_full(&chain, &[], &[], &pixels, w, h);
    assert_eq!(
        gpu.len(),
        cpu.len(),
        "output size disagrees with the oracle"
    );

    let stride = out.w as usize * 4;
    let mut worst = (0u8, 0usize);
    for row in 0..out.h as usize {
        let o = row * stride;
        let d = gpu[o..o + stride]
            .iter()
            .zip(&cpu[o..o + stride])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        if d > worst.0 {
            worst = (d, row);
        }
    }
    let bad: Vec<usize> = (0..out.h as usize)
        .filter(|&row| {
            let o = row * stride;
            gpu[o..o + stride]
                .iter()
                .zip(&cpu[o..o + stride])
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0)
                > 4
        })
        .collect();
    println!(
        "SEAMROWS {} of {} rows differ, first {:?} last {:?}",
        bad.len(),
        out.h,
        bad.first(),
        bad.last()
    );
    assert!(
        worst.0 <= 4,
        "row {} differs from the oracle by {} levels; band boundaries are at \
         1024 and 2048 in this {}x{} document",
        worst.1,
        worst.0,
        out.w,
        out.h
    );
}
