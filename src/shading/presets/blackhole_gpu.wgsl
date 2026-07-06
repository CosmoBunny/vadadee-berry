struct Uniforms {
    time: f32,
    strength: f32,
    disk_radius: f32,
    aspect: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

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

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn stars(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let gv = floor(uv * 420.0);
    let fv = fract(uv * 420.0);
    var col = vec3<f32>(0.01, 0.008, 0.03);
    let h = hash21(gv);
    if h > 0.992 {
        let b = 0.35 + 0.65 * hash21(gv + 17.0);
        let tw = 0.7 + 0.3 * sin(t * 3.0 + h * 40.0);
        let d = length(fv - 0.5);
        let s = smoothstep(0.35, 0.0, d) * b * tw;
        col += vec3<f32>(0.85, 0.9, 1.0) * s;
    }
    return col;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = u.time;
    let center = vec2<f32>(0.5, 0.5);
    var p = uv - center;
    p.x *= u.aspect;
    let r = length(p);
    let a = atan2(p.y, p.x);

    var sky = stars(uv, t);

    let ring_r = u.disk_radius * 1.05;
    let ring = smoothstep(0.02, 0.0, abs(r - ring_r)) * 0.55;
    sky += vec3<f32>(0.55, 0.28, 0.12) * ring;

    let disk_in = u.disk_radius * 0.55;
    let disk_out = u.disk_radius * 1.25;
    if r > disk_in && r < disk_out {
        let band = 1.0 - abs(r - (disk_in + disk_out) * 0.5) / ((disk_out - disk_in) * 0.5);
        let spin = 0.5 + 0.5 * sin(a * 6.0 + t * 1.8);
        let heat = pow(band, 0.65) * spin;
        let hot = mix(vec3<f32>(0.9, 0.25, 0.05), vec3<f32>(1.0, 0.92, 0.55), heat);
        sky = mix(sky, hot, heat * u.strength);
    }

    let pull = smoothstep(u.disk_radius * 1.6, u.disk_radius * 0.25, r);
    sky *= 1.0 - pull * 0.85 * u.strength;

    let hole = smoothstep(u.disk_radius * 0.42, u.disk_radius * 0.18, r);
    sky = mix(sky, vec3<f32>(0.0), hole);

    return vec4<f32>(sky, 1.0);
}