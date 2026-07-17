//! CPU export raster for carbon hex-chain (matches user hex_gv / hex_dist WGSL).
//! Worker-thread only — no shared wgpu. Half-res + NN upscale for export speed.
//! Sequential (no rayon): export x264 already wants the cores; parallel thrash → ~1 fps.

use crate::document::ShadingPass;

#[inline]
fn hash21(px: f32, py: f32) -> f32 {
    let n = px * 127.1 + py * 311.7;
    (n.sin() * 43758.5453).fract().abs()
}

#[inline]
fn hex_dist(px: f32, py: f32) -> f32 {
    let qx = px.abs();
    let qy = py.abs();
    let d = qx * 0.5 + qy * 0.8660254;
    d.max(qx)
}

#[inline]
fn hex_gv(px: f32, py: f32) -> (f32, f32) {
    let rx = 1.0_f32;
    let ry = 1.7320508_f32;
    let hx = rx * 0.5;
    let hy = ry * 0.5;
    let ax = px - rx * (px / rx).floor() - hx;
    let ay = py - ry * (py / ry).floor() - hy;
    let bx = (px - hx) - rx * ((px - hx) / rx).floor() - hx;
    let by = (py - hy) - ry * ((py - hy) / ry).floor() - hy;
    if bx * bx + by * by < ax * ax + ay * ay {
        (bx, by)
    } else {
        (ax, ay)
    }
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// True when WGSL is the hexagonal-chain family (safe for this CPU path).
pub fn is_hex_chain_wgsl(pass: &ShadingPass) -> bool {
    let w = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl);
    let n = pass.name.to_ascii_lowercase();
    n.contains("hex")
        || w.contains("fn hex_gv")
        || w.contains("hexagonal chain")
        || w.contains("2D hexagonal chain")
}

/// Fill `rgba` at native resolution (tight RGBA8). Prefer [`fill_hex_chain_rgba_export`].
pub fn fill_hex_chain_rgba(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    time_secs: f32,
    glow: f32,
) {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return;
    }
    let aspect = (width as f32 / (height as f32).max(1.0)).max(0.25);
    let mut gstr = if glow < 0.001 { 0.85 } else { glow };
    gstr = gstr.clamp(0.0, 2.0);
    let t = time_secs;
    let scale = 12.0_f32;
    let lw = 0.04_f32;

    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        let row = y * w * 4;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let px = (u - 0.5) * aspect;
            let py = v - 0.5;

            let weave = (px * 160.0).sin().abs() * (py * 160.0 + 0.35).sin().abs();
            let grain = hash21((px * 90.0).floor(), (py * 90.0).floor()) * 0.02;
            let mut cr = 0.02 + 0.03 * weave * 0.35 + grain;
            let mut cg = 0.022 + 0.03 * weave * 0.35 + grain;
            let mut cb = 0.025 + 0.03 * weave * 0.35 + grain;

            let (gv_x, gv_y) = hex_gv(px * scale, py * scale);
            let d = 0.5 - hex_dist(gv_x, gv_y);
            let dome = (d / 0.48).clamp(0.0, 1.0).powf(0.75);

            let wave = (px * 5.5 + t * 1.8).sin() * (py * 4.2 - t * 1.4).cos() * 0.35
                + ((px + py) * 3.2 - t * 1.1).sin() * 0.2;
            let band = 0.55 + 0.45 * (px * 4.0 + py * 3.0 + t * 2.0 + wave * 2.0).sin();
            let shade = (0.45 + 0.55 * dome) * (0.65 + 0.55 * band);
            cr *= shade;
            cg *= shade;
            cb *= shade;
            let face = dome * band;
            cr += 0.12 * face;
            cg += 0.125 * face;
            cb += 0.13 * face;

            let edge = 1.0 - smoothstep(0.0, lw, d.abs());
            let bloom = (-d.abs() / lw * 1.2).exp();
            let pulse = 0.88
                + 0.12
                    * (t * 1.3 + hash21((px * scale).floor(), (py * scale).floor()) * 20.0).sin();
            let gwave = gstr * (1.0 + wave * 0.4);
            cg += 0.88 * edge * gwave * pulse + 0.88 * bloom * 0.2 * gwave;
            cb += 1.0 * edge * gwave * pulse + 1.0 * bloom * 0.2 * gwave;

            let i = row + x * 4;
            rgba[i] = (cr.clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 1] = (cg.clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 2] = (cb.clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 3] = 255;
        }
    }
}

/// Export-oriented fill: shade at most ~960px on the long side, nearest-neighbor upscale.
/// ~4–16× fewer pixels than full 1080p/4K, still looks fine in video.
pub fn fill_hex_chain_rgba_export(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    time_secs: f32,
    glow: f32,
) {
    let w = width.max(1);
    let h = height.max(1);
    if rgba.len() < (w * h * 4) as usize {
        return;
    }
    const MAX_SIDE: u32 = 960;
    let long = w.max(h);
    if long <= MAX_SIDE {
        fill_hex_chain_rgba(rgba, w, h, time_secs, glow);
        return;
    }
    let s = MAX_SIDE as f32 / long as f32;
    let sw = ((w as f32 * s).round() as u32).max(2);
    let sh = ((h as f32 * s).round() as u32).max(2);
    let mut small = vec![0u8; (sw * sh * 4) as usize];
    fill_hex_chain_rgba(&mut small, sw, sh, time_secs, glow);

    // Nearest-neighbor upscale (fast, sharp hex edges).
    let sw_f = sw as f32;
    let sh_f = sh as f32;
    for y in 0..h as usize {
        let sy = ((y as f32 + 0.5) * sh_f / h as f32).floor() as u32;
        let sy = sy.min(sh - 1) as usize;
        let row = y * w as usize * 4;
        for x in 0..w as usize {
            let sx = ((x as f32 + 0.5) * sw_f / w as f32).floor() as u32;
            let sx = sx.min(sw - 1) as usize;
            let si = (sy * sw as usize + sx) * 4;
            let di = row + x * 4;
            rgba[di] = small[si];
            rgba[di + 1] = small[si + 1];
            rgba[di + 2] = small[si + 2];
            rgba[di + 3] = 255;
        }
    }
}

/// Fill pixmap from a shading pass when it looks like hex chain; returns true if filled.
pub fn try_fill_pixmap_hex(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    pass: &ShadingPass,
    time_secs: f32,
) -> bool {
    if !is_hex_chain_wgsl(pass) {
        return false;
    }
    let glow = pass.uniforms.get(2).copied().unwrap_or(0.85);
    let w = pixmap.width();
    let h = pixmap.height();
    let t = time_secs + pass.uniforms.first().copied().unwrap_or(0.0);
    fill_hex_chain_rgba_export(pixmap.data_mut(), w, h, t, glow);
    true
}
