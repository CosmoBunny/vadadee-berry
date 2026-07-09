// Raymarched Schwarzschild black hole (procedural, own vertex stage).
// See blackhole.wgsl for notes.

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

const PI: f32 = 3.14159265359;
const RS: f32 = 1.0;
const HORIZON: f32 = 1.02;
const ESCAPE: f32 = 48.0;
const MAX_STEPS: i32 = 96;

fn hash13(q: vec3<f32>) -> f32 {
    var p = fract(q * vec3(0.1031, 0.1107, 0.0973));
    p += dot(p, p.yzx + 19.19);
    return fract((p.x + p.y) * p.z);
}

fn vnoise(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let s = f * f * (3.0 - 2.0 * f);
    let n000 = hash13(i + vec3(0.0, 0.0, 0.0));
    let n100 = hash13(i + vec3(1.0, 0.0, 0.0));
    let n010 = hash13(i + vec3(0.0, 1.0, 0.0));
    let n110 = hash13(i + vec3(1.0, 1.0, 0.0));
    let n001 = hash13(i + vec3(0.0, 0.0, 1.0));
    let n101 = hash13(i + vec3(1.0, 0.0, 1.0));
    let n011 = hash13(i + vec3(0.0, 1.0, 1.0));
    let n111 = hash13(i + vec3(1.0, 1.0, 1.0));
    return mix(
        mix(mix(n000, n100, s.x), mix(n010, n110, s.x), s.y),
        mix(mix(n001, n101, s.x), mix(n011, n111, s.x), s.y),
        s.z
    );
}

fn fbm(x: vec3<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var p = x;
    for (var i = 0; i < 4; i++) {
        v += a * vnoise(p);
        p = p * 2.02 + vec3(11.3, 7.7, 3.1);
        a *= 0.5;
    }
    return v;
}

fn cloud_noise(p_in: vec3<f32>) -> f32 {
    var acc = 1.0;
    var freq = 1.0;
    for (var i = 0; i < 4; i++) {
        acc *= 1.0 + 0.12 * (vnoise(p_in * freq) * 2.0 - 1.0);
        freq *= 2.7;
    }
    return log(1.0 + pow(max(0.0, acc), 28.0));
}

fn blackbody(T: f32) -> vec3<f32> {
    let t = clamp(T, 1200.0, 16000.0) / 1000.0;
    var r = clamp(1.292936 * pow(t, -1.0) + 0.3, 0.0, 1.0);
    var g = clamp(1.12989 * pow(t, -0.75) - 0.15, 0.0, 1.0);
    var b = clamp(1.5 * exp(-2.2 / t) * (t - 1.1), 0.0, 1.0);
    if t < 2.0 {
        r = 1.0;
        g = clamp(0.35 + (t - 1.2) * 0.55, 0.0, 1.0);
        b = clamp((t - 1.4) * 0.4, 0.0, 0.35);
    } else if t > 6.5 {
        r = clamp(1.4 - (t - 6.5) * 0.08, 0.55, 1.0);
        g = clamp(0.95 - (t - 6.5) * 0.02, 0.7, 1.0);
        b = 1.0;
    }
    let col = vec3(r, g, b);
    let y = dot(col, vec3(0.2126, 0.7152, 0.0722));
    return col / max(y, 1e-3);
}

fn starfield(dir: vec3<f32>) -> vec3<f32> {
    var col = vec3(0.0);
    var d = normalize(dir);
    var scale = 180.0;
    for (var k = 0; k < 3; k++) {
        let pos = d * scale;
        let cell = floor(pos);
        let h = hash13(cell);
        if h > 0.985 {
            let local = fract(pos) - 0.5;
            let dist = length(local);
            let bright = pow(smoothstep(0.18, 0.0, dist), 2.2);
            let tone = hash13(cell + 5.3);
            let tint = mix(vec3(1.0, 0.82, 0.62), vec3(0.7, 0.84, 1.0), tone);
            col += bright * tint * (0.35 + 0.75 * fract(h * 113.0));
        }
        d = d.yzx * 1.65 + 3.7;
        scale *= 2.05;
    }
    let bn = normalize(vec3(0.55, 0.72, 0.4));
    let warp = fbm(normalize(dir) * 2.0 + 4.0) - 0.5;
    let band = exp(-pow(dot(normalize(dir), bn) + 0.2 * warp, 2.0) * 10.0);
    col += vec3(0.04, 0.06, 0.12) * band * (0.4 + 0.6 * fbm(dir * 3.0 + 9.0));
    return col * 0.85;
}

fn accel(pos: vec3<f32>, h2: f32) -> vec3<f32> {
    let r2 = dot(pos, pos);
    let r = sqrt(r2);
    let r5 = max(r2 * r2 * r, 1e-6);
    return -1.5 * h2 * pos / r5;
}

struct Ray {
    pos: vec3<f32>,
    vel: vec3<f32>,
}

fn rk2(rin: Ray, h2: f32, dt: f32) -> Ray {
    let a1 = accel(rin.pos, h2);
    let mid_pos = rin.pos + rin.vel * (dt * 0.5);
    let mid_vel = rin.vel + a1 * (dt * 0.5);
    let a2 = accel(mid_pos, h2);
    var out: Ray;
    out.pos = rin.pos + mid_vel * dt;
    out.vel = rin.vel + a2 * dt;
    return out;
}

fn disk_gfactor(pos: vec3<f32>, photon_dir: vec3<f32>) -> f32 {
    let rho = max(length(pos.xz), HORIZON + 1e-3);
    let beta = clamp(sqrt(0.5 / rho), 0.0, 0.92);
    let gamma = inverseSqrt(max(1e-4, 1.0 - beta * beta));
    let tangent = normalize(vec3(-pos.z, 0.0, pos.x));
    let cos_a = dot(tangent, -photon_dir);
    let doppler = 1.0 / max(1e-3, gamma * (1.0 - beta * cos_a));
    let grav = sqrt(max(1e-3, 1.0 - RS / rho));
    return doppler * grav;
}

fn disk_emit(pos: vec3<f32>, photon_dir: vec3<f32>, t: f32, din: f32, dout: f32, thick: f32) -> vec4<f32> {
    let rho = length(pos.xz);
    if rho < din || rho > dout {
        return vec4(0.0);
    }
    let edge = (rho - din) / max(1e-3, dout - din);
    let half_h = thick * (0.4 + 1.1 * (1.0 - edge));
    let yr = pos.y / max(1e-3, half_h);
    let vfall = exp(-yr * yr * 2.6);
    if vfall < 0.02 {
        return vec4(0.0);
    }

    let ang = atan2(pos.z, pos.x);
    let spiral = ang + t * 1.15 - log(max(rho, 1e-3)) * 2.2;
    var q = vec3(rho * 0.85, spiral * 0.28, pos.y * 1.2) * 1.35;
    let n = cloud_noise(q) * (0.55 + 0.7 * cloud_noise(q * 2.4 + 17.0));
    var dens = smoothstep(0.28, 0.95, n);
    dens *= pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 1.45) * vfall;
    if dens < 0.005 {
        return vec4(0.0);
    }

    let g = disk_gfactor(pos, photon_dir);
    let T_radial = 2800.0 + 5200.0 * pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 0.9);
    let T_obs = clamp(T_radial * pow(max(g, 1e-3), 0.85) * mix(0.85, 1.25, dens), 1400.0, 14000.0);
    var emis = blackbody(T_obs);
    let beam = pow(max(g, 1e-3), 1.35);
    emis *= (0.25 + 3.2 * pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 1.2)) * beam;
    let core = exp(-pow((rho - din) / max(din * 0.45, 1e-3), 2.0));
    emis += vec3(1.0, 0.42, 0.12) * core * 1.8;
    let alpha = clamp(dens * 2.4, 0.0, 1.0);
    return vec4(emis * dens, alpha);
}

fn aces(x: f32) -> f32 {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let peak = max(max(c.r, c.g), max(c.b, 1e-5));
    var ratio = c / peak;
    let desat = clamp((peak - 1.4) / 3.2, 0.0, 0.8);
    ratio = mix(ratio, vec3(1.0), desat);
    return ratio * aces(peak);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = u.time;
    let strength = clamp(u.strength, 0.15, 1.5);
    let disk_scale = clamp(u.disk_radius, 0.08, 0.55);
    let aspect = max(u.aspect, 0.25);

    let cam_yaw = 0.35 + sin(t * 0.08) * 0.12;
    let cam_pitch = -0.38 + sin(t * 0.05) * 0.04;
    let cam_dist = 14.5;
    let cy = cos(cam_yaw);
    let sy = sin(cam_yaw);
    let cp = cos(cam_pitch);
    let sp = sin(cam_pitch);
    let forward = normalize(vec3(sy * cp, sp, cy * cp));
    let right = normalize(cross(forward, vec3(0.0, 1.0, 0.0)));
    let upv = cross(right, forward);
    let ro = -forward * cam_dist + vec3(0.0, 0.35, 0.0);

    var ndc = (uv * 2.0 - 1.0) * vec2(aspect, -1.0);
    let fov = 0.95;
    let rd0 = normalize(forward + right * ndc.x * fov + upv * ndc.y * fov);

    let din = 2.2 * (0.7 + disk_scale);
    let dout = din * (2.4 + strength * 0.4);
    let thick = 0.18 + disk_scale * 0.35;

    let h2 = dot(cross(ro, rd0), cross(ro, rd0));
    var ray: Ray;
    ray.pos = ro;
    ray.vel = rd0;

    var col = vec3(0.0);
    var alpha = 0.0;
    var photon_dir = rd0;
    var turn = 0.0;
    var captured = false;

    for (var i = 0; i < MAX_STEPS; i++) {
        if alpha > 0.97 {
            break;
        }
        let r = length(ray.pos);
        if r < HORIZON {
            captured = true;
            break;
        }
        if r > ESCAPE && dot(ray.pos, ray.vel) > 0.0 {
            break;
        }

        let dt = clamp(0.11 * (r - 0.85), 0.02, 0.85);
        let next = rk2(ray, h2, dt);
        let seg = next.pos - ray.pos;
        let seglen = length(seg);
        if seglen > 1e-6 {
            let nd = seg / seglen;
            turn += acos(clamp(dot(photon_dir, nd), -1.0, 1.0));
            photon_dir = nd;
        }

        let rho_a = length(ray.pos.xz);
        let rho_b = length(next.pos.xz);
        let near = max(rho_a, rho_b) > din - 0.8
            && min(rho_a, rho_b) < dout + 0.8
            && (abs(ray.pos.y) < thick * 2.5 + seglen || ray.pos.y * next.pos.y < 0.0);
        if near {
            let subs = clamp(i32(seglen / 0.08) + 1, 1, 8);
            let inv = 1.0 / f32(subs);
            for (var s = 0; s < subs; s++) {
                if alpha > 0.97 {
                    break;
                }
                let ft = (f32(s) + 0.5) * inv;
                let sp = mix(ray.pos, next.pos, ft);
                let smp = disk_emit(sp, photon_dir, t, din, dout, thick);
                if smp.a > 0.0 {
                    let a = clamp(smp.a * seglen * inv * 1.7 * strength, 0.0, 1.0);
                    let tr = 1.0 - alpha;
                    let transm = vec3(tr, pow(tr, 1.35), pow(tr, 1.7));
                    col += smp.rgb * seglen * inv * 1.4 * transm * strength;
                    alpha += (1.0 - alpha) * a;
                }
            }
        }

        ray = next;
    }

    if !captured {
        let glow = smoothstep(1.2, 5.5, turn) * (0.55 + 0.45 * strength);
        if glow > 0.001 {
            let glow_col = mix(vec3(1.0, 0.35, 0.12), vec3(1.0, 0.7, 0.42), smoothstep(3.0, 7.0, turn));
            col += glow * glow_col * (1.0 - alpha) * 0.95;
        }
    }

    if !captured && alpha < 0.99 {
        col += starfield(photon_dir) * (1.0 - alpha);
    }

    let bloom = max(col - vec3(0.65), vec3(0.0));
    col += bloom * 0.35 * strength;
    col = tonemap(col * (0.85 + 0.45 * strength));
    let q = uv - 0.5;
    col *= 1.0 - dot(q, q) * 0.5;

    return vec4(col, 1.0);
}
