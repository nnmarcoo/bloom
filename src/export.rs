use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rayon::prelude::*;

use crate::modifiers::text_raster::{self, TextRaster};
use crate::modifiers::{Modifier, cpu};

pub struct ExportFrame {
    pub pixels: Arc<Vec<u8>>,
    pub delay: Duration,
}

pub struct ExportData {
    pub frames: Vec<ExportFrame>,
    pub still_index: usize,
    pub width: u32,
    pub height: u32,
    pub modifiers: Vec<Modifier>,
    pub crop: Option<[f32; 4]>,
    pub rotation: u8,
}

const STRIP_HEIGHT: u32 = 64;

#[derive(Clone, Copy)]
struct ExportCtx<'a> {
    pixels: &'a [u8],
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    out_w: u32,
    out_h: u32,
    rotation: u8,
    modifiers: &'a [Modifier],
    text_layers: &'a [Option<TextRaster>],
}

fn still_ctx<'a>(
    data: &'a ExportData,
    text_layers: &'a [Option<TextRaster>],
) -> Result<ExportCtx<'a>, String> {
    let img_w = data.width;
    let img_h = data.height;

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

    let (out_w, out_h) = if data.rotation.is_multiple_of(2) {
        (cw, ch)
    } else {
        (ch, cw)
    };

    let still = data
        .frames
        .get(data.still_index)
        .ok_or_else(|| "No frame available.".to_string())?;
    ensure_available(&still.pixels, img_w, img_h)?;

    Ok(ExportCtx {
        pixels: &still.pixels,
        img_w,
        img_h,
        cx0,
        cy0,
        cw,
        ch,
        out_w,
        out_h,
        rotation: data.rotation,
        modifiers: &data.modifiers,
        text_layers,
    })
}

pub fn render_still_rgba(data: &ExportData) -> Result<(u32, u32, Vec<u8>), String> {
    let text_layers = text_raster::build_layers(&data.modifiers, data.width, data.height);
    let ctx = still_ctx(data, &text_layers)?;
    let mut rgba = vec![0u8; ctx.out_w as usize * ctx.out_h as usize * 4];
    render_into(&mut rgba, &ctx);
    Ok((ctx.out_w, ctx.out_h, rgba))
}

pub fn do_export(data: ExportData, path: &Path, progress: impl Fn(f32)) -> Result<String, String> {
    let text_layers = text_raster::build_layers(&data.modifiers, data.width, data.height);
    let ctx = still_ctx(&data, &text_layers)?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    match ext.as_str() {
        "gif" => encode_gif(&ctx, &data.frames, path, &progress)?,
        "apng" => encode_apng(&ctx, &data.frames, path, &progress)?,
        "jpg" | "jpeg" => encode_jpeg(&ctx, path, &progress)?,
        "png" => encode_png(&ctx, path, &progress)?,
        _ => encode_rgba(&ctx, path, &progress)?,
    }

    Ok(path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned()))
}

fn ensure_available(pixels: &[u8], w: u32, h: u32) -> Result<(), String> {
    if pixels.len() < w as usize * h as usize * 4 {
        Err("Image pixels are no longer available. Try reloading the image.".to_string())
    } else {
        Ok(())
    }
}

fn render_strips(
    ctx: &ExportCtx,
    mut sink: impl FnMut(&[u8]) -> Result<(), String>,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let row_bytes = ctx.out_w as usize * 4;
    let mut strip = vec![0u8; row_bytes * STRIP_HEIGHT as usize];

    let mut oy = 0u32;
    while oy < ctx.out_h {
        let strip_h = (ctx.out_h - oy).min(STRIP_HEIGHT);
        let buf = &mut strip[..row_bytes * strip_h as usize];

        buf.par_chunks_mut(row_bytes)
            .enumerate()
            .for_each(|(i, row)| {
                fill_row(row, oy + i as u32, ctx);
            });

        sink(buf)?;
        oy += strip_h;
        progress(oy as f32 / ctx.out_h as f32);
    }

    Ok(())
}

fn render_into(buf: &mut [u8], ctx: &ExportCtx) {
    let row_bytes = ctx.out_w as usize * 4;
    buf.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(oy, row)| fill_row(row, oy as u32, ctx));
}

fn encode_png(ctx: &ExportCtx, path: &Path, progress: &impl Fn(f32)) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), ctx.out_w, ctx.out_h);
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

fn encode_rgba(ctx: &ExportCtx, path: &Path, progress: &impl Fn(f32)) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(ctx.out_w as usize * ctx.out_h as usize * 4);
    render_strips(
        ctx,
        |buf| {
            rgba.extend_from_slice(buf);
            Ok(())
        },
        progress,
    )?;

    image::RgbaImage::from_raw(ctx.out_w, ctx.out_h, rgba)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

fn encode_jpeg(ctx: &ExportCtx, path: &Path, progress: &impl Fn(f32)) -> Result<(), String> {
    let mut rgb = Vec::with_capacity(ctx.out_w as usize * ctx.out_h as usize * 3);
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

    image::RgbImage::from_raw(ctx.out_w, ctx.out_h, rgb)
        .ok_or_else(|| "Failed to create image buffer.".to_string())?
        .save(path)
        .map_err(|e| e.to_string())
}

fn encode_apng(
    ctx: &ExportCtx,
    frames: &[ExportFrame],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), ctx.out_w, ctx.out_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(frames.len() as u32, 0)
        .map_err(|e| e.to_string())?;
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; ctx.out_w as usize * ctx.out_h as usize * 4];
    let n = frames.len();
    for (i, fr) in frames.iter().enumerate() {
        ensure_available(&fr.pixels, ctx.img_w, ctx.img_h)?;
        let fctx = ExportCtx {
            pixels: &fr.pixels,
            ..*ctx
        };
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

fn encode_gif(
    ctx: &ExportCtx,
    frames: &[ExportFrame],
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    if ctx.out_w > u16::MAX as u32 || ctx.out_h > u16::MAX as u32 {
        return Err("Image is too large for the GIF format (max 65535 px).".to_string());
    }

    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = gif::Encoder::new(
        BufWriter::new(file),
        ctx.out_w as u16,
        ctx.out_h as u16,
        &[],
    )
    .map_err(|e| e.to_string())?;
    enc.set_repeat(gif::Repeat::Infinite)
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; ctx.out_w as usize * ctx.out_h as usize * 4];
    let n = frames.len();
    for (i, fr) in frames.iter().enumerate() {
        ensure_available(&fr.pixels, ctx.img_w, ctx.img_h)?;
        let fctx = ExportCtx {
            pixels: &fr.pixels,
            ..*ctx
        };
        render_into(&mut buf, &fctx);
        let mut frame =
            gif::Frame::from_rgba_speed(ctx.out_w as u16, ctx.out_h as u16, &mut buf, 10);
        frame.delay = (fr.delay.as_millis() / 10).clamp(1, u16::MAX as u128) as u16;
        frame.dispose = gif::DisposalMethod::Background;
        enc.write_frame(&frame).map_err(|e| e.to_string())?;
        progress((i + 1) as f32 / n as f32);
    }

    Ok(())
}

fn fill_row(row: &mut [u8], oy: u32, ctx: &ExportCtx) {
    for ox in 0..ctx.out_w {
        let (cx, cy) = match ctx.rotation {
            0 => (ox, oy),
            1 => (oy, ctx.ch - 1 - ox),
            2 => (ctx.cw - 1 - ox, ctx.ch - 1 - oy),
            3 => (ctx.cw - 1 - oy, ox),
            _ => unreachable!(),
        };

        let fx = ctx.cx0 + cx;
        let fy = ctx.cy0 + cy;

        let out = ox as usize * 4;
        if fx >= ctx.img_w || fy >= ctx.img_h {
            row[out..out + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }

        let src = (fy as usize * ctx.img_w as usize + fx as usize) * 4;
        let p = &ctx.pixels[src..src + 4];
        let raw = cpu::pixel_to_f32(p);
        let uv = [fx as f32 / ctx.img_w as f32, fy as f32 / ctx.img_h as f32];
        let result = cpu::apply_modifiers_with_text(
            ctx.modifiers,
            ctx.text_layers,
            ctx.pixels,
            ctx.img_w,
            ctx.img_h,
            fx as f32 + 0.5,
            fy as f32 + 0.5,
            uv,
            raw,
        );
        row[out..out + 4].copy_from_slice(&cpu::f32_to_pixel(result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::Text;

    #[test]
    fn text_appears_in_still_export() {
        let (w, h) = (256u32, 128u32);
        let pixels = Arc::new(vec![0u8; (w * h * 4) as usize]);

        let text = Text {
            content: "Hi".to_string(),
            size: 80.0,
            x: 0.5,
            y: 0.5,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            opacity: 1.0,
            ..Text::default()
        };
        let data = ExportData {
            frames: vec![ExportFrame {
                pixels,
                delay: Duration::ZERO,
            }],
            still_index: 0,
            width: w,
            height: h,
            modifiers: vec![Modifier::new(ModifierKind::Text(text))],
            crop: None,
            rotation: 0,
        };

        let (ow, oh, rgba) = render_still_rgba(&data).expect("render");
        assert_eq!((ow, oh), (w, h));

        let lit = rgba.chunks_exact(4).filter(|p| p[0] > 200).count();
        assert!(lit > 0, "expected white text pixels in export, found none");
    }

    #[test]
    fn chromatic_aberration_does_not_turn_text_green() {
        use crate::modifiers::kinds::ChromaticAberration;

        let (w, h) = (256u32, 128u32);
        let pixels = Arc::new(vec![0u8; (w * h * 4) as usize]);

        let text = Text {
            content: "Hi".to_string(),
            size: 80.0,
            x: 0.5,
            y: 0.5,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            opacity: 1.0,
            ..Text::default()
        };
        let ca = ChromaticAberration { amount: 30.0 };
        let data = ExportData {
            frames: vec![ExportFrame {
                pixels,
                delay: Duration::ZERO,
            }],
            still_index: 0,
            width: w,
            height: h,
            modifiers: vec![
                Modifier::new(ModifierKind::Text(text)),
                Modifier::new(ModifierKind::ChromaticAberration(ca)),
            ],
            crop: None,
            rotation: 0,
        };

        let (_, _, rgba) = render_still_rgba(&data).expect("render");

        let green_only = rgba
            .chunks_exact(4)
            .filter(|p| p[1] > 200 && p[0] < 40 && p[2] < 40)
            .count();
        let white = rgba
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200)
            .count();

        assert!(
            rgba.chunks_exact(4).any(|p| p[0] > 150),
            "CA should leave red text coverage"
        );
        assert!(
            rgba.chunks_exact(4).any(|p| p[2] > 150),
            "CA should leave blue text coverage"
        );
        assert!(
            white > 0,
            "expected a white core where red/green/blue overlap"
        );
        assert!(
            green_only < white,
            "text dominated by green fringe (green-only {green_only} vs white {white})"
        );
    }
}
