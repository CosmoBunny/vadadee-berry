//! GPU compute backend (wgpu, all platforms via Vulkan/Metal/DX12/GL).
//!
//! Audit basis: the app owns `Device`/`Queue` inside eframe `RenderState`
//! (app lifetime); export/shading clone it. There are NO compute pipelines
//! yet — this module is the first. Headless compute creates its own cached
//! device (`force_fallback_adapter` covers llvmpipe/CI); the app will later
//! inject its `RenderState` device instead of a second one.
//!
//! Rule (spec Phase 18): GPU image → GPU kernel → GPU image, and GPU image
//! → GPU reduction → small scalar → Lua. Full-frame CPU downloads happen
//! only on the CPU-fallback path, never per-frame in GPU mode.

use std::sync::{Mutex, OnceLock};

/// Rec.709 luma coefficients (the documented luminance definition,
/// shared by the CPU oracle and the WGSL shader below).
pub const LUMA_R: f32 = 0.2126;
pub const LUMA_G: f32 = 0.7152;
pub const LUMA_B: f32 = 0.0722;

/// Cached headless device bundle (created once, reused — never per frame).
struct DeviceBundle {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

static DEVICE: OnceLock<Mutex<Option<DeviceBundle>>> = OnceLock::new();

fn device_bundle() -> Option<std::sync::MutexGuard<'static, Option<DeviceBundle>>> {
    let slot = DEVICE.get_or_init(|| {
        Mutex::new(pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await
                .ok()?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .ok()?;
            Some(DeviceBundle { device, queue })
        }))
    });
    slot.lock().ok()
}

/// Whether a GPU device is available (drives `Auto` backend choice).
pub fn gpu_available() -> bool {
    device_bundle().is_some_and(|g| g.is_some())
}

/// Luminance compute: RGBA8 texture → per-pixel luminance buffer.
/// Rec.709 on the stored bytes: `0.2126·R + 0.7152·G + 0.0722·B`.
const LUMINANCE_WGSL: &str = r#"
@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> lums: array<f32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) {
        return;
    }
    let px = textureLoad(input_tex, gid.xy, 0);
    let lum = 0.2126 * px.r + 0.7152 * px.g + 0.0722 * px.b;
    lums[gid.y * dims.x + gid.x] = lum;
}
"#;

/// Parallel reduction: N floats → per-256 partial sums (GPU side).
const REDUCE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<uniform> n: u32;

var<workgroup> scratch: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(num_workgroups) ng: vec3<u32>,
) {
    let groups = ng.x;
    var acc = 0.0;
    var i = wg.x * 256u + lid.x;
    while (i < n) {
        acc += src[i];
        i += groups * 256u;
    }
    scratch[lid.x] = acc;
    workgroupBarrier();
    var stride = 128u;
    while (stride > 0u) {
        if (lid.x < stride) {
            scratch[lid.x] += scratch[lid.x + stride];
        }
        workgroupBarrier();
        stride /= 2u;
    }
    if (lid.x == 0u) {
        partials[wg.x] = scratch[0];
    }
}
"#;

/// Finalize on GPU: partials → single sum (one workgroup, strided).
const FINALIZE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> partials: array<f32>;
@group(0) @binding(1) var<storage, read_write> total: array<f32>;
@group(0) @binding(2) var<uniform> n: u32;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    var acc = 0.0;
    var i = lid.x;
    while (i < n) {
        acc += partials[i];
        i += 256u;
    }
    // Single-workgroup intra-group reduce via atomics-free tree in one var:
    // 256 threads each hold a partial acc; fold through shared staging.
    // (Simple correct version: thread 0 folds all 256 accs is racy, so we
    // instead write per-thread accs and let the host sum 256 floats.)
    total[lid.x] = acc;
}
"#;

fn run_compute(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    wgsl: &str,
    label: &str,
    make_bindings: &dyn Fn(&wgpu::Device, &wgpu::BindGroupLayout) -> wgpu::BindGroup,
    layout_entries: Vec<wgpu::BindGroupLayoutEntry>,
    dispatch: (u32, u32, u32),
) -> Result<(), String> {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &layout_entries,
    });
    let pll = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pll),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Validation errors surface here (async in wgpu 29: uncaptured-error scope).
    let bind_group = make_bindings(device, &bgl);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some(label),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(dispatch.0, dispatch.1, dispatch.2);
    }
    queue.submit(Some(encoder.finish()));
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).map_err(|e| format!("GPU poll failed: {e:?}"))?;
    Ok(())
}

fn storage_rw(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_ro(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_f32(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn readback_f32(device: &wgpu::Device, buf: &wgpu::Buffer, floats: usize) -> Result<Vec<f32>, String> {
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).map_err(|e| format!("GPU poll failed: {e:?}"))?;
    let data = slice.get_mapped_range();
    if data.len() < floats * 4 {
        return Err("short GPU readback".to_string());
    }
    let mut out = Vec::with_capacity(floats);
    for chunk in data.chunks_exact(4).take(floats) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    drop(data);
    buf.unmap();
    Ok(out)
}

/// GPU average luminance. Small-result rule: ONE 256-float readback max
/// (finalize pass), host sums 256 values. Returns mean 0..1.
pub fn gpu_average_luminance(w: u32, h: u32, rgba: &[u8]) -> Result<f32, String> {
    use wgpu::util::DeviceExt;
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return Err("invalid image".to_string());
    }
    let guard = device_bundle().ok_or("GPU unavailable".to_string())?;
    let bundle = guard.as_ref().ok_or("GPU unavailable".to_string())?;
    let (device, queue) = (&bundle.device, &bundle.queue);
    let n = (w * h) as usize;

    // Upload.
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vblua-luma-input"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba[..n * 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    // Pass 1: luminance map (GPU resident).
    let lums = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vblua-lums"),
        size: (n * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    run_compute(
        device,
        queue,
        LUMINANCE_WGSL,
        "vblua-luminance",
        &|device, layout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vblua-luminance"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: lums.as_entire_binding() },
                ],
            })
        },
        vec![texture_f32(0), storage_rw(1)],
        (w.div_ceil(8), h.div_ceil(8), 1),
    )?;

    // Pass 2: reduce to per-group partials (GPU resident).
    let groups = n.div_ceil(256).max(1) as u32;
    let partials = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vblua-partials"),
        size: (groups as usize * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let nuniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vblua-n"),
        contents: &(n as u32).to_le_bytes(),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    run_compute(
        device,
        queue,
        REDUCE_WGSL,
        "vblua-reduce",
        &|device, layout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vblua-reduce"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: lums.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: partials.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: nuniform.as_entire_binding() },
                ],
            })
        },
        vec![storage_ro(0), storage_rw(1), uniform(2)],
        (groups, 1, 1),
    )?;

    // Pass 3: finalize to 256 floats → single small readback.
    // MAP_READ requires COPY_DST (not COPY_SRC): stage then copy.
    let total = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vblua-total"),
        size: 256 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vblua-staging"),
        size: 256 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let guniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vblua-g"),
        contents: &groups.to_le_bytes(),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    run_compute(
        device,
        queue,
        FINALIZE_WGSL,
        "vblua-finalize",
        &|device, layout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vblua-finalize"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: partials.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: total.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: guniform.as_entire_binding() },
                ],
            })
        },
        vec![storage_ro(0), storage_rw(1), uniform(2)],
        (1, 1, 1),
    )?;

    {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vblua-readback"),
        });
        encoder.copy_buffer_to_buffer(&total, 0, &staging, 0, 256 * 4);
        queue.submit(Some(encoder.finish()));
    }
    let sums = readback_f32(device, &staging, 256)?;
    let sum: f32 = sums.iter().sum();
    Ok((sum / n as f32).clamp(0.0, 1.0))
}

/// Custom kernel binding contract (documented, enforced at pipeline build):
/// - binding 0: `texture_2d<f32>` input (read-only, RGBA8 normalized)
/// - binding 1: `texture_storage_2d<rgba8unorm, write>` output
/// - binding 2 (optional): `var<uniform> params: VbParams` where
///   `struct VbParams { values: array<vec4<f32>, 16> }` (uniform alignment)
/// Pipelines cache by kernel hash — never compiled per frame (Phase 10).
/// Needs a GPU device (custom WGSL has no CPU fallback — honest error).
pub fn gpu_apply_kernel(
    w: u32,
    h: u32,
    rgba: &[u8],
    kernel: &super::kernel::VbKernel,
    overrides: &[(String, f32)],
) -> Result<Vec<u8>, String> {
    use wgpu::util::DeviceExt;
    if w == 0 || h == 0 || w > 4096 || h > 4096 || rgba.len() < (w * h * 4) as usize {
        return Err("invalid image".to_string());
    }
    // Merge declared defaults with per-apply overrides (unknown rejected).
    let mut values: Vec<f32> = kernel.params.iter().map(|(_, v)| *v).collect();
    for (name, v) in overrides {
        let Some(pos) = kernel.params.iter().position(|(k, _)| k == name) else {
            return Err(format!("unknown kernel parameter '{name}'"));
        };
        if !v.is_finite() {
            return Err(format!("parameter '{name}' must be finite"));
        }
        values[pos] = *v;
    }
    let guard = device_bundle().ok_or(
        "custom kernels need a GPU device (none available on this machine)".to_string(),
    )?;
    let bundle = guard.as_ref().ok_or("GPU unavailable".to_string())?;
    let (device, queue) = (&bundle.device, &bundle.queue);

    // Upload input.
    let n = (w * h) as usize;
    let input = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vblua-kernel-input"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &input,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba[..n * 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());

    // Output storage texture.
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vblua-kernel-output"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

    // Uniforms (64 floats, like the shading path's 256B block).
    let mut uniforms = [0f32; 64];
    for (i, v) in values.iter().take(64).enumerate() {
        uniforms[i] = *v;
    }
    let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vblua-kernel-uniforms"),
        contents: bytemuck::cast_slice(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    // Compile (or fetch from the pipeline cache).
    let pipeline = kernel_pipeline(device, kernel)?;

    // Bind group: input + output (+ uniforms iff the source declares binding 2).
    let bgl = pipeline.get_bind_group_layout(0);
    let mut entries = vec![
        wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&input_view),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&output_view),
        },
    ];
    if kernel.source.contains("binding(2)") {
        entries.push(wgpu::BindGroupEntry {
            binding: 2,
            resource: ubuf.as_entire_binding(),
        });
    }
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vblua-kernel"),
        layout: &bgl,
        entries: &entries,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vblua-kernel"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("vblua-kernel"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
    }
    // Read back the processed frame (export-pattern staging buffer).
    // Rows must pad to COPY_BYTES_PER_ROW_ALIGNMENT (256).
    let row_bytes = w as usize * 4;
    let padded_row = row_bytes.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vblua-kernel-readback"),
        size: (padded_row * h as usize) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row as u32),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
        .map_err(|e| format!("GPU poll failed: {e:?}"))?;
    let data = slice.get_mapped_range();
    if data.len() < padded_row * h as usize {
        return Err("short GPU readback".to_string());
    }
    // Strip row padding back to tight RGBA.
    let mut out = Vec::with_capacity(row_bytes * h as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&data[y * padded_row..y * padded_row + row_bytes]);
    }
    drop(data);
    readback.unmap();
    Ok(out)
}

/// Pipeline cache: kernel hash → compiled compute pipeline.
/// Validation failures become readable errors (never panics).
static PIPELINES: OnceLock<Mutex<HashMapCache>> = OnceLock::new();

type HashMapCache = std::collections::HashMap<u64, wgpu::ComputePipeline>;

fn kernel_pipeline(
    device: &wgpu::Device,
    kernel: &super::kernel::VbKernel,
) -> Result<wgpu::ComputePipeline, String> {
    let cache = PIPELINES.get_or_init(|| Mutex::new(HashMapCache::new()));
    if let Some(p) = cache.lock().unwrap().get(&kernel.cache_key).cloned() {
        return Ok(p);
    }
    // wgpu reports shader validation as panics, not Results, in this build
    // configuration — contain them so invalid user WGSL can never crash us.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compile_pipeline_inner(device, kernel)
    }));
    let pipeline = match outcome {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(format!(
                "VBLua kernel compilation failed\nKernel: {}\nWGSL rejected by the driver",
                kernel.name
            ));
        }
    };
    let cache = PIPELINES.get_or_init(|| Mutex::new(HashMapCache::new()));
    cache.lock().unwrap().insert(kernel.cache_key, pipeline.clone());
    Ok(pipeline)
}

/// Uncached pipeline compile (panics contained by the caller).
fn compile_pipeline_inner(
    device: &wgpu::Device,
    kernel: &super::kernel::VbKernel,
) -> Result<wgpu::ComputePipeline, String> {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("vblua-kernel:{}", kernel.name)),
        source: wgpu::ShaderSource::Wgsl(kernel.source.clone().into()),
    });
    // Synchronous validation capture: pipeline compile errors become strings.
    let guard = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(&format!("vblua-kernel:{}", kernel.name)),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
        .map_err(|e| format!("GPU poll failed: {e:?}"))?;
    if let Some(err) = pollster::block_on(guard.pop()) {
        return Err(format!(
            "VBLua kernel compilation failed\nKernel: {}\n{err}",
            kernel.name
        ));
    }
    Ok(pipeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_image(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        [px[0], px[1], px[2], px[3]].repeat((w * h) as usize)
    }

    #[test]
    fn gpu_luminance_matches_rec709_when_available() {
        if !gpu_available() {
            eprintln!("no GPU device — skipping (CPU fallback covers correctness)");
            return;
        }
        // Pure red: 0.2126. Pure white: 1.0.
        let r = gpu_average_luminance(4, 4, &rgba_image(4, 4, [255, 0, 0, 255])).unwrap();
        assert!((r - 0.2126).abs() < 0.01, "{r}");
        let w = gpu_average_luminance(4, 4, &rgba_image(4, 4, [255, 255, 255, 255])).unwrap();
        assert!((w - 1.0).abs() < 0.01, "{w}");
    }

    const COLOR_BALANCE: &str = r#"
        @group(0) @binding(0) var input_tex: texture_2d<f32>;
        @group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
        struct VbParams {
            values: array<vec4<f32>, 16>,
        }
        @group(0) @binding(2) var<uniform> params: VbParams;

        @compute @workgroup_size(8, 8)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let dims = textureDimensions(input_tex);
            if (gid.x >= dims.x || gid.y >= dims.y) {
                return;
            }
            let px = textureLoad(input_tex, gid.xy, 0);
            let out = vec4<f32>(
                px.r * params.values[0].x, px.g * params.values[0].y, px.b * params.values[0].z, px.a);
            textureStore(output_tex, gid.xy, out);
        }
    "#;

    #[test]
    fn color_balance_reference_kernel() {
        if !gpu_available() {
            eprintln!("no GPU device — skipping");
            return;
        }
        let kernel = super::super::kernel::VbKernel::create(
            "color_balance".into(),
            "wgsl",
            COLOR_BALANCE.into(),
            vec![("red".into(), 1.0), ("green".into(), 1.0), ("blue".into(), 1.0)],
        )
        .unwrap();
        // Gray 128 stays gray at identity; red channel doubles (clamped).
        let gray = rgba_image(8, 8, [128, 128, 128, 255]);
        let out = gpu_apply_kernel(8, 8, &gray, &kernel, &[]).unwrap();
        assert_eq!(out.len(), 8 * 8 * 4);
        assert!((out[0] as i32 - 128).abs() <= 1, "{}", out[0]);
        let out2 = gpu_apply_kernel(
            8,
            8,
            &gray,
            &kernel,
            &[("red".to_string(), 2.0), ("green".to_string(), 0.5), ("blue".to_string(), 0.5)],
        )
        .unwrap();
        assert_eq!(out2[0], 255);
        assert!((out2[1] as i32 - 64).abs() <= 1, "{}", out2[1]);
        assert!((out2[2] as i32 - 64).abs() <= 1, "{}", out2[2]);
        // Unknown params + bad WGSL fail loudly, cached re-runs agree.
        assert!(gpu_apply_kernel(8, 8, &gray, &kernel, &[("nope".to_string(), 1.0)]).is_err());
        let again = gpu_apply_kernel(8, 8, &gray, &kernel, &[]).unwrap();
        assert_eq!(out, again);
        let bad = super::super::kernel::VbKernel::create(
            "bad".into(),
            "wgsl",
            "@compute @workgroup_size(8,8)\nfn main() { pixel }".into(),
            vec![],
        )
        .unwrap();
        let err = gpu_apply_kernel(8, 8, &gray, &bad, &[]).unwrap_err();
        assert!(err.contains("bad"), "{err}");
    }

    #[test]
    fn kernels_chain_a_to_b_to_c() {
        if !gpu_available() {
            eprintln!("no GPU device — skipping");
            return;
        }
        // Chain: brighten x1.5 -> balance half green/blue -> grayscale.
        // Each stage materializes (correctness first; fusion is future work).
        let brighten_src = r#"
            struct VbParams { values: array<vec4<f32>, 16> }
            @group(0) @binding(0) var input_tex: texture_2d<f32>;
            @group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
            @group(0) @binding(2) var<uniform> params: VbParams;
            @compute @workgroup_size(8, 8)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let px = textureLoad(input_tex, gid.xy, 0);
                textureStore(output_tex, gid.xy, vec4<f32>(px.rgb * params.values[0].x, px.a));
            }
        "#;
        let gray_src = r#"
            @group(0) @binding(0) var input_tex: texture_2d<f32>;
            @group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
            @compute @workgroup_size(8, 8)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let px = textureLoad(input_tex, gid.xy, 0);
                let l = 0.2126 * px.r + 0.7152 * px.g + 0.0722 * px.b;
                textureStore(output_tex, gid.xy, vec4<f32>(l, l, l, px.a));
            }
        "#;
        let k1 = super::super::kernel::VbKernel::create(
            "brighten".into(), "wgsl", brighten_src.into(), vec![("k".into(), 1.5)],
        )
        .unwrap();
        let k2 = super::super::kernel::VbKernel::create(
            "gray".into(), "wgsl", gray_src.into(), vec![],
        )
        .unwrap();
        let red = rgba_image(8, 8, [100, 0, 0, 255]);
        let a = gpu_apply_kernel(8, 8, &red, &k1, &[]).unwrap();
        assert_eq!(a[0], 150); // 100 * 1.5
        let b = gpu_apply_kernel(8, 8, &a, &k2, &[]).unwrap();
        let expect = (0.2126_f64 * 150.0).round() as u8;
        assert!((b[0] as i32 - expect as i32).abs() <= 1, "{} vs {expect}", b[0]);
        assert_eq!(b[1], b[0]);
        assert_eq!(b[2], b[0]);
    }
}
