//! Runtime WGSL compile + egui-wgpu paint callbacks for shading layers.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Painter, Rect, Shape};
use egui_wgpu::wgpu;
use egui::epaint::PaintCallbackInfo;
use egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

use crate::document::ShadingPass;

const VERTEX_SHADER: &str = r#"
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
    out.uv = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}
"#;

const UNIFORM_BUFFER_SIZE: u64 = 256;

pub struct ShadingGpuResources {
    device: wgpu::Device,
    target_format: wgpu::TextureFormat,
    msaa_samples: u32,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    input_texture: wgpu::Texture,
    input_view: wgpu::TextureView,
    pipelines: HashMap<u64, Result<Arc<CompiledShadingPipeline>, String>>,
    pending_input: Option<(u32, u32, Vec<u8>)>,
}

struct CompiledShadingPipeline {
    compose: bool,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

pub fn init_callback_resources(render_state: &RenderState, msaa_samples: u32) {
    let mut renderer = render_state.renderer.write();
    if renderer.callback_resources.contains::<ShadingGpuResources>() {
        return;
    }
    let resources = ShadingGpuResources::new(
        render_state.device.clone(),
        render_state.target_format,
        msaa_samples,
    );
    renderer.callback_resources.insert(resources);
}

pub fn queue_shading_input(render_state: &RenderState, width: u32, height: u32, rgba: Vec<u8>) {
    let mut renderer = render_state.renderer.write();
    let Some(res) = renderer.callback_resources.get_mut::<ShadingGpuResources>() else {
        return;
    };
    res.pending_input = Some((width, height, rgba));
}

fn source_key(wgsl: &str, compose: bool) -> u64 {
    let mut h = FxHasher::default();
    wgsl.hash(&mut h);
    compose.hash(&mut h);
    h.finish()
}

fn wgsl_needs_compose(wgsl: &str) -> bool {
    wgsl.contains("input_tex")
}

fn assemble_module(user_wgsl: &str) -> String {
    if user_wgsl.contains("@vertex") {
        user_wgsl.to_string()
    } else {
        format!("{VERTEX_SHADER}\n{user_wgsl}")
    }
}

fn fragment_entry(wgsl: &str) -> &'static str {
    if wgsl.contains("fn fs_main") {
        "fs_main"
    } else {
        "main"
    }
}

/// Static checks before wgpu so users get actionable errors (compute multipass, missing entry, …).
pub fn validate_shading_wgsl(wgsl: &str) -> Result<(), String> {
    let src = wgsl.trim();
    if src.is_empty() {
        return Err("WGSL source is empty.".into());
    }
    // CPU-only stub passes are allowed through; they never reach GPU compile.
    if src.contains("// Starfield — rendered via CPU starfield path.") {
        return Ok(());
    }

    // Common GLSL leftovers that compile-fail only on GPU (white canvas + Validation Error).
    if src.contains("mod(") || src.contains("mod (") {
        return Err(
            "WGSL has no GLSL-style `mod(a, b)` function.\n\
             Use remainder: `a % b` (same-type floats/vecs), or:\n\
               fn mod2(x: vec2<f32>, y: vec2<f32>) -> vec2<f32> { return x - y * floor(x / y); }"
                .into(),
        );
    }
    if src.contains("texture2D(") || src.contains("gl_FragColor") || src.contains("varying ") {
        return Err(
            "Looks like GLSL, not WGSL. Use textureSample / @location outputs / @location inputs."
                .into(),
        );
    }

    let has_fragment = src.contains("@fragment");
    let has_compute = src.contains("@compute");
    if has_compute && !has_fragment {
        return Err(
            "This looks like a compute multipass shader (@compute only).\n\
             Vadadee shading layers need a single fragment entry:\n\
               @fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32>\n\
             Engines like Cuneus use storage textures + several compute kernels; \
             those cannot be pasted here. Use Custom template or a fragment port."
                .into(),
        );
    }
    if !has_fragment && !src.contains("@vertex") {
        // Vertex may be auto-prepended; still require a fragment entry named main/fs_main.
        let entry = fragment_entry(src);
        if !src.contains(&format!("fn {entry}")) {
            return Err(format!(
                "Missing fragment entry point `{entry}`.\n\
                 Add:\n  @fragment\n  fn {entry}(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{ ... }}"
            ));
        }
        return Err(
            "Missing `@fragment` on the entry function.\n\
             Shading layers use a render pipeline (vertex + fragment), not compute."
                .into(),
        );
    }
    let entry = fragment_entry(src);
    // Require the chosen entry to appear near @fragment (best-effort).
    if has_fragment {
        let needle_main = "fn main";
        let needle_fs = "fn fs_main";
        let has_named = match entry {
            "fs_main" => src.contains(needle_fs),
            _ => src.contains(needle_main) || src.contains(needle_fs),
        };
        if !has_named {
            return Err(format!(
                "Unable to find fragment entry `{entry}`. Name it `main` or `fs_main` with @fragment."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod validate_tests {
    use super::validate_shading_wgsl;

    #[test]
    fn rejects_glsl_mod() {
        let src = r#"
@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let a = mod(uv, vec2(1.0));
    return vec4(a, 0.0, 1.0);
}
"#;
        let err = validate_shading_wgsl(src).unwrap_err();
        assert!(err.contains("mod"), "{err}");
    }

    #[test]
    fn accepts_fragment_main() {
        let src = r#"
struct Uniforms { time: f32, strength: f32, _pad2: f32, aspect: f32, }
@group(0) @binding(0) var<uniform> u: Uniforms;
@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4(uv, 0.0, 1.0);
}
"#;
        validate_shading_wgsl(src).unwrap();
    }
}

/// GPU compile probe (full fragment pipeline). Call after apply from MCP so broken WGSL
/// fails the tool instead of painting a blank white page with a Validation Error panel.
///
/// When `render_state` is `None` (headless tests), only static `validate_shading_wgsl` runs.
pub fn probe_compile_shading_wgsl(
    render_state: Option<&RenderState>,
    wgsl: &str,
) -> Result<(), String> {
    validate_shading_wgsl(wgsl)?;
    let Some(rs) = render_state else {
        return Ok(());
    };
    // Ensure callback resources exist (first shading paint also does this).
    {
        let mut renderer = rs.renderer.write();
        if !renderer.callback_resources.contains::<ShadingGpuResources>() {
            let resources =
                ShadingGpuResources::new(rs.device.clone(), rs.target_format, 1);
            renderer.callback_resources.insert(resources);
        }
        let res = renderer
            .callback_resources
            .get_mut::<ShadingGpuResources>()
            .ok_or_else(|| "shading GPU resources missing".to_string())?;
        // Match live target format / MSAA from the eframe surface when possible.
        res.target_format = rs.target_format;
        res.pipeline(&rs.device, wgsl).map_err(|e| {
            format!(
                "WGSL GPU compile failed (shader not applied).\n\
                 Fix the source and retry. Detail:\n{e}"
            )
        })?;
    }
    Ok(())
}

fn compile_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    msaa_samples: u32,
    wgsl: &str,
    compose: bool,
) -> Result<CompiledShadingPipeline, String> {
    validate_shading_wgsl(wgsl)?;

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let module_src = assemble_module(wgsl);
    let module = device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vadadee_shading_pass"),
            source: wgpu::ShaderSource::Wgsl(module_src.into()),
        });

    let bind_group_layout = if compose {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shading_compose_bgl"),
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
        })
    } else {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shading_proc_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
    };

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shading_pipeline_layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let blend = wgpu::BlendState::ALPHA_BLENDING;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shading_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment_entry(wgsl)),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            alpha_to_coverage_enabled: false,
            count: msaa_samples.max(1),
            mask: !0,
        },
        multiview_mask: None,
        cache: None,
    });

    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(match err {
            wgpu::Error::Validation { description, .. } => description,
            wgpu::Error::OutOfMemory { .. } => "Out of memory".to_string(),
            wgpu::Error::Internal { description, .. } => description,
        });
    }

    Ok(CompiledShadingPipeline {
        compose,
        pipeline,
        bind_group_layout,
    })
}

impl ShadingGpuResources {
    pub fn new(device: wgpu::Device, target_format: wgpu::TextureFormat, msaa_samples: u32) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shading_input_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shading_uniforms"),
            size: UNIFORM_BUFFER_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let input_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shading_input_tex"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let input_view = input_texture.create_view(&Default::default());
        Self {
            device,
            target_format,
            msaa_samples,
            sampler,
            uniform_buffer,
            input_texture,
            input_view,
            pipelines: HashMap::new(),
            pending_input: None,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        wgsl: &str,
    ) -> Result<Arc<CompiledShadingPipeline>, String> {
        let compose = wgsl_needs_compose(wgsl);
        let key = source_key(wgsl, compose);
        if let Some(res) = self.pipelines.get(&key) {
            return res.clone();
        }
        let result = compile_pipeline(device, self.target_format, self.msaa_samples, wgsl, compose)
            .map(Arc::new);
        self.pipelines.insert(key, result.clone());
        result
    }

    fn upload_input(&mut self, queue: &wgpu::Queue) {
        let Some((w, h, rgba)) = self.pending_input.take() else {
            return;
        };
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return;
        }
        if self.input_texture.width() != w || self.input_texture.height() != h {
            self.input_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shading_input_tex"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.input_view = self.input_texture.create_view(&Default::default());
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    fn write_uniforms(&mut self, queue: &wgpu::Queue, floats: &[f32]) {
        let mut buf = [0u8; UNIFORM_BUFFER_SIZE as usize];
        let n = floats.len().min(UNIFORM_BUFFER_SIZE as usize / 4);
        buf[..n * 4].copy_from_slice(bytemuck::cast_slice(&floats[..n]));
        queue.write_buffer(&self.uniform_buffer, 0, &buf);
    }

    fn bind_group(&self, layout: &wgpu::BindGroupLayout, compose: bool) -> wgpu::BindGroup {
        if compose {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shading_compose_bg"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniform_buffer.as_entire_binding(),
                    },
                ],
            })
        } else {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shading_proc_bg"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                }],
            })
        }
    }
}

fn uniform_floats(pass: &ShadingPass, time_secs: f32, aspect: f32) -> Vec<f32> {
    let mut floats = if pass.uniforms.is_empty() {
        vec![0.0, 1.0, 0.22]
    } else {
        pass.uniforms.clone()
    };
    floats[0] += time_secs;
    if floats.len() < 4 {
        floats.resize(4, 0.0);
    }
    floats[3] = aspect;
    floats
}

struct ShadingPaintCallback {
    wgsl: Arc<str>,
    uniforms: Vec<f32>,
}

impl CallbackTrait for ShadingPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<ShadingGpuResources>() else {
            return Vec::new();
        };
        gpu.upload_input(queue);
        gpu.write_uniforms(queue, &self.uniforms);
        if gpu.pipeline(device, &self.wgsl).is_err() {
            log::warn!("WGSL shading compile failed for pass");
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(gpu) = resources.get::<ShadingGpuResources>() else {
            return;
        };
        let key = source_key(&self.wgsl, wgsl_needs_compose(&self.wgsl));
        let Some(Ok(pipeline)) = gpu.pipelines.get(&key) else {
            return;
        };
        let bind_group = gpu.bind_group(&pipeline.bind_group_layout, pipeline.compose);

        // IMPORTANT: do NOT use viewport_in_pixels() for set_viewport.
        // That helper clamps the rect to the screen. When the page is partially
        // panned off-screen, the clamped viewport is only the *visible slice*,
        // but the fullscreen triangle still maps UV 0..1 across it — so the
        // shader appears "squeezed". Keep the full page as the NDC viewport
        // (allowing coords outside the framebuffer) and scissor to the visible area.
        let ppp = info.pixels_per_point;
        let page = info.viewport; // full PaintCallback::rect (page), unclamped
        let left = ppp * page.min.x;
        let top = ppp * page.min.y;
        let mut width = (ppp * page.width()).max(1.0);
        let mut height = (ppp * page.height()).max(1.0);
        if width < 1.0 || height < 1.0 {
            return;
        }
        // wgpu validates: origin >= -2*max_texture_dimension_2d and
        // origin+size <= 2*max_texture_dimension_2d - 1 (often ±16384 around 8192).
        // High zoom/pan can exceed either the size or the origin limit → panic.
        const MAX_DIM: f32 = 8192.0;
        const ORIGIN_MIN: f32 = -2.0 * MAX_DIM; // -16384
        const EXTENT_MAX: f32 = 2.0 * MAX_DIM - 1.0; // 16383
        if width > MAX_DIM || height > MAX_DIM {
            let s = (MAX_DIM / width).min(MAX_DIM / height).min(1.0);
            width = (width * s).max(1.0);
            height = (height * s).max(1.0);
        }
        width = width.clamp(1.0, MAX_DIM);
        height = height.clamp(1.0, MAX_DIM);
        let left = left.clamp(ORIGIN_MIN, EXTENT_MAX - width);
        let top = top.clamp(ORIGIN_MIN, EXTENT_MAX - height);
        render_pass.set_viewport(left, top, width, height, 0.0, 1.0);

        let clip = info.clip_rect_in_pixels();
        let sx = clip.left_px.max(0) as u32;
        let sy = clip.top_px.max(0) as u32;
        let sw = clip.width_px.max(0) as u32;
        let sh = clip.height_px.max(0) as u32;
        if sw == 0 || sh == 0 {
            return;
        }
        // Intersect scissor with framebuffer just in case.
        let fb_w = info.screen_size_px[0];
        let fb_h = info.screen_size_px[1];
        let sx2 = sx.min(fb_w);
        let sy2 = sy.min(fb_h);
        let sw2 = sw.min(fb_w.saturating_sub(sx2));
        let sh2 = sh.min(fb_h.saturating_sub(sy2));
        if sw2 == 0 || sh2 == 0 {
            return;
        }
        render_pass.set_scissor_rect(sx2, sy2, sw2, sh2);

        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Placeholder renderer hook (kept for API stability).
pub struct ShadingRenderer {
    pub enabled: bool,
}

impl Default for ShadingRenderer {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl ShadingRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn is_cpu_only_pass(pass: &ShadingPass) -> bool {
    let name = pass.name.to_ascii_lowercase();
    let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl);
    name == "starfield" || wgsl.contains("// Starfield — rendered via CPU starfield path.")
}

/// Active pass for a layer: last enabled (so MCP/custom appended after a default survives).
fn active_shading_pass(passes: &[ShadingPass]) -> Option<&ShadingPass> {
    passes.iter().rev().find(|p| p.enabled)
}

pub fn try_draw_shading_passes_gpu(
    painter: &Painter,
    page_rect: Rect,
    passes: &[ShadingPass],
    time_secs: f32,
    render_state: &RenderState,
) -> bool {
    let aspect = (page_rect.width() / page_rect.height().max(1.0)).max(0.25);
    let Some(pass) = active_shading_pass(passes) else {
        return false;
    };
    if is_cpu_only_pass(pass) {
        return false;
    }
    let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl).trim();
    if wgsl.is_empty() {
        return false;
    }

    // Check if there is a cached compile error
    {
        if let Ok(err_lock) = pass.compile_error.lock() {
            if err_lock.is_some() {
                return false;
            }
        }
    }

    let device = &render_state.device;
    {
        let mut renderer = render_state.renderer.write();
        let Some(gpu) = renderer.callback_resources.get_mut::<ShadingGpuResources>() else {
            return false;
        };
        match gpu.pipeline(device, wgsl) {
            Ok(_) => {
                if let Ok(mut err_lock) = pass.compile_error.lock() {
                    *err_lock = None;
                }
            }
            Err(err_msg) => {
                log::debug!(
                    "WGSL compile failed for shading pass \"{}\"; falling back to CPU",
                    pass.name
                );
                if let Ok(mut err_lock) = pass.compile_error.lock() {
                    *err_lock = Some(err_msg);
                }
                return false;
            }
        }
    }
    let uniforms = uniform_floats(pass, time_secs, aspect);
    let callback = Callback::new_paint_callback(
        page_rect,
        ShadingPaintCallback {
            wgsl: Arc::from(wgsl),
            uniforms,
        },
    );
    painter.add(Shape::Callback(callback));
    true
}

pub fn shading_passes_need_input(passes: &[ShadingPass]) -> bool {
    active_shading_pass(passes)
        .map(|p| {
            let wgsl = p.compiled_wgsl.as_ref().unwrap_or(&p.wgsl);
            wgsl_needs_compose(wgsl)
        })
        .unwrap_or(false)
}

/// Cache offscreen (Rgba8Unorm / MSAA1) pipelines so export doesn't recompile WGSL every frame.
fn offscreen_pipeline(
    device: &wgpu::Device,
    wgsl: &str,
) -> Result<Arc<CompiledShadingPipeline>, String> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u64, Result<Arc<CompiledShadingPipeline>, String>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = source_key(wgsl, false);
    {
        let guard = cache.lock().map_err(|e| e.to_string())?;
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let compiled = compile_pipeline(device, wgpu::TextureFormat::Rgba8Unorm, 1, wgsl, false)
        .map(Arc::new);
    let mut guard = cache.lock().map_err(|e| e.to_string())?;
    // Another thread may have filled it; prefer existing.
    if let Some(hit) = guard.get(&key) {
        return hit.clone();
    }
    guard.insert(key, compiled.clone());
    compiled
}

/// Offscreen GPU render of one procedural (or compose) shading pass → tightly packed RGBA8.
/// Used by MCP capture / export preview so custom WGSL is not left as blank page color.
///
/// Pipelines are cached — safe to call every export frame without recompiling WGSL.
pub fn render_shading_pass_to_rgba(
    render_state: &RenderState,
    pass: &ShadingPass,
    width: u32,
    height: u32,
    time_secs: f32,
) -> Result<Vec<u8>, String> {
    let width = width.clamp(1, 4096);
    let height = height.clamp(1, 4096);
    if is_cpu_only_pass(pass) {
        return Err("CPU-only pass; use starfield path".into());
    }
    let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl).trim();
    if wgsl.is_empty() {
        return Err("empty WGSL".into());
    }
    let compose = wgsl_needs_compose(wgsl);
    if compose {
        // Compose needs an input tex; MCP capture does not feed one yet.
        return Err("compose shaders need input_tex; capture supports procedural only".into());
    }

    let device = &render_state.device;
    let queue = &render_state.queue;
    let aspect = (width as f32 / (height as f32).max(1.0)).max(0.25);
    let uniforms = uniform_floats(pass, time_secs, aspect);

    // Always Rgba8Unorm + 1 sample for readback (independent of swapchain format).
    let pipeline = offscreen_pipeline(device, wgsl)?;

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shading_capture_tex"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());

    let ub_size = UNIFORM_BUFFER_SIZE;
    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shading_capture_uniforms"),
        size: ub_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut ubuf = [0u8; UNIFORM_BUFFER_SIZE as usize];
    let n = uniforms.len().min(UNIFORM_BUFFER_SIZE as usize / 4);
    ubuf[..n * 4].copy_from_slice(bytemuck::cast_slice(&uniforms[..n]));
    queue.write_buffer(&uniform_buffer, 0, &ubuf);

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shading_capture_bg"),
        layout: &pipeline.bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shading_capture_enc"),
    });
    {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shading_capture_rp"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&pipeline.pipeline);
        rp.set_bind_group(0, &bind_group, &[]);
        rp.draw(0..3, 0..1);
    }

    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = (unpadded + align - 1) / align * align;
    let buf_size = (padded * height) as u64;
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shading_capture_read"),
        size: buf_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = out_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv()
        .map_err(|e| format!("map channel: {e}"))?
        .map_err(|e| format!("map failed: {e}"))?;

    let data = slice.get_mapped_range();
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as usize {
        let src = y * padded as usize;
        let dst = y * unpadded as usize;
        rgba[dst..dst + unpadded as usize]
            .copy_from_slice(&data[src..src + unpadded as usize]);
    }
    drop(data);
    out_buf.unmap();
    Ok(rgba)
}

/// Composite every enabled procedural shading layer into `rgba` (full frame, bottom→top).
/// Returns true if at least one pass was drawn.
pub fn composite_shading_layers_into_rgba(
    render_state: &RenderState,
    project: &crate::document::ProjectFile,
    rgba: &mut [u8],
    width: u32,
    height: u32,
    time_secs: f32,
) -> bool {
    if rgba.len() < (width * height * 4) as usize {
        return false;
    }
    let mut any = false;
    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        if layer.kind != crate::document::LayerKind::Shading {
            continue;
        }
        let Some(pass) = active_shading_pass(&layer.shading_passes) else {
            continue;
        };
        if is_cpu_only_pass(pass) {
            continue;
        }
        match render_shading_pass_to_rgba(render_state, pass, width, height, time_secs) {
            Ok(src) => {
                // Procedural passes are opaque full-page — replace (they sit on page bg).
                rgba.copy_from_slice(&src);
                any = true;
            }
            Err(e) => {
                log::warn!("shading capture failed for \"{}\": {e}", pass.name);
            }
        }
    }
    any
}