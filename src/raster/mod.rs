//! Editable RGBA paint surface: soft/hard stamps, continuous stroke spacing, PNG I/O.
//!
//! Paint targets are `NodeKind::Image` buffers (decoded RGBA). Preview and export share the
//! same pixels once committed back to `Image.bytes`.

use image::ImageEncoder;

/// In-memory paint buffer (RGBA8, unpremultiplied).
#[derive(Debug, Clone)]
pub struct RasterBuffer {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RasterBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let n = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        Self {
            width: w,
            height: h,
            rgba: vec![0u8; n],
        }
    }

    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let need = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if rgba.len() != need {
            return None;
        }
        Some(Self {
            width: width.max(1),
            height: height.max(1),
            rgba,
        })
    }

    pub fn from_png_bytes(bytes: &[u8]) -> Option<Self> {
        let dyn_img = image::load_from_memory(bytes).ok()?;
        let rgba = dyn_img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Some(Self {
            width: w.max(1),
            height: h.max(1),
            rgba: rgba.into_raw(),
        })
    }

    pub fn encode_png(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut buf);
        enc.write_image(
            &self.rgba,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
        Some(buf)
    }

    pub fn transparent_png(width: u32, height: u32) -> Option<Vec<u8>> {
        Self::new(width, height).encode_png()
    }

    /// Stamp a circular brush. `radius` in pixels. `hardness` 0=soft … 1=hard edge.
    /// `color` RGBA 0–255 unpremultiplied. `erase` multiplies destination alpha down.
    pub fn stamp_circle(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        hardness: f32,
        color: [u8; 4],
        opacity: f32,
        erase: bool,
    ) {
        let r = radius.max(0.5);
        let hard = hardness.clamp(0.0, 1.0);
        let op = opacity.clamp(0.0, 1.0);
        if op <= 1e-6 {
            return;
        }
        let x0 = (cx - r - 1.0).floor().max(0.0) as i32;
        let y0 = (cy - r - 1.0).floor().max(0.0) as i32;
        let x1 = ((cx + r + 1.0).ceil() as i32).min(self.width as i32);
        let y1 = ((cy + r + 1.0).ceil() as i32).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let r2 = r * r;
        let hard_r = hard * r;
        let hard_r2 = hard_r * hard_r;
        let soft_span = (r - hard_r).max(1e-3);
        let w = self.width as usize;
        // Fast hard brush: no soft falloff, integer-ish alpha.
        let fully_hard = hard >= 0.999 && op >= 0.999;

        for y in y0..y1 {
            let row = y as usize * w;
            let dy = y as f32 + 0.5 - cy;
            let dy2 = dy * dy;
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - cx;
                let d2 = dx * dx + dy2;
                if d2 >= r2 {
                    continue;
                }
                let a = if fully_hard {
                    1.0
                } else if d2 <= hard_r2 {
                    op
                } else {
                    let d = d2.sqrt();
                    let t = ((d - hard_r) / soft_span).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    op * (1.0 - t)
                };
                if a <= 1e-6 {
                    continue;
                }
                let idx = (row + x as usize) * 4;
                if erase {
                    let keep = 1.0 - a;
                    // Integer-ish multiply for speed when fully opaque erase.
                    if keep <= 0.0 {
                        self.rgba[idx] = 0;
                        self.rgba[idx + 1] = 0;
                        self.rgba[idx + 2] = 0;
                        self.rgba[idx + 3] = 0;
                    } else if keep < 0.999 {
                        self.rgba[idx] = (self.rgba[idx] as f32 * keep) as u8;
                        self.rgba[idx + 1] = (self.rgba[idx + 1] as f32 * keep) as u8;
                        self.rgba[idx + 2] = (self.rgba[idx + 2] as f32 * keep) as u8;
                        self.rgba[idx + 3] = (self.rgba[idx + 3] as f32 * keep) as u8;
                    }
                } else if fully_hard && color[3] == 255 {
                    // Opaque hard stamp: overwrite.
                    self.rgba[idx] = color[0];
                    self.rgba[idx + 1] = color[1];
                    self.rgba[idx + 2] = color[2];
                    self.rgba[idx + 3] = 255;
                } else {
                    // Src-over (unpremultiplied).
                    let sa = (color[3] as f32 / 255.0) * a;
                    let da = self.rgba[idx + 3] as f32 / 255.0;
                    let out_a = sa + da * (1.0 - sa);
                    if out_a <= 1e-6 {
                        self.rgba[idx] = 0;
                        self.rgba[idx + 1] = 0;
                        self.rgba[idx + 2] = 0;
                        self.rgba[idx + 3] = 0;
                        continue;
                    }
                    let inv = 1.0 / out_a;
                    for c in 0..3 {
                        let s = color[c] as f32 / 255.0;
                        let dch = self.rgba[idx + c] as f32 / 255.0;
                        let o = (s * sa + dch * da * (1.0 - sa)) * inv;
                        self.rgba[idx + c] = (o * 255.0).clamp(0.0, 255.0) as u8;
                    }
                    self.rgba[idx + 3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// Walk from `from` → `to` placing stamp centers every `spacing` pixels (continuous pen).
/// `carry` is leftover distance since the previous stamp (0..spacing).
/// Returns (stamp positions, new carry).
pub fn stamps_along(
    from: (f32, f32),
    to: (f32, f32),
    spacing: f32,
    carry: f32,
    force_first: bool,
) -> (Vec<(f32, f32)>, f32) {
    let spacing = spacing.max(0.25);
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let dist = (dx * dx + dy * dy).sqrt();
    let mut out = Vec::new();
    if force_first {
        out.push(from);
    }
    if dist < 1e-6 {
        // Still sitting on the same pixel — keep carry, or reset if we just stamped.
        return (out, if force_first { 0.0 } else { carry });
    }
    let ux = dx / dist;
    let uy = dy / dist;
    // Distance along segment until next stamp.
    let mut remaining = if force_first {
        spacing
    } else {
        (spacing - carry).max(0.0)
    };
    let mut walked = 0.0_f32;
    while walked + remaining <= dist + 1e-4 {
        walked += remaining;
        out.push((from.0 + ux * walked, from.1 + uy * walked));
        remaining = spacing;
    }
    let carry_out = dist - walked;
    (out, carry_out.clamp(0.0, spacing))
}

#[inline]
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    // Centripetal-ish uniform Catmull-Rom (standard cubic).
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Stamp along a **Catmull-Rom** segment from `p1` → `p2` using neighbors `p0`/`p3`.
/// Turns sparse frame samples into smooth freehand (spirals, curves) instead of a polyline.
pub fn stamps_along_catmull(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    spacing: f32,
    carry: f32,
    force_first: bool,
) -> (Vec<(f32, f32)>, f32) {
    let spacing = spacing.max(0.25);
    // Adaptive subdivision: denser when the chord is long or bends hard.
    let chord = ((p2.0 - p1.0).hypot(p2.1 - p1.1)).max(0.0);
    if chord < 1e-5 {
        let mut out = Vec::new();
        if force_first {
            out.push(p1);
        }
        return (out, if force_first { 0.0 } else { carry });
    }
    // Mid-curve deviation vs chord → more steps when curving.
    let mid = (
        catmull_rom(p0.0, p1.0, p2.0, p3.0, 0.5),
        catmull_rom(p0.1, p1.1, p2.1, p3.1, 0.5),
    );
    let chord_mid = ((p1.0 + p2.0) * 0.5, (p1.1 + p2.1) * 0.5);
    let bend = (mid.0 - chord_mid.0).hypot(mid.1 - chord_mid.1);
    // ~1 sample per spacing along chord, extra for bend + long jumps.
    let steps = ((chord / spacing.max(0.5)).ceil() as usize)
        .max(2)
        .saturating_add((bend / spacing.max(0.5)).ceil() as usize)
        .min(256)
        .max(4);

    // Polyline approximation of the curve with arc-length parameterization.
    let mut poly = Vec::with_capacity(steps + 1);
    poly.push(p1);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        poly.push((
            catmull_rom(p0.0, p1.0, p2.0, p3.0, t),
            catmull_rom(p0.1, p1.1, p2.1, p3.1, t),
        ));
    }

    // Walk the polyline with spacing + carry (same semantics as stamps_along).
    let mut out = Vec::new();
    if force_first {
        out.push(p1);
    }
    let mut rem = if force_first {
        spacing
    } else {
        (spacing - carry).max(0.0)
    };
    let mut carry_out = carry;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let seg = (dx * dx + dy * dy).sqrt();
        if seg < 1e-8 {
            continue;
        }
        let ux = dx / seg;
        let uy = dy / seg;
        let mut walked = 0.0_f32;
        while walked + rem <= seg + 1e-4 {
            walked += rem;
            out.push((a.0 + ux * walked, a.1 + uy * walked));
            rem = spacing;
        }
        carry_out = seg - walked;
        rem = (spacing - carry_out).max(0.0);
    }
    (out, carry_out.clamp(0.0, spacing))
}

/// Given a short history of samples (oldest → newest, last is the new tip),
/// stamp the newest segment with Catmull-Rom when possible.
///
/// History should contain the new point already pushed. Returns stamps + new carry.
pub fn stamps_for_new_sample(
    hist: &[(f32, f32)],
    spacing: f32,
    carry: f32,
    force_first: bool,
) -> (Vec<(f32, f32)>, f32) {
    match hist.len() {
        0 => (Vec::new(), carry),
        1 => {
            if force_first {
                (vec![hist[0]], 0.0)
            } else {
                (Vec::new(), carry)
            }
        }
        2 => stamps_along(hist[0], hist[1], spacing, carry, force_first),
        n => {
            // Segment from hist[n-2] → hist[n-1], neighbors hist[n-3] and extrapolated tip.
            let p1 = hist[n - 2];
            let p2 = hist[n - 1];
            let p0 = if n >= 3 {
                hist[n - 3]
            } else {
                p1
            };
            // Extrapolate p3 past p2 along last direction for end tension.
            let p3 = {
                let dx = p2.0 - p1.0;
                let dy = p2.1 - p1.1;
                (p2.0 + dx, p2.1 + dy)
            };
            stamps_along_catmull(p0, p1, p2, p3, spacing, carry, force_first)
        }
    }
}

/// Flood-fill contiguous pixels matching the seed (within `tolerance` RGB L∞).
/// Writes `fill` into matching pixels. Returns number of pixels changed.
pub fn flood_fill(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    seed_x: i32,
    seed_y: i32,
    fill: [u8; 4],
    tolerance: u8,
) -> usize {
    let w = width as i32;
    let h = height as i32;
    if w <= 0 || h <= 0 || seed_x < 0 || seed_y < 0 || seed_x >= w || seed_y >= h {
        return 0;
    }
    let idx = |x: i32, y: i32| -> usize { ((y as usize) * (w as usize) + (x as usize)) * 4 };
    let get = |buf: &[u8], x: i32, y: i32| -> [u8; 4] {
        let i = idx(x, y);
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    let target = get(rgba, seed_x, seed_y);
    // No-op if already same color (and fully opaque match).
    if color_match(target, fill, 0) && target[3] == fill[3] {
        return 0;
    }
    let tol = tolerance as i16;
    let matches = |c: [u8; 4]| -> bool {
        (c[0] as i16 - target[0] as i16).abs() <= tol
            && (c[1] as i16 - target[1] as i16).abs() <= tol
            && (c[2] as i16 - target[2] as i16).abs() <= tol
            // Treat nearly-transparent as matching transparent seed.
            && ((target[3] < 8 && c[3] < 8) || (c[3] as i16 - target[3] as i16).abs() <= tol)
    };
    if !matches(target) {
        return 0;
    }

    let mut stack = vec![(seed_x, seed_y)];
    let mut visited = vec![false; (w * h) as usize];
    let mut painted = 0usize;
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        let vi = (y * w + x) as usize;
        if visited[vi] {
            continue;
        }
        visited[vi] = true;
        if !matches(get(rgba, x, y)) {
            continue;
        }
        let i = idx(x, y);
        rgba[i] = fill[0];
        rgba[i + 1] = fill[1];
        rgba[i + 2] = fill[2];
        rgba[i + 3] = fill[3];
        painted += 1;
        stack.push((x + 1, y));
        stack.push((x - 1, y));
        stack.push((x, y + 1));
        stack.push((x, y - 1));
    }
    painted
}

fn color_match(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    let t = tol as i16;
    (a[0] as i16 - b[0] as i16).abs() <= t
        && (a[1] as i16 - b[1] as i16).abs() <= t
        && (a[2] as i16 - b[2] as i16).abs() <= t
}

/// Brush radius in **image pixels** from document-space brush size and image placement.
pub fn doc_size_to_pixel_radius(
    size_doc: f32,
    image_w_doc: f64,
    image_h_doc: f64,
    pixel_w: u32,
    pixel_h: u32,
) -> f32 {
    let sx = if image_w_doc > 1e-6 {
        pixel_w as f64 / image_w_doc
    } else {
        1.0
    };
    let sy = if image_h_doc > 1e-6 {
        pixel_h as f64 / image_h_doc
    } else {
        1.0
    };
    let scale = ((sx + sy) * 0.5) as f32;
    (size_doc * 0.5 * scale).max(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_paints_opaque_pixel() {
        let mut buf = RasterBuffer::new(32, 32);
        buf.stamp_circle(16.0, 16.0, 4.0, 1.0, [255, 0, 0, 255], 1.0, false);
        let idx = (16 * 32 + 16) * 4;
        assert!(buf.rgba[idx] > 200, "center red");
        assert_eq!(buf.rgba[idx + 3], 255);
    }

    #[test]
    fn erase_clears_alpha() {
        let mut buf = RasterBuffer::new(16, 16);
        buf.stamp_circle(8.0, 8.0, 6.0, 1.0, [0, 0, 0, 255], 1.0, false);
        buf.stamp_circle(8.0, 8.0, 6.0, 1.0, [0, 0, 0, 255], 1.0, true);
        let idx = (8 * 16 + 8) * 4;
        assert!(buf.rgba[idx + 3] < 10, "erased alpha");
    }

    #[test]
    fn continuous_stamps_fill_gap() {
        let (pts, _) = stamps_along((0.0, 0.0), (20.0, 0.0), 5.0, 0.0, true);
        assert!(pts.len() >= 4, "got {} stamps", pts.len());
        assert!((pts[0].0 - 0.0).abs() < 1e-3);
    }

    #[test]
    fn flood_fill_fills_region() {
        let mut buf = RasterBuffer::new(8, 8);
        // Vertical barrier of red down the middle.
        for y in 0..8 {
            let i = (y * 8 + 3) * 4;
            buf.rgba[i] = 255;
            buf.rgba[i + 3] = 255;
        }
        let n = flood_fill(&mut buf.rgba, 8, 8, 0, 0, [0, 255, 0, 255], 0);
        assert!(n > 0);
        // Left of barrier green
        assert_eq!(buf.rgba[0], 0);
        assert_eq!(buf.rgba[1], 255);
        // Right of barrier still empty
        let r = (0 * 8 + 5) * 4;
        assert_eq!(buf.rgba[r + 3], 0);
    }

    #[test]
    fn catmull_spiral_segment_is_not_just_chord() {
        // Right-angle control points — Catmull bows outward (x > 10) off the chord x=10.
        let p0 = (0.0, 0.0);
        let p1 = (10.0, 0.0);
        let p2 = (10.0, 10.0);
        let p3 = (0.0, 10.0);
        let (pts, _) = stamps_along_catmull(p0, p1, p2, p3, 1.0, 0.0, true);
        assert!(pts.len() > 5, "expected dense curve stamps, got {}", pts.len());
        let off_chord = pts.iter().any(|(x, y)| *x > 10.2 && *y > 1.0 && *y < 9.0);
        assert!(off_chord, "Catmull path stayed on the chord x=10");
    }

    #[test]
    fn png_roundtrip() {
        let mut buf = RasterBuffer::new(8, 8);
        buf.stamp_circle(4.0, 4.0, 2.0, 1.0, [10, 20, 30, 255], 1.0, false);
        let png = buf.encode_png().expect("png");
        let back = RasterBuffer::from_png_bytes(&png).expect("decode");
        assert_eq!(back.width, 8);
        assert_eq!(back.height, 8);
    }
}
