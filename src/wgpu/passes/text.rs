use bytemuck::{Pod, Zeroable};
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

const REFERENCE_SIZE: f32 = 1024.0;
const MIN_DENSITY: f32 = 1.0;
const MAX_RASTER_DIM: u32 = 8192;

fn downsample_r8(src: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut dst = vec![0u8; (nw * nh) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let sx = x * 2;
            let sy = y * 2;
            let mut sum = 0u32;
            let mut count = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let px = sx + dx;
                    let py = sy + dy;
                    if px < w && py < h {
                        sum += src[(py * w + px) as usize] as u32;
                        count += 1;
                    }
                }
            }
            dst[(y * nw + x) as usize] = (sum / count.max(1)) as u8;
        }
    }
    (dst, nw, nh)
}

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

impl TextLayer {
    pub fn refresh_transform(&mut self, text: &Text) {
        self.x = text.x;
        self.y = text.y;
        self.rotation = text.rotation;
        self.opacity = text.opacity;
        self.color = [text.r, text.g, text.b];
    }
}

pub struct TextPass {
    pipeline: RenderPipeline,
    copy_pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
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
            mipmap_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            copy_pipeline,
            bgl,
            sampler,
        }
    }

    pub fn build_layer(
        &self,
        device: &Device,
        queue: &Queue,
        text: &Text,
        display_density: f32,
    ) -> Option<TextLayer> {
        let mut raster_size =
            (text.size * display_density.max(MIN_DENSITY)).clamp(1.0, REFERENCE_SIZE);

        let rasterize = |raster_size: f32| {
            let mut raster_text = text.clone();
            raster_text.size = raster_size;
            let mut guard = text_render::lock_font_resources();
            text_render::rasterize_text(&raster_text, &mut guard.font_system).pack_alpha()
        };

        let max_dim = device.limits().max_texture_dimension_2d.min(MAX_RASTER_DIM);

        let mut packed = rasterize(raster_size)?;

        let largest = packed.width.max(packed.height);
        if largest > max_dim {
            let fit = (max_dim as f32 / largest as f32) * 0.98;
            raster_size = (raster_size * fit).max(1.0);
            packed = rasterize(raster_size)?;
            if packed.width > max_dim || packed.height > max_dim {
                return None;
            }
        }

        let bbox_w = packed.bbox_w;
        let bbox_h = packed.bbox_h;
        let tw = packed.width;
        let th = packed.height;
        let buf = packed.alpha;

        let mip_count = gpu::hw_mip_count(tw, th);
        let texture = gpu::texture_2d_mipmapped(
            device,
            tw,
            th,
            mip_count,
            TextureFormat::R8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("text-glyph-texture"),
        );

        let mut level = buf;
        let mut lw = tw;
        let mut lh = th;
        for mip in 0..mip_count {
            queue.write_texture(
                iced::wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: iced::wgpu::Origin3d::ZERO,
                    aspect: iced::wgpu::TextureAspect::All,
                },
                &level,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(lw),
                    rows_per_image: Some(lh),
                },
                iced::wgpu::Extent3d {
                    width: lw,
                    height: lh,
                    depth_or_array_layers: 1,
                },
            );
            if mip + 1 < mip_count {
                let (next, nw, nh) = downsample_r8(&level, lw, lh);
                level = next;
                lw = nw;
                lh = nh;
            }
        }
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
