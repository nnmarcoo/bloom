use bytemuck::bytes_of;
use glam::{Mat4, Vec2, vec2, vec3, vec4};
use iced::{
    Rectangle,
    wgpu::{
        AddressMode, BindGroup, BindGroupLayout, Buffer, CommandEncoder, Device, Extent3d,
        FilterMode, LoadOp, Operations, Queue, RenderPassColorAttachment, RenderPassDescriptor,
        RenderPipeline, Sampler, SamplerDescriptor, StoreOp, TexelCopyBufferLayout, TextureFormat,
        TextureUsages, TextureView,
    },
    widget::shader::Pipeline,
};

use crate::wgpu::{
    error::ViewError,
    gpu,
    lanczos_build::LanczosBuildState,
    media::image_data::{ImageData, ImageId},
    passes::{display::DisplayPass, lanczos::LanczosPass},
    tiled_source::TiledSource,
};

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    pub transform: Mat4,
}

/// Returns the NDC axis-aligned bounding box of a quad after `transform` is applied.
fn ndc_rect_of_transform(transform: &Mat4) -> (Vec2, Vec2) {
    let corners = [
        vec4(-1.0, -1.0, 0.0, 1.0),
        vec4(1.0, -1.0, 0.0, 1.0),
        vec4(-1.0, 1.0, 0.0, 1.0),
        vec4(1.0, 1.0, 0.0, 1.0),
    ];
    let clip = corners.map(|c| (*transform * c).truncate().truncate());
    let min = clip.iter().copied().fold(clip[0], Vec2::min);
    let max = clip.iter().copied().fold(clip[0], Vec2::max);
    (min, max)
}

pub struct ViewPipeline {
    display: DisplayPass,
    lanczos_h: LanczosPass,
    lanczos_v: LanczosPass,
    trilinear_sampler: Sampler,
    nearest_sampler: Sampler,
    blit_pipeline: RenderPipeline,
    blit_bgl: BindGroupLayout,
    placeholder_bind_group: BindGroup,
    _placeholder_uniform: Buffer,
    source: Option<TiledSource>,
    lanczos_build: Option<LanczosBuildState>,
    lanczos_enabled: bool,
    scale_factor: f32,
}

impl ViewPipeline {
    pub fn upload_image(
        &mut self,
        device: &Device,
        queue: &Queue,
        image: &ImageData,
    ) -> Result<(), ViewError> {
        self.lanczos_build = None;
        self.source = Some(TiledSource::new(
            device,
            queue,
            image,
            &self.display,
            &self.trilinear_sampler,
            &self.nearest_sampler,
            &self.blit_pipeline,
            &self.blit_bgl,
        )?);
        if self.lanczos_enabled {
            if let Some(ref src) = self.source {
                self.lanczos_build = Some(LanczosBuildState::new(src.tiles.len()));
            }
        }
        Ok(())
    }

    pub fn set_lanczos_enabled(&mut self, enabled: bool) {
        self.lanczos_enabled = enabled;
        if enabled {
            if let Some(ref src) = self.source {
                if !src.lanczos_all_ready() && self.lanczos_build.is_none() {
                    self.lanczos_build = Some(LanczosBuildState::new(src.tiles.len()));
                }
            }
        } else {
            self.lanczos_build = None;
        }
    }

    pub fn update(
        &mut self,
        device: &Device,
        queue: &Queue,
        scale: f32,
        scale_factor: f32,
        uniforms: &Uniforms,
        viewport: Vec2,
        pan_ndc: Vec2,
        lanczos_enabled: bool,
    ) {
        if lanczos_enabled != self.lanczos_enabled {
            self.set_lanczos_enabled(lanczos_enabled);
        }

        self.scale_factor = scale_factor;
        let physical_scale = scale * scale_factor;

        let source = match &mut self.source {
            Some(s) => s,
            None => return,
        };
        source.physical_scale = physical_scale;

        // Advance Lanczos build by one tile per frame.
        if let Some(ref mut build) = self.lanczos_build {
            let done = build.step(
                device,
                queue,
                source,
                &self.display,
                &self.lanczos_h,
                &self.lanczos_v,
                &self.trilinear_sampler,
            );
            if done {
                self.lanczos_build = None;
            }
        }

        // Write per-tile transform uniforms, skipping redundant writes.
        if source.tiles.len() == 1 {
            let tile = &mut source.tiles[0];
            if tile.last_transform != Some(uniforms.transform) {
                queue.write_buffer(&tile.uniform_buffer, 0, bytes_of(uniforms));
                tile.last_ndc_rect = Some(ndc_rect_of_transform(&uniforms.transform));
                tile.last_transform = Some(uniforms.transform);
            }
            return;
        }

        let full_w = source.full_width as f32;
        let full_h = source.full_height as f32;
        let inv_viewport = vec2(1.0 / viewport.x, 1.0 / viewport.y);

        for tile in &mut source.tiles {
            let tw = tile.width as f32;
            let th = tile.height as f32;
            let tile_cx = (tile.x as f32 + tw * 0.5) - full_w * 0.5;
            let tile_cy = (full_h * 0.5) - (tile.y as f32 + th * 0.5);
            let tile_offset = 2.0 * vec2(tile_cx, tile_cy) * inv_viewport;
            let tile_aspect = vec2(tw, th) * inv_viewport;

            let transform = Mat4::from_scale(vec3(scale, scale, 1.0))
                * Mat4::from_translation(vec3(
                    pan_ndc.x + tile_offset.x,
                    pan_ndc.y + tile_offset.y,
                    0.0,
                ))
                * Mat4::from_scale(vec3(tile_aspect.x, tile_aspect.y, 1.0));

            if tile.last_transform != Some(transform) {
                queue.write_buffer(&tile.uniform_buffer, 0, bytes_of(&Uniforms { transform }));
                tile.last_ndc_rect = Some(ndc_rect_of_transform(&transform));
                tile.last_transform = Some(transform);
            }
        }
    }

    pub fn render_display(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
    ) {
        if let Some(source) = &self.source {
            for tile in &source.tiles {
                // Viewport culling.
                if let Some((min_ndc, max_ndc)) = tile.last_ndc_rect {
                    if max_ndc.x < -1.0 || min_ndc.x > 1.0 || max_ndc.y < -1.0 || min_ndc.y > 1.0 {
                        continue;
                    }
                }

                let bind_group = if source.physical_scale >= 1.0 - 1e-6 {
                    &tile.nearest_bind_group
                } else if self.lanczos_enabled {
                    tile.lanczos_bind_group
                        .as_ref()
                        .unwrap_or(&tile.hw_mip_bind_group)
                } else {
                    &tile.hw_mip_bind_group
                };

                self.draw_display_pass(encoder, target, clip_bounds, bounds, bind_group);
            }
        } else {
            self.draw_display_pass(
                encoder,
                target,
                clip_bounds,
                bounds,
                &self.placeholder_bind_group,
            );
        }
    }

    fn draw_display_pass(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
        bind_group: &BindGroup,
    ) {
        let sf = self.scale_factor;
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("display-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_viewport(
            bounds.x * sf,
            bounds.y * sf,
            bounds.width * sf,
            bounds.height * sf,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        self.display.draw(&mut pass, bind_group);
    }

    pub fn needs_upload(&self, image_id: ImageId) -> bool {
        match &self.source {
            Some(s) => s.image_id != image_id,
            None => true,
        }
    }
}

impl Pipeline for ViewPipeline {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self
    where
        Self: Sized,
    {
        let display = DisplayPass::new(device, format);

        let trilinear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("trilinear-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("nearest-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let (blit_pipeline, blit_bgl) = gpu::blit_pipeline(device, TextureFormat::Rgba8Unorm);

        let placeholder_texture = gpu::texture_2d(
            device,
            1,
            1,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("placeholder-texture"),
        );
        queue.write_texture(
            placeholder_texture.as_image_copy(),
            &[128u8, 128, 128, 255],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let placeholder_view = placeholder_texture.create_view(&Default::default());
        let placeholder_uniform =
            gpu::uniform_buffer::<Uniforms>(device, Some("placeholder-uniform"));
        let placeholder_bind_group = display.create_bind_group(
            device,
            &placeholder_uniform,
            &placeholder_view,
            &trilinear_sampler,
            Some("placeholder-bg"),
        );

        let lanczos_h = LanczosPass::new(
            device,
            TextureFormat::Rgba16Float,
            include_str!("shaders/lanczos_h.wgsl"),
        );
        let lanczos_v = LanczosPass::new(
            device,
            TextureFormat::Rgba16Float,
            include_str!("shaders/lanczos_v.wgsl"),
        );

        Self {
            display,
            lanczos_h,
            lanczos_v,
            trilinear_sampler,
            nearest_sampler,
            blit_pipeline,
            blit_bgl,
            placeholder_bind_group,
            _placeholder_uniform: placeholder_uniform,
            source: None,
            lanczos_build: None,
            lanczos_enabled: false,
            scale_factor: 1.0,
        }
    }

    fn trim(&mut self) {}
}
