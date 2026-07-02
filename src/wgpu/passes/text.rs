use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

use iced::wgpu::{
    BindGroupLayout, BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites,
    CommandEncoder, Device, FragmentState, LoadOp, MultisampleState, Operations,
    PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TexelCopyBufferLayout,
    Texture, TextureFormat, TextureUsages, TextureView, VertexAttribute, VertexBufferLayout,
    VertexState, VertexStepMode,
};
use std::borrow::Cow;

use crate::{
    modifiers::{
        gpu::{TileInfo, UvRect},
        kinds::Text,
        text_render,
    },
    wgpu::gpu,
};

const SDF_TILE: u32 = text_render::SDF_TILE;
const ATLAS_START: u32 = 2048;
const PX_RANGE: f32 = text_render::SDF_PX_RANGE;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct GlyphInstance {
    rect: [f32; 4],
    uv: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TextUniforms {
    anchor: [f32; 2],
    block_size: [f32; 2],
    pivot: [f32; 2],
    tile_origin: [f32; 2],
    tile_size: [f32; 2],
    block_min: [f32; 2],
    rotation: f32,
    opacity: f32,
    px_range: f32,
    _pad0: f32,
    color: [f32; 4],
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
}

pub struct TextLayer {
    instances: Buffer,
    instance_count: u32,
    block_min: [f32; 2],
    block_w: f32,
    block_h: f32,
    x: f32,
    y: f32,
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

struct Atlas {
    _texture: Texture,
    view: TextureView,
    side: u32,
    cols: u32,
    next: u32,
    residency: HashMap<text_render::SdfKey, [f32; 4]>,
}

impl Atlas {
    fn new(device: &Device, side: u32) -> Self {
        let texture = gpu::texture_2d(
            device,
            side,
            side,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("text-msdf-atlas"),
        );
        let view = texture.create_view(&Default::default());
        Self {
            _texture: texture,
            view,
            side,
            cols: side / SDF_TILE,
            next: 0,
            residency: HashMap::new(),
        }
    }

    fn capacity(&self) -> u32 {
        self.cols * self.cols
    }

    fn ensure(
        &mut self,
        queue: &Queue,
        key: text_render::SdfKey,
        sdf: &text_render::GlyphSdf,
    ) -> Option<[f32; 4]> {
        if let Some(uv) = self.residency.get(&key) {
            return Some(*uv);
        }
        if self.next >= self.capacity() || sdf.width > SDF_TILE || sdf.height > SDF_TILE {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        let cx = (slot % self.cols) * SDF_TILE;
        let cy = (slot / self.cols) * SDF_TILE;

        let mut rgba = vec![0u8; (sdf.width * sdf.height * 4) as usize];
        for (i, px) in sdf.data.chunks_exact(3).enumerate() {
            rgba[i * 4] = px[0];
            rgba[i * 4 + 1] = px[1];
            rgba[i * 4 + 2] = px[2];
            rgba[i * 4 + 3] = 255;
        }

        queue.write_texture(
            iced::wgpu::TexelCopyTextureInfo {
                texture: &self._texture,
                mip_level: 0,
                origin: iced::wgpu::Origin3d { x: cx, y: cy, z: 0 },
                aspect: iced::wgpu::TextureAspect::All,
            },
            &rgba,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(sdf.width * 4),
                rows_per_image: Some(sdf.height),
            },
            iced::wgpu::Extent3d {
                width: sdf.width,
                height: sdf.height,
                depth_or_array_layers: 1,
            },
        );

        let s = self.side as f32;
        let uv = [
            cx as f32 / s,
            cy as f32 / s,
            sdf.width as f32 / s,
            sdf.height as f32 / s,
        ];
        self.residency.insert(key, uv);
        Some(uv)
    }
}

pub struct TextPass {
    pipeline: RenderPipeline,
    copy_pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
    atlas: Atlas,
}

impl TextPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::VERTEX_FRAGMENT,
            Some("text-sdf-bgl"),
        );

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("text-sdf-shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/text_sdf.wgsl"))),
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("text-sdf-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let instance_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphInstance>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                VertexAttribute {
                    format: iced::wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttribute {
                    format: iced::wgpu::VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 1,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("text-sdf-pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[instance_layout],
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
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            cache: None,
            multiview: None,
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
            label: Some("text-sdf-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            mipmap_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let atlas = Atlas::new(device, ATLAS_START);

        Self {
            pipeline,
            copy_pipeline,
            bgl,
            sampler,
            atlas,
        }
    }

    pub fn build_layer(
        &mut self,
        device: &Device,
        queue: &Queue,
        text: &Text,
    ) -> Option<TextLayer> {
        if text.content.is_empty() || text.opacity <= 0.0 {
            return None;
        }

        let shaped = text_render::shape_glyphs(text);
        if shaped.is_empty() {
            return None;
        }
        let (block_w, block_h) = shaped.bbox();

        let mut instances: Vec<GlyphInstance> = Vec::with_capacity(shaped.glyphs.len());
        for g in &shaped.glyphs {
            let Some(uv) = self.atlas.ensure(queue, g.key, &g.sdf) else {
                continue;
            };
            instances.push(GlyphInstance {
                rect: [g.x, g.y, g.w, g.h],
                uv,
            });
        }
        if instances.is_empty() {
            return None;
        }

        let instances_buf = device.create_buffer(&iced::wgpu::BufferDescriptor {
            label: Some("text-sdf-instances"),
            size: (instances.len() * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&instances_buf, 0, bytemuck::cast_slice(&instances));

        Some(TextLayer {
            instances: instances_buf,
            instance_count: instances.len() as u32,
            block_min: [shaped.min_x, shaped.min_y],
            block_w,
            block_h,
            x: text.x,
            y: text.y,
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
        proc: UvRect,
        src: UvRect,
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

        let full_w = tile.full_w as f32;
        let full_h = tile.full_h as f32;
        let uniforms = TextUniforms {
            anchor: [layer.x * full_w, layer.y * full_h],
            block_size: [layer.block_w, layer.block_h],
            pivot: [0.5, 0.5],
            tile_origin: [proc.origin[0] * full_w, proc.origin[1] * full_h],
            tile_size: [proc.size[0] * full_w, proc.size[1] * full_h],
            block_min: layer.block_min,
            rotation: layer.rotation.to_radians(),
            opacity: layer.opacity,
            px_range: PX_RANGE,
            _pad0: 0.0,
            color: [layer.color[0], layer.color[1], layer.color[2], 1.0],
            proc_origin: proc.origin,
            proc_size: proc.size,
            src_origin: src.origin,
            src_size: src.size,
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);

        let bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            uniform_buffer,
            &self.atlas.view,
            &self.sampler,
            Some("text-sdf-bg"),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("text-sdf-pass"),
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
        pass.set_vertex_buffer(0, layer.instances.slice(..));
        pass.draw(0..4, 0..layer.instance_count);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<TextUniforms>(device, Some("text-sdf-uniform"))
    }
}
