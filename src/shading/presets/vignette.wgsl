@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

struct Uniforms {
    strength: f32,
    _pad: vec3<f32>,
}
@group(0) @binding(2) var<uniform> u: Uniforms;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, input_sampler, uv);
    let d = distance(uv, vec2(0.5));
    let vig = smoothstep(0.8, 0.2 * (1.0 - u.strength), d);
    return vec4(c.rgb * vig, c.a);
}