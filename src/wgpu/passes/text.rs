use bytemuck::{Pod, Zeroable};
use cosmic_text::{FontSystem, SwashCache};
use std::borrow::Cow;

use iced::wgpu::{
    BindGroupLayout, BlendState, Buffer, ColorTargetState, ColorWrites, CommandEncoder, Device,
    FragmentState, LoadOp, MultisampleState, Operations, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TexelCopyBufferLayout, Texture,
    TextureFormat, TextureUsages, TextureView, VertexState,
};

use crate::{
    modifiers::{gpu::TileInfo, kinds::Text, text_render},
    wgpu::gpu,
};

const REFERENCE_SIZE: f32 = 256.0;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TextUniforms {
    anchor: [f32; 2],
    block_size: [f32; 2],
    pivot: [f32; 2],
    tile_origin: [f32; 2],
    tile_size: [f32; 2],
    rotation: f32,
    opacity: f32,
    color: [f32; 3],
    _pad: f32,
}

pub struct TextLayer {
    _texture: Texture,
    view: TextureView,
    bbox_w: f32,
    bbox_h: f32,
    raster_size: f32,
    x: f32,
    y: f32,
    size: f32,
    rotation: f32,
    opacity: f32,
    color: [f32; 3],
}

pub struct TextPass {
    pipeline: RenderPipeline,
    copy_pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
    font_system: FontSystem,
    swash: SwashCache,
}

impl TextPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("text-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/text.wgsl"),
            Some("text-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::ALPHA_BLENDING,
            &bgl,
        );

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("text-copy-shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/text.wgsl"))),
        });
        let copy_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("text-copy-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let copy_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("text-copy-pipeline"),
            layout: Some(&copy_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_copy"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_copy"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            cache: None,
            multiview: None,
        });

        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("text-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            copy_pipeline,
            bgl,
            sampler,
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
        }
    }

    pub fn build_layer(
        &mut self,
        device: &Device,
        queue: &Queue,
        text: &Text,
    ) -> Option<TextLayer> {
        let raster_size = text.size.clamp(1.0, REFERENCE_SIZE);
        let mut raster_text = text.clone();
        raster_text.size = raster_size;

        let bmp = text_render::rasterize_text(&raster_text, &mut self.font_system, &mut self.swash);
        if bmp.is_empty() {
            return None;
        }

        let bbox_w = (bmp.max_x - bmp.min_x).ceil().max(1.0);
        let bbox_h = (bmp.max_y - bmp.min_y).ceil().max(1.0);
        let tw = bbox_w as u32;
        let th = bbox_h as u32;

        let mut buf = vec![0u8; (tw as usize) * (th as usize)];
        for g in &bmp.glyphs {
            let ox = (g.dst_x - bmp.min_x).round() as i32;
            let oy = (g.dst_y - bmp.min_y).round() as i32;
            let gw = g.width.round() as i32;
            let gh = g.height.round() as i32;
            for row in 0..gh {
                let py = oy + row;
                if py < 0 || py >= th as i32 {
                    continue;
                }
                for col in 0..gw {
                    let px = ox + col;
                    if px < 0 || px >= tw as i32 {
                        continue;
                    }
                    let src = (row as usize) * (gw as usize) + col as usize;
                    let Some(&a) = g.alpha.get(src) else { continue };
                    if a == 0 {
                        continue;
                    }
                    let dst = (py as usize) * (tw as usize) + px as usize;
                    buf[dst] = buf[dst].max(a);
                }
            }
        }

        let texture = gpu::texture_2d(
            device,
            tw,
            th,
            TextureFormat::R8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("text-glyph-texture"),
        );
        queue.write_texture(
            texture.as_image_copy(),
            &buf,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tw),
                rows_per_image: Some(th),
            },
            iced::wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());

        Some(TextLayer {
            _texture: texture,
            view,
            bbox_w,
            bbox_h,
            raster_size,
            x: text.x,
            y: text.y,
            size: text.size,
            rotation: text.rotation,
            opacity: text.opacity,
            color: [text.r, text.g, text.b],
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform_buffer: &Buffer,
        layer: &TextLayer,
        tile: &TileInfo,
        input: &TextureView,
        output: &TextureView,
    ) {
        let copy_bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            uniform_buffer,
            input,
            &self.sampler,
            Some("text-copy-bg"),
        );
        {
            let mut copy = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("text-copy-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: output,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(iced::wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            copy.set_pipeline(&self.copy_pipeline);
            copy.set_bind_group(0, &copy_bg, &[]);
            copy.draw(0..4, 0..1);
        }

        let scale = layer.size / layer.raster_size;
        let uniforms = TextUniforms {
            anchor: [layer.x * tile.full_w as f32, layer.y * tile.full_h as f32],
            block_size: [layer.bbox_w * scale, layer.bbox_h * scale],
            pivot: [0.5, 0.5],
            tile_origin: [tile.tile_x as f32, tile.tile_y as f32],
            tile_size: [tile.tile_w as f32, tile.tile_h as f32],
            rotation: layer.rotation.to_radians(),
            opacity: layer.opacity,
            color: layer.color,
            _pad: 0.0,
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);

        let bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            uniform_buffer,
            &layer.view,
            &self.sampler,
            Some("text-bg"),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("text-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: output,
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..4, 0..1);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<TextUniforms>(device, Some("text-uniform"))
    }
}
