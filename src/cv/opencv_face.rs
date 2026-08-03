//! Optional OpenCV Haar face detection. Falls back when cascade/lib unavailable.
//!
//! Enabled with Cargo feature `opencv` (links system libopencv). Runtime still
//! falls through to native if cascade load fails or finds no faces.

use image::RgbaImage;

use super::CvRegion;

/// Try OpenCV frontal-face cascade.
///
/// - `None` = OpenCV unavailable / hard failure → caller should use native
/// - `Some(empty)` should be rare after internal retries; prefer treating empty as miss
pub fn detect_faces_opencv(img: &RgbaImage) -> Option<Vec<CvRegion>> {
    #[cfg(feature = "opencv")]
    {
        detect_faces_opencv_inner(img)
    }
    #[cfg(not(feature = "opencv"))]
    {
        let _ = img;
        None
    }
}

pub fn opencv_available() -> bool {
    #[cfg(feature = "opencv")]
    {
        !cascade_paths().is_empty()
    }
    #[cfg(not(feature = "opencv"))]
    {
        false
    }
}

#[cfg(feature = "opencv")]
fn cascade_paths() -> Vec<std::path::PathBuf> {
    const CANDIDATES: &[&str] = &[
        // Prefer alt2 — often more robust on selfies / uneven light
        "/usr/share/opencv4/haarcascades/haarcascade_frontalface_alt2.xml",
        "/usr/share/opencv4/haarcascades/haarcascade_frontalface_default.xml",
        "/usr/share/opencv4/haarcascades/haarcascade_frontalface_alt.xml",
        "/usr/share/opencv/haarcascades/haarcascade_frontalface_alt2.xml",
        "/usr/share/opencv/haarcascades/haarcascade_frontalface_default.xml",
        "/usr/local/share/opencv4/haarcascades/haarcascade_frontalface_alt2.xml",
        "/usr/share/OpenCV/haarcascades/haarcascade_frontalface_default.xml",
    ];
    CANDIDATES
        .iter()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(feature = "opencv")]
fn detect_faces_opencv_inner(img: &RgbaImage) -> Option<Vec<CvRegion>> {
    use opencv::core::{Mat, Size, Vector, CV_8UC1};
    use opencv::imgproc;
    use opencv::objdetect::CascadeClassifier;
    use opencv::prelude::*;

    let paths = cascade_paths();
    if paths.is_empty() {
        log::warn!("[cv] OpenCV: no Haar cascade XML found on disk");
        return None;
    }

    let (w, h) = img.dimensions();
    if w < 16 || h < 16 {
        return None;
    }

    // Owned grayscale Mat (h × w), row-major — copy so we don't depend on slice lifetime.
    let mut gray_buf = vec![0u8; (w as usize) * (h as usize)];
    for (i, px) in img.pixels().enumerate() {
        let r = px.0[0] as f32;
        let g = px.0[1] as f32;
        let b = px.0[2] as f32;
        gray_buf[i] = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
    }

    let mut gray = Mat::new_rows_cols_with_default(
        h as i32,
        w as i32,
        CV_8UC1,
        opencv::core::Scalar::all(0.0),
    )
    .ok()?;
    // Copy row by row in case Mat has padding.
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let v = gray_buf[(y as u32 * w + x as u32) as usize];
            *gray.at_2d_mut::<u8>(y, x).ok()? = v;
        }
    }

    let mut eq = Mat::default();
    if imgproc::equalize_hist(&gray, &mut eq).is_err() {
        // Use gray as-is if equalize fails
        eq = gray.clone();
    }

    // Try several cascade files / param sets until we get hits.
    let min_side_loose = ((w.min(h) as f64) * 0.05).max(20.0) as i32;
    let min_side_tight = ((w.min(h) as f64) * 0.08).max(24.0) as i32;
    let param_sets: &[(f64, i32, i32)] = &[
        (1.05, 3, min_side_loose), // sensitive
        (1.1, 3, min_side_tight),
        (1.1, 2, min_side_loose),
        (1.2, 3, min_side_tight),
    ];

    let mut best: Vec<CvRegion> = Vec::new();
    for path in &paths {
        let mut cascade = match CascadeClassifier::new(path.to_str().unwrap_or("")) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("[cv] cascade load failed {:?}: {}", path, e);
                continue;
            }
        };
        if cascade.empty().unwrap_or(true) {
            continue;
        }
        for &(scale, neighbors, min_sz) in param_sets {
            let mut faces = Vector::<opencv::core::Rect>::new();
            if cascade
                .detect_multi_scale(
                    &eq,
                    &mut faces,
                    scale,
                    neighbors,
                    0,
                    Size::new(min_sz, min_sz),
                    Size::new(0, 0),
                )
                .is_err()
            {
                continue;
            }
            if faces.is_empty() {
                continue;
            }
            let wf = w as f32;
            let hf = h as f32;
            let mut out = Vec::new();
            for i in 0..faces.len() {
                let Ok(r) = faces.get(i) else { continue };
                if r.width < 8 || r.height < 8 {
                    continue;
                }
                let pad = 0.08f32;
                let pad_x = r.width as f32 * pad;
                let pad_y = r.height as f32 * pad;
                let x0 = (r.x as f32 - pad_x).max(0.0) / wf;
                let y0 = (r.y as f32 - pad_y).max(0.0) / hf;
                let x1 = (r.x as f32 + r.width as f32 + pad_x).min(wf) / wf;
                let y1 = (r.y as f32 + r.height as f32 + pad_y).min(hf) / hf;
                out.push(
                    CvRegion {
                        label: "face".into(),
                        x: x0,
                        y: y0,
                        w: (x1 - x0).max(0.02),
                        h: (y1 - y0).max(0.02),
                        confidence: 0.9,
                        track_id: None,
                    }
                    .clamp_norm(),
                );
            }
            if out.len() > best.len() {
                best = out;
            }
            // First solid hit is enough
            if !best.is_empty() {
                log::info!(
                    "[cv] OpenCV Haar {} faces via {} (scale={} neigh={} min={})",
                    best.len(),
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    scale,
                    neighbors,
                    min_sz
                );
                return Some(best);
            }
        }
    }

    if best.is_empty() {
        log::debug!(
            "[cv] OpenCV Haar found 0 faces on {}x{} (cascades tried={})",
            w,
            h,
            paths.len()
        );
        // Signal "try native" rather than "confident empty"
        return None;
    }
    Some(best)
}
