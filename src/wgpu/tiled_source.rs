use glam::{Mat4, Vec2};
use iced::wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Extent3d, Queue, RenderPipeline, Sampler,
    TexelCopyBufferLayout, Texture, TextureFormat, TextureUsages, TextureView,
};

use crate::wgpu::{
    error::ViewError,
    gpu,
    media::image_data::{ImageData, ImageId},
    passes::display::DisplayPass,
    view_pipeline::Uniforms,
};

pub struct Tile {
    pub _source_texture: Texture,
    pub source_view: TextureView,
    pub uniform_buffer: Buffer,
    pub zoom_out_bind_group: BindGroup,
    pub nearest_bind_group: BindGroup,
    pub linear_bind_group: BindGroup,
    pub last_ndc_rect: Option<(Vec2, Vec2)>,
    pub last_transform: Option<Mat4>,
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
    pub physical_scale: f32,
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
    ) -> Result<Self, ViewError> {
        let image_pixels = image.pixels.lock().unwrap();

        if image_pixels.len() < image.size_bytes() {
            return Err(ViewError::ImageDataMismatch {
                expected: image.size_bytes(),
                actual: image_pixels.len(),
            });
        }

        let max_dim = device.limits().max_texture_dimension_2d;
        let cols = image.width.div_ceil(max_dim);
        let rows = image.height.div_ceil(max_dim);
        let src_stride = (image.width * 4) as usize;

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
                            | TextureUsages::RENDER_ATTACHMENT
                    } else {
                        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST
                    },
                    Some(&format!("{label}:source")),
                );

                let row_bytes = (tw * 4) as usize;
                tile_pixels.clear();
                for r in 0..th {
                    let row_start = (ty + r) as usize * src_stride + tx as usize * 4;
                    tile_pixels.extend_from_slice(&image_pixels[row_start..row_start + row_bytes]);
                }

                queue.write_texture(
                    source_texture.as_image_copy(),
                    &tile_pixels,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(tw * 4),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: tw,
                        height: th,
                        depth_or_array_layers: 1,
                    },
                );

                if mipmap_zoom_out {
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
                } else {
                    queue.submit(std::iter::empty());
                }

                let source_view = source_texture.create_view(&Default::default());

                let uniform_buffer = gpu::uniform_buffer::<Uniforms>(
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
                    _source_texture: source_texture,
                    source_view,
                    uniform_buffer,
                    zoom_out_bind_group,
                    nearest_bind_group,
                    linear_bind_group,
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
}
