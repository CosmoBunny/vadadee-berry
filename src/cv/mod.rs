//! Computer-vision analysis plumbing for the node editor.
//!
//! Intermediate types, cache keys, in-process result cache, and pure-Rust ops
//! (chroma key, apply mask, background blur). OpenCV / plugins can later write
//! the same [`CvCacheValue`] shapes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod jobs;
pub mod opencv_face;
pub mod ops;
pub mod track;
// `opencv_face` used from Settings UI for cascade availability.
pub use jobs::{
    BLACK_PLACEHOLDER_KEY, FACE_MAX_SIDE, JobOutcome, PREVIEW_MAX_SIDE, ensure_black_placeholder,
    get_or_run_blocking, get_or_schedule, last_good_key, preview_bake_key, remember_last_good,
    take_dirty,
};
pub use ops::{
    apply_mask_rgba, background_blur_rgba, chroma_key_mask, chroma_key_rgba, detect_face_regions,
    privacy_blur_rgba, rgba_to_cache_value,
};

/// Which face detector to use (Settings › CV). Default: Auto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CvFaceBackend {
    /// OpenCV Haar when available, else native skin ROI.
    #[default]
    Auto,
    /// Force OpenCV (falls back to native if cascade missing).
    OpenCv,
    /// Pure-Rust skin-tone ROI (usually faster; good for previews).
    Native,
}

impl CvFaceBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (OpenCV → native)",
            Self::OpenCv => "OpenCV Haar",
            Self::Native => "Native (fast)",
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::OpenCv => 1,
            Self::Native => 2,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::OpenCv,
            2 => Self::Native,
            _ => Self::Auto,
        }
    }
}

static FACE_BACKEND: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn face_backend() -> CvFaceBackend {
    CvFaceBackend::from_u8(FACE_BACKEND.load(std::sync::atomic::Ordering::Relaxed))
}

/// Set face backend and drop face/privacy caches so the next resolve recomputes.
pub fn set_face_backend(b: CvFaceBackend) {
    FACE_BACKEND.store(b.as_u8(), std::sync::atomic::Ordering::Relaxed);
    global_cv_cache().clear_keys_containing("|face|");
    global_cv_cache().clear_keys_containing("|privacy|");
    // Also clear sticky last-good so we don't show wrong backend result.
    jobs::clear_last_good();
    log::info!("[cv] face backend = {:?}", b);
}

/// Detect faces using the Settings-selected backend.
pub fn detect_faces_auto(img: &image::RgbaImage) -> Vec<CvRegion> {
    match face_backend() {
        CvFaceBackend::Native => {
            let regs = detect_face_regions(img);
            log::debug!("[cv] face backend=native n={}", regs.len());
            regs
        }
        CvFaceBackend::OpenCv => {
            // Only accept non-empty OpenCV hits; empty/None → native so preview still works.
            match opencv_face::detect_faces_opencv(img) {
                Some(regs) if !regs.is_empty() => {
                    log::debug!("[cv] face backend=opencv n={}", regs.len());
                    regs
                }
                Some(_) => {
                    log::debug!("[cv] OpenCV 0 faces — native fallback");
                    detect_face_regions(img)
                }
                None => {
                    log::debug!("[cv] OpenCV unavailable — native fallback");
                    detect_face_regions(img)
                }
            }
        }
        CvFaceBackend::Auto => match opencv_face::detect_faces_opencv(img) {
            Some(regs) if !regs.is_empty() => {
                log::debug!("[cv] face backend=auto/opencv n={}", regs.len());
                regs
            }
            _ => {
                let regs = detect_face_regions(img);
                log::debug!(
                    "[cv] face backend=auto/native n={} (opencv_avail={})",
                    regs.len(),
                    opencv_face::opencv_available()
                );
                regs
            }
        },
    }
}

// ── Intermediate types (backend-agnostic) ─────────────────────────────────────

/// Labeled region in normalized image coordinates (0..1, origin top-left).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvRegion {
    /// Semantic label: `"face"`, `"text"`, `"person"`, `"manual"`, …
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub confidence: f32,
    #[serde(default)]
    pub track_id: Option<u32>,
}

impl CvRegion {
    pub fn clamp_norm(mut self) -> Self {
        self.x = self.x.clamp(0.0, 1.0);
        self.y = self.y.clamp(0.0, 1.0);
        self.w = self.w.clamp(0.0, 1.0 - self.x);
        self.h = self.h.clamp(0.0, 1.0 - self.y);
        self.confidence = self.confidence.clamp(0.0, 1.0);
        self
    }
}

/// Single-channel matte (R8), full-frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CvMask {
    pub width: u32,
    pub height: u32,
    /// Length = width * height; 0 = transparent / background, 255 = opaque / subject.
    pub data: Arc<Vec<u8>>,
}

impl CvMask {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        let need = (width as usize).checked_mul(height as usize)?;
        if data.len() != need {
            return None;
        }
        Some(Self {
            width,
            height,
            data: Arc::new(data),
        })
    }

    pub fn solid(width: u32, height: u32, value: u8) -> Self {
        let n = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            data: Arc::new(vec![value; n]),
        }
    }
}

/// One sample of a motion track (normalized coords).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvTrackSample {
    pub id: u32,
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub angle_deg: f32,
    pub confidence: f32,
    pub time_sec: f64,
}

impl CvTrackSample {
    pub fn center_position(&self) -> (f64, f64) {
        (self.cx as f64, self.cy as f64)
    }
}

/// Named joints for armature / pose (later PRs).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CvPose {
    /// joint name → (x, y, confidence) normalized
    pub joints: std::collections::BTreeMap<String, (f32, f32, f32)>,
}

/// Payload stored under one analysis / bake cache key.
#[derive(Debug, Clone)]
pub enum CvCacheValue {
    Mask(CvMask),
    Regions(Vec<CvRegion>),
    Track(CvTrackSample),
    Pose(CvPose),
    /// Materialized RGBA after a spatial effect (chroma, privacy, …).
    Rgba {
        width: u32,
        height: u32,
        data: Arc<Vec<u8>>,
    },
}

impl CvCacheValue {
    pub fn as_mask(&self) -> Option<&CvMask> {
        match self {
            Self::Mask(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_regions(&self) -> Option<&[CvRegion]> {
        match self {
            Self::Regions(r) => Some(r.as_slice()),
            _ => None,
        }
    }

    pub fn as_track(&self) -> Option<&CvTrackSample> {
        match self {
            Self::Track(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_rgba(&self) -> Option<(u32, u32, &[u8])> {
        match self {
            Self::Rgba {
                width,
                height,
                data,
            } => Some((*width, *height, data.as_slice())),
            _ => None,
        }
    }
}

// ── Analyze context / cache keys ──────────────────────────────────────────────

/// Job name for analyze backends (`chroma`, `detect`, `track`, `matte`, `bake`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CvJob(&'static str);

impl CvJob {
    pub const CHROMA: Self = Self("chroma");
    pub const DETECT: Self = Self("detect");
    pub const TRACK: Self = Self("track");
    pub const MATTE: Self = Self("matte");
    pub const BAKE: Self = Self("bake");
    pub const PRIVACY: Self = Self("privacy");
    pub const BG_BLUR: Self = Self("bg_blur");

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

/// Inputs that identify a unique analysis result for a graph node at a media time.
#[derive(Debug, Clone)]
pub struct CvAnalyzeContext {
    pub job: CvJob,
    /// Media file path (empty if pure procedural / baked-only).
    pub path: String,
    /// Media time in seconds (None = still / untimed).
    pub time_sec: Option<f64>,
    /// Node that owns the analyze / materialize step.
    pub node_id: Uuid,
    /// Stable hash of node params (tolerance, labels, seed box, …).
    pub params_hash: u64,
    /// Optional frame size stamp so resizes invalidate.
    pub width: u32,
    pub height: u32,
}

impl CvAnalyzeContext {
    /// Snap time to milliseconds (matches graph media cache snapping).
    pub fn time_ms(&self) -> Option<i64> {
        self.time_sec.map(|t| ((t * 1000.0).floor() as i64).max(0))
    }

    /// Stable string key for [`CvCache`].
    pub fn cache_key(&self) -> String {
        let t = self
            .time_ms()
            .map(|ms| format!("t{ms}"))
            .unwrap_or_else(|| "still".into());
        format!(
            "cv|{}|{}|{}|n{}|p{:x}|{}x{}",
            self.job.as_str(),
            self.path,
            t,
            self.node_id.as_simple(),
            self.params_hash,
            self.width,
            self.height
        )
    }
}

/// FNV-1a 64-bit hash for small param blobs (no extra deps).
pub fn hash_params_bytes(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Hash common scalar param lists (order-sensitive).
pub fn hash_params_f64(vals: &[f64]) -> u64 {
    let mut bytes = Vec::with_capacity(vals.len() * 8);
    for v in vals {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    hash_params_bytes(&bytes)
}

// ── Process-wide cache ────────────────────────────────────────────────────────

/// Soft cap so long scrubbing sessions do not grow forever.
const DEFAULT_CAP: usize = 256;

#[derive(Debug, Default)]
struct CvCacheInner {
    map: HashMap<String, CvCacheValue>,
    /// Insertion / touch order for crude LRU eviction (front = oldest).
    order: Vec<String>,
    cap: usize,
}

impl CvCacheInner {
    fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: Vec::new(),
            // Allow small caps in tests; production uses DEFAULT_CAP.
            cap: cap.max(1),
        }
    }

    fn touch(&mut self, key: &str) {
        if let Some(i) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(i);
            self.order.push(k);
        }
    }

    fn insert(&mut self, key: String, value: CvCacheValue) {
        if self.map.contains_key(&key) {
            self.touch(&key);
            self.map.insert(key, value);
            return;
        }
        while self.map.len() >= self.cap && !self.order.is_empty() {
            if let Some(old) = self.order.first().cloned() {
                self.order.remove(0);
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push(key.clone());
        self.map.insert(key, value);
    }

    fn get(&mut self, key: &str) -> Option<CvCacheValue> {
        if self.map.contains_key(key) {
            self.touch(key);
            self.map.get(key).cloned()
        } else {
            None
        }
    }
}

/// Thread-safe analysis / bake cache shared by preview and export.
#[derive(Debug)]
pub struct CvCache {
    inner: Mutex<CvCacheInner>,
}

impl CvCache {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Mutex::new(CvCacheInner::new(cap)),
        }
    }

    pub fn insert(&self, key: impl Into<String>, value: CvCacheValue) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(key.into(), value);
        }
    }

    pub fn get(&self, key: &str) -> Option<CvCacheValue> {
        self.inner.lock().ok().and_then(|mut g| g.get(key))
    }

    pub fn get_rgba_image(&self, key: &str) -> Option<image::RgbaImage> {
        let v = self.get(key)?;
        let (w, h, data) = v.as_rgba()?;
        image::RgbaImage::from_raw(w, h, data.to_vec())
    }

    pub fn contains(&self, key: &str) -> bool {
        self.inner
            .lock()
            .ok()
            .map(|g| g.map.contains_key(key))
            .unwrap_or(false)
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.map.clear();
            g.order.clear();
        }
    }

    /// Drop cache entries whose key contains `needle` (e.g. `"|face|"`).
    pub fn clear_keys_containing(&self, needle: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.map.retain(|k, _| !k.contains(needle));
            g.order.retain(|k| !k.contains(needle));
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().ok().map(|g| g.map.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for CvCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAP)
    }
}

static GLOBAL_CV_CACHE: OnceLock<CvCache> = OnceLock::new();

/// Process-wide CV result cache (preview + export share keys).
pub fn global_cv_cache() -> &'static CvCache {
    GLOBAL_CV_CACHE.get_or_init(CvCache::default)
}

/// Build a media-aware analyze context from graph eval fields.
pub fn analyze_context_for_media(
    job: CvJob,
    path: &str,
    time_sec: Option<f64>,
    node_id: Uuid,
    params_hash: u64,
    width: u32,
    height: u32,
) -> CvAnalyzeContext {
    CvAnalyzeContext {
        job,
        path: path.to_string(),
        time_sec,
        node_id,
        params_hash,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_stable_for_same_context() {
        let id = Uuid::nil();
        let a = CvAnalyzeContext {
            job: CvJob::CHROMA,
            path: "/tmp/clip.mp4".into(),
            time_sec: Some(1.23456),
            node_id: id,
            params_hash: 0xabc,
            width: 1920,
            height: 1080,
        };
        let b = CvAnalyzeContext {
            time_sec: Some(1.23499), // same ms floor
            ..a.clone()
        };
        assert_eq!(a.cache_key(), b.cache_key());
        assert!(a.cache_key().contains("t1234"));
        assert!(a.cache_key().contains("chroma"));
    }

    #[test]
    fn cache_key_differs_on_time_ms_or_params() {
        let id = Uuid::nil();
        let base = CvAnalyzeContext {
            job: CvJob::DETECT,
            path: "/v.mp4".into(),
            time_sec: Some(0.0),
            node_id: id,
            params_hash: 1,
            width: 64,
            height: 64,
        };
        let t2 = CvAnalyzeContext {
            time_sec: Some(0.002),
            ..base.clone()
        };
        let p2 = CvAnalyzeContext {
            params_hash: 2,
            ..base.clone()
        };
        assert_ne!(base.cache_key(), t2.cache_key());
        assert_ne!(base.cache_key(), p2.cache_key());
    }

    #[test]
    fn cache_roundtrip_mask_and_lru() {
        let cache = CvCache::new(2);
        let m = CvMask::solid(2, 2, 255);
        cache.insert("k1", CvCacheValue::Mask(m.clone()));
        cache.insert("k2", CvCacheValue::Regions(vec![]));
        assert!(cache.contains("k1"));
        // Evict oldest when inserting third.
        cache.insert(
            "k3",
            CvCacheValue::Track(CvTrackSample {
                id: 0,
                cx: 0.5,
                cy: 0.5,
                w: 0.1,
                h: 0.1,
                angle_deg: 0.0,
                confidence: 1.0,
                time_sec: 0.0,
            }),
        );
        assert!(!cache.contains("k1"), "oldest key should be evicted");
        assert!(cache.contains("k2"));
        assert!(cache.contains("k3"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn hash_params_deterministic() {
        assert_eq!(hash_params_f64(&[1.0, 2.0]), hash_params_f64(&[1.0, 2.0]));
        assert_ne!(hash_params_f64(&[1.0, 2.0]), hash_params_f64(&[2.0, 1.0]));
    }

    #[test]
    fn mask_len_check() {
        assert!(CvMask::new(2, 2, vec![0; 4]).is_some());
        assert!(CvMask::new(2, 2, vec![0; 3]).is_none());
    }
}
