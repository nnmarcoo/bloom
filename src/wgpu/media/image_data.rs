use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytemuck;
use icns::{IconFamily, PixelFormat as IcnsPixelFormat};
use image::{
    AnimationDecoder, ColorType, ImageDecoder, ImageError, ImageReader, codecs::hdr::HdrDecoder,
    codecs::openexr::OpenExrDecoder, codecs::png::PngDecoder,
};
use jxl_oxide::JxlImage;
use psd::Psd;
use resvg;
use tiny_skia::Pixmap;
use usvg::Options as SvgOptions;
use zip::ZipArchive;

use super::animation::{Animation, Frame};

#[derive(Debug, Clone)]
pub enum MediaData {
    Image(ImageData),
    Animation(Animation),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageId(u64);

#[derive(Debug, Clone)]
pub struct ImageData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub id: ImageId,
}

impl ImageData {
    pub fn size_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    pub fn load(path: &Path) -> Result<Self, ImageError> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        reader.no_limits();
        let img = reader.decode()?.into_rgba8();
        let (width, height) = img.dimensions();
        let pixels = img.into_raw();
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels,
            width,
            height,
            id,
        })
    }

    pub fn load_gif(path: &Path) -> Result<Animation, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut gif_opts = gif::DecodeOptions::new();
        gif_opts.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = gif_opts
            .read_info(BufReader::new(file))
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let mut screen = gif_dispose::Screen::new_decoder(&decoder);

        let mut frames = Vec::new();
        while let Some(frame) = decoder
            .read_next_frame()
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?
        {
            let delay_ms = frame.delay as u64 * 10;
            let delay = Duration::from_millis(if delay_ms < 20 { 100 } else { delay_ms });

            screen
                .blit_frame(frame)
                .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;

            let img = screen.pixels_rgba();
            let (pixels_rgba, width, height) = img.to_contiguous_buf();
            let pixels = pixels_rgba
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect();
            let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
            frames.push(Frame {
                data: Arc::new(ImageData {
                    pixels,
                    width: width as u32,
                    height: height as u32,
                    id,
                }),
                delay,
            });
        }

        Ok(Animation::new(frames))
    }

    // Extended Reinhard tonemapping (Reinhard et al. 2002, eq. 4) preserving hue.
    // Scale is derived from the peak channel so all three channels compress
    // proportionally — no channel clips relative to the others.
    fn tonemap_scale(r: f32, g: f32, b: f32) -> f32 {
        let peak = r.max(g).max(b);
        if peak > 1e-6 {
            (peak / (1.0 + peak)) / peak
        } else {
            1.0
        }
    }

    fn tonemap_channel(v: f32, scale: f32) -> u8 {
        ((v * scale).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8
    }

    pub fn load_hdr(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let decoder = HdrDecoder::new(BufReader::new(file))?;
        let meta = decoder.metadata();
        let width = meta.width;
        let height = meta.height;
        let pixel_count = width as usize * height as usize;
        let mut buf = vec![0u8; pixel_count * 12];
        decoder.read_image(&mut buf)?;

        let floats: &[f32] = bytemuck::cast_slice(&buf);
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        for chunk in floats.chunks_exact(3) {
            let scale = Self::tonemap_scale(chunk[0], chunk[1], chunk[2]);
            pixels.push(Self::tonemap_channel(chunk[0], scale));
            pixels.push(Self::tonemap_channel(chunk[1], scale));
            pixels.push(Self::tonemap_channel(chunk[2], scale));
            pixels.push(255);
        }

        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels,
            width,
            height,
            id,
        })
    }

    pub fn load_exr(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let decoder = OpenExrDecoder::new(BufReader::new(file))?;
        let (width, height) = decoder.dimensions();
        let color_type = decoder.color_type();
        let pixel_count = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(pixel_count * 4);

        match color_type {
            ColorType::Rgb32F => {
                let mut buf = vec![0u8; pixel_count * 12];
                decoder.read_image(&mut buf)?;
                let floats: &[f32] = bytemuck::cast_slice(&buf);
                for chunk in floats.chunks_exact(3) {
                    let scale = Self::tonemap_scale(chunk[0], chunk[1], chunk[2]);
                    pixels.push(Self::tonemap_channel(chunk[0], scale));
                    pixels.push(Self::tonemap_channel(chunk[1], scale));
                    pixels.push(Self::tonemap_channel(chunk[2], scale));
                    pixels.push(255);
                }
            }
            ColorType::Rgba32F => {
                let mut buf = vec![0u8; pixel_count * 16];
                decoder.read_image(&mut buf)?;
                let floats: &[f32] = bytemuck::cast_slice(&buf);
                for chunk in floats.chunks_exact(4) {
                    let scale = Self::tonemap_scale(chunk[0], chunk[1], chunk[2]);
                    pixels.push(Self::tonemap_channel(chunk[0], scale));
                    pixels.push(Self::tonemap_channel(chunk[1], scale));
                    pixels.push(Self::tonemap_channel(chunk[2], scale));
                    pixels.push((chunk[3].clamp(0.0, 1.0) * 255.0) as u8);
                }
            }
            _ => return Self::load(path),
        }

        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels,
            width,
            height,
            id,
        })
    }

    pub fn load_jxl(path: &Path) -> Result<Self, ImageError> {
        let image = JxlImage::open_with_defaults(path)
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let width = image.width();
        let height = image.height();
        let render = image
            .render_frame(0)
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;

        let fb = render.image_all_channels();
        let channels = fb.channels();
        let buf = fb.buf();

        let pixel_count = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        for chunk in buf.chunks_exact(channels) {
            pixels.push((chunk[0].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push((chunk[1].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push((chunk[2].clamp(0.0, 1.0) * 255.0) as u8);
            pixels.push(if channels >= 4 {
                (chunk[3].clamp(0.0, 1.0) * 255.0) as u8
            } else {
                255
            });
        }

        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels,
            width,
            height,
            id,
        })
    }

    pub fn load_psd(path: &Path) -> Result<Self, ImageError> {
        let bytes = std::fs::read(path).map_err(ImageError::IoError)?;
        let psd =
            Psd::from_bytes(&bytes).map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels: psd.rgba(),
            width: psd.width(),
            height: psd.height(),
            id,
        })
    }

    pub fn load_kra(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let mut entry = archive
            .by_name("mergedimage.png")
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let mut png_bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut png_bytes).map_err(ImageError::IoError)?;
        let img =
            image::load_from_memory_with_format(&png_bytes, image::ImageFormat::Png)?.into_rgba8();
        let (width, height) = img.dimensions();
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels: img.into_raw(),
            width,
            height,
            id,
        })
    }

    pub fn load_icns(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let family = IconFamily::read(BufReader::new(file)).map_err(ImageError::IoError)?;
        let icon_type = family
            .available_icons()
            .into_iter()
            .max_by_key(|t| t.pixel_width() * t.pixel_height())
            .ok_or_else(|| ImageError::IoError(std::io::Error::other("no icons in ICNS file")))?;
        let image = family
            .get_icon_with_type(icon_type)
            .map_err(ImageError::IoError)?;
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels: image
                .convert_to(IcnsPixelFormat::RGBA)
                .into_data()
                .into_vec(),
            width: image.width(),
            height: image.height(),
            id,
        })
    }

    pub fn load_svg(path: &Path) -> Result<Self, ImageError> {
        let svg_data = std::fs::read(path).map_err(ImageError::IoError)?;
        let mut opt = SvgOptions {
            resources_dir: path.parent().map(|p| p.to_path_buf()),
            ..SvgOptions::default()
        };
        opt.fontdb_mut().load_system_fonts();
        let tree = usvg::Tree::from_data(&svg_data, &opt)
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let size = tree.size().to_int_size();
        let width = size.width().max(1);
        let height = size.height().max(1);
        let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
            ImageError::IoError(std::io::Error::other("SVG dimensions too large"))
        })?;
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Ok(Self {
            pixels: pixmap.take(),
            width,
            height,
            id,
        })
    }

    pub fn load_apng(path: &Path) -> Result<Animation, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let decoder = PngDecoder::new(BufReader::new(file))?.apng()?;
        let mut frames = Vec::new();
        for frame_result in decoder.into_frames() {
            let frame = frame_result?;
            let (numer, denom) = frame.delay().numer_denom_ms();
            let delay_ms = (numer as u64) / (denom as u64).max(1);
            let delay = Duration::from_millis(if delay_ms < 20 { 100 } else { delay_ms });
            let rgba = frame.into_buffer();
            let (width, height) = rgba.dimensions();
            let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
            frames.push(Frame {
                data: Arc::new(ImageData {
                    pixels: rgba.into_raw(),
                    width,
                    height,
                    id,
                }),
                delay,
            });
        }
        Ok(Animation::new(frames))
    }

    pub fn load_webp_animated(path: &Path) -> Result<Animation, ImageError> {
        let data = std::fs::read(path).map_err(ImageError::IoError)?;
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&data))
            .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
        let (width, height) = decoder.dimensions();
        let num_frames = decoder.num_frames();
        let has_alpha = decoder.has_alpha();
        let buf_size = decoder.output_buffer_size().ok_or_else(|| {
            ImageError::IoError(std::io::Error::other("unknown WebP buffer size"))
        })?;
        let mut frames = Vec::new();
        let mut buf = vec![0u8; buf_size];
        for _ in 0..num_frames {
            let duration_ms = decoder
                .read_frame(&mut buf)
                .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
            let delay = Duration::from_millis(if duration_ms < 20 {
                100
            } else {
                duration_ms as u64
            });
            let pixels = if has_alpha {
                buf.clone()
            } else {
                buf.chunks_exact(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect()
            };
            let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
            frames.push(Frame {
                data: Arc::new(ImageData {
                    pixels,
                    width,
                    height,
                    id,
                }),
                delay,
            });
        }
        Ok(Animation::new(frames))
    }

    pub fn load_media(path: &Path) -> Result<MediaData, ImageError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("gif") {
            Ok(MediaData::Animation(Self::load_gif(path)?))
        } else if ext.eq_ignore_ascii_case("hdr") {
            Ok(MediaData::Image(Self::load_hdr(path)?))
        } else if ext.eq_ignore_ascii_case("exr") {
            Ok(MediaData::Image(Self::load_exr(path)?))
        } else if ext.eq_ignore_ascii_case("jxl") {
            Ok(MediaData::Image(Self::load_jxl(path)?))
        } else if ext.eq_ignore_ascii_case("psd") {
            Ok(MediaData::Image(Self::load_psd(path)?))
        } else if ext.eq_ignore_ascii_case("icns") {
            Ok(MediaData::Image(Self::load_icns(path)?))
        } else if ext.eq_ignore_ascii_case("kra") {
            Ok(MediaData::Image(Self::load_kra(path)?))
        } else if ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz") {
            Ok(MediaData::Image(Self::load_svg(path)?))
        } else if ext.eq_ignore_ascii_case("apng") {
            Ok(MediaData::Animation(Self::load_apng(path)?))
        } else if ext.eq_ignore_ascii_case("webp") {
            let file = File::open(path).map_err(ImageError::IoError)?;
            let decoder = image_webp::WebPDecoder::new(BufReader::new(file))
                .map_err(|e| ImageError::IoError(std::io::Error::other(e)))?;
            if decoder.is_animated() {
                Ok(MediaData::Animation(Self::load_webp_animated(path)?))
            } else {
                Ok(MediaData::Image(Self::load(path)?))
            }
        } else {
            Ok(MediaData::Image(Self::load(path)?))
        }
    }
}
