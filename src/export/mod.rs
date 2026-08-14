//! Export: turning the current document plus its modifier stack into a file.
//!
//! Large images go through the streaming path in raster.rs, which requires a
//! bandable plan and no rotation that would reorder rows. Everything else falls
//! back to rendering the full frame.
//!
//! Video frames and the JPEG and raw RGBA encoders still buffer whole frames.

#[cfg(test)]
mod bench;
mod image;
#[cfg(test)]
mod oracle;
mod raster;
#[cfg(feature = "av")]
mod video;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::modifiers::drawing_raster::{self, DrawingRaster, LayerView};
use crate::modifiers::plan::{ImageSpec, chain_output_spec, plan_modifiers};
use crate::modifiers::text_raster::{self, TextRaster};
use crate::modifiers::{Modifier, cpu};

use raster::{ExportCtx, render_into};

fn layer_views(layers: &[Option<DrawingRaster>]) -> Vec<Option<LayerView<'_>>> {
    layers
        .iter()
        .map(|l| l.as_ref().map(|r| r.view()))
        .collect()
}

pub struct ExportFrame {
    pub pixels: Arc<Vec<u8>>,
    pub delay: Duration,
}

pub enum ExportSource {
    Frames {
        frames: Vec<ExportFrame>,
        still_index: usize,
    },
    #[cfg(feature = "av")]
    Video(VideoExportInfo),
}

#[cfg(feature = "av")]
pub struct VideoExportInfo {
    pub path: std::path::PathBuf,
    pub frame_count: u64,
    pub duration: Duration,
}

pub struct ExportData {
    pub source: ExportSource,
    pub width: u32,
    pub height: u32,
    pub modifiers: Vec<Modifier>,
    pub rotation: u8,
    pub trim: Option<(Duration, Duration)>,
}

impl ExportData {
    pub fn is_animated(&self) -> bool {
        matches!(&self.source, ExportSource::Frames { frames, .. } if self.trimmed(frames).len() > 1)
    }

    pub fn is_video(&self) -> bool {
        #[cfg(feature = "av")]
        {
            matches!(&self.source, ExportSource::Video(_))
        }
        #[cfg(not(feature = "av"))]
        {
            false
        }
    }

    fn in_memory(&self) -> Result<(&[ExportFrame], usize), String> {
        match &self.source {
            ExportSource::Frames {
                frames,
                still_index,
            } => Ok((frames, *still_index)),
            #[cfg(feature = "av")]
            ExportSource::Video(_) => {
                Err("Video frames are not available for this format.".to_string())
            }
        }
    }

    fn trimmed<'a>(&self, frames: &'a [ExportFrame]) -> &'a [ExportFrame] {
        let (offset, len) = self.trim_bounds(frames);
        &frames[offset..offset + len]
    }

    fn trim_bounds(&self, frames: &[ExportFrame]) -> (usize, usize) {
        let Some((start, end)) = self.trim else {
            return (0, frames.len());
        };
        let mut first = frames.len();
        let mut last = 0usize;
        let mut clock = Duration::ZERO;
        for (i, f) in frames.iter().enumerate() {
            let frame_end = clock + f.delay;
            if frame_end > start && clock < end {
                first = first.min(i);
                last = i;
            }
            clock = frame_end;
        }
        if first > last {
            return (0, frames.len().min(1));
        }
        (first, last - first + 1)
    }
}

#[derive(Clone, Copy)]
struct Geom {
    img_w: u32,
    img_h: u32,
    cx0: u32,
    cy0: u32,
    cw: u32,
    ch: u32,
    out_w: u32,
    out_h: u32,
    rotation: u8,
}

fn geom_of(data: &ExportData) -> Geom {
    let plan = plan_modifiers(&data.modifiers);
    let processed = chain_output_spec(ImageSpec::new(data.width, data.height), &plan);
    let img_w = processed.w;
    let img_h = processed.h;

    let (cx0, cy0, cw, ch) = (0, 0, img_w, img_h);

    let (out_w, out_h) = if data.rotation.is_multiple_of(2) {
        (cw, ch)
    } else {
        (ch, cw)
    };

    Geom {
        img_w,
        img_h,
        cx0,
        cy0,
        cw,
        ch,
        out_w,
        out_h,
        rotation: data.rotation,
    }
}

fn ctx_with<'a>(geom: &Geom, processed: &'a [u8]) -> ExportCtx<'a> {
    ExportCtx {
        geom: *geom,
        processed,
    }
}

fn ensure_available(pixels: &[u8], w: u32, h: u32) -> Result<(), String> {
    if pixels.len() < w as usize * h as usize * 4 {
        Err("Image pixels are no longer available. Try reloading the image.".to_string())
    } else {
        Ok(())
    }
}

fn process_frame(
    data: &ExportData,
    text_layers: &[Option<TextRaster>],
    drawing_layers: &[Option<LayerView<'_>>],
    pixels: &[u8],
) -> Result<Vec<u8>, String> {
    ensure_available(pixels, data.width, data.height)?;
    Ok(cpu::render_full(
        &data.modifiers,
        text_layers,
        drawing_layers,
        pixels,
        data.width,
        data.height,
    ))
}

fn can_stream_bands(data: &ExportData) -> bool {
    data.rotation.is_multiple_of(2)
        && cpu::plan_is_bandable(
            ImageSpec::new(data.width, data.height),
            &plan_modifiers(&data.modifiers),
        )
}

pub fn render_still_rgba(data: &ExportData) -> Result<(u32, u32, Vec<u8>), String> {
    let text_layers = text_raster::build_layers(&data.modifiers, data.width, data.height);
    let drawing_rasters = drawing_raster::build_layers(&data.modifiers, data.width, data.height);
    let drawing_layers = layer_views(&drawing_rasters);
    let geom = geom_of(data);
    let (all_frames, still_index) = data.in_memory()?;
    let (offset, len) = data.trim_bounds(all_frames);
    let frames = &all_frames[offset..offset + len];
    let still_index = still_index
        .saturating_sub(offset)
        .min(len.saturating_sub(1));
    let still = frames
        .get(still_index)
        .ok_or_else(|| "No frame available.".to_string())?;
    let processed = process_frame(data, &text_layers, &drawing_layers, &still.pixels)?;
    let ctx = ctx_with(&geom, &processed);
    let mut rgba = vec![0u8; geom.out_w as usize * geom.out_h as usize * 4];
    render_into(&mut rgba, &ctx);
    Ok((geom.out_w, geom.out_h, rgba))
}

pub fn do_export(data: ExportData, path: &Path, progress: impl Fn(f32)) -> Result<String, String> {
    #[cfg(feature = "av")]
    if let ExportSource::Video(info) = &data.source {
        if let Err(e) = video::encode_video(&data, info, path, &progress) {
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        return Ok(export_name(path));
    }

    let text_layers = text_raster::build_layers(&data.modifiers, data.width, data.height);
    let drawing_rasters = drawing_raster::build_layers(&data.modifiers, data.width, data.height);
    let drawing_layers = layer_views(&drawing_rasters);
    let geom = geom_of(&data);
    let (all_frames, still_index) = data.in_memory()?;
    let (offset, len) = data.trim_bounds(all_frames);
    let frames = &all_frames[offset..offset + len];
    let still_index = still_index
        .saturating_sub(offset)
        .min(len.saturating_sub(1));

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    match ext.as_str() {
        "gif" => image::encode_gif(
            &geom,
            &data,
            frames,
            &text_layers,
            &drawing_layers,
            path,
            &progress,
        )?,
        "apng" => image::encode_apng(
            &geom,
            &data,
            frames,
            &text_layers,
            &drawing_layers,
            path,
            &progress,
        )?,
        _ => {
            let still = frames
                .get(still_index)
                .ok_or_else(|| "No frame available.".to_string())?;

            if ext == "png" && can_stream_bands(&data) {
                ensure_available(&still.pixels, data.width, data.height)?;
                image::encode_png_streaming(
                    &geom,
                    &data,
                    &text_layers,
                    &drawing_layers,
                    &still.pixels,
                    path,
                    &progress,
                )?;
            } else {
                let processed = process_frame(&data, &text_layers, &drawing_layers, &still.pixels)?;
                let ctx = ctx_with(&geom, &processed);
                match ext.as_str() {
                    "jpg" | "jpeg" => image::encode_jpeg(&ctx, path, &progress)?,
                    "png" => image::encode_png(&ctx, path, &progress)?,
                    _ => image::encode_rgba(&ctx, path, &progress)?,
                }
            }
        }
    }

    Ok(export_name(path))
}

fn export_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod trim_tests {
    use super::*;

    fn ten_frames() -> Vec<ExportFrame> {
        (0..10)
            .map(|_| ExportFrame {
                pixels: Arc::new(vec![0u8; 4]),
                delay: Duration::from_millis(100),
            })
            .collect()
    }

    fn data_with(trim: Option<(Duration, Duration)>, still_index: usize) -> ExportData {
        ExportData {
            source: ExportSource::Frames {
                frames: ten_frames(),
                still_index,
            },
            width: 1,
            height: 1,
            modifiers: Vec::new(),
            rotation: 0,
            trim,
        }
    }

    fn bounds(trim: Option<(Duration, Duration)>) -> (usize, usize) {
        let data = data_with(trim, 0);
        let frames = match &data.source {
            ExportSource::Frames { frames, .. } => frames,
            #[cfg(feature = "av")]
            _ => unreachable!(),
        };
        data.trim_bounds(frames)
    }

    #[test]
    fn no_trim_keeps_every_frame() {
        assert_eq!(bounds(None), (0, 10));
    }

    #[test]
    fn trim_selects_overlapping_frames() {
        let trim = Some((Duration::from_millis(250), Duration::from_millis(650)));
        assert_eq!(bounds(trim), (2, 5));
    }

    #[test]
    fn frame_boundary_aligned_trim_is_exact() {
        let trim = Some((Duration::from_millis(300), Duration::from_millis(500)));
        assert_eq!(bounds(trim), (3, 2), "300..500ms is exactly frames 3 and 4");
    }

    #[test]
    fn degenerate_span_still_yields_a_frame() {
        let trim = Some((Duration::from_secs(99), Duration::from_secs(100)));
        let (_, len) = bounds(trim);
        assert_eq!(len, 1, "an out-of-range span must not encode zero frames");
    }

    #[test]
    fn still_index_is_reanchored_into_the_kept_span() {
        let trim = Some((Duration::from_millis(500), Duration::from_millis(900)));
        let data = data_with(trim, 0);
        let (_, _, rgba) = render_still_rgba(&data).expect("render");
        assert_eq!(rgba.len(), 4, "still export should survive an offset trim");
    }

    #[test]
    fn trimmed_single_frame_is_not_animated() {
        let trim = Some((Duration::from_millis(0), Duration::from_millis(50)));
        assert!(
            !data_with(trim, 0).is_animated(),
            "a one-frame span should export as a still, not a GIF"
        );
        assert!(data_with(None, 0).is_animated());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::ModifierKind;
    use crate::modifiers::kinds::Text;

    #[test]
    fn export_geometry_follows_the_chain_output() {
        use crate::modifiers::kinds::GaussianBlur;

        let (w, h) = (128u32, 96u32);
        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: Arc::new(vec![0u8; (w * h * 4) as usize]),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
                radius: 3.0,
            }))],
            rotation: 0,
            trim: None,
        };

        let plan = plan_modifiers(&data.modifiers);
        let chain_out = chain_output_spec(ImageSpec::new(w, h), &plan);
        let geom = geom_of(&data);
        assert_eq!(
            (geom.img_w, geom.img_h),
            (chain_out.w, chain_out.h),
            "geom_of must size the processed buffer from the chain"
        );
        assert_eq!((geom.out_w, geom.out_h), (w, h));
    }

    fn assert_streamed_png_matches_buffered(
        label: &str,
        mut modifiers: Vec<Modifier>,
        crop: Option<(f32, f32, f32, f32)>,
        rotation: u8,
        w: u32,
        h: u32,
    ) {
        if let Some((x, y, cw, ch)) = crop {
            use crate::modifiers::kinds::Crop;
            modifiers.push(Modifier::new(ModifierKind::Crop(Crop {
                x,
                y,
                width: cw,
                height: ch,
            })));
        }
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut s = 0x1234567u32;
        for b in px.chunks_mut(4) {
            for c in b.iter_mut().take(3) {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *c = (s >> 24) as u8;
            }
            b[3] = 255;
        }

        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: Arc::new(px.clone()),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers,
            rotation,
            trim: None,
        };
        assert!(
            can_stream_bands(&data),
            "{label}: chain is not streamable, the test would prove nothing"
        );

        let geom = geom_of(&data);
        let dir = std::env::temp_dir().join("bloom-band-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join(format!("{label}-stream.png"));
        let b = dir.join(format!("{label}-buffer.png"));

        image::encode_png_streaming(&geom, &data, &[], &[], &px, &a, &|_| {}).unwrap();

        let processed = process_frame(&data, &[], &[], &px).unwrap();
        let ctx = ctx_with(&geom, &processed);
        image::encode_png(&ctx, &b, &|_| {}).unwrap();

        let (ba, bb) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        assert_eq!(
            ba,
            bb,
            "{label}: streamed PNG differs from buffered PNG ({} vs {} bytes)",
            ba.len(),
            bb.len()
        );
    }

    #[test]
    fn streamed_png_matches_buffered_pointwise() {
        use crate::modifiers::kinds::Exposure;
        assert_streamed_png_matches_buffered(
            "pointwise",
            vec![Modifier::new(ModifierKind::Exposure(Exposure {
                exposure: 0.3,
            }))],
            None,
            0,
            70,
            90,
        );
    }

    #[test]
    fn streamed_png_matches_buffered_blur() {
        use crate::modifiers::kinds::GaussianBlur;
        assert_streamed_png_matches_buffered(
            "blur",
            vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
                radius: 4.0,
            }))],
            None,
            0,
            70,
            90,
        );
    }

    #[test]
    fn streamed_png_matches_buffered_with_crop() {
        use crate::modifiers::kinds::GaussianBlur;
        assert_streamed_png_matches_buffered(
            "crop",
            vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
                radius: 3.0,
            }))],
            Some((16.0, 30.0, 48.0, 60.0)),
            0,
            80,
            100,
        );
    }

    #[test]
    fn streamed_png_matches_buffered_rotated_180() {
        use crate::modifiers::kinds::GaussianBlur;
        assert_streamed_png_matches_buffered(
            "rot180",
            vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
                radius: 3.0,
            }))],
            None,
            2,
            64,
            96,
        );
    }

    #[test]
    fn streamed_png_matches_buffered_vignette() {
        use crate::modifiers::kinds::Vignette;
        assert_streamed_png_matches_buffered(
            "vignette",
            vec![Modifier::new(ModifierKind::Vignette(Vignette::default()))],
            None,
            0,
            72,
            88,
        );
    }

    #[test]
    fn unstreamable_exports_are_rejected() {
        use crate::modifiers::kinds::{ChromaticAberration, GaussianBlur, PixelSort};

        let mk = |mods: Vec<Modifier>, rotation: u8| ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: Arc::new(vec![0u8; 16 * 16 * 4]),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: 16,
            height: 16,
            modifiers: mods,
            rotation,
            trim: None,
        };

        let ca = vec![Modifier::new(ModifierKind::ChromaticAberration(
            ChromaticAberration { amount: 4.0 },
        ))];
        assert!(!can_stream_bands(&mk(ca, 0)), "CA must not stream");

        let vsort = vec![Modifier::new(ModifierKind::PixelSort(PixelSort {
            threshold: 0.4,
            angle: 90.0,
        }))];
        assert!(
            !can_stream_bands(&mk(vsort, 0)),
            "column sort must not stream"
        );

        let blur = vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
            radius: 2.0,
        }))];
        assert!(
            !can_stream_bands(&mk(blur.clone(), 1)),
            "90 degree rotation must not stream"
        );
        assert!(can_stream_bands(&mk(blur, 0)), "plain blur should stream");
    }

    fn resize_data(
        w: u32,
        h: u32,
        mut modifiers: Vec<Modifier>,
        crop: Option<(f32, f32, f32, f32)>,
    ) -> ExportData {
        if let Some((x, y, cw, ch)) = crop {
            use crate::modifiers::kinds::Crop;
            modifiers.push(Modifier::new(ModifierKind::Crop(Crop {
                x,
                y,
                width: cw,
                height: ch,
            })));
        }
        ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: Arc::new(vec![0u8; (w * h * 4) as usize]),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers,
            rotation: 0,
            trim: None,
        }
    }

    #[test]
    fn export_dimensions_follow_a_resize() {
        use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

        let data = resize_data(
            128,
            96,
            vec![Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 50.0,
                height: 50.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            }))],
            None,
        );
        let geom = geom_of(&data);
        assert_eq!((geom.img_w, geom.img_h), (64, 48));
        assert_eq!((geom.out_w, geom.out_h), (64, 48));
    }

    #[test]
    fn crop_applies_to_the_resized_buffer() {
        use crate::modifiers::kinds::{Resize, ResizeFilter, ResizeMode};

        let data = resize_data(
            128,
            96,
            vec![Modifier::new(ModifierKind::Resize(Resize {
                mode: ResizeMode::Percent,
                width: 50.0,
                height: 50.0,
                filter: ResizeFilter::Lanczos,
                lock_aspect: true,
            }))],
            Some((0.0, 0.0, 32.0, 24.0)),
        );
        let geom = geom_of(&data);
        assert_eq!(
            (geom.img_w, geom.img_h),
            (32, 24),
            "the chain's output is already cropped"
        );
        assert_eq!((geom.out_w, geom.out_h), (32, 24));
        assert_eq!(
            (geom.cx0, geom.cy0),
            (0, 0),
            "nothing is left for export to offset by"
        );
    }

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
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels,
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![Modifier::new(ModifierKind::Text(text))],
            rotation: 0,
            trim: None,
        };

        let (ow, oh, rgba) = render_still_rgba(&data).expect("render");
        assert_eq!((ow, oh), (w, h));

        let lit = rgba.chunks_exact(4).filter(|p| p[0] > 200).count();
        assert!(lit > 0, "expected white text pixels in export, found none");
    }

    #[test]
    fn drawing_appears_in_still_export() {
        use crate::modifiers::kinds::{Drawing, Stroke};

        let (w, h) = (128u32, 128u32);
        let pixels = Arc::new(vec![0u8; (w * h * 4) as usize]);

        let drawing = Drawing {
            strokes: vec![Stroke {
                points: vec![[0.2, 0.5], [0.8, 0.5]],
                size: 12.0,
                hardness: 0.8,
                opacity: 1.0,
                color: [1.0, 0.0, 0.0],
            }],
            ..Drawing::default()
        };
        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels,
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![Modifier::new(ModifierKind::Drawing(drawing))],
            rotation: 0,
            trim: None,
        };

        let (ow, oh, rgba) = render_still_rgba(&data).expect("render");
        assert_eq!((ow, oh), (w, h));

        let center = ((h / 2) as usize * w as usize + (w / 2) as usize) * 4;
        assert!(
            rgba[center] > 200 && rgba[center + 1] < 40,
            "expected red stroke at image center"
        );
        assert!(
            rgba[..w as usize * 4].chunks_exact(4).all(|p| p[3] == 0),
            "top row should stay untouched"
        );
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
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels,
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![
                Modifier::new(ModifierKind::Text(text)),
                Modifier::new(ModifierKind::ChromaticAberration(ca)),
            ],
            rotation: 0,
            trim: None,
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

    #[test]
    fn gaussian_blur_spreads_and_conserves_energy() {
        use crate::modifiers::kinds::GaussianBlur;

        let (w, h) = (64u32, 64u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        let cx = (h / 2 * w + w / 2) as usize * 4;
        px[cx..cx + 4].copy_from_slice(&[255, 255, 255, 255]);
        let pixels = Arc::new(px);

        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels,
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![Modifier::new(ModifierKind::GaussianBlur(GaussianBlur {
                radius: 4.0,
            }))],
            rotation: 0,
            trim: None,
        };

        let (_, _, rgba) = render_still_rgba(&data).expect("render");

        let center = rgba[cx];
        let nonzero = rgba.chunks_exact(4).filter(|p| p[0] > 0).count();
        assert!(center < 255, "blur should lower the peak (got {center})");
        assert!(center > 0, "center should stay non-zero (got {center})");
        assert!(
            nonzero > 9,
            "blur should spread to many pixels (got {nonzero})"
        );
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn gradient(w: u32, h: u32) -> Arc<Vec<u8>> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                px[o] = (x * 255 / w.max(1)) as u8;
                px[o + 1] = (y * 255 / h.max(1)) as u8;
                px[o + 2] = ((x + y) * 255 / (w + h).max(1)) as u8;
                px[o + 3] = 255;
            }
        }
        Arc::new(px)
    }

    #[test]
    fn mixed_chain_render_is_byte_stable() {
        use crate::modifiers::kinds::{ChromaticAberration, Exposure, GaussianBlur, Posterize};

        let (w, h) = (96u32, 72u32);
        let data = ExportData {
            source: ExportSource::Frames {
                frames: vec![ExportFrame {
                    pixels: gradient(w, h),
                    delay: Duration::ZERO,
                }],
                still_index: 0,
            },
            width: w,
            height: h,
            modifiers: vec![
                Modifier::new(ModifierKind::Exposure(Exposure { exposure: 0.5 })),
                Modifier::new(ModifierKind::GaussianBlur(GaussianBlur { radius: 3.0 })),
                Modifier::new(ModifierKind::ChromaticAberration(ChromaticAberration {
                    amount: 8.0,
                })),
                Modifier::new(ModifierKind::Posterize(Posterize { levels: 6 })),
            ],
            rotation: 0,
            trim: None,
        };

        let (ow, oh, rgba) = render_still_rgba(&data).expect("render");
        assert_eq!((ow, oh), (w, h));
        assert_eq!(
            fnv1a(&rgba),
            0xfbe26999ac19e5be,
            "mixed-chain render output changed"
        );
    }
}
