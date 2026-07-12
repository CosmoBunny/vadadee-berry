//! Two-pass separable Gaussian blur on wgpu (Node Editor LinearBlur).
//!
//! Keeps the pipeline warm and leaves the result on the GPU (native egui texture)
//! — no CPU readback on the hot path.

use egui_wgpu::wgpu;
use image::RgbaImage;
use std::sync::OnceLock;

const BLUR_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = positions[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    // Match egui-style UV: y flips for texture sampling.
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

struct BlurUniforms {
    // xy = texel size (1/w, 1/h); z = direction x (1=horiz,0=vert); w = direction y
    dir_texel: vec4<f32>,
    // weights[0] = center; weights[1..7] = pairs for offset 1..7 (max 15 taps)
    weights: array<vec4<f32>, 2>,
    // x = half_kernel (number of side taps, 1..7) — not named `meta` (WGSL reserved)
    kernel_info: vec4<f32>,
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> u: BlurUniforms;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = u.dir_texel.xy;
    let dir = u.dir_texel.zw;
    let half_k = i32(u.kernel_info.x);
    let w0 = u.weights[0].x;
    var color = textureSample(src_tex, src_samp, in.uv) * w0;
    for (var i: i32 = 1; i <= half_k; i = i + 1) {
        var w: f32 = 0.0;
        if (i == 1) { w = u.weights[0].y; }
        else if (i == 2) { w = u.weights[0].z; }
        else if (i == 3) { w = u.weights[0].w; }
        else if (i == 4) { w = u.weights[1].x; }
        else if (i == 5) { w = u.weights[1].y; }
        else if (i == 6) { w = u.weights[1].z; }
        else { w = u.weights[1].w; }
        let off = dir * texel * f32(i);
        color = color + textureSample(src_tex, src_samp, in.uv + off) * w;
        color = color + textureSample(src_tex, src_samp, in.uv - off) * w;
    }
    return color;
}
"#;

/// Cached pipeline + sampler. One per process (shared across frames).
pub struct GraphBlurEngine {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    ub_h: wgpu::Buffer,
    ub_v: wgpu::Buffer,
}

static ENGINE: OnceLock<std::sync::Mutex<Option<GraphBlurEngine>>> = OnceLock::new();

fn engine_slot() -> &'static std::sync::Mutex<Option<GraphBlurEngine>> {
    ENGINE.get_or_init(|| std::sync::Mutex::new(None))
}

impl GraphBlurEngine {
    fn create(device: &wgpu::Device) -> Option<Self> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph_blur_shader"),
            source: wgpu::ShaderSource::Wgsl(BLUR_WGSL.into()),
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            log::warn!("graph blur shader invalid: {err}");
            return None;
        }

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph_blur_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph_blur_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph_blur_pipe"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("graph_blur_samp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let ub_h = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph_blur_ub_h"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ub_v = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph_blur_ub_v"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            pipeline,
            bgl,
            sampler,
            ub_h,
            ub_v,
        })
    }

    fn with_engine<R>(device: &wgpu::Device, f: impl FnOnce(&GraphBlurEngine) -> R) -> Option<R> {
        let slot = engine_slot();
        let mut guard = slot.lock().ok()?;
        if guard.is_none() {
            *guard = Self::create(device);
        }
        let eng = guard.as_ref()?;
        Some(f(eng))
    }

    /// GPU H+V Gaussian blur. Result stays on GPU (no readback).
    /// Returns `(dst_texture, view, w, h)`.
    pub fn blur_to_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &RgbaImage,
        radius_px: f32,
    ) -> Option<(wgpu::Texture, wgpu::TextureView, u32, u32)> {
        let (w, h) = img.dimensions();
        if w < 2 || h < 2 || radius_px < 0.05 {
            return None;
        }
        if w > 4096 || h > 4096 {
            return None;
        }

        match Self::with_engine(device, |eng| eng.blur_inner(device, queue, img, radius_px)) {
            Some(inner) => inner,
            None => None,
        }
    }

    fn blur_inner(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &RgbaImage,
        radius_px: f32,
    ) -> Option<(wgpu::Texture, wgpu::TextureView, u32, u32)> {
        let (w, h) = img.dimensions();
        // Continuous σ from radius (frame-smooth).
        let sigma = (radius_px * 0.5).clamp(0.12, 24.0);
        let half = ((sigma * 3.0).ceil() as usize).clamp(1, 7);
        let mut weights = vec![0.0_f32; half + 1];
        {
            let inv = 1.0 / (2.0 * sigma * sigma);
            let mut sum = 0.0_f32;
            for i in 0..=half {
                let wgt = (-(i as f32) * (i as f32) * inv).exp();
                weights[i] = wgt;
                sum += if i == 0 { wgt } else { wgt * 2.0 };
            }
            for wgt in &mut weights {
                *wgt /= sum;
            }
        }

        let make_tex = |label: &'static str, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };

        let src_tex = make_tex(
            "graph_blur_src",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let mid_tex = make_tex(
            "graph_blur_mid",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        // Final: sampleable by egui + render target.
        let dst_tex = make_tex(
            "graph_blur_dst",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );

        let src_view = src_tex.create_view(&Default::default());
        let mid_view = mid_tex.create_view(&Default::default());
        let dst_view = dst_tex.create_view(&Default::default());

        // Upload with 256-byte row alignment.
        let align = 256u32;
        let unpadded_row = 4 * w;
        let padded_row = (unpadded_row + align - 1) / align * align;
        let raw = img.as_raw();
        let mut padded_upload = vec![0u8; (padded_row * h) as usize];
        for row in 0..h {
            let src_off = (row * unpadded_row) as usize;
            let dst_off = (row * padded_row) as usize;
            padded_upload[dst_off..dst_off + unpadded_row as usize]
                .copy_from_slice(&raw[src_off..src_off + unpadded_row as usize]);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &src_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &padded_upload,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let pack_uniforms = |dir_x: f32, dir_y: f32| -> [u8; 64] {
            let mut data = [0u8; 64];
            let texel_x = 1.0 / w as f32;
            let texel_y = 1.0 / h as f32;
            let dir_texel = [texel_x, texel_y, dir_x, dir_y];
            let mut side = [0.0_f32; 7];
            for i in 1..=half {
                side[i - 1] = weights[i];
            }
            let wpack0 = [weights[0], side[0], side[1], side[2]];
            let wpack1 = [side[3], side[4], side[5], side[6]];
            let meta = [half as f32, 0.0, 0.0, 0.0];
            data[0..16].copy_from_slice(bytemuck_bytes(&dir_texel));
            data[16..32].copy_from_slice(bytemuck_bytes(&wpack0));
            data[32..48].copy_from_slice(bytemuck_bytes(&wpack1));
            data[48..64].copy_from_slice(bytemuck_bytes(&meta));
            data
        };

        queue.write_buffer(&self.ub_h, 0, &pack_uniforms(1.0, 0.0));
        queue.write_buffer(&self.ub_v, 0, &pack_uniforms(0.0, 1.0));

        let make_bg = |view: &wgpu::TextureView, ub: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("graph_blur_bg"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: ub.as_entire_binding(),
                    },
                ],
            })
        };

        let bg_h = make_bg(&src_view, &self.ub_h);
        let bg_v = make_bg(&mid_view, &self.ub_v);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("graph_blur_enc"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph_blur_h"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &mid_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg_h, &[]);
            pass.draw(0..3, 0..1);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph_blur_v"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg_v, &[]);
            pass.draw(0..3, 0..1);
        }

        queue.submit(Some(encoder.finish()));
        // No readback — keep dst on GPU for egui native texture.
        Some((dst_tex, dst_view, w, h))
    }
}

/// Register (or update) a GPU texture with egui_wgpu — no CPU pixels.
pub fn register_or_update_native(
    render_state: &egui_wgpu::RenderState,
    view: &wgpu::TextureView,
    existing: Option<egui::TextureId>,
) -> Option<egui::TextureId> {
    let device = &render_state.device;
    let mut renderer = render_state.renderer.write();
    if let Some(id) = existing {
        renderer.update_egui_texture_from_wgpu_texture(
            device,
            view,
            wgpu::FilterMode::Linear,
            id,
        );
        Some(id)
    } else {
        Some(renderer.register_native_texture(device, view, wgpu::FilterMode::Linear))
    }
}

pub fn free_native_texture(render_state: &egui_wgpu::RenderState, id: egui::TextureId) {
    let mut renderer = render_state.renderer.write();
    renderer.free_texture(&id);
}

/// Legacy: GPU blur with CPU readback (export / tests). Prefer `blur_to_texture`.
pub fn gpu_gaussian_blur(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &RgbaImage,
    radius_px: f32,
) -> Option<RgbaImage> {
    let (dst_tex, _view, w, h) = GraphBlurEngine::blur_to_texture(device, queue, img, radius_px)?;

    let align = 256u32;
    let unpadded_row = 4 * w;
    let padded_row = (unpadded_row + align - 1) / align * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("graph_blur_read"),
        size: (padded_row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("graph_blur_readback_enc"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &dst_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().ok()?.ok()?;

    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h {
        let start = (row * padded_row) as usize;
        let end = start + unpadded_row as usize;
        out.extend_from_slice(&data[start..end]);
    }
    drop(data);
    readback.unmap();
    RgbaImage::from_raw(w, h, out)
}

fn bytemuck_bytes(v: &[f32; 4]) -> &[u8] {
    bytemuck::bytes_of(v)
}
