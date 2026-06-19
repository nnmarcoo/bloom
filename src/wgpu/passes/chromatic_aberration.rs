use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroupLayout, BlendState, Buffer, CommandEncoder, Device, LoadOp, Operations,
    PrimitiveTopology, Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    Sampler, ShaderStages, StoreOp, TextureFormat, TextureView,
};

use crate::{modifiers::gpu::TileInfo, wgpu::gpu};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CaUniforms {
    amount: f32,
    tile_origin: [f32; 2],
    tile_size: [f32; 2],
    _pad: [f32; 3],
}

pub struct ChromaticAberrationPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl ChromaticAberrationPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::FRAGMENT,
            Some("chromatic-aberration-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/chromatic_aberration.wgsl"),
            Some("chromatic-aberration-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("chromatic-aberration-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Linear,
            min_filter: iced::wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bgl,
            sampler,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform_buffer: &Buffer,
        amount: f32,
        tile: &TileInfo,
        input: &TextureView,
        output: &TextureView,
    ) {
        let uniforms = CaUniforms {
            amount: amount / tile.full_w as f32,
            tile_origin: [
                tile.tile_x as f32 / tile.full_w as f32,
                tile.tile_y as f32 / tile.full_h as f32,
            ],
            tile_size: [
                tile.tile_w as f32 / tile.full_w as f32,
                tile.tile_h as f32 / tile.full_h as f32,
            ],
            _pad: [0.0; 3],
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);
        let bg = gpu::standard_bind_group(
            device,
            &self.bgl,
            uniform_buffer,
            input,
            &self.sampler,
            Some("chromatic-aberration-bg"),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("chromatic-aberration-pass"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..4, 0..1);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<CaUniforms>(device, Some("chromatic-aberration-uniform"))
    }
}
