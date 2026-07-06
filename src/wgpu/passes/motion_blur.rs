use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, CommandEncoder,
    ComputePassDescriptor, ComputePipeline, Device, LoadOp, Operations, PrimitiveTopology, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, ShaderStages,
    StoreOp, TextureFormat, TextureView,
};

use crate::modifiers::motion_blur_samples;
use crate::{modifiers::gpu::UvRect, wgpu::gpu};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MbUniforms {
    dir: [f32; 2],
    samples: f32,
    _pad0: f32,
    proc_origin: [f32; 2],
    proc_size: [f32; 2],
    src_origin: [f32; 2],
    src_size: [f32; 2],
}

pub struct MotionBlurPass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl MotionBlurPass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = gpu::standard_bind_group_layout(
            device,
            ShaderStages::FRAGMENT,
            Some("motion-blur-bgl"),
        );
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/motion_blur.wgsl"),
            Some("motion-blur-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("motion-blur-sampler"),
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
        angle: f32,
        distance: f32,
        full_w: f32,
        full_h: f32,
        proc: UvRect,
        src: UvRect,
        input: &TextureView,
        output: &TextureView,
    ) {
        let rad = angle.to_radians();
        let dist_u = distance / full_w;
        let dist_v = distance / full_h;
        let dir = [rad.cos() * dist_u, rad.sin() * dist_v];
        let uniforms = MbUniforms {
            dir,
            samples: motion_blur_samples(distance) as f32,
            _pad0: 0.0,
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
            input,
            &self.sampler,
            Some("motion-blur-bg"),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("motion-blur-pass"),
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
        gpu::uniform_buffer::<MbUniforms>(device, Some("motion-blur-uniform"))
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MbComputeUniforms {
    width: u32,
    height: u32,
    row_words: u32,
    samples: u32,
    du: f32,
    dv: f32,
    _pad0: f32,
    _pad1: f32,
}

pub struct MotionBlurCompute {
    pipeline: ComputePipeline,
    bgl: BindGroupLayout,
}

impl MotionBlurCompute {
    pub fn new(device: &Device) -> Self {
        let storage_entry = |binding: u32, read_only: bool| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("motion-blur-compute-bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1, true),
                storage_entry(2, false),
            ],
        });
        let pipeline = gpu::compute_pipeline(
            device,
            include_str!("../shaders/motion_blur_compute.wgsl"),
            "main",
            Some("motion-blur-compute-pipeline"),
            &bgl,
        );
        Self { pipeline, bgl }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform: &Buffer,
        src: &Buffer,
        dst: &Buffer,
        width: u32,
        height: u32,
        row_words: u32,
        angle: f32,
        distance: f32,
    ) {
        let rad = angle.to_radians();
        gpu::write_uniform(
            queue,
            uniform,
            &MbComputeUniforms {
                width,
                height,
                row_words,
                samples: crate::modifiers::motion_blur_samples(distance),
                du: rad.cos() * distance,
                dv: rad.sin() * distance,
                _pad0: 0.0,
                _pad1: 0.0,
            },
        );
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("motion-blur-compute-bg"),
            layout: &self.bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: src.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: dst.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("motion-blur-compute-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }

    pub fn uniform_buffer(&self, device: &Device) -> Buffer {
        gpu::uniform_buffer::<MbComputeUniforms>(device, Some("motion-blur-compute-uniform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::motion_blur_samples;
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

    fn pack(rgba: &[u8]) -> Vec<u32> {
        rgba.chunks_exact(4)
            .map(|p| {
                (p[0] as u32) | ((p[1] as u32) << 8) | ((p[2] as u32) << 16) | ((p[3] as u32) << 24)
            })
            .collect()
    }

    fn load(src: &[u8], w: i32, h: i32, x: i32, y: i32) -> [f32; 4] {
        let cx = x.clamp(0, w - 1);
        let cy = y.clamp(0, h - 1);
        let o = ((cy * w + cx) * 4) as usize;
        [
            src[o] as f32 / 255.0,
            src[o + 1] as f32 / 255.0,
            src[o + 2] as f32 / 255.0,
            src[o + 3] as f32 / 255.0,
        ]
    }

    fn bilinear(src: &[u8], w: i32, h: i32, fx: f32, fy: f32) -> [f32; 4] {
        let px = fx - 0.5;
        let py = fy - 0.5;
        let x0 = px.floor() as i32;
        let y0 = py.floor() as i32;
        let tx = px - x0 as f32;
        let ty = py - y0 as f32;
        let c00 = load(src, w, h, x0, y0);
        let c10 = load(src, w, h, x0 + 1, y0);
        let c01 = load(src, w, h, x0, y0 + 1);
        let c11 = load(src, w, h, x0 + 1, y0 + 1);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = c00[i] + (c10[i] - c00[i]) * tx;
            let bot = c01[i] + (c11[i] - c01[i]) * tx;
            out[i] = top + (bot - top) * ty;
        }
        out
    }

    fn cpu_reference(src: &[u8], w: u32, h: u32, angle: f32, distance: f32) -> Vec<u8> {
        let (wi, hi) = (w as i32, h as i32);
        let rad = angle.to_radians();
        let du = rad.cos() * distance;
        let dv = rad.sin() * distance;
        let n = motion_blur_samples(distance) as i32;
        let mut out = vec![0u8; src.len()];
        for y in 0..hi {
            for x in 0..wi {
                let cx = x as f32 + 0.5;
                let cy = y as f32 + 0.5;
                let mut acc = [0.0f32; 4];
                for i in 0..n {
                    let t = i as f32 / (n - 1) as f32 - 0.5;
                    let s = bilinear(src, wi, hi, cx + du * t, cy + dv * t);
                    for c in 0..4 {
                        acc[c] += s[c];
                    }
                }
                let o = ((y * wi + x) * 4) as usize;
                for c in 0..4 {
                    out[o + c] = ((acc[c] / n as f32).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
        out
    }

    fn gpu_motion_blur(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        w: u32,
        h: u32,
        angle: f32,
        distance: f32,
    ) -> Vec<u8> {
        let words = pack(src);
        let bytes = (words.len() * 4) as u64;
        let src_buf = gpu::storage_buffer(device, bytes, Some("mb-src"));
        let dst_buf = gpu::storage_buffer(device, bytes, Some("mb-dst"));
        let read = gpu::readback_buffer(device, bytes, Some("mb-read"));
        queue.write_buffer(&src_buf, 0, bytemuck::cast_slice(&words));

        let pass = MotionBlurCompute::new(device);
        let uniform = pass.uniform_buffer(device);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        pass.record(
            device,
            queue,
            &mut encoder,
            &uniform,
            &src_buf,
            &dst_buf,
            w,
            h,
            w,
            angle,
            distance,
        );
        encoder.copy_buffer_to_buffer(&dst_buf, 0, &read, 0, bytes);
        queue.submit([encoder.finish()]);

        let raw = gpu::read_buffer_blocking(device, &read);
        let out_words: Vec<u32> = bytemuck::cast_slice(&raw).to_vec();
        let mut out = Vec::with_capacity(out_words.len() * 4);
        for word in out_words {
            out.push((word & 0xff) as u8);
            out.push(((word >> 8) & 0xff) as u8);
            out.push(((word >> 16) & 0xff) as u8);
            out.push(((word >> 24) & 0xff) as u8);
        }
        out
    }

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let v = if (x / 3 + y / 3) % 2 == 0 { 230 } else { 20 };
                px[o] = v;
                px[o + 1] = (x * 255 / w.max(1)) as u8;
                px[o + 2] = (y * 255 / h.max(1)) as u8;
                px[o + 3] = 255;
            }
        }
        px
    }

    #[test]
    fn gpu_motion_blur_matches_reference() {
        let Some((device, queue)) = try_device() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let (w, h) = (40u32, 28u32);
        let src = checker(w, h);
        for &(angle, dist) in &[(0.0, 30.0), (90.0, 24.0), (37.0, 18.0), (200.0, 40.0)] {
            let gpu = gpu_motion_blur(&device, &queue, &src, w, h, angle, dist);
            let cpu = cpu_reference(&src, w, h, angle, dist);
            let mut worst = 0i32;
            for (g, c) in gpu.iter().zip(&cpu) {
                worst = worst.max((*g as i32 - *c as i32).abs());
            }
            assert!(
                worst <= 2,
                "angle {angle} dist {dist}: GPU vs reference differ by {worst}"
            );
        }
    }
}
