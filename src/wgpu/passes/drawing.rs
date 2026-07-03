use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer, BufferBindingType,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, SamplerBindingType,
    ShaderStages, StoreOp, TexelCopyBufferLayout, Texture, TextureFormat, TextureSampleType,
    TextureUsages, TextureView, TextureViewDimension,
};

use crate::modifiers::drawing_raster::DrawingLayerCache;
use crate::modifiers::gpu::UvRect;
use crate::modifiers::kinds::Drawing;
use crate::wgpu::gpu;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct DrawingUniforms {
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
}

pub struct DrawingLayer {
    cache: DrawingLayerCache,
    texture: Texture,
    pub view: TextureView,
}

impl DrawingLayer {
    pub fn new(device: &Device, full_w: u32, full_h: u32) -> Self {
        let cache = DrawingLayerCache::new(full_w, full_h);
        let texture = gpu::texture_2d(
            device,
            cache.w,
            cache.h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("drawing-layer"),
        );
        let view = texture.create_view(&Default::default());
        Self {
            cache,
            texture,
            view,
        }
    }

    pub fn matches(&self, full_w: u32, full_h: u32) -> bool {
        let (w, h) = crate::modifiers::drawing_raster::layer_dims(full_w, full_h);
        self.cache.w == w && self.cache.h == h
    }

    pub fn sync(&mut self, queue: &Queue, d: &Drawing, full_w: u32, full_h: u32) -> Option<[f32; 4]> {
        let [x0, y0, x1, y1] = self.cache.sync(d)?;
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let w = self.cache.w;
        let offset = ((y0 * w + x0) * 4) as usize;
        let len = (((y1 - 1 - y0) * w + (x1 - x0)) * 4) as usize;
        queue.write_texture(
            iced::wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: iced::wgpu::Origin3d { x: x0, y: y0, z: 0 },
                aspect: iced::wgpu::TextureAspect::All,
            },
            &self.cache.layer_data()[offset..offset + len],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(y1 - y0),
            },
            iced::wgpu::Extent3d {
                width: x1 - x0,
                height: y1 - y0,
                depth_or_array_layers: 1,
            },
        );
        let ix = full_w as f32 / self.cache.w as f32;
        let iy = full_h as f32 / self.cache.h as f32;
        Some([
            x0 as f32 * ix,
            y0 as f32 * iy,
            x1 as f32 * ix,
            y1 as f32 * iy,
        ])
    }
}

pub struct DrawingPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl DrawingPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let texture_entry = |binding: u32| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("drawing-bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/drawing.wgsl"),
            Some("drawing-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("drawing-sampler"),
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
        layer: &DrawingLayer,
        proc: UvRect,
        src: UvRect,
        input: &TextureView,
        output: &TextureView,
    ) {
        let uniforms = DrawingUniforms {
            proc_origin: proc.origin,
            proc_size: proc.size,
            src_origin: src.origin,
            src_size: src.size,
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);
        let bg: BindGroup = device.create_bind_group(&BindGroupDescriptor {
            label: Some("drawing-bg"),
            layout: &self.bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(input),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&layer.view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("drawing-pass"),
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
        gpu::uniform_buffer::<DrawingUniforms>(device, Some("drawing-uniform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::kinds::Stroke;
    use crate::modifiers::{Modifier, ModifierKind, cpu, drawing_raster};
    use iced::wgpu::{
        CommandEncoderDescriptor, DeviceDescriptor, Instance, PowerPreference,
        RequestAdapterOptions,
    };

    fn try_device() -> Option<(Device, Queue)> {
        let instance = Instance::default();
        let adapter =
            futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .ok()?;
        futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).ok()
    }

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                px[o] = (x * 255 / w.max(1)) as u8;
                px[o + 1] = (y * 255 / h.max(1)) as u8;
                px[o + 2] = 90;
                px[o + 3] = 255;
            }
        }
        px
    }

    #[test]
    fn gpu_drawing_matches_cpu_reference() {
        let Some((device, queue)) = try_device() else {
            return;
        };
        let (w, h) = (96u32, 64u32);
        let base = gradient(w, h);

        let drawing = crate::modifiers::kinds::Drawing {
            strokes: vec![
                Stroke {
                    points: vec![[0.15, 0.3], [0.85, 0.6]],
                    size: 10.0,
                    hardness: 0.6,
                    opacity: 0.7,
                    color: [1.0, 0.1, 0.2],
                },
                Stroke {
                    points: vec![[0.5, 0.1], [0.5, 0.9]],
                    size: 6.0,
                    hardness: 1.0,
                    opacity: 1.0,
                    color: [0.1, 0.9, 0.3],
                },
            ],
            ..Default::default()
        };
        let modifiers = vec![Modifier::new(ModifierKind::Drawing(drawing.clone()))];

        let input = gpu::texture_2d(
            &device,
            w,
            h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("drawing-test-input"),
        );
        queue.write_texture(
            input.as_image_copy(),
            &base,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            iced::wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let input_view = input.create_view(&Default::default());

        let output = gpu::texture_2d(
            &device,
            w,
            h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            Some("drawing-test-output"),
        );
        let output_view = output.create_view(&Default::default());

        let mut layer = DrawingLayer::new(&device, w, h);
        assert!(layer.sync(&queue, &drawing, w, h).is_some());

        let pass = DrawingPass::new(&device, TextureFormat::Rgba8Unorm);
        let uniform = pass.uniform_buffer(&device);
        let full = UvRect {
            origin: [0.0, 0.0],
            size: [1.0, 1.0],
        };
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        pass.record(
            &device,
            &queue,
            &mut encoder,
            &uniform,
            &layer,
            full,
            full,
            &input_view,
            &output_view,
        );

        let row_bytes = (w * 4).div_ceil(256) * 256;
        let read = gpu::readback_buffer(&device, (row_bytes * h) as u64, Some("drawing-test-read"));
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            iced::wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(h),
                },
            },
            iced::wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let raw = gpu::read_buffer_blocking(&device, &read);

        let layers = drawing_raster::build_layers(&modifiers, w, h);
        let views: Vec<_> = layers
            .iter()
            .map(|l| l.as_ref().map(|r| r.view()))
            .collect();
        let expected = cpu::render_full(&modifiers, &[], &views, &base, w, h);

        let mut max_diff = 0i32;
        for y in 0..h {
            for x in 0..w {
                let g = (y * row_bytes + x * 4) as usize;
                let c = ((y * w + x) * 4) as usize;
                for k in 0..4 {
                    max_diff = max_diff.max((raw[g + k] as i32 - expected[c + k] as i32).abs());
                }
            }
        }
        assert!(max_diff <= 2, "GPU vs CPU drawing diff {max_diff} > 2");
    }
}
