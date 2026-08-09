//! Separable resampling on the GPU, matching `cpu::resample`.
//!
//! Two passes per resize, horizontal then vertical, with an intermediate
//! texture between them. That is not an optimization detail: a single 2D gather
//! over the same radius is a different filter and would not match the CPU.
//!
//! The sampler is nearest even for Lanczos. Tap positions are computed in the
//! shader and must land on exact texel centers; a linear sampler would blend
//! each tap with its neighbor before weighting, which is a second filter
//! applied underneath the first.

use bytemuck::{Pod, Zeroable};
use iced::wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer, BufferBindingType,
    CommandEncoder, Device, PrimitiveTopology, Queue, RenderPipeline, Sampler, SamplerBindingType,
    ShaderStages, TextureFormat, TextureView,
};

use crate::modifiers::kinds::ResizeFilter;
use crate::wgpu::gpu;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ResampleUniforms {
    axis: [f32; 4],
    region: [f32; 4],
}

/// Which part of the image a single resample invocation is responsible for.
///
/// A resample is defined by the whole image's lengths -- that ratio is the
/// resize the user asked for. But the executor renders one band or tile at a
/// time, so the pass also has to know where the target sits and which slice of
/// the source is actually bound. Without that the region's first row is treated
/// as the image's first row, and every region except the one at the origin
/// resamples the wrong part of the source.
#[derive(Clone, Copy)]
pub struct ResampleRegion {
    /// First output index the render target covers, in whole-image coordinates.
    pub out_base: u32,
    /// Length of the render target on the resampled axis.
    pub dst_len: u32,
    /// First source index the bound input texture holds.
    pub src_base: u32,
    /// Length of the bound input texture on the resampled axis.
    pub tex_len: u32,
}

impl ResampleRegion {
    /// The whole image: the target is the full output and the bound texture is
    /// the full source. This is the identity case, and the only one where
    /// region-agnostic arithmetic happens to be correct.
    ///
    /// The executor always renders a region, so this is used by the tests that
    /// resample a whole image -- `allow` rather than `expect` because whether
    /// it is reached depends on the target configuration.
    #[allow(dead_code)]
    pub fn full(out_len: u32, src_len: u32) -> Self {
        Self {
            out_base: 0,
            dst_len: out_len,
            src_base: 0,
            tex_len: src_len,
        }
    }
}

fn filter_code(f: ResizeFilter) -> f32 {
    match f {
        ResizeFilter::Nearest => 0.0,
        ResizeFilter::Bilinear => 1.0,
        ResizeFilter::Lanczos => 2.0,
    }
}

pub struct ResamplePass {
    pipeline: RenderPipeline,
    bgl: BindGroupLayout,
    sampler: Sampler,
}

impl ResamplePass {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("resample-bgl"),
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
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: iced::wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: iced::wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline = gpu::fullscreen_pipeline(
            device,
            include_str!("../shaders/resample.wgsl"),
            Some("resample-pipeline"),
            PrimitiveTopology::TriangleStrip,
            format,
            BlendState::REPLACE,
            &bgl,
        );
        let sampler = device.create_sampler(&iced::wgpu::SamplerDescriptor {
            label: Some("resample-sampler"),
            address_mode_u: iced::wgpu::AddressMode::ClampToEdge,
            address_mode_v: iced::wgpu::AddressMode::ClampToEdge,
            mag_filter: iced::wgpu::FilterMode::Nearest,
            min_filter: iced::wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            pipeline,
            bgl,
            sampler,
        }
    }

    /// Resample one axis into `target`.
    ///
    /// `out_len` and `src_len` are the **whole image's** lengths along the axis
    /// being resampled -- their ratio is the resize itself, and it must not
    /// change when only part of the image is being rendered. `region` says
    /// which part that is. The other axis passes through unchanged, so the
    /// caller sizes `target` accordingly.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        uniform_buffer: &Buffer,
        input: &TextureView,
        target: &TextureView,
        out_len: u32,
        src_len: u32,
        vertical: bool,
        filter: ResizeFilter,
        region: ResampleRegion,
    ) {
        gpu::write_uniform(
            queue,
            uniform_buffer,
            &ResampleUniforms {
                axis: [
                    out_len as f32,
                    src_len as f32,
                    if vertical { 1.0 } else { 0.0 },
                    filter_code(filter),
                ],
                region: [
                    region.out_base as f32,
                    region.src_base as f32,
                    region.tex_len.max(1) as f32,
                    region.dst_len.max(1) as f32,
                ],
            },
        );
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("resample-bg"),
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
            ],
        });
        let mut pass = encoder.begin_render_pass(&iced::wgpu::RenderPassDescriptor {
            label: Some("resample-pass"),
            color_attachments: &[Some(iced::wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: iced::wgpu::Operations {
                    load: iced::wgpu::LoadOp::Clear(iced::wgpu::Color::TRANSPARENT),
                    store: iced::wgpu::StoreOp::Store,
                },
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
        gpu::uniform_buffer::<ResampleUniforms>(device, Some("resample-uniform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifiers::cpu;
    use crate::wgpu::test_device::{GPU_LOCK, try_device};

    const SRC_W: u32 = 64;
    const SRC_H: u32 = 48;

    /// A source with hard edges and a full value sweep.
    ///
    /// Smooth gradients hide filter differences, since every reasonable kernel
    /// agrees on a linear ramp. The checker and the borders are where Nearest,
    /// Bilinear, and Lanczos visibly disagree, and where edge normalization
    /// goes wrong if the per-pixel weight sum is not honored.
    fn source_pixels() -> Vec<u8> {
        let mut v = vec![0u8; (SRC_W * SRC_H * 4) as usize];
        for y in 0..SRC_H {
            for x in 0..SRC_W {
                let o = ((y * SRC_W + x) * 4) as usize;
                let checker = ((x / 4) + (y / 4)) % 2 == 0;
                v[o] = if checker { 255 } else { 0 };
                v[o + 1] = (x * 255 / SRC_W.max(1)) as u8;
                v[o + 2] = (y * 255 / SRC_H.max(1)) as u8;
                v[o + 3] = 255;
            }
        }
        v
    }

    fn texture_from(
        device: &Device,
        queue: &Queue,
        px: &[u8],
        w: u32,
        h: u32,
    ) -> iced::wgpu::Texture {
        let tex = gpu::texture_2d(
            device,
            w,
            h,
            TextureFormat::Rgba8Unorm,
            iced::wgpu::TextureUsages::TEXTURE_BINDING
                | iced::wgpu::TextureUsages::COPY_DST
                | iced::wgpu::TextureUsages::RENDER_ATTACHMENT
                | iced::wgpu::TextureUsages::COPY_SRC,
            Some("resample-test-src"),
        );
        queue.write_texture(
            iced::wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: iced::wgpu::Origin3d::ZERO,
                aspect: iced::wgpu::TextureAspect::All,
            },
            px,
            iced::wgpu::TexelCopyBufferLayout {
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
        tex
    }

    fn readback(
        device: &Device,
        queue: &Queue,
        tex: &iced::wgpu::Texture,
        w: u32,
        h: u32,
    ) -> Vec<u8> {
        let row_bytes = (w * 4).div_ceil(256) * 256;
        let buf = gpu::readback_buffer(device, row_bytes as u64 * h as u64, Some("resample-read"));
        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-read"),
        });
        enc.copy_texture_to_buffer(
            iced::wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: iced::wgpu::Origin3d::ZERO,
                aspect: iced::wgpu::TextureAspect::All,
            },
            iced::wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: iced::wgpu::TexelCopyBufferLayout {
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
        queue.submit([enc.finish()]);
        let raw = gpu::read_buffer_blocking(device, &buf);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row_bytes) as usize;
            out.extend_from_slice(&raw[s..s + (w * 4) as usize]);
        }
        out
    }

    /// Run both axes on the GPU and return the result at `dst_w` x `dst_h`.
    fn gpu_resample(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        dst_w: u32,
        dst_h: u32,
        filter: ResizeFilter,
    ) -> Vec<u8> {
        let pass = ResamplePass::new(device, TextureFormat::Rgba8Unorm);
        let src_tex = texture_from(device, queue, src, SRC_W, SRC_H);
        let src_view = src_tex.create_view(&Default::default());

        let usage = iced::wgpu::TextureUsages::TEXTURE_BINDING
            | iced::wgpu::TextureUsages::RENDER_ATTACHMENT
            | iced::wgpu::TextureUsages::COPY_SRC;
        let mid = gpu::texture_2d(
            device,
            dst_w,
            SRC_H,
            TextureFormat::Rgba8Unorm,
            usage,
            Some("resample-mid"),
        );
        let mid_view = mid.create_view(&Default::default());
        let dst = gpu::texture_2d(
            device,
            dst_w,
            dst_h,
            TextureFormat::Rgba8Unorm,
            usage,
            Some("resample-dst"),
        );
        let dst_view = dst.create_view(&Default::default());

        // Separate uniform buffers per axis: one submission per pass, and the
        // horizontal write must not be overwritten before its pass runs.
        let ub_h = pass.uniform_buffer(device);
        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-test-h"),
        });
        pass.record(
            device,
            queue,
            &mut enc,
            &ub_h,
            &src_view,
            &mid_view,
            dst_w,
            SRC_W,
            false,
            filter,
            ResampleRegion::full(dst_w, SRC_W),
        );
        queue.submit([enc.finish()]);

        let ub_v = pass.uniform_buffer(device);
        let mut enc2 = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-test-v"),
        });
        pass.record(
            device,
            queue,
            &mut enc2,
            &ub_v,
            &mid_view,
            &dst_view,
            dst_h,
            SRC_H,
            true,
            filter,
            ResampleRegion::full(dst_h, SRC_H),
        );
        queue.submit([enc2.finish()]);

        readback(device, queue, &dst, dst_w, dst_h)
    }

    /// Resample output rows `band_y0..band_y1` the way the executor does.
    ///
    /// The horizontal pass runs over the source rows the band needs; the
    /// vertical pass then produces just the band. Both are given the region
    /// they operate on, which is exactly the executor's situation.
    #[allow(clippy::too_many_arguments)]
    fn gpu_resample_band(
        device: &Device,
        queue: &Queue,
        src: &[u8],
        dst_w: u32,
        dst_h: u32,
        band_y0: u32,
        band_y1: u32,
        filter: ResizeFilter,
    ) -> Vec<u8> {
        let pass = ResamplePass::new(device, TextureFormat::Rgba8Unorm);
        let src_tex = texture_from(device, queue, src, SRC_W, SRC_H);
        let src_view = src_tex.create_view(&Default::default());

        // Source rows this band reads, with a Lanczos-3 apron, mirroring the
        // executor's backward walk through `input_needed`.
        let sy = SRC_H as f32 / dst_h as f32;
        let src_y0 = (((band_y0 as f32) * sy).floor() - 3.0).max(0.0) as u32;
        let src_y1 = ((((band_y1 as f32) * sy).ceil() + 3.0) as u32).min(SRC_H);
        let src_rows = src_y1 - src_y0;

        let usage = iced::wgpu::TextureUsages::TEXTURE_BINDING
            | iced::wgpu::TextureUsages::RENDER_ATTACHMENT
            | iced::wgpu::TextureUsages::COPY_SRC
            | iced::wgpu::TextureUsages::COPY_DST;

        // The band's slice of the source, as its own texture: the executor's
        // `prev` stage holds only the region the band needs.
        let sub = gpu::texture_2d(
            device,
            SRC_W,
            src_rows,
            TextureFormat::Rgba8Unorm,
            usage,
            Some("resample-band-src"),
        );
        let mut enc0 = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-band-copy"),
        });
        enc0.copy_texture_to_texture(
            iced::wgpu::TexelCopyTextureInfo {
                texture: &src_tex,
                mip_level: 0,
                origin: iced::wgpu::Origin3d {
                    x: 0,
                    y: src_y0,
                    z: 0,
                },
                aspect: iced::wgpu::TextureAspect::All,
            },
            iced::wgpu::TexelCopyTextureInfo {
                texture: &sub,
                mip_level: 0,
                origin: iced::wgpu::Origin3d::ZERO,
                aspect: iced::wgpu::TextureAspect::All,
            },
            iced::wgpu::Extent3d {
                width: SRC_W,
                height: src_rows,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc0.finish()]);
        let _ = &src_view;
        let sub_view = sub.create_view(&Default::default());

        let mid = gpu::texture_2d(
            device,
            dst_w,
            src_rows,
            TextureFormat::Rgba8Unorm,
            usage,
            Some("resample-band-mid"),
        );
        let mid_view = mid.create_view(&Default::default());
        let band_h = band_y1 - band_y0;
        let dst = gpu::texture_2d(
            device,
            dst_w,
            band_h,
            TextureFormat::Rgba8Unorm,
            usage,
            Some("resample-band-dst"),
        );
        let dst_view = dst.create_view(&Default::default());

        // Horizontal: the full width, so this axis is the whole image and needs
        // no offset. Only the vertical axis is banded here.
        let ub_h = pass.uniform_buffer(device);
        let mut enc = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-band-h"),
        });
        pass.record(
            device,
            queue,
            &mut enc,
            &ub_h,
            &sub_view,
            &mid_view,
            dst_w,
            SRC_W,
            false,
            filter,
            ResampleRegion::full(dst_w, SRC_W),
        );
        queue.submit([enc.finish()]);

        let ub_v = pass.uniform_buffer(device);
        let mut enc2 = device.create_command_encoder(&iced::wgpu::CommandEncoderDescriptor {
            label: Some("resample-band-v"),
        });
        // The vertical axis is the banded one: the target covers output rows
        // `band_y0..band_y1` of a `dst_h`-tall image, and the bound texture
        // holds source rows `src_y0..src_y1` of a `SRC_H`-tall image.
        pass.record(
            device,
            queue,
            &mut enc2,
            &ub_v,
            &mid_view,
            &dst_view,
            dst_h,
            SRC_H,
            true,
            filter,
            ResampleRegion {
                out_base: band_y0,
                dst_len: band_h,
                src_base: src_y0,
                tex_len: src_rows,
            },
        );
        queue.submit([enc2.finish()]);

        readback(device, queue, &dst, dst_w, band_h)
    }

    fn max_diff(a: &[u8], b: &[u8]) -> u8 {
        a.iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0)
    }

    fn check(filter: ResizeFilter, dst_w: u32, dst_h: u32, tol: u8, label: &str) {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            return;
        };
        let src = source_pixels();
        let got = gpu_resample(&device, &queue, &src, dst_w, dst_h, filter);
        let want = cpu::resample(&src, SRC_W, SRC_H, dst_w, dst_h, filter);
        assert_eq!(got.len(), want.len(), "{label}: size mismatch");
        let d = max_diff(&got, &want);
        assert!(
            d <= tol,
            "{label}: GPU resample differs from cpu::resample by {d} > {tol}. \
             The two must agree or a resize previews differently than it exports."
        );
    }

    #[test]
    fn nearest_downscale_matches_cpu() {
        check(ResizeFilter::Nearest, 32, 24, 0, "nearest/down");
    }

    #[test]
    fn nearest_upscale_matches_cpu() {
        check(ResizeFilter::Nearest, 128, 96, 0, "nearest/up");
    }

    #[test]
    fn bilinear_downscale_matches_cpu() {
        check(ResizeFilter::Bilinear, 32, 24, 2, "bilinear/down");
    }

    #[test]
    fn bilinear_upscale_matches_cpu() {
        check(ResizeFilter::Bilinear, 128, 96, 2, "bilinear/up");
    }

    #[test]
    fn lanczos_downscale_matches_cpu() {
        check(ResizeFilter::Lanczos, 32, 24, 2, "lanczos/down");
    }

    #[test]
    fn lanczos_upscale_matches_cpu() {
        check(ResizeFilter::Lanczos, 128, 96, 2, "lanczos/up");
    }

    /// A large reduction is where the `inv` widening matters. Without it the
    /// kernel stays narrow, undersamples, and aliases.
    #[test]
    fn lanczos_large_reduction_matches_cpu() {
        check(ResizeFilter::Lanczos, 8, 6, 2, "lanczos/8x");
    }

    /// Non-uniform scaling, so a bug that only shows when the two axes differ
    /// cannot hide behind a square resize.
    #[test]
    fn lanczos_anisotropic_matches_cpu() {
        check(ResizeFilter::Lanczos, 96, 12, 2, "lanczos/aniso");
    }

    /// Resampling a band must agree with the same rows of a whole-image resize.
    ///
    /// Every test above resamples the entire image, so `out_len`/`src_len` are
    /// the image's own lengths and output index 0 is image row 0. The executor
    /// never calls it that way once a document needs banding: it hands the pass
    /// one band at a time. If the pass cannot express where that band sits, it
    /// resamples every band as though it were the whole image, and only the
    /// first band -- the one that really does start at zero -- comes out right.
    #[test]
    fn a_band_matches_the_same_rows_of_a_full_resize() {
        let _serialize = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some((device, queue)) = try_device() else {
            return;
        };
        let src = source_pixels();
        let (dst_w, dst_h) = (SRC_W * 2, SRC_H * 2);
        let want = cpu::resample(&src, SRC_W, SRC_H, dst_w, dst_h, ResizeFilter::Lanczos);

        // A band in the lower half of the output, chosen so it starts well away
        // from the origin and the taps it needs are interior rather than
        // clamped -- a band at the edge could pass on the edge case alone.
        let (band_y0, band_y1) = (dst_h / 2, dst_h / 2 + 16);
        let got = gpu_resample_band(
            &device,
            &queue,
            &src,
            dst_w,
            dst_h,
            band_y0,
            band_y1,
            ResizeFilter::Lanczos,
        );

        let stride = (dst_w * 4) as usize;
        let mut worst = (0u8, 0usize);
        for row in 0..(band_y1 - band_y0) as usize {
            let g = &got[row * stride..row * stride + stride];
            let w = &want[(band_y0 as usize + row) * stride..][..stride];
            let d = g
                .iter()
                .zip(w)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            if d > worst.0 {
                worst = (d, band_y0 as usize + row);
            }
        }
        assert!(
            worst.0 <= 2,
            "output row {} of the band differs from the full resize by {} levels; \
             a band must resample to the same pixels as the rows it stands for",
            worst.1,
            worst.0
        );
    }
}
