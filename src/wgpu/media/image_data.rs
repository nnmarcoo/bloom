use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rayon::prelude::*;

use basis_universal::transcoding::{
    DecodeFlags, TranscodeParameters, Transcoder, TranscoderTextureFormat,
};
use bytemuck::cast_slice;
use dds::{ColorFormat, Decoder as DdsDecoder, ImageViewMut};
use dicom_object::open_file as dicom_open_file;
use dicom_pixeldata::PixelDecoder;
use fitsrs::hdu::data::image::Pixels;
use fitsrs::{Fits, HDU};
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
use xcf_rs::data::xcf::Xcf;
use zip::ZipArchive;

use super::animation::{Animation, Frame};
use super::exif_data::ExifData;

#[derive(Debug, Clone)]
pub enum MediaData {
    Image(Box<ImageData>),
    Animation(Animation),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageId(u64);

#[derive(Debug)]
pub struct ImageData {
    pub pixels: Mutex<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub id: ImageId,
    histogram: OnceLock<([u32; 256], [u32; 256], [u32; 256])>,
    pub exif: ExifData,
    pub bit_depth: u8,
    pub color_space: Option<&'static str>,
}

impl Clone for ImageData {
    fn clone(&self) -> Self {
        Self {
            pixels: Mutex::new(self.pixels.lock().unwrap().clone()),
            width: self.width,
            height: self.height,
            id: self.id,
            histogram: OnceLock::new(),
            exif: self.exif.clone(),
            bit_depth: self.bit_depth,
            color_space: self.color_space,
        }
    }
}

impl ImageData {
    pub fn size_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    pub fn new(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let id = ImageId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        Self {
            pixels: Mutex::new(pixels),
            width,
            height,
            id,
            histogram: OnceLock::new(),
            exif: ExifData::default(),
            bit_depth: 8,
            color_space: None,
        }
    }

    pub fn pixels_available(&self) -> bool {
        self.pixels.lock().unwrap().len() >= self.size_bytes()
    }

    pub fn release_pixels(&self) {
        *self.pixels.lock().unwrap() = Vec::new();
    }

    pub fn histogram(&self) -> &([u32; 256], [u32; 256], [u32; 256]) {
        self.histogram.get_or_init(|| {
            let guard = self.pixels.lock().unwrap();
            Self::compute_histogram(&guard)
        })
    }

    fn compute_histogram(pixels: &[u8]) -> ([u32; 256], [u32; 256], [u32; 256]) {
        const CHUNK: usize = 65536 * 4;
        pixels
            .par_chunks(CHUNK)
            .map(|chunk| {
                let mut r = [0u32; 256];
                let mut g = [0u32; 256];
                let mut b = [0u32; 256];
                for px in chunk.chunks_exact(4) {
                    r[px[0] as usize] += 1;
                    g[px[1] as usize] += 1;
                    b[px[2] as usize] += 1;
                }
                (r, g, b)
            })
            .reduce(
                || ([0u32; 256], [0u32; 256], [0u32; 256]),
                |(mut ra, mut ga, mut ba), (rb, gb, bb)| {
                    for i in 0..256 {
                        ra[i] += rb[i];
                        ga[i] += gb[i];
                        ba[i] += bb[i];
                    }
                    (ra, ga, ba)
                },
            )
    }

    pub fn load(path: &Path) -> Result<Self, ImageError> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        reader.no_limits();
        let dyn_img = reader.decode()?;
        let bit_depth = match dyn_img.color() {
            ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => 16,
            ColorType::Rgb32F | ColorType::Rgba32F => 32,
            _ => 8,
        };
        let img = dyn_img.into_rgba8();
        let (width, height) = img.dimensions();
        let mut data = Self::new(img.into_raw(), width, height);
        data.bit_depth = bit_depth;
        Ok(data)
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

        Animation::new(frames)
    }

    fn tonemap_scale(r: f32, g: f32, b: f32) -> f32 {
        let peak = r.max(g).max(b);
        if peak > 1e-6 { 1.0 / (1.0 + peak) } else { 1.0 }
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

        let floats: &[f32] = cast_slice(&buf);
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        for chunk in floats.chunks_exact(3) {
            let scale = Self::tonemap_scale(chunk[0], chunk[1], chunk[2]);
            pixels.push(Self::tonemap_channel(chunk[0], scale));
            pixels.push(Self::tonemap_channel(chunk[1], scale));
            pixels.push(Self::tonemap_channel(chunk[2], scale));
            pixels.push(255);
        }

        let mut data = Self::new(pixels, width, height);
        data.bit_depth = 32;
        data.color_space = Some("Linear");
        Ok(data)
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
                let floats: &[f32] = cast_slice(&buf);
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
                let floats: &[f32] = cast_slice(&buf);
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

        let mut data = Self::new(pixels, width, height);
        data.bit_depth = 32;
        data.color_space = Some("Linear");
        Ok(data)
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
        Animation::new(frames)
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
                buf.to_vec()
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
        Animation::new(frames)
    }

    pub fn load_jp2(path: &Path) -> Result<Self, ImageError> {
        let bytes = std::fs::read(path).map_err(ImageError::IoError)?;
        let img = Jp2Image::from_bytes(&bytes).map_err(|e| ImageError::IoError(Error::other(e)))?;
        let img_data = img
            .get_pixels(Some(255))
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let width = img_data.width;
        let height = img_data.height;
        let bit_depth = match &img_data.data {
            jpeg2k::ImagePixelData::Rgba16(_)
            | jpeg2k::ImagePixelData::Rgb16(_)
            | jpeg2k::ImagePixelData::La16(_)
            | jpeg2k::ImagePixelData::L16(_) => 16u8,
            _ => 8u8,
        };
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
        let mut data = Self::new(pixels, width, height);
        data.bit_depth = bit_depth;
        Ok(data)
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
        use ktx2::Format;

        let data = std::fs::read(path).map_err(ImageError::IoError)?;
        let reader = Ktx2Reader::new(&data)
            .map_err(|e| ImageError::IoError(Error::other(format!("{e:?}"))))?;
        let header = reader.header();
        let width = header.pixel_width;
        let height = header.pixel_height;
        let level = reader
            .levels()
            .next()
            .ok_or_else(|| ImageError::IoError(Error::other("KTX2 has no mip levels")))?;

        let is_basis = header.supercompression_scheme == Some(SupercompressionScheme::BasisLZ)
            || header.format.is_none();
        if is_basis {
            let basis_data = level.data.to_vec();
            let mut transcoder = Transcoder::new();
            transcoder.prepare_transcoding(&basis_data).map_err(|_| {
                ImageError::IoError(Error::other("KTX2 prepare_transcoding failed"))
            })?;
            let desc = transcoder
                .image_level_description(&basis_data, 0, 0)
                .ok_or_else(|| {
                    ImageError::IoError(Error::other("KTX2 no image level description"))
                })?;
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
            return Ok(Self::new(pixels, desc.original_width, desc.original_height));
        }

        let raw: Vec<u8> = match header.supercompression_scheme {
            Some(s) if s == SupercompressionScheme::Zstandard => {
                zstd::decode_all(level.data).map_err(|e| ImageError::IoError(Error::other(e)))?
            }
            _ => level.data.to_vec(),
        };

        let fmt = header
            .format
            .ok_or_else(|| ImageError::IoError(Error::other("KTX2: missing format field")))?;
        let pixels: Vec<u8> = match fmt {
            f if f == Format::R8G8B8A8_UNORM || f == Format::R8G8B8A8_SRGB => raw,
            f if f == Format::B8G8R8A8_UNORM || f == Format::B8G8R8A8_SRGB => raw
                .chunks_exact(4)
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect(),
            f if f == Format::R8G8B8_UNORM || f == Format::R8G8B8_SRGB => raw
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            f if f == Format::B8G8R8_UNORM || f == Format::B8G8R8_SRGB => raw
                .chunks_exact(3)
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect(),
            _ => {
                return Err(ImageError::IoError(Error::other(format!(
                    "KTX2: unsupported Vulkan format {:?}",
                    fmt.value()
                ))));
            }
        };

        Ok(Self::new(pixels, width, height))
    }

    pub fn load_raw(path: &Path) -> Result<Self, ImageError> {
        use rawler::imgop::develop::RawDevelop;

        let raw = rawler::decode_file(path).map_err(|e| ImageError::IoError(Error::other(e)))?;
        let intermediate = RawDevelop::default()
            .develop_intermediate(&raw)
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let img = intermediate
            .to_dynamic_image()
            .ok_or_else(|| ImageError::IoError(Error::other("failed to convert RAW to image")))?
            .into_rgba8();
        let (width, height) = img.dimensions();
        let mut data = Self::new(img.into_raw(), width, height);
        data.bit_depth = 16;
        Ok(data)
    }

    #[cfg(feature = "heif")]
    pub fn load_heic(path: &Path) -> Result<Self, ImageError> {
        use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

        let lib_heif = LibHeif::new();
        let ctx = HeifContext::read_from_file(path.to_str().unwrap_or_default())
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let handle = ctx
            .primary_image_handle()
            .map_err(|e| ImageError::IoError(Error::other(e)))?;

        let has_alpha = handle.has_alpha_channel();
        let chroma = if has_alpha {
            RgbChroma::Rgba
        } else {
            RgbChroma::Rgb
        };

        let image = lib_heif
            .decode(&handle, ColorSpace::Rgb(chroma), None)
            .map_err(|e| ImageError::IoError(Error::other(e)))?;

        let plane = image
            .planes()
            .interleaved
            .ok_or_else(|| ImageError::IoError(Error::other("no interleaved plane in HEIC")))?;

        let width = plane.width as usize;
        let height = plane.height as usize;
        let stride = plane.stride;

        let mut pixels = Vec::with_capacity(width * height * 4);
        if has_alpha {
            for y in 0..height {
                pixels.extend_from_slice(&plane.data[y * stride..y * stride + width * 4]);
            }
        } else {
            for y in 0..height {
                for rgb in plane.data[y * stride..y * stride + width * 3].chunks_exact(3) {
                    pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }

        Ok(Self::new(pixels, plane.width, plane.height))
    }

    pub fn load_xcf(path: &Path) -> Result<Self, ImageError> {
        let path_buf = path.to_path_buf();
        let xcf = std::panic::catch_unwind(|| Xcf::open(&path_buf))
            .map_err(|_| {
                ImageError::IoError(Error::other("xcf-rs panicked parsing XCF tile data"))
            })?
            .map_err(|e| ImageError::IoError(Error::other(e)))?;
        let (width, height) = xcf.dimensions();
        let mut canvas = vec![0u8; width as usize * height as usize * 4];

        for layer in xcf.layers.iter().rev() {
            let (lw, lh): (u32, u32) = layer.dimensions();
            let buf = layer.raw_rgba_buffer();
            for py in 0..lh.min(height) {
                for px in 0..lw.min(width) {
                    let src = buf[(py * lw + px) as usize].0;
                    let dst = (py * width + px) as usize * 4;
                    let sa = src[3] as u32;
                    let da = canvas[dst + 3] as u32;
                    let out_a = sa + da * (255 - sa) / 255;
                    for c in 0..3 {
                        let num =
                            src[c] as u32 * sa + canvas[dst + c] as u32 * da * (255 - sa) / 255;
                        if let Some(v) = num.checked_div(out_a) {
                            canvas[dst + c] = v as u8;
                        }
                    }
                    if out_a > 0 {
                        canvas[dst + 3] = out_a as u8;
                    }
                }
            }
        }

        Ok(Self::new(canvas, width, height))
    }

    pub fn load_fits(path: &Path) -> Result<Self, ImageError> {
        let file = File::open(path).map_err(ImageError::IoError)?;
        let mut hdu_list = Fits::from_reader(BufReader::new(file));

        let hdu = hdu_list
            .next()
            .ok_or_else(|| ImageError::IoError(Error::other("FITS file has no HDUs")))?
            .map_err(|e| ImageError::IoError(Error::other(e)))?;

        let HDU::Primary(primary) = hdu else {
            return Err(ImageError::IoError(Error::other(
                "FITS primary HDU is not an image",
            )));
        };

        let naxis = primary.get_header().get_xtension().get_naxis();
        let (width, height) = match naxis {
            [w, h] | [w, h, _] => (*w as u32, *h as u32),
            _ => {
                return Err(ImageError::IoError(Error::other(
                    "FITS image must be 2D or 3D",
                )));
            }
        };

        let image_data = hdu_list.get_data(&primary);
        let floats: Vec<f32> = match image_data.pixels() {
            Pixels::I16(it) => it.map(|v| v as f32).collect(),
            Pixels::I32(it) => it.map(|v| v as f32).collect(),
            Pixels::I64(it) => it.map(|v| v as f32).collect(),
            Pixels::F32(it) => it.collect(),
            Pixels::F64(it) => it.map(|v| v as f32).collect(),
            Pixels::U8(it) => it.map(|v| v as f32).collect(),
        };

        let plane_len = (width * height) as usize;
        let floats = &floats[..plane_len.min(floats.len())];

        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for &v in floats {
            if v.is_finite() {
                min = min.min(v);
                max = max.max(v);
            }
        }
        let range = (max - min).max(1e-10);

        let mut pixels = Vec::with_capacity(plane_len * 4);
        for &v in floats {
            let byte = ((v - min) / range * 255.0) as u8;
            pixels.extend_from_slice(&[byte, byte, byte, 255]);
        }
        pixels.resize(plane_len * 4, 0);

        Ok(Self::new(pixels, width, height))
    }

    pub fn load_eps(path: &Path) -> Result<Self, ImageError> {
        use std::process::Command;

        let tmp = std::env::temp_dir().join(format!(
            "bloom_eps_{}_{}.png",
            std::process::id(),
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("out")
        ));

        let gs_candidates: &[&str] = if cfg!(windows) {
            &["gswin64c", "gswin32c", "gs"]
        } else {
            &["gs"]
        };

        let gs = gs_candidates
            .iter()
            .find(|&&bin| Command::new(bin).arg("-v").output().is_ok())
            .copied()
            .ok_or_else(|| {
                ImageError::IoError(Error::other(
                    "Ghostscript not found on PATH (install gs / gswin64c)",
                ))
            })?;

        let status = Command::new(gs)
            .args([
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=pngalpha",
                "-r150",
                &format!("-sOutputFile={}", tmp.display()),
                path.to_str().unwrap_or_default(),
            ])
            .status()
            .map_err(|e| ImageError::IoError(Error::other(e)))?;

        if !status.success() {
            return Err(ImageError::IoError(Error::other(
                "Ghostscript failed to render EPS",
            )));
        }

        let result = Self::load(&tmp);
        let _ = std::fs::remove_file(&tmp);
        result
    }

    pub fn load_media(path: &Path) -> Result<MediaData, ImageError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let media = match ext.as_str() {
            "gif" => MediaData::Animation(Self::load_gif(path)?),
            "apng" => MediaData::Animation(Self::load_apng(path)?),
            "webp" => {
                let file = File::open(path).map_err(ImageError::IoError)?;
                let decoder = image_webp::WebPDecoder::new(BufReader::new(file))
                    .map_err(|e| ImageError::IoError(Error::other(e)))?;
                if decoder.is_animated() {
                    MediaData::Animation(Self::load_webp_animated(path)?)
                } else {
                    MediaData::Image(Box::new(Self::load(path)?))
                }
            }
            _ => {
                #[allow(clippy::type_complexity)]
                static TABLE: &[(&[&str], fn(&Path) -> Result<ImageData, ImageError>)] = &[
                    (&["hdr"], ImageData::load_hdr),
                    (&["exr"], ImageData::load_exr),
                    (&["jxl"], ImageData::load_jxl),
                    (&["psd", "psb"], ImageData::load_psd),
                    (&["icns"], ImageData::load_icns),
                    (&["kra"], ImageData::load_kra),
                    (&["xcf"], ImageData::load_xcf),
                    (&["svg", "svgz"], ImageData::load_svg),
                    (&["avif"], ImageData::load),
                    (&["jp2", "j2k", "j2c", "jpx"], ImageData::load_jp2),
                    (&["dcm", "dicom"], ImageData::load_dicom),
                    (&["dds"], ImageData::load_dds),
                    (&["ktx2"], ImageData::load_ktx2),
                    (&["fits", "fit", "fts"], ImageData::load_fits),
                    (&["eps", "ps", "epsf"], ImageData::load_eps),
                    (
                        &[
                            "ari", "arw", "cr2", "cr3", "crm", "crw", "dcr", "dcs", "dng", "erf",
                            "fff", "iiq", "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "ori",
                            "pef", "qtk", "raf", "raw", "rw2", "rwl", "srw", "x3f", "3fr",
                        ],
                        ImageData::load_raw,
                    ),
                    #[cfg(feature = "heif")]
                    (&["heic", "heif"], ImageData::load_heic),
                ];

                let loader = TABLE
                    .iter()
                    .find(|(exts, _)| exts.contains(&ext.as_str()))
                    .map(|(_, f)| *f)
                    .unwrap_or(ImageData::load);

                MediaData::Image(Box::new(loader(path)?))
            }
        };

        Ok(Self::attach_exif(path, media))
    }

    fn attach_exif(path: &Path, media: MediaData) -> MediaData {
        if let MediaData::Image(mut img) = media {
            img.exif = ExifData::read(path);
            MediaData::Image(img)
        } else {
            media
        }
    }
}
