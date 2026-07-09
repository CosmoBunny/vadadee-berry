// Raymarched Schwarzschild black hole (single-pass, performance-tuned).
// Procedural only (uniform binding 0) — avoids full-document CPU raster every frame.
// uniforms: [0] time  [1] strength  [2] disk scale  [3] aspect (runtime)

struct Uniforms {
    time: f32,
    strength: f32,
    disk_radius: f32,
    aspect: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

const RS: f32 = 1.0;
const HORIZON: f32 = 1.02;
const ESCAPE: f32 = 40.0;
// Cap ray budget — zoom AA / 112 steps / 14 volume samples melted FPS.
const MAX_STEPS: i32 = 56;

fn hash13(q: vec3<f32>) -> f32 {
    var p = fract(q * vec3(0.1031, 0.1107, 0.0973));
    p += dot(p, p.yzx + 19.19);
    return fract((p.x + p.y) * p.z);
}

fn fade(t: vec3<f32>) -> vec3<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

fn vnoise(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let s = fade(f);
    let n000 = hash13(i);
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

// 3-octave fbm only
fn fbm3(x: vec3<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var p = x;
    v += a * vnoise(p); p = p * 2.02 + vec3(11.3, 7.7, 3.1); a *= 0.5;
    v += a * vnoise(p); p = p * 2.02 + vec3(11.3, 7.7, 3.1); a *= 0.5;
    v += a * vnoise(p);
    return v / 0.875;
}

// Cheap soft density (2 noise taps, no harsh power curves)
fn cloud_soft(p: vec3<f32>) -> f32 {
    let n = vnoise(p) * 0.65 + vnoise(p * 2.1 + 3.0) * 0.35;
    return smoothstep(0.28, 0.72, n);
}

fn blackbody(T: f32) -> vec3<f32> {
    let t = clamp(T, 1200.0, 16000.0) / 1000.0;
    var r = clamp(1.29 * pow(t, -1.0) + 0.3, 0.0, 1.0);
    var g = clamp(1.13 * pow(t, -0.75) - 0.15, 0.0, 1.0);
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
    return col / max(dot(col, vec3(0.2126, 0.7152, 0.0722)), 1e-3);
}

fn starfield(dir: vec3<f32>) -> vec3<f32> {
    var col = vec3(0.0);
    var d = normalize(dir);
    var scale = 140.0;
    // 2 layers only
    for (var k = 0; k < 2; k++) {
        let pos = d * scale;
        let cell = floor(pos);
        let h = hash13(cell);
        if h > 0.988 {
            let local = fract(pos) - 0.5;
            let bright = pow(smoothstep(0.22, 0.0, length(local)), 1.6);
            let tint = mix(vec3(1.0, 0.82, 0.62), vec3(0.7, 0.84, 1.0), hash13(cell + 5.3));
            col += bright * tint * (0.35 + 0.65 * fract(h * 113.0));
        }
        d = d.yzx * 1.65 + 3.7;
        scale *= 2.1;
    }
    let bn = normalize(vec3(0.55, 0.72, 0.4));
    let band = exp(-pow(dot(normalize(dir), bn), 2.0) * 9.0);
    col += vec3(0.03, 0.05, 0.1) * band * 0.7;
    return col * 0.85;
}

fn accel(pos: vec3<f32>, h2: f32) -> vec3<f32> {
    let r2 = max(dot(pos, pos), 1e-6);
    let r = sqrt(r2);
    return -1.5 * h2 * pos / (r2 * r2 * r);
}

struct Ray {
    pos: vec3<f32>,
    vel: vec3<f32>,
}

// RK2 midpoint
fn step_ray(rin: Ray, h2: f32, dt: f32) -> Ray {
    let a1 = accel(rin.pos, h2);
    let mid_p = rin.pos + rin.vel * (dt * 0.5);
    let mid_v = rin.vel + a1 * (dt * 0.5);
    let a2 = accel(mid_p, h2);
    var o: Ray;
    o.pos = rin.pos + mid_v * dt;
    o.vel = rin.vel + a2 * dt;
    return o;
}

fn disk_gfactor(pos: vec3<f32>, photon_dir: vec3<f32>) -> f32 {
    let rho = max(length(pos.xz), HORIZON + 1e-3);
    let beta = clamp(sqrt(0.5 / rho), 0.0, 0.92);
    let gamma = inverseSqrt(max(1e-4, 1.0 - beta * beta));
    let tangent = normalize(vec3(-pos.z, 0.0, pos.x));
    let doppler = 1.0 / max(1e-3, gamma * (1.0 - beta * dot(tangent, -photon_dir)));
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
    let vfall = exp(-yr * yr * 2.4);
    if vfall < 0.02 {
        return vec4(0.0);
    }

    let ang = atan2(pos.z, pos.x);
    let spiral = ang + t * 1.05 - log(max(rho, 1e-3)) * 2.0;
    let q = vec3(rho * 0.5, spiral * 0.16, pos.y * 0.8);
    let macro_n = cloud_soft(q * 0.5 + 8.0);
    let fine_n = cloud_soft(q + 2.5);
    var dens = mix(0.4, 1.0, macro_n) * mix(0.8, 1.1, fine_n);
    dens = smoothstep(0.18, 0.88, dens);
    dens *= pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 1.35) * vfall;
    dens *= smoothstep(0.0, 0.1, edge) * smoothstep(1.0, 0.8, edge);
    if dens < 0.006 {
        return vec4(0.0);
    }

    let g = disk_gfactor(pos, photon_dir);
    let T = clamp(
        (2800.0 + 5200.0 * pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 0.9))
            * pow(max(g, 1e-3), 0.85),
        1400.0,
        14000.0,
    );
    var emis = blackbody(T);
    emis *= (0.3 + 2.8 * pow(clamp(din / max(rho, 1e-3), 0.0, 1.0), 1.15)) * pow(max(g, 1e-3), 1.25);
    let core = exp(-pow((rho - din) / max(din * 0.5, 1e-3), 2.0));
    emis += vec3(1.0, 0.45, 0.14) * core * 1.5;
    emis *= 0.78 + 0.22 * macro_n;
    return vec4(emis * dens, clamp(dens * 1.9, 0.0, 1.0));
}

fn aces(x: f32) -> f32 {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let peak = max(max(c.r, c.g), max(c.b, 1e-5));
    var ratio = c / peak;
    ratio = mix(ratio, vec3(1.0), clamp((peak - 1.4) / 3.2, 0.0, 0.8));
    return ratio * aces(peak);
}

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time;
    let strength = clamp(u.strength, 0.15, 1.5);
    let disk_scale = clamp(u.disk_radius, 0.08, 0.55);
    let aspect = max(u.aspect, 0.25);

    let cam_yaw = 0.35 + sin(t * 0.08) * 0.12;
    let cam_pitch = -0.38 + sin(t * 0.05) * 0.04;
    let cy = cos(cam_yaw);
    let sy = sin(cam_yaw);
    let cp = cos(cam_pitch);
    let sp = sin(cam_pitch);
    let forward = normalize(vec3(sy * cp, sp, cy * cp));
    let right = normalize(cross(forward, vec3(0.0, 1.0, 0.0)));
    let upv = cross(right, forward);
    let ro = -forward * 14.5 + vec3(0.0, 0.35, 0.0);

    let ndc = (uv * 2.0 - 1.0) * vec2(aspect, -1.0);
    let rd0 = normalize(forward + right * ndc.x * 0.95 + upv * ndc.y * 0.95);

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
        if alpha > 0.96 {
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

        // Larger steps far out = fewer iterations overall
        let dt = clamp(0.12 * (r - 0.75), 0.025, 1.0);
        let next = step_ray(ray, h2, dt);
        let seg = next.pos - ray.pos;
        let seglen = length(seg);
        if seglen > 1e-6 {
            let nd = seg / seglen;
            turn += acos(clamp(dot(photon_dir, nd), -1.0, 1.0));
            photon_dir = nd;
        }

        let rho_a = length(ray.pos.xz);
        let rho_b = length(next.pos.xz);
        let near = max(rho_a, rho_b) > din - 0.6
            && min(rho_a, rho_b) < dout + 0.6
            && (abs(ray.pos.y) < thick * 2.2 + seglen || ray.pos.y * next.pos.y < 0.0);
        if near {
            // Max 4 volume samples per segment (was up to 14)
            let subs = clamp(i32(seglen / 0.1) + 1, 1, 4);
            let inv = 1.0 / f32(subs);
            for (var s = 0; s < subs; s++) {
                if alpha > 0.96 {
                    break;
                }
                let sp = mix(ray.pos, next.pos, (f32(s) + 0.5) * inv);
                let smp = disk_emit(sp, photon_dir, t, din, dout, thick);
                if smp.a > 0.0 {
                    let a = clamp(smp.a * seglen * inv * 1.6 * strength, 0.0, 1.0);
                    let tr = 1.0 - alpha;
                    col += smp.rgb * seglen * inv * 1.3 * vec3(tr, pow(tr, 1.3), pow(tr, 1.5)) * strength;
                    alpha += (1.0 - alpha) * a;
                }
            }
        }
        ray = next;
    }

    if !captured {
        let glow = smoothstep(1.2, 5.0, turn) * (0.55 + 0.4 * strength);
        if glow > 0.002 {
            let gc = mix(vec3(1.0, 0.35, 0.12), vec3(1.0, 0.7, 0.42), smoothstep(2.8, 6.0, turn));
            col += glow * gc * (1.0 - alpha) * 0.9;
        }
    }
    if !captured && alpha < 0.98 {
        col += starfield(photon_dir) * (1.0 - alpha);
    }

    let bloom = max(col - vec3(0.7), vec3(0.0));
    col += bloom * 0.28 * strength;
    col = tonemap(col * (0.85 + 0.4 * strength));

    let q = uv - 0.5;
    col *= 1.0 - dot(q, q) * 0.48;
    return vec4(col, 1.0);
}
