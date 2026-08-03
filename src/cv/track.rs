//! Multi-target template tracking.
//!
//! Provide one or more **target** images of the object. Each scene frame is searched
//! for the best match among all targets (multi-scale NCC). If the object is not found
//! (confidence below threshold), the **last successful position** is held.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use image::RgbaImage;
use uuid::Uuid;

use super::CvTrackSample;

struct HoldState {
    last: CvTrackSample,
    media_key: String,
}

static HOLD: OnceLock<Mutex<HashMap<Uuid, HoldState>>> = OnceLock::new();

fn hold_map() -> &'static Mutex<HashMap<Uuid, HoldState>> {
    HOLD.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clear_tracker(node_id: Uuid) {
    if let Ok(mut g) = hold_map().lock() {
        g.remove(&node_id);
    }
}

pub fn clear_all_trackers() {
    if let Ok(mut g) = hold_map().lock() {
        g.clear();
    }
}

fn to_gray(img: &RgbaImage) -> (u32, u32, Vec<u8>) {
    let (w, h) = img.dimensions();
    let mut g = Vec::with_capacity((w * h) as usize);
    for px in img.pixels() {
        let y = (0.299 * px.0[0] as f32 + 0.587 * px.0[1] as f32 + 0.114 * px.0[2] as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        g.push(y);
    }
    (w, h, g)
}

fn resize_gray(gray: &[u8], gw: u32, gh: u32, nw: u32, nh: u32) -> Vec<u8> {
    if nw == 0 || nh == 0 {
        return vec![];
    }
    if nw == gw && nh == gh {
        return gray.to_vec();
    }
    let mut out = vec![0u8; (nw * nh) as usize];
    for y in 0..nh {
        let sy = (y as u64 * gh as u64) / nh as u64;
        for x in 0..nw {
            let sx = (x as u64 * gw as u64) / nw as u64;
            out[(y * nw + x) as usize] = gray[(sy * gw as u64 + sx) as usize];
        }
    }
    out
}

/// NCC of template over full image; returns (cx, cy, conf, tw, th) in scene pixel space
/// then normalized by caller. Coarse-to-fine for speed.
fn ncc_best_full(
    scene: &[u8],
    sw: u32,
    sh: u32,
    template: &[u8],
    tw: u32,
    th: u32,
) -> Option<(f32, f32, f32)> {
    if tw < 4 || th < 4 || tw >= sw || th >= sh {
        return None;
    }
    if template.len() != (tw * th) as usize {
        return None;
    }
    let n = (tw * th) as f32;
    let mut t_mean = 0.0f32;
    for &v in template {
        t_mean += v as f32;
    }
    t_mean /= n;
    let mut t_var = 0.0f32;
    for &v in template {
        let d = v as f32 - t_mean;
        t_var += d * d;
    }
    if t_var < 1.0 {
        return None; // flat template
    }
    let t_std = t_var.sqrt();

    let x_max = (sw - tw) as i32;
    let y_max = (sh - th) as i32;
    if x_max < 0 || y_max < 0 {
        return None;
    }

    let step = ((tw.min(th) as i32) / 4).clamp(2, 8);
    let mut best = -1.0f32;
    let mut bx = 0i32;
    let mut by = 0i32;

    for y in (0..=y_max).step_by(step as usize) {
        for x in (0..=x_max).step_by(step as usize) {
            let s = ncc_at(scene, sw, template, tw, th, x, y, t_mean, t_std, n);
            if s > best {
                best = s;
                bx = x;
                by = y;
            }
        }
    }
    // Refine locally
    let r = step + 1;
    for y in (by - r).max(0)..=(by + r).min(y_max) {
        for x in (bx - r).max(0)..=(bx + r).min(x_max) {
            let s = ncc_at(scene, sw, template, tw, th, x, y, t_mean, t_std, n);
            if s > best {
                best = s;
                bx = x;
                by = y;
            }
        }
    }

    let cx = (bx as f32 + tw as f32 * 0.5) / sw as f32;
    let cy = (by as f32 + th as f32 * 0.5) / sh as f32;
    Some((cx.clamp(0.0, 1.0), cy.clamp(0.0, 1.0), best.clamp(0.0, 1.0)))
}

#[inline]
fn ncc_at(
    gray: &[u8],
    gw: u32,
    template: &[u8],
    tw: u32,
    th: u32,
    x0: i32,
    y0: i32,
    t_mean: f32,
    t_std: f32,
    n: f32,
) -> f32 {
    let mut i_mean = 0.0f32;
    for y in 0..th {
        let row = ((y0 as u32 + y) * gw + x0 as u32) as usize;
        for x in 0..tw {
            i_mean += gray[row + x as usize] as f32;
        }
    }
    i_mean /= n;
    let mut num = 0.0f32;
    let mut i_var = 0.0f32;
    let mut ti = 0usize;
    for y in 0..th {
        let row = ((y0 as u32 + y) * gw + x0 as u32) as usize;
        for x in 0..tw {
            let iv = gray[row + x as usize] as f32 - i_mean;
            let tv = template[ti] as f32 - t_mean;
            num += iv * tv;
            i_var += iv * iv;
            ti += 1;
        }
    }
    let denom = (i_var.sqrt() * t_std).max(1e-3);
    (num / denom).clamp(-1.0, 1.0)
}

/// Match `targets` (object reference crops) inside `scene`.
///
/// - Tries several scales of each target against a downscaled scene proxy.
/// - If best confidence &lt; `conf_thresh`, returns **last successful** sample for `node_id`
///   (or center with conf=0 if never found).
pub fn track_targets(
    node_id: Uuid,
    scene: &RgbaImage,
    targets: &[RgbaImage],
    conf_thresh: f32,
    media_key: &str,
    time_sec: f64,
) -> CvTrackSample {
    let conf_thresh = conf_thresh.clamp(0.15, 0.95);

    let mut map = hold_map().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(st) = map.get(&node_id) {
        if st.media_key == media_key {
            return st.last.clone();
        }
    }

    let default = CvTrackSample {
        id: 0,
        cx: 0.5,
        cy: 0.5,
        w: 0.1,
        h: 0.1,
        angle_deg: 0.0,
        confidence: 0.0,
        time_sec,
    };

    if targets.is_empty() {
        let last = map
            .get(&node_id)
            .map(|s| {
                let mut l = s.last.clone();
                l.time_sec = time_sec;
                l.confidence = 0.0; // no targets wired
                l
            })
            .unwrap_or(default);
        return last;
    }

    // Scene proxy for speed
    let (sw0, sh0, scene_full) = to_gray(scene);
    let max_side = 360u32;
    let scale_s = if sw0.max(sh0) > max_side {
        max_side as f32 / sw0.max(sh0) as f32
    } else {
        1.0
    };
    let sw = ((sw0 as f32) * scale_s).round().max(8.0) as u32;
    let sh = ((sh0 as f32) * scale_s).round().max(8.0) as u32;
    let scene_g = resize_gray(&scene_full, sw0, sh0, sw, sh);

    // Multi-scale relative sizes for each target (target max-side vs scene min-side)
    let scene_min = sw.min(sh) as f32;
    let scale_factors: &[f32] = &[0.35, 0.5, 0.7, 0.9, 1.1, 1.35];

    let mut best_cx = 0.5f32;
    let mut best_cy = 0.5f32;
    let mut best_conf = -1.0f32;
    let mut best_w = 0.1f32;
    let mut best_h = 0.1f32;

    for target in targets {
        let (tw0, th0, tgray) = to_gray(target);
        if tw0 < 4 || th0 < 4 {
            continue;
        }
        for &sf in scale_factors {
            // Desired template max side as fraction of scene
            let want = (scene_min * 0.12 * sf).clamp(12.0, scene_min * 0.55);
            let t_long = tw0.max(th0) as f32;
            let ts = want / t_long;
            let tw = ((tw0 as f32) * ts).round().clamp(8.0, (sw - 2) as f32) as u32;
            let th = ((th0 as f32) * ts).round().clamp(8.0, (sh - 2) as f32) as u32;
            if tw >= sw || th >= sh {
                continue;
            }
            let tmpl = resize_gray(&tgray, tw0, th0, tw, th);
            if let Some((cx, cy, conf)) = ncc_best_full(&scene_g, sw, sh, &tmpl, tw, th) {
                if conf > best_conf {
                    best_conf = conf;
                    best_cx = cx;
                    best_cy = cy;
                    best_w = tw as f32 / sw as f32;
                    best_h = th as f32 / sh as f32;
                }
            }
        }
    }

    let found = best_conf >= conf_thresh;
    let sample = if found {
        CvTrackSample {
            id: 0,
            cx: best_cx,
            cy: best_cy,
            w: best_w,
            h: best_h,
            angle_deg: 0.0,
            confidence: best_conf,
            time_sec,
        }
    } else {
        // Hold last successive success
        if let Some(st) = map.get(&node_id) {
            let mut l = st.last.clone();
            l.time_sec = time_sec;
            // Keep last confidence as positive-but-stale if we ever found it
            if l.confidence > 0.01 {
                l.confidence = (l.confidence * 0.95).max(0.01);
            } else {
                l.confidence = 0.0;
            }
            l
        } else {
            CvTrackSample {
                confidence: best_conf.max(0.0),
                ..default
            }
        }
    };

    // Only update "last success" when we actually found the object
    if found {
        map.insert(
            node_id,
            HoldState {
                last: sample.clone(),
                media_key: media_key.to_string(),
            },
        );
    } else if let Some(st) = map.get_mut(&node_id) {
        st.media_key = media_key.to_string();
        st.last = sample.clone();
    } else {
        map.insert(
            node_id,
            HoldState {
                last: sample.clone(),
                media_key: media_key.to_string(),
            },
        );
    }

    sample
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn blob_scene(w: u32, h: u32, bx: u32, by: u32, bw: u32, bh: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for px in img.pixels_mut() {
            *px = Rgba([30, 30, 30, 255]);
        }
        for y in by..by + bh {
            for x in bx..bx + bw {
                if x < w && y < h {
                    img.put_pixel(x, y, Rgba([220, 200, 40, 255]));
                }
            }
        }
        img
    }

    #[test]
    fn multi_target_finds_object() {
        let target = blob_scene(32, 32, 4, 4, 24, 24);
        let scene = blob_scene(128, 128, 80, 40, 28, 28);
        let id = Uuid::new_v4();
        let s = track_targets(id, &scene, &[target], 0.35, "s0", 0.0);
        assert!(s.confidence >= 0.35, "conf={}", s.confidence);
        assert!(s.cx > 0.5, "expected rightish cx={}", s.cx);
        clear_tracker(id);
    }

    #[test]
    fn hold_last_when_missing() {
        let target = blob_scene(24, 24, 2, 2, 20, 20);
        let scene_ok = blob_scene(96, 96, 60, 30, 22, 22);
        let scene_empty = blob_scene(96, 96, 0, 0, 0, 0); // all dark
        let id = Uuid::new_v4();
        let s0 = track_targets(id, &scene_ok, &[target.clone()], 0.4, "a", 0.0);
        assert!(s0.confidence >= 0.4, "first conf={}", s0.confidence);
        let s1 = track_targets(id, &scene_empty, &[target], 0.4, "b", 0.1);
        // Hold last position
        assert!((s1.cx - s0.cx).abs() < 0.05, "hold cx {} vs {}", s1.cx, s0.cx);
        assert!((s1.cy - s0.cy).abs() < 0.05);
        clear_tracker(id);
    }
}
