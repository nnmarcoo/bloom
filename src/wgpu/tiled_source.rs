use glam::{Mat4, Vec2};
use iced::wgpu::{
    BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, Extent3d, Queue, RenderPipeline,
    Sampler, TexelCopyBufferLayout, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::wgpu::{
    error::ViewError,
    gpu,
    media::image_data::{ImageData, ImageId},
    passes::display::DisplayPass,
    view_pipeline::DisplayUniforms,
};

/// A tile's GPU-side resources.
///
/// Split out from [`Tile`] so they can be dropped and rebuilt independently of
/// the tile's identity. Holding every tile's textures for the lifetime of the
/// image makes resident VRAM a function of image size rather than of what is
/// visible, which is what makes very large images thrash: past the card's
/// capacity the driver pages textures across PCIe instead of failing, and
/// sampling cost rises roughly 8x with no error anywhere for the pipeline to
/// observe (see `large_image_probe::probe_vram_spill`).
///
/// Nothing evicts these yet. This type exists so that "not resident" is
/// representable and every consumer has to handle it, before any policy decides
/// when to drop them.
pub struct TileResidency {
    pub _source_texture: Texture,
    pub source_view: TextureView,
    pub zoom_out_bind_group: BindGroup,
    pub nearest_bind_group: BindGroup,
    pub linear_bind_group: BindGroup,
}

pub struct Tile {
    /// GPU resources, or `None` when the tile is not resident.
    ///
    /// Always `Some` today. Consumers must still handle `None`, since that is
    /// the state eviction will introduce.
    pub residency: Option<TileResidency>,
    pub uniform_buffer: Buffer,
    pub last_ndc_rect: Option<(Vec2, Vec2)>,
    pub last_transform: Option<Mat4>,
    pub last_crop_uv: Option<[f32; 4]>,
    pub proc_rect_uv: Option<[f32; 4]>,
    pub proc_rect_px: Option<[f32; 4]>,
    pub isec_px: Option<[f32; 4]>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
}

impl Tile {
    /// The tile's sampleable view, or `None` when it is not resident.
    pub fn source_view(&self) -> Option<&TextureView> {
        self.residency.as_ref().map(|r| &r.source_view)
    }

    /// The bind group for the given sampling mode, or `None` when the tile is
    /// not resident.
    pub fn display_bind_group(&self, mode: TileSampling) -> Option<&BindGroup> {
        let r = self.residency.as_ref()?;
        Some(match mode {
            TileSampling::ZoomOut => &r.zoom_out_bind_group,
            TileSampling::Linear => &r.linear_bind_group,
            TileSampling::Nearest => &r.nearest_bind_group,
        })
    }

    /// The bytes this tile occupies in VRAM while resident, mip chain included.
    // No caller yet: the residency budget that will use it is the next step.
    #[allow(dead_code)]
    pub fn resident_bytes(&self) -> u64 {
        tile_resident_bytes(self.width, self.height, self.mip_count)
    }
}

/// VRAM cost of one resident tile, mip chain included.
///
/// Free-standing so the residency budget can size a tile it has not built yet,
/// and so the accounting is testable without a GPU.
// Exercised by tests; the production caller arrives with the residency budget.
#[allow(dead_code)]
pub fn tile_resident_bytes(width: u32, height: u32, mip_count: u32) -> u64 {
    let base = width as u64 * height as u64 * 4;
    if mip_count > 1 {
        // A full mip chain converges to 4/3 of the base level.
        base * 4 / 3
    } else {
        base
    }
}

/// How a tile is sampled for display, chosen by zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileSampling {
    /// Zoomed out: sample the mip chain.
    ZoomOut,
    /// Zoomed in with smoothing.
    Linear,
    /// Zoomed in without smoothing.
    Nearest,
}

pub struct TiledSource {
    pub tiles: Vec<Tile>,
    pub image_id: ImageId,
    pub full_width: u32,
    pub full_height: u32,
    pub physical_scale: f32,
    pub has_mipmaps: bool,
    pub mips_dirty: bool,
}

#[allow(clippy::too_many_arguments)]
fn write_tile_texture(
    queue: &Queue,
    texture: &Texture,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    full_width: u32,
    image_pixels: &[u8],
    scratch: &mut Vec<u8>,
) {
    let src_stride = (full_width * 4) as usize;
    if width == full_width {
        queue.write_texture(
            texture.as_image_copy(),
            image_pixels,
            TexelCopyBufferLayout {
                offset: (y as usize * src_stride) as u64,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    } else {
        let row_bytes = (width * 4) as usize;
        scratch.clear();
        for r in 0..height {
            let row_start = (y + r) as usize * src_stride + x as usize * 4;
            scratch.extend_from_slice(&image_pixels[row_start..row_start + row_bytes]);
        }
        queue.write_texture(
            texture.as_image_copy(),
            scratch,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn mip_encoder<'a>(
    encoder: &'a mut Option<CommandEncoder>,
    device: &Device,
) -> &'a mut CommandEncoder {
    encoder.get_or_insert_with(|| {
        device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("tiled-source-mip-encoder"),
        })
    })
}

fn regen_tile_mipmaps(
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    mip_count: u32,
    blit_pipeline: &RenderPipeline,
    blit_bgl: &BindGroupLayout,
    linear_sampler: &Sampler,
) {
    let mut encoder = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
        label: Some("tiled-source-mip-encoder"),
    });
    gpu::generate_hw_mipmaps(
        &mut encoder,
        device,
        texture,
        mip_count,
        TextureFormat::Rgba8Unorm,
        blit_pipeline,
        blit_bgl,
        linear_sampler,
    );
    queue.submit([encoder.finish()]);
}

impl TiledSource {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &Device,
        queue: &Queue,
        image: &ImageData,
        display_pass: &DisplayPass,
        trilinear_sampler: &Sampler,
        nearest_sampler: &Sampler,
        linear_sampler: &Sampler,
        mipmap_zoom_out: bool,
        blit_pipeline: &RenderPipeline,
        blit_bgl: &BindGroupLayout,
        tile_dim: Option<u32>,
    ) -> Result<Self, ViewError> {
        let image_pixels = image.pixels_snapshot();

        if image_pixels.len() < image.size_bytes() {
            return Err(ViewError::ImageDataMismatch {
                expected: image.size_bytes(),
                actual: image_pixels.len(),
            });
        }

        let limit = device.limits().max_texture_dimension_2d;
        let max_dim = tile_dim.map_or(limit, |d| d.clamp(1, limit));
        let cols = image.width.div_ceil(max_dim);
        let rows = image.height.div_ceil(max_dim);

        let max_tile_bytes = (max_dim * max_dim * 4) as usize;
        let mut tile_pixels = Vec::with_capacity(max_tile_bytes);

        let mut tiles = Vec::with_capacity((cols * rows) as usize);

        for row in 0..rows {
            for col in 0..cols {
                let tx = col * max_dim;
                let ty = row * max_dim;
                let tw = (image.width - tx).min(max_dim);
                let th = (image.height - ty).min(max_dim);
                let label = format!("tile[{col},{row}]");

                let mip_count = if mipmap_zoom_out {
                    gpu::hw_mip_count(tw, th)
                } else {
                    1
                };

                let source_texture = gpu::texture_2d_mipmapped(
                    device,
                    tw,
                    th,
                    mip_count,
                    TextureFormat::Rgba8Unorm,
                    if mipmap_zoom_out {
                        TextureUsages::TEXTURE_BINDING
                            | TextureUsages::COPY_DST
                            | TextureUsages::COPY_SRC
                            | TextureUsages::RENDER_ATTACHMENT
                    } else {
                        TextureUsages::TEXTURE_BINDING
                            | TextureUsages::COPY_DST
                            | TextureUsages::COPY_SRC
                    },
                    Some(&format!("{label}:source")),
                );

                write_tile_texture(
                    queue,
                    &source_texture,
                    tx,
                    ty,
                    tw,
                    th,
                    image.width,
                    image_pixels.as_slice(),
                    &mut tile_pixels,
                );

                if mipmap_zoom_out && mip_count > 1 {
                    regen_tile_mipmaps(
                        device,
                        queue,
                        &source_texture,
                        mip_count,
                        blit_pipeline,
                        blit_bgl,
                        linear_sampler,
                    );
                }

                let source_view = source_texture.create_view(&Default::default());

                let uniform_buffer = gpu::uniform_buffer::<DisplayUniforms>(
                    device,
                    Some(&format!("{label}:display-uniform")),
                );
                let zoom_out_bind_group = display_pass.create_bind_group(
                    device,
                    &uniform_buffer,
                    &source_view,
                    if mipmap_zoom_out {
                        trilinear_sampler
                    } else {
                        nearest_sampler
                    },
                    Some(&format!("{label}:zoom-out-bg")),
                );
                let nearest_bind_group = display_pass.create_bind_group(
                    device,
                    &uniform_buffer,
                    &source_view,
                    nearest_sampler,
                    Some(&format!("{label}:nearest-bg")),
                );
                let linear_bind_group = display_pass.create_bind_group(
                    device,
                    &uniform_buffer,
                    &source_view,
                    linear_sampler,
                    Some(&format!("{label}:linear-bg")),
                );

                tiles.push(Tile {
                    residency: Some(TileResidency {
                        _source_texture: source_texture,
                        source_view,
                        zoom_out_bind_group,
                        nearest_bind_group,
                        linear_bind_group,
                    }),
                    uniform_buffer,
                    last_ndc_rect: None,
                    last_transform: None,
                    last_crop_uv: None,
                    proc_rect_uv: None,
                    proc_rect_px: None,
                    isec_px: None,
                    x: tx,
                    y: ty,
                    width: tw,
                    height: th,
                    mip_count,
                });
            }
        }

        Ok(TiledSource {
            tiles,
            image_id: image.id,
            full_width: image.width,
            full_height: image.height,
            physical_scale: 1.0,
            has_mipmaps: mipmap_zoom_out,
            mips_dirty: false,
        })
    }

    pub fn matches(&self, image: &ImageData, mipmap_zoom_out: bool) -> bool {
        self.full_width == image.width
            && self.full_height == image.height
            && self.has_mipmaps == mipmap_zoom_out
    }

    pub fn write_frame(
        &mut self,
        device: &Device,
        queue: &Queue,
        image: &ImageData,
        blit_pipeline: &RenderPipeline,
        blit_bgl: &BindGroupLayout,
        linear_sampler: &Sampler,
    ) -> Result<(), ViewError> {
        let image_pixels = image.pixels_snapshot();
        if image_pixels.len() < image.size_bytes() {
            return Err(ViewError::ImageDataMismatch {
                expected: image.size_bytes(),
                actual: image_pixels.len(),
            });
        }

        let full_width = self.full_width;
        let needs_mips = self.has_mipmaps && self.physical_scale < 1.0 - 1e-6;
        let mut scratch = Vec::new();

        for tile in &self.tiles {
            // A non-resident tile has no texture to write into. It will pick up
            // the new contents when it is next made resident.
            let Some(res) = tile.residency.as_ref() else {
                continue;
            };
            write_tile_texture(
                queue,
                &res._source_texture,
                tile.x,
                tile.y,
                tile.width,
                tile.height,
                full_width,
                image_pixels.as_slice(),
                &mut scratch,
            );

            if needs_mips && tile.mip_count > 1 {
                regen_tile_mipmaps(
                    device,
                    queue,
                    &res._source_texture,
                    tile.mip_count,
                    blit_pipeline,
                    blit_bgl,
                    linear_sampler,
                );
            }
        }

        self.mips_dirty = self.has_mipmaps && !needs_mips;
        self.image_id = image.id;
        Ok(())
    }

    pub fn regen_mipmaps(
        &mut self,
        device: &Device,
        queue: &Queue,
        blit_pipeline: &RenderPipeline,
        blit_bgl: &BindGroupLayout,
        linear_sampler: &Sampler,
    ) {
        let mut encoder: Option<CommandEncoder> = None;

        for tile in &self.tiles {
            let Some(res) = tile.residency.as_ref() else {
                continue;
            };
            if tile.mip_count > 1 {
                gpu::generate_hw_mipmaps(
                    mip_encoder(&mut encoder, device),
                    device,
                    &res._source_texture,
                    tile.mip_count,
                    TextureFormat::Rgba8Unorm,
                    blit_pipeline,
                    blit_bgl,
                    linear_sampler,
                );
            }
        }

        if let Some(encoder) = encoder {
            queue.submit([encoder.finish()]);
        }

        self.mips_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmipped_tile_costs_its_pixels() {
        assert_eq!(tile_resident_bytes(1024, 1024, 1), 1024 * 1024 * 4);
    }

    #[test]
    fn mipped_tile_costs_four_thirds_of_the_base_level() {
        let base = 1024u64 * 1024 * 4;
        assert_eq!(tile_resident_bytes(1024, 1024, 11), base * 4 / 3);
    }

    /// The measured failure this work exists to fix: a 50000x50000 source needs
    /// far more VRAM than a typical discrete card has, and with mipmaps on (the
    /// default) it is worse still. Pinning the arithmetic keeps the motivating
    /// numbers honest if tile sizing ever changes.
    #[test]
    fn a_gigapixel_source_far_exceeds_a_typical_cards_vram() {
        let tile_dim = 8192u32;
        let tiles_per_side = 50000u32.div_ceil(tile_dim) as u64;
        let n_tiles = tiles_per_side * tiles_per_side;

        let unmipped = n_tiles * tile_resident_bytes(tile_dim, tile_dim, 1);
        let mipped = n_tiles * tile_resident_bytes(tile_dim, tile_dim, 14);

        // An 8GB card, all of it, which is already optimistic.
        let vram = 8u64 * 1024 * 1024 * 1024;
        assert!(
            unmipped > vram,
            "expected a 50000^2 source to exceed 8GB even without mips"
        );
        assert!(
            mipped > unmipped,
            "mips must be accounted for; they are on by default"
        );
    }
}
