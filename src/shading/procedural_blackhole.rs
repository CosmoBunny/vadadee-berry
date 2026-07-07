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