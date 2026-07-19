use super::*;
use crate::modifiers::kinds::{
    ChromaticAberration, Exposure, GaussianBlur, MotionBlur, PixelSort, Posterize,
};
use crate::wgpu::media::image_data::ImageData;
use crate::wgpu::passes::display::DisplayPass;
use iced::wgpu::{
    CommandEncoderDescriptor, DeviceDescriptor, Instance, PowerPreference, RequestAdapterOptions,
};

const GOLDEN_W: u32 = 96;
const GOLDEN_H: u32 = 64;
const FORCED_TILE_DIM: u32 = 48;

pub(super) fn try_device() -> Option<(Device, Queue)> {
    let instance = Instance::default();
    let adapter = futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).ok()
}

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
        o.proc_scale < 1.0,
        "expected reduced processing scale, got {}",
        o.proc_scale
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

pub(super) static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
fn golden_ca_single_tile() {
    run_golden("ca/1-tile", &ca_chain(), None, 4);
}

#[test]
fn golden_ca_multi_tile() {
    run_golden("ca/2x2", &ca_chain(), Some(FORCED_TILE_DIM), 4);
}
