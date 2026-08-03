//! Pure-Rust spatial ops for node-editor CV effects (no OpenCV).
//! Used by ChromaKey, ApplyMask, BackgroundBlur materialize paths.

use super::CvMask;
use image::RgbaImage;

/// Convert sRGB 0..1 → HSV (H in 0..360, S/V in 0..1).
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let v = max;
    let s = if max > 1e-6 { delta / max } else { 0.0 };
    let h = if delta < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    (h, s, v)
}

#[inline]
fn hue_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).abs();
    d.min(360.0 - d)
}

/// Build an R8 matte: 255 = keep (not key color), 0 = key (transparent).
///
/// `tol` is hue tolerance in degrees (soft edge uses `soft` additional degrees).
/// Low-saturation pixels are treated as non-key (protects shadows/skin).
pub fn chroma_key_mask(
    img: &RgbaImage,
    key_r: f32,
    key_g: f32,
    key_b: f32,
    tol_deg: f32,
    soft_deg: f32,
) -> CvMask {
    let (w, h) = img.dimensions();
    let (kh, ks, kv) = rgb_to_hsv(key_r.clamp(0.0, 1.0), key_g.clamp(0.0, 1.0), key_b.clamp(0.0, 1.0));
    let tol = tol_deg.max(0.0);
    let soft = soft_deg.max(0.0);
    let outer = tol + soft;
    let mut data = vec![0u8; (w as usize) * (h as usize)];
    for (i, px) in img.pixels().enumerate() {
        let r = px.0[0] as f32 / 255.0;
        let g = px.0[1] as f32 / 255.0;
        let b = px.0[2] as f32 / 255.0;
        let (h0, s0, v0) = rgb_to_hsv(r, g, b);
        // Desaturated / very dark: keep (not screen).
        if s0 < 0.12 || v0 < 0.08 {
            data[i] = 255;
            continue;
        }
        // Key itself should be fairly saturated.
        let sat_gate = if ks > 0.2 {
            ((s0 - 0.15) / (ks.max(0.2) - 0.15 + 1e-3)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let dh = hue_dist(h0, kh);
        // 0 = fully key (transparent), 1 = fully keep
        let keep = if dh <= tol {
            0.0
        } else if dh >= outer || outer <= tol {
            1.0
        } else {
            (dh - tol) / (outer - tol + 1e-6)
        };
        // Only punch key when both hue matches and pixel is saturated enough.
        let key_amount = (1.0 - keep) * sat_gate;
        let alpha = ((1.0 - key_amount) * 255.0).round().clamp(0.0, 255.0) as u8;
        data[i] = alpha;
    }
    CvMask {
        width: w,
        height: h,
        data: std::sync::Arc::new(data),
    }
}

/// Apply matte to alpha channel. `invert`: high mask becomes transparent.
pub fn apply_mask_rgba(img: &mut RgbaImage, mask: &CvMask, invert: bool) {
    let (w, h) = img.dimensions();
    if mask.width != w || mask.height != h {
        // Nearest resize mask to image size.
        let resized = resize_mask_nearest(mask, w, h);
        apply_mask_rgba(img, &resized, invert);
        return;
    }
    for (px, &m) in img.pixels_mut().zip(mask.data.iter()) {
        let a = if invert { 255u8.saturating_sub(m) } else { m };
        // Multiply existing alpha so chained mattes work.
        let na = ((px.0[3] as u16 * a as u16) / 255) as u8;
        px.0[3] = na;
    }
}

/// Soften matte edges with a cheap box blur on the mask, then re-apply.
pub fn feather_mask(mask: &CvMask, radius: f32) -> CvMask {
    if radius < 0.35 || mask.width < 2 || mask.height < 2 {
        return mask.clone();
    }
    let mut img = RgbaImage::new(mask.width, mask.height);
    for (i, px) in img.pixels_mut().enumerate() {
        let v = mask.data[i];
        *px = image::Rgba([v, v, v, 255]);
    }
    crate::document::fast_box_blur_rgba(&mut img, radius.min(12.0));
    let mut data = vec![0u8; mask.data.len()];
    for (i, px) in img.pixels().enumerate() {
        data[i] = px.0[0];
    }
    CvMask {
        width: mask.width,
        height: mask.height,
        data: std::sync::Arc::new(data),
    }
}

/// Keyed image: RGB preserved, alpha from matte (optionally feathered).
pub fn chroma_key_rgba(
    img: &RgbaImage,
    key_r: f32,
    key_g: f32,
    key_b: f32,
    tol_deg: f32,
    soft_deg: f32,
) -> (RgbaImage, CvMask) {
    let mask = chroma_key_mask(img, key_r, key_g, key_b, tol_deg, soft_deg);
    let feather = soft_deg.max(0.0) * 0.15;
    let mask = if feather > 0.2 {
        feather_mask(&mask, feather)
    } else {
        mask
    };
    let mut out = img.clone();
    apply_mask_rgba(&mut out, &mask, false);
    // Mild spill suppress toward gray on semi-transparent fringe.
    for px in out.pixels_mut() {
        if px.0[3] > 0 && px.0[3] < 250 {
            let a = px.0[3] as f32 / 255.0;
            let g = px.0[1] as f32;
            let r = px.0[0] as f32;
            let b = px.0[2] as f32;
            // Pull green toward mean of r/b when partially transparent.
            let mean = (r + b) * 0.5;
            let spill = ((g - mean).max(0.0) * (1.0 - a) * 0.65).min(g);
            px.0[1] = (g - spill).round().clamp(0.0, 255.0) as u8;
        }
    }
    (out, mask)
}

/// Blur regions where mask is low (background). Subject (high mask) stays sharp.
pub fn background_blur_rgba(img: &RgbaImage, mask: &CvMask, blur_px: f32) -> RgbaImage {
    let blur = blur_px.clamp(0.0, 64.0);
    if blur < 0.05 {
        return img.clone();
    }
    let (w, h) = img.dimensions();
    let mask = if mask.width != w || mask.height != h {
        resize_mask_nearest(mask, w, h)
    } else {
        mask.clone()
    };
    let mut blurred = img.clone();
    crate::document::export_fast_blur_rgba(&mut blurred, blur);
    let mut out = img.clone();
    for ((dst, sharp), (blur_px, &m)) in out
        .pixels_mut()
        .zip(img.pixels())
        .zip(blurred.pixels().zip(mask.data.iter()))
    {
        // m=255 subject → keep sharp; m=0 background → full blur
        let t = m as f32 / 255.0;
        for c in 0..4 {
            let s = sharp.0[c] as f32;
            let b = blur_px.0[c] as f32;
            dst.0[c] = (s * t + b * (1.0 - t)).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn resize_mask_nearest(mask: &CvMask, nw: u32, nh: u32) -> CvMask {
    if mask.width == 0 || mask.height == 0 {
        return CvMask::solid(nw, nh, 255);
    }
    let mut data = vec![0u8; (nw as usize) * (nh as usize)];
    for y in 0..nh {
        let sy = (y as u64 * mask.height as u64) / nh as u64;
        for x in 0..nw {
            let sx = (x as u64 * mask.width as u64) / nw as u64;
            let si = (sy as usize) * (mask.width as usize) + (sx as usize);
            data[(y as usize) * (nw as usize) + (x as usize)] = mask.data[si];
        }
    }
    CvMask {
        width: nw,
        height: nh,
        data: std::sync::Arc::new(data),
    }
}

/// Lightweight face-ish ROI finder (no OpenCV).
///
/// Skin-tone (YCbCr) mask → largest connected component with face-like aspect ratio.
/// Good enough for selfies / head-and-shoulders; not a real face detector.
pub fn detect_face_regions(img: &RgbaImage) -> Vec<super::CvRegion> {
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 {
        return vec![];
    }
    // Work on a small grid for speed.
    let tw = 80u32.min(w);
    let th = ((h as f32) * (tw as f32) / (w as f32)).round().max(8.0) as u32;
    let th = th.min(h).max(8);
    let small = image::imageops::resize(img, tw, th, image::imageops::FilterType::Triangle);
    let n = (tw * th) as usize;
    let mut skin = vec![false; n];
    for (i, px) in small.pixels().enumerate() {
        let r = px.0[0] as f32;
        let g = px.0[1] as f32;
        let b = px.0[2] as f32;
        // ITU-R BT.601-ish YCbCr
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let cb = 128.0 - 0.168736 * r - 0.331264 * g + 0.5 * b;
        let cr = 128.0 + 0.5 * r - 0.418688 * g - 0.081312 * b;
        // Common skin ranges (broad, works on many tones under room light).
        let is_skin = y > 40.0
            && y < 240.0
            && cb > 77.0
            && cb < 135.0
            && cr > 130.0
            && cr < 180.0
            && r > 60.0
            && r >= g * 0.85;
        skin[i] = is_skin;
    }
    // 4-connected components
    let mut label = vec![-1i32; n];
    let mut areas: Vec<u32> = Vec::new();
    let mut bounds: Vec<(u32, u32, u32, u32)> = Vec::new(); // minx,miny,maxx,maxy
    let mut stack = Vec::new();
    let tw_i = tw as i32;
    let th_i = th as i32;
    for y in 0..th_i {
        for x in 0..tw_i {
            let i = (y as u32 * tw + x as u32) as usize;
            if !skin[i] || label[i] >= 0 {
                continue;
            }
            let id = areas.len() as i32;
            areas.push(0);
            bounds.push((x as u32, y as u32, x as u32, y as u32));
            stack.clear();
            stack.push((x, y));
            label[i] = id;
            while let Some((cx, cy)) = stack.pop() {
                let ci = (cy as u32 * tw + cx as u32) as usize;
                areas[id as usize] += 1;
                let b = &mut bounds[id as usize];
                b.0 = b.0.min(cx as u32);
                b.1 = b.1.min(cy as u32);
                b.2 = b.2.max(cx as u32);
                b.3 = b.3.max(cy as u32);
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = cx + dx;
                    let ny = cy + dy;
                    if nx < 0 || ny < 0 || nx >= tw_i || ny >= th_i {
                        continue;
                    }
                    let ni = (ny as u32 * tw + nx as u32) as usize;
                    if skin[ni] && label[ni] < 0 {
                        label[ni] = id;
                        stack.push((nx, ny));
                    }
                }
            }
        }
    }
    let img_area = (tw * th) as f32;
    let mut candidates: Vec<(f32, super::CvRegion)> = Vec::new();
    for (id, &area) in areas.iter().enumerate() {
        let frac = area as f32 / img_area;
        // Face-ish size: not tiny speck, not whole frame
        if frac < 0.01 || frac > 0.55 {
            continue;
        }
        let (minx, miny, maxx, maxy) = bounds[id];
        let bw = (maxx - minx + 1) as f32;
        let bh = (maxy - miny + 1) as f32;
        if bw < 4.0 || bh < 4.0 {
            continue;
        }
        let aspect = bh / bw; // face taller than wide-ish
        if !(0.8..=2.2).contains(&aspect) {
            continue;
        }
        // Prefer upper-half centers (heads)
        let cy = (miny + maxy) as f32 * 0.5 / th as f32;
        if cy > 0.75 {
            continue;
        }
        // Expand box a bit (hair/forehead/chin)
        let pad_x = bw * 0.12;
        let pad_y = bh * 0.15;
        let x0 = ((minx as f32 - pad_x) / tw as f32).clamp(0.0, 1.0);
        let y0 = ((miny as f32 - pad_y) / th as f32).clamp(0.0, 1.0);
        let x1 = ((maxx as f32 + pad_x) / tw as f32).clamp(0.0, 1.0);
        let y1 = ((maxy as f32 + pad_y) / th as f32).clamp(0.0, 1.0);
        let score = frac * (1.2 - (aspect - 1.3).abs() * 0.3) * (1.0 - cy * 0.4);
        candidates.push((
            score,
            super::CvRegion {
                label: "face".into(),
                x: x0,
                y: y0,
                w: (x1 - x0).max(0.02),
                h: (y1 - y0).max(0.02),
                confidence: score.clamp(0.0, 1.0),
                track_id: None,
            }
            .clamp_norm(),
        ));
    }
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // Top 3 faces
    candidates.into_iter().take(3).map(|(_, r)| r).collect()
}

/// Pack RGBA into cache value.
pub fn rgba_to_cache_value(img: &RgbaImage) -> super::CvCacheValue {
    let (w, h) = img.dimensions();
    super::CvCacheValue::Rgba {
        width: w,
        height: h,
        data: std::sync::Arc::new(img.clone().into_raw()),
    }
}

/// Pixelate a rectangular ROI (inclusive min, exclusive max in pixel coords).
pub fn pixelate_roi(img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, block: u32) {
    let block = block.max(2);
    let (iw, ih) = img.dimensions();
    let x0 = x0.min(iw);
    let y0 = y0.min(ih);
    let x1 = x1.min(iw).max(x0);
    let y1 = y1.min(ih).max(y0);
    let mut y = y0;
    while y < y1 {
        let yb = (y + block).min(y1);
        let mut x = x0;
        while x < x1 {
            let xb = (x + block).min(x1);
            // Average block color
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for py in y..yb {
                for px in x..xb {
                    let p = img.get_pixel(px, py).0;
                    for c in 0..4 {
                        acc[c] += p[c] as u64;
                    }
                    n += 1;
                }
            }
            if n == 0 {
                x = xb;
                continue;
            }
            let avg = [
                (acc[0] / n) as u8,
                (acc[1] / n) as u8,
                (acc[2] / n) as u8,
                (acc[3] / n) as u8,
            ];
            for py in y..yb {
                for px in x..xb {
                    img.put_pixel(px, py, image::Rgba(avg));
                }
            }
            x = xb;
        }
        y = yb;
    }
}

/// Blur a rectangular ROI by copying ROI → blur → write back.
pub fn blur_roi(img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, radius: f32) {
    let (iw, ih) = img.dimensions();
    let x0 = x0.min(iw);
    let y0 = y0.min(ih);
    let x1 = x1.min(iw).max(x0);
    let y1 = y1.min(ih).max(y0);
    let rw = x1.saturating_sub(x0);
    let rh = y1.saturating_sub(y0);
    if rw < 2 || rh < 2 || radius < 0.35 {
        return;
    }
    let mut roi = RgbaImage::new(rw, rh);
    for py in 0..rh {
        for px in 0..rw {
            roi.put_pixel(px, py, *img.get_pixel(x0 + px, y0 + py));
        }
    }
    crate::document::fast_box_blur_rgba(&mut roi, radius.min(24.0));
    for py in 0..rh {
        for px in 0..rw {
            img.put_pixel(x0 + px, y0 + py, *roi.get_pixel(px, py));
        }
    }
}

/// Apply privacy blur / pixelate over labeled regions (normalized 0..1 boxes).
///
/// - `strength`: blur radius (mode 0) or pixel block size (mode 1)
/// - `pad`: expand each box by this fraction of image min-side
/// - `mode`: 0 = gaussian-ish box blur, 1 = pixelate
/// - `label_filter`: if non-empty, only regions whose label matches (case-insensitive)
pub fn privacy_blur_rgba(
    img: &RgbaImage,
    regions: &[super::CvRegion],
    strength: f32,
    pad: f32,
    mode: i32,
    label_filter: &str,
) -> RgbaImage {
    let mut out = img.clone();
    let (iw, ih) = out.dimensions();
    if iw == 0 || ih == 0 || regions.is_empty() {
        return out;
    }
    let filter = label_filter.trim().to_ascii_lowercase();
    let pad_px = (pad.max(0.0) * (iw.min(ih) as f32)).round() as i32;
    for r in regions {
        if !filter.is_empty() && r.label.to_ascii_lowercase() != filter {
            continue;
        }
        let mut x0 = (r.x * iw as f32).floor() as i32 - pad_px;
        let mut y0 = (r.y * ih as f32).floor() as i32 - pad_px;
        let mut x1 = ((r.x + r.w) * iw as f32).ceil() as i32 + pad_px;
        let mut y1 = ((r.y + r.h) * ih as f32).ceil() as i32 + pad_px;
        x0 = x0.clamp(0, iw as i32);
        y0 = y0.clamp(0, ih as i32);
        x1 = x1.clamp(0, iw as i32).max(x0);
        y1 = y1.clamp(0, ih as i32).max(y0);
        if mode >= 1 {
            let block = strength.round().clamp(2.0, 64.0) as u32;
            pixelate_roi(&mut out, x0 as u32, y0 as u32, x1 as u32, y1 as u32, block);
        } else {
            blur_roi(
                &mut out,
                x0 as u32,
                y0 as u32,
                x1 as u32,
                y1 as u32,
                strength.max(0.0),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for px in img.pixels_mut() {
            *px = Rgba(c);
        }
        img
    }

    #[test]
    fn green_screen_punches_green_keeps_red() {
        let mut img = RgbaImage::new(4, 1);
        img.put_pixel(0, 0, Rgba([0, 255, 0, 255])); // pure green
        img.put_pixel(1, 0, Rgba([255, 0, 0, 255])); // red
        img.put_pixel(2, 0, Rgba([0, 200, 0, 255])); // green-ish
        img.put_pixel(3, 0, Rgba([40, 40, 40, 255])); // gray
        let mask = chroma_key_mask(&img, 0.0, 1.0, 0.0, 40.0, 15.0);
        assert!(mask.data[0] < 40, "green should be keyed out");
        assert!(mask.data[1] > 200, "red should keep");
        assert!(mask.data[3] > 200, "gray should keep");
    }

    #[test]
    fn apply_mask_sets_alpha() {
        let mut img = solid(2, 2, [10, 20, 30, 255]);
        let mask = CvMask::solid(2, 2, 128);
        apply_mask_rgba(&mut img, &mask, false);
        assert_eq!(img.get_pixel(0, 0).0[3], 128);
    }

    #[test]
    fn privacy_blur_dims_roi() {
        let mut img = solid(32, 32, [200, 200, 200, 255]);
        // Bright square in corner
        for y in 0..8 {
            for x in 0..8 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let regions = vec![super::super::CvRegion {
            label: "manual".into(),
            x: 0.0,
            y: 0.0,
            w: 0.25,
            h: 0.25,
            confidence: 1.0,
            track_id: None,
        }];
        // Pad pulls gray into the ROI so blur is not a pure-red island.
        let out = privacy_blur_rgba(&img, &regions, 6.0, 0.05, 0, "");
        let edge = out.get_pixel(7, 0).0;
        assert!(
            edge[1] > 0 || edge[0] < 255,
            "padded ROI edge should soften, got {:?}",
            edge
        );
        // Outside padded ROI stays gray
        let o = out.get_pixel(30, 30).0;
        assert_eq!(o[0], 200);
    }

    #[test]
    fn background_blur_changes_low_mask() {
        let mut img = solid(16, 16, [0, 0, 0, 255]);
        // White subject in center, black bg with a bright speck in corner.
        for y in 4..12 {
            for x in 4..12 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let mut mask_data = vec![0u8; 16 * 16];
        for y in 4..12 {
            for x in 4..12 {
                mask_data[y * 16 + x] = 255;
            }
        }
        let mask = CvMask::new(16, 16, mask_data).unwrap();
        let out = background_blur_rgba(&img, &mask, 8.0);
        // Subject center stays near white.
        let c = out.get_pixel(8, 8).0;
        assert!(c[0] > 200 && c[1] > 200);
        // Corner speck should soften (not pure 255,0,0 after blur mix).
        let corner = out.get_pixel(0, 0).0;
        assert!(corner[0] < 255 || corner[1] > 0);
    }
}
