//! CPU mirror of `presets/blackhole.wgsl` (until wgpu pass is wired).

pub struct BlackholeParams {
    pub time: f32,
    pub strength: f32,
    pub disk_radius: f32,
    /// Page width / height — keeps the hole circular on non-square pages.
    pub aspect: f32,
}

impl Default for BlackholeParams {
    fn default() -> Self {
        Self {
            time: 0.0,
            strength: 0.95,
            disk_radius: 0.22,
            aspect: 1.0,
        }
    }
}

fn hash21(p: (f32, f32)) -> f32 {
    let v = (p.0 * 127.1 + p.1 * 311.7).sin() * 43758.5453;
    v - v.floor()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Aspect-correct position from UV (0..1, origin top-left).
fn aspect_pos(uv: (f32, f32), aspect: f32) -> (f32, f32) {
    ((uv.0 - 0.5) * aspect, uv.1 - 0.5)
}

fn lens_uv(uv: (f32, f32), aspect: f32, disk_r: f32, strength: f32) -> (f32, f32) {
    let p = aspect_pos(uv, aspect);
    let r = (p.0 * p.0 + p.1 * p.1).sqrt();
    let warp = strength * 0.12 / (r + disk_r * 0.35).powf(1.4);
    let scale = 1.0 + warp;
    (
        0.5 + p.0 * scale / aspect,
        0.5 + p.1 * scale,
    )
}

fn stars(uv: (f32, f32), t: f32) -> [f32; 3] {
    let gx = (uv.0 * 380.0).floor();
    let gy = (uv.1 * 380.0).floor();
    let fx = uv.0 * 380.0 - gx;
    let fy = uv.1 * 380.0 - gy;
    let mut col = [0.008_f32, 0.006, 0.025];
    let h = hash21((gx, gy));
    if h > 0.988 {
        let b = 0.4 + 0.6 * hash21((gx + 17.0, gy));
        let tw = 0.75 + 0.25 * (t * 2.5 + h * 40.0).sin();
        let d = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
        let s = (1.0 - smoothstep(0.0, 0.4, d)) * b * tw;
        col[0] += 0.9 * s;
        col[1] += 0.92 * s;
        col[2] += 1.0 * s;
    }
    col
}

fn disk_color(a: f32, r: f32, disk_r: f32, t: f32) -> f32 {
    let disk_in = disk_r * 0.72;
    let disk_out = disk_r * 1.45;
    if r < disk_in || r > disk_out {
        return 0.0;
    }
    let mid = (disk_in + disk_out) * 0.5;
    let half = (disk_out - disk_in) * 0.5;
    let band = 1.0 - smoothstep(0.0, 1.0, (r - mid).abs() / half);
    // Rotating Doppler beaming (one bright side), not 6-fold lobes.
    let spin_angle = a - t * 1.6;
    let doppler = 0.55 + 0.45 * spin_angle.cos();
    let turbulence = 0.85
        + 0.15 * (a * 2.0 + r * 28.0 - t * 3.0).sin()
        + 0.08 * hash21((a * 4.0, r * 30.0 + t));
    (band.powf(0.55) * doppler * turbulence).clamp(0.0, 1.0)
}

/// UV in 0..1 (origin top-left like egui UV for our grid).
pub fn sample(uv: (f32, f32), u: &BlackholeParams) -> [u8; 3] {
    let aspect = u.aspect.max(0.25);
    let star_uv = lens_uv(uv, aspect, u.disk_radius, u.strength);
    let mut sky = stars(star_uv, u.time);

    let p = aspect_pos(uv, aspect);
    let r = (p.0 * p.0 + p.1 * p.1).sqrt();
    let a = p.1.atan2(p.0);
    let t = u.time;
    let disk_r = u.disk_radius;

    // Soft photon-ring glow (broad, not a hard circle).
    let ring_r = disk_r * 1.12;
    let ring_w = disk_r * 0.08;
    let ring = (1.0 - smoothstep(ring_w, 0.0, (r - ring_r).abs())) * 0.35;
    sky[0] += 0.75 * ring;
    sky[1] += 0.45 * ring;
    sky[2] += 0.18 * ring;

    let heat = disk_color(a, r, disk_r, t);
    if heat > 0.0 {
        let inner = smoothstep(disk_r * 1.5, disk_r * 0.85, r);
        let hot = [
            0.95 + 0.05 * inner,
            0.22 + 0.7 * heat * inner,
            0.04 + 0.5 * heat,
        ];
        let mix_k = heat * u.strength;
        sky[0] = sky[0] * (1.0 - mix_k) + hot[0] * mix_k;
        sky[1] = sky[1] * (1.0 - mix_k) + hot[1] * mix_k;
        sky[2] = sky[2] * (1.0 - mix_k) + hot[2] * mix_k;
    }

    // Gravitational dimming toward center.
    let pull = smoothstep(disk_r * 2.0, disk_r * 0.35, r);
    let dark = 1.0 - pull * 0.9 * u.strength;
    sky[0] *= dark;
    sky[1] *= dark;
    sky[2] *= dark;

    // Event horizon.
    let hole = smoothstep(disk_r * 0.55, disk_r * 0.22, r);
    sky[0] *= 1.0 - hole;
    sky[1] *= 1.0 - hole;
    sky[2] *= 1.0 - hole;

    [
        (sky[0].clamp(0.0, 1.0) * 255.0) as u8,
        (sky[1].clamp(0.0, 1.0) * 255.0) as u8,
        (sky[2].clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

/// Pure starfield renderer — no gravitational distortion, just twinkling stars.
/// `uv` is 0..1 (origin top-left), `time_secs` drives the twinkle animation.
/// `aspect` keeps star-grid cells square on non-square pages (page_w / page_h).
pub fn sample_starfield(uv: (f32, f32), time_secs: f32, aspect: f32) -> [u8; 3] {
    // Undo aspect so the cell grid covers the page without stretching.
    let u_scaled = uv.0 * aspect.max(0.25);
    let rgb = stars((u_scaled, uv.1), time_secs);
    [
        (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

fn noise2(p: (f32, f32)) -> f32 {
    let i = (p.0.floor(), p.1.floor());
    let f = (p.0 - i.0, p.1 - i.1);
    let u = (f.0 * f.0 * (3.0 - 2.0 * f.0), f.1 * f.1 * (3.0 - 2.0 * f.1));
    let a = hash21(i);
    let b = hash21((i.0 + 1.0, i.1));
    let c = hash21((i.0, i.1 + 1.0));
    let d = hash21((i.0 + 1.0, i.1 + 1.0));
    a * (1.0 - u.0) * (1.0 - u.1)
        + b * u.0 * (1.0 - u.1)
        + c * (1.0 - u.0) * u.1
        + d * u.0 * u.1
}

fn fbm2(mut p: (f32, f32)) -> f32 {
    let mut v = 0.0;
    let mut a = 0.5;
    for _ in 0..5 {
        v += a * noise2(p);
        p = (p.0 * 2.02 + 17.0, p.1 * 2.02 + 17.0);
        a *= 0.5;
    }
    v
}

/// CPU mirror of the procedural galaxy + emitting stars WGSL (export / snapshot).
/// Live look is meant to be driven by GPU WGSL via MCP / Custom pass — this is fallback only.
pub fn sample_galaxy(uv: (f32, f32), time_secs: f32, aspect: f32) -> [u8; 3] {
    let mut p = (uv.0 * 2.0 - 1.0, uv.1 * 2.0 - 1.0);
    p.0 *= aspect.max(0.25);
    let ang = time_secs * 0.05;
    let (cs, sn) = (ang.cos(), ang.sin());
    let q = (cs * p.0 - sn * p.1, sn * p.0 + cs * p.1);
    let r = (q.0 * q.0 + q.1 * q.1).sqrt();
    let a = q.1.atan2(q.0);
    let arms = 0.5 + 0.5 * (a * 2.0 + r * 6.0 - time_secs * 0.4).cos();
    let dens = fbm2((q.0 * 1.8 + time_secs * 0.03, q.1 * 1.8));
    let core = (-r * r * 2.2).exp();
    let disk = (-r * 1.1).exp() * (0.35 + 0.65 * dens) * (0.4 + 0.6 * arms);

    let purple = (0.25, 0.08, 0.45);
    let pink = (0.55, 0.15, 0.35);
    let blue = (0.1, 0.2, 0.55);
    let gold = (0.9, 0.75, 0.45);
    let mut col = (
        purple.0 * dens + pink.0 * arms * dens + blue.0 * (1.0 - dens) * 0.4,
        purple.1 * dens + pink.1 * arms * dens + blue.1 * (1.0 - dens) * 0.4,
        purple.2 * dens + pink.2 * arms * dens + blue.2 * (1.0 - dens) * 0.4,
    );
    col.0 += gold.0 * core * 1.4 + disk * 0.35;
    col.1 += gold.1 * core * 1.4 + disk * 0.25;
    col.2 += gold.2 * core * 1.4 + disk * 0.55;

    let n = noise2((p.0 * 3.0, p.1 * 3.0));
    let bg = (0.02 * (0.6 + 0.4 * n), 0.01 * (0.6 + 0.4 * n), 0.06 * (0.6 + 0.4 * n));
    let m = smoothstep(1.6, 0.15, r);
    col = (
        bg.0 + (col.0 - bg.0) * m,
        bg.1 + (col.1 - bg.1) * m,
        bg.2 + (col.2 - bg.2) * m,
    );

    let st = stars((uv.0 * aspect.max(0.25), uv.1), time_secs);
    let st2 = stars((uv.0 * 1.7 + 10.0, uv.1 * 1.7), time_secs * 1.3);
    let s = (st[0] + st2[0] * 0.75).min(2.0);
    col.0 += s * 0.95;
    col.1 += s * 0.95;
    col.2 += s * 1.05;

    let vig = smoothstep(1.4, 0.35, r);
    col.0 *= vig;
    col.1 *= vig;
    col.2 *= vig;

    [
        (col.0.clamp(0.0, 1.0) * 255.0) as u8,
        (col.1.clamp(0.0, 1.0) * 255.0) as u8,
        (col.2.clamp(0.0, 1.0) * 255.0) as u8,
    ]
}