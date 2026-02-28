use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use basis_universal::transcoding::{
    DecodeFlags, TranscodeParameters, Transcoder, TranscoderTextureFormat,
};
use bytemuck;
use dds::{ColorFormat, Decoder as DdsDecoder, ImageViewMut};
use dicom_object::open_file as dicom_open_file;
use dicom_pixeldata::PixelDecoder;
use icns::{IconFamily, PixelFormat as IcnsPixelFormat};
use image::{
    AnimationDecoder, ColorType, ImageDecoder, ImageError, ImageReader, codecs::hdr::HdrDecoder,
    codecs::openexr::OpenExrDecoder, codecs::png::PngDecoder,
};
use jpeg2k::Image as Jp2Image;
use jxl_oxide::JxlImage;
use ktx2::{Reader as Ktx2Reader, SupercompressionScheme};
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

    fn new(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Self {
            pixels,
            width,
            height,
            id,
        }
    }

    pub fn load(path: &Path) -> Result<Self, ImageError> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        reader.no_limits();
        let img = reader.decode()?.into_rgba8();
        let (width, height) = img.dimensions();
        Ok(Self::new(img.into_raw(), width, height))
    }

    pub fn load_gif(path: &Path) -> Result<Animation, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut gif_opts = gif::DecodeOptions::new();
        gif_opts.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = gif_opts
            .read_info(BufReader::new(file))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let mut screen = gif_dispose::Screen::new_decoder(&decoder);

        let mut frames = Vec::new();
        while let Some(frame) = decoder
            .read_next_frame()
            .map_err(|e| ImageError::IoError(Error::other(e)))?
        {
            let delay_ms = frame.delay as u64 * 10;
            let delay = Duration::from_millis(if delay_ms < 20 { 100 } else { delay_ms });

            screen
                .blit_frame(frame)
                .map_err(|e| ImageError::IoError(Error::other(e)))?;

            let img = screen.pixels_rgba();
            let (pixels_rgba, width, height) = img.to_contiguous_buf();
            let pixels = pixels_rgba
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect();
            frames.push(Frame {
                data: Arc::new(ImageData::new(pixels, width as u32, height as u32)),
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

        Ok(Self::new(pixels, width, height))
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

        Ok(Self::new(pixels, width, height))
    }

    pub fn load_jxl(path: &Path) -> Result<Self, ImageError> {
        let image =
            JxlImage::open_with_defaults(path).map_err(|e| ImageError::IoError(Error::other(e)))?;
        let width = image.width();
        let height = image.height();
        let render = image
            .render_frame(0)
            .map_err(|e| ImageError::IoError(Error::other(e)))?;

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

        Ok(Self::new(pixels, width, height))
    }

    pub fn load_psd(path: &Path) -> Result<Self, ImageError> {
        let bytes = std::fs::read(path).map_err(ImageError::IoError)?;
        let psd = Psd::from_bytes(&bytes).map_err(|e| ImageError::IoError(Error::other(e)))?;
        Ok(Self::new(psd.rgba(), psd.width(), psd.height()))
    }

    pub fn load_kra(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let mut entry = archive
            .by_name("mergedimage.png")
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let mut png_bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut png_bytes).map_err(ImageError::IoError)?;
        let img =
            image::load_from_memory_with_format(&png_bytes, image::ImageFormat::Png)?.into_rgba8();
        let (width, height) = img.dimensions();
        Ok(Self::new(img.into_raw(), width, height))
    }

    pub fn load_icns(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let family = IconFamily::read(BufReader::new(file)).map_err(ImageError::IoError)?;
        let icon_type = family
            .available_icons()
            .into_iter()
            .max_by_key(|t| t.pixel_width() * t.pixel_height())
            .ok_or_else(|| ImageError::IoError(Error::other("no icons in ICNS file")))?;
        let image = family
            .get_icon_with_type(icon_type)
            .map_err(ImageError::IoError)?;
        Ok(Self::new(
            image
                .convert_to(IcnsPixelFormat::RGBA)
                .into_data()
                .into_vec(),
            image.width(),
            image.height(),
        ))
    }

    pub fn load_svg(path: &Path) -> Result<Self, ImageError> {
        let svg_data = std::fs::read(path).map_err(ImageError::IoError)?;
        let mut opt = SvgOptions {
            resources_dir: path.parent().map(|p| p.to_path_buf()),
            ..SvgOptions::default()
        };
        opt.fontdb_mut().load_system_fonts();
        let tree = usvg::Tree::from_data(&svg_data, &opt)
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let size = tree.size().to_int_size();
        let width = size.width().max(1);
        let height = size.height().max(1);
        let mut pixmap = Pixmap::new(width, height)
            .ok_or_else(|| ImageError::IoError(Error::other("SVG dimensions too large")))?;
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
        Ok(Self::new(pixmap.take(), width, height))
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
            frames.push(Frame {
                data: Arc::new(ImageData::new(rgba.into_raw(), width, height)),
                delay,
            });
        }
        Ok(Animation::new(frames))
    }

    pub fn load_webp_animated(path: &Path) -> Result<Animation, ImageError> {
        let data = std::fs::read(path).map_err(ImageError::IoError)?;
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&data))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let (width, height) = decoder.dimensions();
        let num_frames = decoder.num_frames();
        let has_alpha = decoder.has_alpha();
        let buf_size = decoder
            .output_buffer_size()
            .ok_or_else(|| ImageError::IoError(Error::other("unknown WebP buffer size")))?;
        let mut frames = Vec::new();
        let mut buf = vec![0u8; buf_size];
        for _ in 0..num_frames {
            let duration_ms = decoder
                .read_frame(&mut buf)
                .map_err(|e| ImageError::IoError(Error::other(e)))?;
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
            frames.push(Frame {
                data: Arc::new(ImageData::new(pixels, width, height)),
                delay,
            });
        }
        Ok(Animation::new(frames))
    }

    pub fn load_jp2(path: &Path) -> Result<Self, ImageError> {
        let img = Jp2Image::from_file(path).map_err(|e| ImageError::IoError(Error::other(e)))?;
        let img_data = img
            .get_pixels(Some(255))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let width = img_data.width;
        let height = img_data.height;
        let pixels = match img_data.data {
            jpeg2k::ImagePixelData::Rgba8(p) => p,
            jpeg2k::ImagePixelData::Rgb8(p) => p
                .chunks_exact(3)
                .flat_map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            jpeg2k::ImagePixelData::La8(p) => p
                .chunks_exact(2)
                .flat_map(|c| [c[0], c[0], c[0], c[1]])
                .collect(),
            jpeg2k::ImagePixelData::L8(p) => p.into_iter().flat_map(|v| [v, v, v, 255]).collect(),
            jpeg2k::ImagePixelData::Rgba16(p) => p
                .chunks_exact(4)
                .flat_map(|c| c.iter().map(|&v| (v >> 8) as u8))
                .collect(),
            jpeg2k::ImagePixelData::Rgb16(p) => p
                .chunks_exact(3)
                .flat_map(|c| [(c[0] >> 8) as u8, (c[1] >> 8) as u8, (c[2] >> 8) as u8, 255])
                .collect(),
            jpeg2k::ImagePixelData::La16(p) => p
                .chunks_exact(2)
                .flat_map(|c| {
                    let v = (c[0] >> 8) as u8;
                    [v, v, v, (c[1] >> 8) as u8]
                })
                .collect(),
            jpeg2k::ImagePixelData::L16(p) => p
                .into_iter()
                .flat_map(|v| {
                    let b = (v >> 8) as u8;
                    [b, b, b, 255]
                })
                .collect(),
        };
        Ok(Self::new(pixels, width, height))
    }

    pub fn load_dicom(path: &Path) -> Result<Self, ImageError> {
        let obj = dicom_open_file(path).map_err(|e| ImageError::IoError(Error::other(e)))?;
        let pixel_data = obj
            .decode_pixel_data()
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let img = pixel_data
            .to_dynamic_image(0)
            .map_err(|e| ImageError::IoError(Error::other(e)))?
            .into_rgba8();
        let (width, height) = img.dimensions();
        Ok(Self::new(img.into_raw(), width, height))
    }

    pub fn load_dds(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut decoder = DdsDecoder::new(BufReader::new(file))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let size = decoder.main_size();
        let width = size.width;
        let height = size.height;
        let buf_len = ColorFormat::RGBA_U8
            .buffer_size(size)
            .ok_or_else(|| ImageError::IoError(Error::other("DDS dimensions overflow")))?;
        let mut pixels = vec![0u8; buf_len];
        let view = ImageViewMut::new(&mut pixels, size, ColorFormat::RGBA_U8)
            .ok_or_else(|| ImageError::IoError(Error::other("DDS buffer size mismatch")))?;
        decoder
            .read_surface(view)
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        Ok(Self::new(pixels, width, height))
    }

    pub fn load_ktx2(path: &Path) -> Result<Self, ImageError> {
        let data = std::fs::read(path).map_err(ImageError::IoError)?;
        let reader = Ktx2Reader::new(&data)
            .map_err(|e| ImageError::IoError(Error::other(format!("{e:?}"))))?;
        let header = reader.header();
        let level = reader
            .levels()
            .next()
            .ok_or_else(|| ImageError::IoError(Error::other("KTX2 has no mip levels")))?;

        let basis_data: Vec<u8> = match header.supercompression_scheme {
            Some(s) if s == SupercompressionScheme::Zstandard => {
                zstd::decode_all(level.data).map_err(|e| ImageError::IoError(Error::other(e)))?
            }
            _ => level.data.to_vec(),
        };

        let mut transcoder = Transcoder::new();
        transcoder
            .prepare_transcoding(&basis_data)
            .map_err(|_| ImageError::IoError(Error::other("KTX2 prepare_transcoding failed")))?;

        let desc = transcoder
            .image_level_description(&basis_data, 0, 0)
            .ok_or_else(|| ImageError::IoError(Error::other("KTX2 no image level description")))?;

        let params = TranscodeParameters {
            image_index: 0,
            level_index: 0,
            decode_flags: Some(DecodeFlags::HIGH_QUALITY),
            output_row_pitch_in_blocks_or_pixels: None,
            output_rows_in_pixels: None,
        };
        let pixels = transcoder
            .transcode_image_level(&basis_data, TranscoderTextureFormat::RGBA32, params)
            .map_err(|e| ImageError::IoError(Error::other(format!("{e:?}"))))?;

        Ok(Self::new(pixels, desc.original_width, desc.original_height))
    }

    pub fn load_media(path: &Path) -> Result<MediaData, ImageError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "gif" => Ok(MediaData::Animation(Self::load_gif(path)?)),
            "hdr" => Ok(MediaData::Image(Self::load_hdr(path)?)),
            "exr" => Ok(MediaData::Image(Self::load_exr(path)?)),
            "jxl" => Ok(MediaData::Image(Self::load_jxl(path)?)),
            "psd" => Ok(MediaData::Image(Self::load_psd(path)?)),
            "icns" => Ok(MediaData::Image(Self::load_icns(path)?)),
            "kra" => Ok(MediaData::Image(Self::load_kra(path)?)),
            "svg" | "svgz" => Ok(MediaData::Image(Self::load_svg(path)?)),
            "avif" => Ok(MediaData::Image(Self::load(path)?)),
            "jp2" | "j2k" | "j2c" | "jpx" => Ok(MediaData::Image(Self::load_jp2(path)?)),
            "dcm" | "dicom" => Ok(MediaData::Image(Self::load_dicom(path)?)),
            "dds" => Ok(MediaData::Image(Self::load_dds(path)?)),
            "ktx2" => Ok(MediaData::Image(Self::load_ktx2(path)?)),
            "apng" => Ok(MediaData::Animation(Self::load_apng(path)?)),
            "webp" => {
                let file = File::open(path).map_err(ImageError::IoError)?;
                let decoder = image_webp::WebPDecoder::new(BufReader::new(file))
                    .map_err(|e| ImageError::IoError(Error::other(e)))?;
                if decoder.is_animated() {
                    Ok(MediaData::Animation(Self::load_webp_animated(path)?))
                } else {
                    Ok(MediaData::Image(Self::load(path)?))
                }
            }
            _ => Ok(MediaData::Image(Self::load(path)?)),
        }
    }
}
