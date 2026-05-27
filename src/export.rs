use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;

use crate::modifier_cpu;
use crate::modifiers::Modifier;
use crate::wgpu::media::image_data::ImageData;

pub struct ExportData {
    pub image: Arc<ImageData>,
    pub modifiers: Vec<Modifier>,
    pub crop: Option<[f32; 4]>,
    pub rotation: u8,
}

const STRIP_HEIGHT: u32 = 64;

pub fn do_export(data: ExportData, path: &Path, progress: impl Fn(f32)) -> Result<String, String> {
    let img_w = data.image.width;
    let img_h = data.image.height;

    let guard = data.image.pixels.lock().unwrap_or_else(|e| e.into_inner());
    let pixels: &[u8] = &guard;

    if pixels.len() < img_w as usize * img_h as usize * 4 {
        return Err(
            "Image pixels are no longer available. Try reloading the image.".to_string(),
        );
    }

    let (cx0, cy0, cw, ch) = match data.crop {
        Some([min_u, min_v, max_u, max_v]) => {
            let cx0 = (min_u * img_w as f32).round() as u32;
            let cy0 = (min_v * img_h as f32).round() as u32;
            let cw = ((max_u - min_u) * img_w as f32).round() as u32;
            let ch = ((max_v - min_v) * img_h as f32).round() as u32;
            (cx0, cy0, cw.max(1), ch.max(1))
        }
        None => (0, 0, img_w, img_h),
    };

    let (out_w, out_h) = if data.rotation % 2 == 0 {
        (cw, ch)
    } else {
        (ch, cw)
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => encode_jpeg(
            pixels, img_w, img_h, cx0, cy0, cw, ch, out_w, out_h, data.rotation,
            &data.modifiers, path, &progress,
        )?,
        "png" => encode_png(
            pixels, img_w, img_h, cx0, cy0, cw, ch, out_w, out_h, data.rotation,
            &data.modifiers, path, &progress,
        )?,
        _ => encode_rgba(
            pixels, img_w, img_h, cx0, cy0, cw, ch, out_w, out_h, data.rotation,
            &data.modifiers, path, &progress,
        )?,
    }

    Ok(path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned()))
}

fn encode_png(
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    out_w: u32,
    out_h: u32,
    rotation: u8,
    modifiers: &[Modifier],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), out_w, out_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let mut stream = writer.stream_writer().map_err(|e| e.to_string())?;

    let row_bytes = out_w as usize * 4;
    let mut strip = vec![0u8; row_bytes * STRIP_HEIGHT as usize];

    let mut oy = 0u32;
    while oy < out_h {
        let strip_h = (out_h - oy).min(STRIP_HEIGHT);
        let buf = &mut strip[..row_bytes * strip_h as usize];

        buf.par_chunks_mut(row_bytes).enumerate().for_each(|(i, row)| {
            fill_row(
                row, oy + i as u32, out_w, pixels, img_w, img_h, cx0, cy0, cw, ch,
                rotation, modifiers,
            );
        });

        stream.write_all(buf).map_err(|e| e.to_string())?;
        oy += strip_h;
        progress(oy as f32 / out_h as f32);
    }

    Ok(())
}

fn encode_rgba(
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    out_w: u32,
    out_h: u32,
    rotation: u8,
    modifiers: &[Modifier],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(out_w as usize * out_h as usize * 4);
    let row_bytes = out_w as usize * 4;
    let mut strip = vec![0u8; row_bytes * STRIP_HEIGHT as usize];

    let mut oy = 0u32;
    while oy < out_h {
        let strip_h = (out_h - oy).min(STRIP_HEIGHT);
        let buf = &mut strip[..row_bytes * strip_h as usize];

        buf.par_chunks_mut(row_bytes).enumerate().for_each(|(i, row)| {
            fill_row(
                row, oy + i as u32, out_w, pixels, img_w, img_h, cx0, cy0, cw, ch,
                rotation, modifiers,
            );
        });

        rgba.extend_from_slice(buf);
        oy += strip_h;
        progress(oy as f32 / out_h as f32);
    }

    image::RgbaImage::from_raw(out_w, out_h, rgba)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

fn encode_jpeg(
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    out_w: u32,
    out_h: u32,
    rotation: u8,
    modifiers: &[Modifier],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let row_bytes = out_w as usize * 4;
    let mut strip = vec![0u8; row_bytes * STRIP_HEIGHT as usize];
    let mut rgb = Vec::with_capacity(out_w as usize * out_h as usize * 3);

    let mut oy = 0u32;
    while oy < out_h {
        let strip_h = (out_h - oy).min(STRIP_HEIGHT);
        let buf = &mut strip[..row_bytes * strip_h as usize];

        buf.par_chunks_mut(row_bytes).enumerate().for_each(|(i, row)| {
            fill_row(
                row, oy + i as u32, out_w, pixels, img_w, img_h, cx0, cy0, cw, ch, rotation,
                modifiers,
            );
        });

        for p in buf.chunks_exact(4) {
            let a = p[3] as f32 / 255.0;
            let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
            rgb.push(blend(p[0]));
            rgb.push(blend(p[1]));
            rgb.push(blend(p[2]));
        }

        oy += strip_h;
        progress(oy as f32 / out_h as f32);
    }

    image::RgbImage::from_raw(out_w, out_h, rgb)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

fn fill_row(
    row: &mut [u8],
    oy: u32,
    out_w: u32,
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    rotation: u8,
    modifiers: &[Modifier],
) {
    for ox in 0..out_w {
        let (cx, cy) = match rotation {
            0 => (ox, oy),
            1 => (oy, ch - 1 - ox),
            2 => (cw - 1 - ox, ch - 1 - oy),
            3 => (cw - 1 - oy, ox),
            _ => unreachable!(),
        };

        let fx = cx0 + cx;
        let fy = cy0 + cy;

        let out = ox as usize * 4;
        if fx >= img_w || fy >= img_h {
            row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }

        let src = (fy as usize * img_w as usize + fx as usize) * 4;
        let p = &pixels[src..src + 4];
        let raw = modifier_cpu::pixel_to_f32(p);
        let uv = [fx as f32 / img_w as f32, fy as f32 / img_h as f32];
        let result = modifier_cpu::apply_modifiers(modifiers, pixels, img_w, img_h, uv, raw);
        row[out..out + 4].copy_from_slice(&modifier_cpu::f32_to_pixel(result));
    }
}
