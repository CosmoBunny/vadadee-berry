// Procedural black hole + starfield (full-screen shading pass)
@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

struct Uniforms {
    time: f32,
    strength: f32,
    disk_radius: f32,
    aspect: f32,
}
@group(0) @binding(2) var<uniform> u: Uniforms;

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn stars(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let gv = floor(uv * 380.0);
    let fv = fract(uv * 380.0);
    var col = vec3<f32>(0.008, 0.006, 0.025);
    let h = hash21(gv);
    if h > 0.988 {
        let b = 0.4 + 0.6 * hash21(gv + 17.0);
        let tw = 0.75 + 0.25 * sin(t * 2.5 + h * 40.0);
        let d = length(fv - 0.5);
        let s = smoothstep(0.4, 0.0, d) * b * tw;
        col += vec3<f32>(0.9, 0.92, 1.0) * s;
    }
    return col;
}

fn aspect_pos(uv: vec2<f32>) -> vec2<f32> {
    let c = uv - vec2(0.5, 0.5);
    return vec2(c.x * u.aspect, c.y);
}

fn lens_uv(uv: vec2<f32>) -> vec2<f32> {
    let p = aspect_pos(uv);
    let r = length(p);
    let warp = u.strength * 0.12 / pow(r + u.disk_radius * 0.35, 1.4);
    let scale = 1.0 + warp;
    return vec2(0.5, 0.5) + vec2(p.x * scale / u.aspect, p.y * scale);
}

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time;
    let base = textureSample(input_tex, input_sampler, uv);

    let star_uv = lens_uv(uv);
    var sky = stars(star_uv, t);

    let p = aspect_pos(uv);
    let r = length(p);
    let a = atan2(p.y, p.x);
    let disk_r = u.disk_radius;

    let ring_r = disk_r * 1.12;
    let ring_w = disk_r * 0.08;
    let ring = smoothstep(ring_w, 0.0, abs(r - ring_r)) * 0.35;
    sky += vec3<f32>(0.75, 0.45, 0.18) * ring;

    let disk_in = disk_r * 0.72;
    let disk_out = disk_r * 1.45;
    if r > disk_in && r < disk_out {
        let mid = (disk_in + disk_out) * 0.5;
        let half = (disk_out - disk_in) * 0.5;
        let band = 1.0 - smoothstep(0.0, 1.0, abs(r - mid) / half);
        let spin_angle = a - t * 1.6;
        let doppler = 0.55 + 0.45 * cos(spin_angle);
        let turbulence = 0.85 + 0.15 * sin(a * 2.0 + r * 28.0 - t * 3.0)
            + 0.08 * hash21(vec2(a * 4.0, r * 30.0 + t));
        let heat = pow(band, 0.55) * doppler * turbulence;
        let inner = smoothstep(disk_r * 1.5, disk_r * 0.85, r);
        let hot = vec3<f32>(0.95 + 0.05 * inner, 0.22 + 0.7 * heat * inner, 0.04 + 0.5 * heat);
        sky = mix(sky, hot, clamp(heat, 0.0, 1.0) * u.strength);
    }

    let pull = smoothstep(disk_r * 2.0, disk_r * 0.35, r);
    sky *= 1.0 - pull * 0.9 * u.strength;

    let hole = smoothstep(disk_r * 0.55, disk_r * 0.22, r);
    sky = mix(sky, vec3<f32>(0.0), hole);

    let out_rgb = mix(base.rgb, sky, 0.92);
    return vec4(out_rgb, 1.0);
}