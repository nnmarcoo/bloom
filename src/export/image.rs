use std::io::{BufWriter, Write};
use std::path::Path;

use crate::modifiers::drawing_raster::LayerView;
use crate::modifiers::text_raster::TextRaster;

use super::raster::{ExportCtx, render_into, render_strips};
use super::{ExportData, ExportFrame, Geom, ctx_with, process_frame};

pub(super) fn encode_png(
    ctx: &ExportCtx,
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), ctx.out_w(), ctx.out_h());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let mut stream = writer.stream_writer().map_err(|e| e.to_string())?;

    render_strips(
        ctx,
        |buf| stream.write_all(buf).map_err(|e| e.to_string()),
        progress,
    )
}

pub(super) fn encode_rgba(
    ctx: &ExportCtx,
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(ctx.out_w() as usize * ctx.out_h() as usize * 4);
    render_strips(
        ctx,
        |buf| {
            rgba.extend_from_slice(buf);
            Ok(())
        },
        progress,
    )?;

    image::RgbaImage::from_raw(ctx.out_w(), ctx.out_h(), rgba)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

pub(super) fn encode_jpeg(
    ctx: &ExportCtx,
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let mut rgb = Vec::with_capacity(ctx.out_w() as usize * ctx.out_h() as usize * 3);
    render_strips(
        ctx,
        |buf| {
            for p in buf.chunks_exact(4) {
                let a = p[3] as f32 / 255.0;
                let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                rgb.push(blend(p[0]));
                rgb.push(blend(p[1]));
                rgb.push(blend(p[2]));
            }
            Ok(())
        },
        progress,
    )?;

    image::RgbImage::from_raw(ctx.out_w(), ctx.out_h(), rgb)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn encode_apng(
    geom: &Geom,
    data: &ExportData,
    frames: &[ExportFrame],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), geom.out_w, geom.out_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(frames.len() as u32, 0)
        .map_err(|e| e.to_string())?;
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; geom.out_w as usize * geom.out_h as usize * 4];
    let n = frames.len();
    for (i, fr) in frames.iter().enumerate() {
        let processed = process_frame(data, text_layers, drawing_layers, &fr.pixels)?;
        let fctx = ctx_with(geom, &processed);
        render_into(&mut buf, &fctx);
        let ms = (fr.delay.as_millis().min(u16::MAX as u128) as u16).max(1);
        writer
            .set_frame_delay(ms, 1000)
            .map_err(|e| e.to_string())?;
        writer.write_image_data(&buf).map_err(|e| e.to_string())?;
        progress((i + 1) as f32 / n as f32);
    }

    writer.finish().map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn encode_gif(
    geom: &Geom,
    data: &ExportData,
    frames: &[ExportFrame],
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    if geom.out_w > u16::MAX as u32 || geom.out_h > u16::MAX as u32 {
        return Err("Image is too large for the GIF format (max 65535 px).".to_string());
    }

    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = gif::Encoder::new(
        BufWriter::new(file),
        geom.out_w as u16,
        geom.out_h as u16,
        &[],
    )
    .map_err(|e| e.to_string())?;
    enc.set_repeat(gif::Repeat::Infinite)
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; geom.out_w as usize * geom.out_h as usize * 4];
    let n = frames.len();
    for (i, fr) in frames.iter().enumerate() {
        let processed = process_frame(data, text_layers, drawing_layers, &fr.pixels)?;
        let fctx = ctx_with(geom, &processed);
        render_into(&mut buf, &fctx);
        let mut frame =
            gif::Frame::from_rgba_speed(geom.out_w as u16, geom.out_h as u16, &mut buf, 10);
        frame.delay = (fr.delay.as_millis() / 10).clamp(1, u16::MAX as u128) as u16;
        frame.dispose = gif::DisposalMethod::Background;
        enc.write_frame(&frame).map_err(|e| e.to_string())?;
        progress((i + 1) as f32 / n as f32);
    }

    Ok(())
}
