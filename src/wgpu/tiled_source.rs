use glam::{Mat4, Vec2};
use iced::wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Extent3d, Queue, RenderPipeline, Sampler,
    TexelCopyBufferLayout, Texture, TextureFormat, TextureUsages,
};

use crate::wgpu::{
    error::ViewError,
    gpu,
    media::image_data::{ImageData, ImageId},
    passes::display::DisplayPass,
    view_pipeline::Uniforms,
};

pub struct Tile {
    /// Source texture (Rgba8Unorm) with hardware-generated mip levels.
    pub _source_texture: Texture,
    /// Per-tile display transform uniform buffer.
    pub uniform_buffer: Buffer,
    /// HW mip pyramid + trilinear sampler — used when zoomed out (physical_scale < 1).
    pub hw_mip_bind_group: BindGroup,
    /// Nearest-neighbor sampler on mip 0 — used when zoomed in (physical_scale >= 1).
    pub nearest_bind_group: BindGroup,
    /// Lanczos mip pyramid bind group — built incrementally, None until ready.
    pub lanczos_bind_group: Option<BindGroup>,
    /// Keeps the Lanczos Rgba16Float mip texture alive.
    pub _lanczos_texture: Option<Texture>,

    /// Cached NDC bounding rect for viewport culling (min, max).
    pub last_ndc_rect: Option<(Vec2, Vec2)>,
    /// Last transform written to `uniform_buffer` — skips redundant GPU writes.
    pub last_transform: Option<Mat4>,

    /// Position and size within the full image.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct TiledSource {
    pub tiles: Vec<Tile>,
    pub image_id: ImageId,
    pub full_width: u32,
    pub full_height: u32,
    /// Cached physical scale for sampler selection.
    pub physical_scale: f32,
}

impl TiledSource {
    pub fn new(
        device: &Device,
        queue: &Queue,
        image: &ImageData,
        display_pass: &DisplayPass,
        trilinear_sampler: &Sampler,
        nearest_sampler: &Sampler,
        blit_pipeline: &RenderPipeline,
        blit_bgl: &BindGroupLayout,
    ) -> Result<Self, ViewError> {
        if image.pixels.len() < image.size_bytes() {
            return Err(ViewError::ImageDataMismatch {
                expected: image.size_bytes(),
                actual: image.pixels.len(),
            });
        }

        let max_dim = device.limits().max_texture_dimension_2d;
        let cols = (image.width + max_dim - 1) / max_dim;
        let rows = (image.height + max_dim - 1) / max_dim;
        let src_stride = (image.width * 4) as usize;

        let mut tiles = Vec::with_capacity((cols * rows) as usize);

        for row in 0..rows {
            for col in 0..cols {
                let tx = col * max_dim;
                let ty = row * max_dim;
                let tw = (image.width - tx).min(max_dim);
                let th = (image.height - ty).min(max_dim);
                let label = format!("tile[{col},{row}]");

                let mip_count = gpu::hw_mip_count(tw, th);

                let source_texture = gpu::texture_2d_mipmapped(
                    device,
                    tw,
                    th,
                    mip_count,
                    TextureFormat::Rgba8Unorm,
                    TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_DST
                        | TextureUsages::RENDER_ATTACHMENT,
                    Some(&format!("{label}:source")),
                );

                let offset = ty as usize * src_stride + tx as usize * 4;
                queue.write_texture(
                    source_texture.as_image_copy(),
                    &image.pixels[offset..],
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(src_stride as u32),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: tw,
                        height: th,
                        depth_or_array_layers: 1,
                    },
                );

                let mut encoder =
                    device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
                        label: Some(&format!("{label}:mip-encoder")),
                    });
                gpu::generate_hw_mipmaps(
                    &mut encoder,
                    device,
                    &source_texture,
                    mip_count,
                    TextureFormat::Rgba8Unorm,
                    blit_pipeline,
                    blit_bgl,
                );
                queue.submit(std::iter::once(encoder.finish()));

                let source_view = source_texture.create_view(&Default::default());

                let uniform_buffer = gpu::uniform_buffer::<Uniforms>(
                    device,
                    Some(&format!("{label}:display-uniform")),
                );
                let hw_mip_bind_group = display_pass.create_bind_group(
                    device,
                    &uniform_buffer,
                    &source_view,
                    trilinear_sampler,
                    Some(&format!("{label}:hw-mip-bg")),
                );
                let nearest_bind_group = display_pass.create_bind_group(
                    device,
                    &uniform_buffer,
                    &source_view,
                    nearest_sampler,
                    Some(&format!("{label}:nearest-bg")),
                );

                tiles.push(Tile {
                    _source_texture: source_texture,
                    uniform_buffer,
                    hw_mip_bind_group,
                    nearest_bind_group,
                    lanczos_bind_group: None,
                    _lanczos_texture: None,
                    last_ndc_rect: None,
                    last_transform: None,
                    x: tx,
                    y: ty,
                    width: tw,
                    height: th,
                });
            }
        }

        Ok(TiledSource {
            tiles,
            image_id: image.id,
            full_width: image.width,
            full_height: image.height,
            physical_scale: 1.0,
        })
    }

    pub fn lanczos_all_ready(&self) -> bool {
        self.tiles.iter().all(|t| t.lanczos_bind_group.is_some())
    }
}
