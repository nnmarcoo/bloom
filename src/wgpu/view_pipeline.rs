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

use crate::{
    modifiers::Modifier,
    wgpu::{
        error::ViewError,
        gpu,
        media::image_data::{ImageData, ImageId},
        modifier_pipeline::ModifierPipeline,
        passes::{
            checkerboard::{CheckerboardPass, CheckerboardUniforms},
            display::DisplayPass,
        },
        tiled_source::TiledSource,
    },
};

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    pub transform: Mat4,
}

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
    checkerboard: CheckerboardPass,
    trilinear_sampler: Sampler,
    nearest_sampler: Sampler,
    linear_sampler: Sampler,
    blit_pipeline: RenderPipeline,
    blit_bgl: BindGroupLayout,
    placeholder_bind_group: BindGroup,
    _placeholder_uniform: Buffer,
    source: Option<TiledSource>,
    modifier_pipeline: Option<ModifierPipeline>,
    scale_factor: f32,
    last_checker_uniforms: Option<CheckerboardUniforms>,
    pub mipmap_zoom_out: bool,
    format: TextureFormat,
}

impl ViewPipeline {
    pub fn clear_source(&mut self, device: &Device) {
        if self.source.is_none() {
            return;
        }
        self.modifier_pipeline = None;
        self.source = None;
        let _ = device.poll(iced::wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
    }

    pub fn upload_image(
        &mut self,
        device: &Device,
        queue: &Queue,
        image: &ImageData,
    ) -> Result<(), ViewError> {
        if !image.pixels_available() {
            return Ok(());
        }
        self.modifier_pipeline = None;
        self.source = None;
        let _ = device.poll(iced::wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });

        self.source = Some(TiledSource::new(
            device,
            queue,
            image,
            &self.display,
            &self.trilinear_sampler,
            &self.nearest_sampler,
            &self.linear_sampler,
            self.mipmap_zoom_out,
            &self.blit_pipeline,
            &self.blit_bgl,
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        _device: &Device,
        queue: &Queue,
        scale: f32,
        scale_factor: f32,
        uniforms: &Uniforms,
        viewport: Vec2,
        pan_ndc: Vec2,
        rotation: u8,
    ) {
        self.scale_factor = scale_factor;
        let physical_scale = scale * scale_factor;

        let source = match &mut self.source {
            Some(s) => s,
            None => return,
        };
        source.physical_scale = physical_scale;

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
        let angle = -(rotation as f32) * std::f32::consts::FRAC_PI_2;
        let inv_tile_vp = if rotation.is_multiple_of(2) {
            vec2(1.0 / viewport.x, 1.0 / viewport.y)
        } else {
            vec2(1.0 / viewport.y, 1.0 / viewport.x)
        };

        for tile in &mut source.tiles {
            let tw = tile.width as f32;
            let th = tile.height as f32;
            let tile_cx = (tile.x as f32 + tw * 0.5) - full_w * 0.5;
            let tile_cy = (full_h * 0.5) - (tile.y as f32 + th * 0.5);
            let tile_offset = 2.0 * vec2(tile_cx, tile_cy) * inv_tile_vp;
            let tile_aspect = vec2(tw, th) * inv_tile_vp;

            let transform = Mat4::from_scale(vec3(scale, scale, 1.0))
                * Mat4::from_translation(vec3(pan_ndc.x, pan_ndc.y, 0.0))
                * Mat4::from_rotation_z(angle)
                * Mat4::from_translation(vec3(tile_offset.x, tile_offset.y, 0.0))
                * Mat4::from_scale(vec3(tile_aspect.x, tile_aspect.y, 1.0));

            if tile.last_transform != Some(transform) {
                queue.write_buffer(&tile.uniform_buffer, 0, bytes_of(&Uniforms { transform }));
                tile.last_ndc_rect = Some(ndc_rect_of_transform(&transform));
                tile.last_transform = Some(transform);
            }
        }
    }

    pub fn update_checkerboard(&mut self, queue: &Queue, uniforms: CheckerboardUniforms) {
        if self.last_checker_uniforms != Some(uniforms) {
            self.checkerboard.update_colors(queue, &uniforms);
            self.last_checker_uniforms = Some(uniforms);
        }
    }

    pub fn prepare_modifiers(
        &mut self,
        device: &Device,
        queue: &Queue,
        modifiers: &[Modifier],
        dirty_from: Option<usize>,
    ) {
        let source = match &self.source {
            Some(s) => s,
            None => {
                self.modifier_pipeline = None;
                return;
            }
        };

        if !modifiers.iter().any(|m| m.has_visible_effect()) {
            self.modifier_pipeline = None;
            return;
        }

        let (w, h) = (source.full_width, source.full_height);

        let needs_create = self
            .modifier_pipeline
            .as_ref()
            .is_none_or(|mp| mp.width != w || mp.height != h);

        if needs_create {
            let mut mp = ModifierPipeline::new(device, self.format, w, h);
            mp.prepare(device, queue, source, modifiers, None);
            self.modifier_pipeline = Some(mp);
        } else if let Some(mp) = &mut self.modifier_pipeline {
            mp.prepare(device, queue, source, modifiers, dirty_from);
        }
    }

    pub fn render_checkerboard(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
    ) {
        self.checkerboard
            .draw(encoder, target, clip_bounds, bounds, self.scale_factor);
    }

    pub fn render_display(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        clip_bounds: &Rectangle<u32>,
        bounds: &Rectangle,
        smooth_zoom_in: bool,
    ) {
        if let Some(mp) = &self.modifier_pipeline {
            if let Some(source) = &self.source {
                let zoomed_out = source.physical_scale < 1.0 - 1e-6;
                let nearest = !smooth_zoom_in && !zoomed_out;
                for (i, tile) in source.tiles.iter().enumerate() {
                    if let Some((min_ndc, max_ndc)) = tile.last_ndc_rect
                        && (max_ndc.x < -1.0
                            || min_ndc.x > 1.0
                            || max_ndc.y < -1.0
                            || min_ndc.y > 1.0)
                    {
                        continue;
                    }
                    if let Some(bg) = mp.tile_display_bg(i, nearest) {
                        self.draw_display_pass(encoder, target, clip_bounds, bounds, bg);
                    }
                }
            }
            return;
        }

        if let Some(source) = &self.source {
            for tile in &source.tiles {
                if let Some((min_ndc, max_ndc)) = tile.last_ndc_rect
                    && (max_ndc.x < -1.0 || min_ndc.x > 1.0 || max_ndc.y < -1.0 || min_ndc.y > 1.0)
                {
                    continue;
                }

                let zoomed_out = source.physical_scale < 1.0 - 1e-6;
                let bind_group = if zoomed_out {
                    &tile.zoom_out_bind_group
                } else if smooth_zoom_in {
                    &tile.linear_bind_group
                } else {
                    &tile.nearest_bind_group
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

        let linear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("linear-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
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

        let checkerboard = CheckerboardPass::new(device, format);

        Self {
            display,
            checkerboard,
            trilinear_sampler,
            nearest_sampler,
            linear_sampler,
            blit_pipeline,
            mipmap_zoom_out: true,
            blit_bgl,
            placeholder_bind_group,
            _placeholder_uniform: placeholder_uniform,
            source: None,
            modifier_pipeline: None,
            scale_factor: 1.0,
            last_checker_uniforms: None,
            format,
        }
    }
}
