use std::borrow::Cow;

use bytemuck::{Pod, bytes_of};
use iced::wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer, BufferBindingType,
    BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, CommandEncoder, Device,
    Extent3d, FragmentState, LoadOp, MultisampleState, Operations, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

pub fn fullscreen_pipeline(
    device: &Device,
    shader_src: &str,
    label: Option<&str>,
    topology: PrimitiveTopology,
    format: TextureFormat,
    blend: BlendState,
    bind_group_layout: &BindGroupLayout,
) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label,
        source: ShaderSource::Wgsl(Cow::Borrowed(shader_src)),
    });

    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label,
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label,
        layout: Some(&layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: PipelineCompilationOptions::default(),
        },
        primitive: PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: PipelineCompilationOptions::default(),
        }),
        cache: None,
        multiview: None,
    })
}

pub fn uniform_buffer<T: Sized>(device: &Device, label: Option<&str>) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label,
        size: std::mem::size_of::<T>() as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_uniform<T: Pod>(queue: &Queue, buffer: &Buffer, value: &T) {
    queue.write_buffer(buffer, 0, bytes_of(value));
}

pub fn standard_bind_group_layout(
    device: &Device,
    uniform_visibility: ShaderStages,
    label: Option<&str>,
) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label,
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: uniform_visibility,
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
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
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
    })
}

pub fn standard_bind_group(
    device: &Device,
    layout: &BindGroupLayout,
    uniform_buffer: &Buffer,
    texture_view: &TextureView,
    sampler: &Sampler,
    label: Option<&str>,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label,
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(texture_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(sampler),
            },
        ],
    })
}

pub fn texture_2d(
    device: &Device,
    width: u32,
    height: u32,
    format: TextureFormat,
    usage: TextureUsages,
    label: Option<&str>,
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label,
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

pub fn blit_pipeline(device: &Device, format: TextureFormat) -> (RenderPipeline, BindGroupLayout) {
    let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("blit-bgl"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("blit-shader"),
        source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/blit.wgsl"))),
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("blit-layout"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("blit-pipeline"),
        layout: Some(&layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
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
            entry_point: Some("fs_main"),
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

    (pipeline, bgl)
}

pub fn generate_hw_mipmaps(
    encoder: &mut CommandEncoder,
    device: &Device,
    texture: &Texture,
    mip_level_count: u32,
    format: TextureFormat,
    blit_pipeline: &RenderPipeline,
    blit_bgl: &BindGroupLayout,
) {
    let linear_sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("blit-linear-sampler"),
        mag_filter: iced::wgpu::FilterMode::Linear,
        min_filter: iced::wgpu::FilterMode::Linear,
        ..Default::default()
    });

    for mip in 1..mip_level_count {
        let src_view = texture.create_view(&TextureViewDescriptor {
            label: Some(&format!("blit-src-mip{}", mip - 1)),
            format: Some(format),
            dimension: Some(TextureViewDimension::D2),
            base_mip_level: mip - 1,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let dst_view = texture.create_view(&TextureViewDescriptor {
            label: Some(&format!("blit-dst-mip{mip}")),
            format: Some(format),
            dimension: Some(TextureViewDimension::D2),
            base_mip_level: mip,
            mip_level_count: Some(1),
            ..Default::default()
        });

        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some(&format!("blit-bg-mip{mip}")),
            layout: blit_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&src_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&linear_sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(&format!("blit-pass-mip{mip}")),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &dst_view,
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
        pass.set_pipeline(blit_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..4, 0..1);
    }
}

// Standard formula: floor(log2(max(w,h))) + 1.
pub fn hw_mip_count(w: u32, h: u32) -> u32 {
    let max_dim = w.max(h);
    if max_dim == 0 {
        return 1;
    }
    32 - max_dim.leading_zeros()
}

pub fn texture_2d_mipmapped(
    device: &Device,
    width: u32,
    height: u32,
    mip_level_count: u32,
    format: TextureFormat,
    usage: TextureUsages,
    label: Option<&str>,
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label,
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
