use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer, BufferBindingType,
    CommandEncoder, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, SamplerBindingType,
    ShaderStages, StoreOp, TextureFormat, TextureSampleType, TextureView, TextureViewDimension,
};

use crate::wgpu::gpu;

#[derive(Copy, Clone)]
pub struct TileRect {
    pub origin: [f32; 2],
    pub size: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct BlurUniforms {
    direction: [f32; 2],
    radius: f32,
    sigma: f32,
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
    lo_origin: [f32; 2],
    lo_size: [f32; 2],
    hi_origin: [f32; 2],
    hi_size: [f32; 2],
    has_lo: f32,
    has_hi: f32,
    src_lod: f32,
    _pad: f32,
}

pub struct GaussianBlurPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl GaussianBlurPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("gaussian-blur-bgl"),
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
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
            ],
        });
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/gaussian_blur.wgsl"),
            Some("gaussian-blur-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("gaussian-blur-sampler"),
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

    fn bind_group(
        &self,
        device: &Device,
        uniform_buffer: &Buffer,
        input: &TextureView,
        lo: &TextureView,
        hi: &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("gaussian-blur-bg"),
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
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(lo),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(hi),
                },
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform_buffer: &Buffer,
        direction: [f32; 2],
        radius: f32,
        sigma: f32,
        tile: TileRect,
        src: TileRect,
        lo: Option<(&TextureView, TileRect)>,
        hi: Option<(&TextureView, TileRect)>,
        input: &TextureView,
        output: &TextureView,
        band: Option<[u32; 4]>,
        src_lod: f32,
    ) {
        let (lo_view, lo_rect) = lo.unzip();
        let (hi_view, hi_rect) = hi.unzip();
        let lo_rect = lo_rect.unwrap_or(tile);
        let hi_rect = hi_rect.unwrap_or(tile);

        let uniforms = BlurUniforms {
            direction,
            radius,
            sigma,
            proc_origin: tile.origin,
            proc_size: tile.size,
            src_origin: src.origin,
            src_size: src.size,
            lo_origin: lo_rect.origin,
            lo_size: lo_rect.size,
            hi_origin: hi_rect.origin,
            hi_size: hi_rect.size,
            has_lo: if lo_view.is_some() { 1.0 } else { 0.0 },
            has_hi: if hi_view.is_some() { 1.0 } else { 0.0 },
            src_lod,
            _pad: 0.0,
        };
        gpu::write_uniform(queue, uniform_buffer, &uniforms);

        let bg = self.bind_group(
            device,
            uniform_buffer,
            input,
            lo_view.unwrap_or(input),
            hi_view.unwrap_or(input),
        );

        let load = match band {
            Some(_) => LoadOp::Load,
            None => LoadOp::Clear(iced::wgpu::Color::TRANSPARENT),
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("gaussian-blur-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: Operations {
                    load,
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
        if let Some([x, y, w, h]) = band {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<BlurUniforms>(device, Some("gaussian-blur-uniform"))
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wgpu::gpu;
    use iced::wgpu::{
        CommandEncoderDescriptor, DeviceDescriptor, Extent3d, Instance, PowerPreference,
        RequestAdapterOptions, TexelCopyBufferInfo, TexelCopyBufferLayout, TextureUsages,
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

    fn rt(device: &Device, w: u32, h: u32, label: &str) -> iced::wgpu::Texture {
        gpu::texture_2d(
            device,
            w,
            h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            Some(label),
        )
    }

    #[test]
    fn banded_blur_matches_single_pass() {
        let Some((device, queue)) = try_device() else {
            eprintln!("banded_blur_matches_single_pass: no GPU adapter, skipping");
            return;
        };
        let (w, h) = (64u32, 96u32);
        let radius = 7.0f32;
        let sigma = (radius / 3.0).max(0.5);

        let src_tex = gpu::texture_2d(
            &device,
            w,
            h,
            TextureFormat::Rgba8Unorm,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            Some("src"),
        );
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut s: u32 = 0xDEADBEEF;
        for b in px.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (s >> 24) as u8;
        }
        queue.write_texture(
            src_tex.as_image_copy(),
            &px,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let src_view = src_tex.create_view(&Default::default());

        let pass = GaussianBlurPass::new(&device, TextureFormat::Rgba8Unorm);
        let whole = TileRect {
            origin: [0.0, 0.0],
            size: [1.0, 1.0],
        };
        let dir_h = [1.0 / w as f32, 0.0];
        let dir_v = [0.0, 1.0 / h as f32];

        let hmid_a = rt(&device, w, h, "hmid_a");
        let out_a = rt(&device, w, h, "out_a");
        let (hv, vv) = (
            hmid_a.create_view(&Default::default()),
            out_a.create_view(&Default::default()),
        );
        let ub_h = pass.uniform_buffer(&device);
        let ub_v = pass.uniform_buffer(&device);
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        pass.record(
            &device, &queue, &mut enc, &ub_h, dir_h, radius, sigma, whole, whole, None, None,
            &src_view, &hv, None, 0.0,
        );
        pass.record(
            &device, &queue, &mut enc, &ub_v, dir_v, radius, sigma, whole, whole, None, None, &hv,
            &vv, None, 0.0,
        );
        queue.submit([enc.finish()]);

        let hmid_b = rt(&device, w, h, "hmid_b");
        let out_b = rt(&device, w, h, "out_b");
        let (hvb, vvb) = (
            hmid_b.create_view(&Default::default()),
            out_b.create_view(&Default::default()),
        );
        let apron = radius.ceil() as u32;
        let band_h = 24u32;
        let mut by = 0u32;
        while by < h {
            let by1 = (by + band_h).min(h);
            let h0 = by.saturating_sub(apron);
            let h1 = (by1 + apron).min(h);
            let ubh = pass.uniform_buffer(&device);
            let ubv = pass.uniform_buffer(&device);
            let mut e = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
            pass.record(
                &device,
                &queue,
                &mut e,
                &ubh,
                dir_h,
                radius,
                sigma,
                whole,
                whole,
                None,
                None,
                &src_view,
                &hvb,
                Some([0, h0, w, h1 - h0]),
                0.0,
            );
            pass.record(
                &device,
                &queue,
                &mut e,
                &ubv,
                dir_v,
                radius,
                sigma,
                whole,
                whole,
                None,
                None,
                &hvb,
                &vvb,
                Some([0, by, w, by1 - by]),
                0.0,
            );
            queue.submit([e.finish()]);
            by = by1;
        }

        let read = |tex: &iced::wgpu::Texture| -> Vec<u8> {
            let padded = (w * 4).div_ceil(256) * 256;
            let buf = gpu::readback_buffer(&device, (padded * h) as u64, Some("rb"));
            let mut e = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
            e.copy_texture_to_buffer(
                tex.as_image_copy(),
                TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(h),
                    },
                },
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([e.finish()]);
            let raw = gpu::read_buffer_blocking(&device, &buf);
            let mut out = Vec::with_capacity((w * h * 4) as usize);
            for row in 0..h {
                let o = (row * padded) as usize;
                out.extend_from_slice(&raw[o..o + (w * 4) as usize]);
            }
            out
        };

        let a = read(&out_a);
        let b = read(&out_b);
        let mismatches = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert_eq!(
            mismatches, 0,
            "banded blur != single-pass in {mismatches} bytes"
        );
    }
}
