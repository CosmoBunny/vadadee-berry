// CRT scanline + vignette (placeholder — wired via wgpu in Phase 4b)
@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

struct Uniforms {
    intensity: f32,
    _pad: vec3<f32>,
}
@group(0) @binding(2) var<uniform> u: Uniforms;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, input_sampler, uv);
    let scan = 0.85 + 0.15 * sin(uv.y * 800.0);
    let vig = 1.0 - dot(uv - vec2(0.5), uv - vec2(0.5)) * u.intensity;
    return vec4(c.rgb * scan * vig, c.a);
}