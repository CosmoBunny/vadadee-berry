//! Septic screen record (canonical `.sepscrr`) — video + mouse track.
//! Keyboard is intentionally out of scope for v1.
//! Septic Player samples by truth time; Mouse Encoder derives pos / shakiness / event.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// Magic bytes at start of `.sepscrr` JSON wrapper (file is JSON for v1).
pub const SEPSCRR_VERSION: u32 = 1;

/// Primary button event codes (Mouse Encoder / Septic Player).
pub const EVENT_NOTHING: f64 = 0.0;
pub const EVENT_CLICKED: f64 = 1.0;

/// One mouse sample on the septic timeline (truth time).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MouseSample {
    /// Seconds from session start (truth time).
    pub t: f64,
    /// Normalized 0..1 (or pixels if `meta.normalized == false`).
    pub x: f64,
    pub y: f64,
    /// Primary button held.
    #[serde(default)]
    pub button_down: bool,
}

impl Default for MouseSample {
    fn default() -> Self {
        Self {
            t: 0.0,
            x: 0.5,
            y: 0.5,
            button_down: false,
        }
    }
}

/// Session metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SepticMeta {
    pub version: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: f64,
    /// Duration in seconds (truth).
    #[serde(default)]
    pub duration_sec: f64,
    /// Coordinates stored as 0..1.
    #[serde(default = "default_true")]
    pub normalized: bool,
    /// Whether OS capturer burned the system cursor into video pixels.
    #[serde(default)]
    pub cursor_in_pixels: bool,
    /// Optional path to muxed/sibling video (relative or absolute).
    #[serde(default)]
    pub video_path: String,
    #[serde(default)]
    pub source_label: String,
}

fn default_fps() -> f64 {
    60.0
}
fn default_true() -> bool {
    true
}

impl Default for SepticMeta {
    fn default() -> Self {
        Self {
            version: SEPSCRR_VERSION,
            width: 1920,
            height: 1080,
            fps: 60.0,
            duration_sec: 0.0,
            normalized: true,
            cursor_in_pixels: true,
            video_path: String::new(),
            source_label: String::new(),
        }
    }
}

/// Canonical septic session (in-memory / on disk as JSON `.sepscrr`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SepticSession {
    pub meta: SepticMeta,
    /// Sorted by `t` ascending.
    #[serde(default)]
    pub mouse: Vec<MouseSample>,
}

struct SepticCacheEntry {
    mtime: SystemTime,
    len: u64,
    session: Arc<SepticSession>,
}

fn septic_session_cache() -> &'static Mutex<HashMap<String, SepticCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SepticCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn septic_cache_invalidate(path: &Path) {
    let key = path.to_string_lossy();
    if let Ok(mut g) = septic_session_cache().lock() {
        g.remove(key.as_ref());
    }
}

impl SepticSession {
    pub fn new_empty() -> Self {
        Self::default()
    }

    pub fn load_path(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("read sepscrr: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("parse sepscrr: {e}"))
    }

    /// Load with process-wide `Arc` cache (mtime+len). Hot path: eval_reals / resolve
    /// every frame. Uncached re-parse of large mouse tracks was crushing UI FPS.
    pub fn load_path_cached(path: &Path) -> Result<Self, String> {
        Ok((*Self::load_path_cached_arc(path)?).clone())
    }

    /// Same as [`load_path_cached`] but returns `Arc` so callers avoid deep-cloning
    /// the mouse track when they only need to sample.
    pub fn load_path_cached_arc(path: &Path) -> Result<Arc<Self>, String> {
        let key = path.to_string_lossy().into_owned();
        let meta = std::fs::metadata(path).map_err(|e| format!("stat sepscrr: {e}"))?;
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let len = meta.len();

        if let Ok(guard) = septic_session_cache().lock() {
            if let Some(e) = guard.get(&key) {
                if e.mtime == mtime && e.len == len {
                    return Ok(e.session.clone());
                }
            }
        }

        let session = Arc::new(Self::load_path(path)?);
        if let Ok(mut guard) = septic_session_cache().lock() {
            if guard.len() > 32 {
                guard.clear();
            }
            guard.insert(
                key,
                SepticCacheEntry {
                    mtime,
                    len,
                    session: session.clone(),
                },
            );
        }
        Ok(session)
    }

    /// Drop cached entry so the next load sees a fresh write.
    pub fn invalidate_cache(path: &Path) {
        septic_cache_invalidate(path);
    }

    pub fn save_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(path, raw).map_err(|e| format!("write sepscrr: {e}"))?;
        septic_cache_invalidate(path);
        Ok(())
    }

    /// Clamp request time into session.
    pub fn truth_time(&self, request_sec: f64) -> f64 {
        let d = self.meta.duration_sec.max(0.0);
        if d <= 0.0 {
            return request_sec.max(0.0);
        }
        request_sec.clamp(0.0, d)
    }

    /// Sample mouse at truth time `t` (linear interp of position; button from nearest prior).
    pub fn sample_mouse(&self, t: f64) -> MouseSample {
        let t = self.truth_time(t);
        if self.mouse.is_empty() {
            return MouseSample {
                t,
                ..MouseSample::default()
            };
        }
        // Before first
        if t <= self.mouse[0].t {
            let mut s = self.mouse[0];
            s.t = t;
            return s;
        }
        // After last
        if let Some(last) = self.mouse.last() {
            if t >= last.t {
                let mut s = *last;
                s.t = t;
                return s;
            }
        }
        // Binary search interval
        let mut lo = 0usize;
        let mut hi = self.mouse.len() - 1;
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.mouse[mid].t <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let a = self.mouse[lo];
        let b = self.mouse[hi];
        let dt = (b.t - a.t).max(1e-9);
        let u = ((t - a.t) / dt).clamp(0.0, 1.0);
        MouseSample {
            t,
            x: a.x + (b.x - a.x) * u,
            y: a.y + (b.y - a.y) * u,
            button_down: a.button_down,
        }
    }

    /// Event at time `t`: 1 if primary down-edge in a small window around t, else 0.
    pub fn sample_event(&self, t: f64) -> f64 {
        let t = self.truth_time(t);
        // Window half-width ~ one frame at session fps
        let half = (0.5 / self.meta.fps.max(1.0)).max(1.0 / 240.0);
        let mut prev_down = false;
        for s in &self.mouse {
            if s.t < t - half {
                prev_down = s.button_down;
                continue;
            }
            if s.t > t + half {
                break;
            }
            if s.button_down && !prev_down {
                return EVENT_CLICKED;
            }
            prev_down = s.button_down;
        }
        EVENT_NOTHING
    }

    /// Samples in `[t - window, t]` (inclusive), oldest first.
    pub fn mouse_window(&self, t: f64, window_sec: f64) -> Vec<MouseSample> {
        let t = self.truth_time(t);
        let start = (t - window_sec.max(0.0)).max(0.0);
        self.mouse
            .iter()
            .copied()
            .filter(|s| s.t >= start - 1e-12 && s.t <= t + 1e-12)
            .collect()
    }
}

/// Mouse Encoder: pos + shakiness + event from a mouse track window.
#[derive(Debug, Clone, Copy)]
pub struct MouseEncoderParams {
    /// History window (seconds) for shakiness — “time threshold”.
    pub time_threshold: f64,
    /// Scales raw shake into 0..1.
    pub gain: f64,
}

impl Default for MouseEncoderParams {
    fn default() -> Self {
        Self {
            time_threshold: 0.20,
            gain: 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseEncoderOut {
    pub x: f64,
    pub y: f64,
    pub shakiness: f64,
    pub event: f64,
    pub time: f64,
}

/// Path residual (tremor) → soft-saturated shakiness in 0..1 (no hard snap).
pub fn encode_mouse(
    session: &SepticSession,
    time_sec: f64,
    params: MouseEncoderParams,
) -> MouseEncoderOut {
    let t = session.truth_time(time_sec);
    let sample = session.sample_mouse(t);
    let event = session.sample_event(t);
    let thr = params.time_threshold.max(1e-4);
    // Multi-scale: short window reacts, long window anchors — blend for smooth ramps.
    let win_short = session.mouse_window(t, (thr * 0.45).max(0.04));
    let win_long = session.mouse_window(t, thr);
    let s_short = shakiness_from_samples(&win_short, params.gain);
    let s_long = shakiness_from_samples(&win_long, params.gain);
    // Prefer long-window (smoother); short only lifts slightly on brief spikes.
    let shakiness = (0.62 * s_long + 0.38 * s_short).clamp(0.0, 1.0);
    MouseEncoderOut {
        x: sample.x,
        y: sample.y,
        shakiness,
        event,
        time: t,
    }
}

/// Shakiness from an explicit sample list (for tests / live buffers).
///
/// Uses **path residual** (polyline length − endpoint chord) rather than speed
/// stddev: steady pans stay low; tremor raises residual. Soft exponential gain
/// avoids sudden 0→1 jumps when a single segment is noisy.
pub fn shakiness_from_samples(samples: &[MouseSample], gain: f64) -> f64 {
    if samples.len() < 3 {
        return 0.0;
    }
    let first = samples[0];
    let last = *samples.last().unwrap();
    let dt = (last.t - first.t).max(1e-4);

    let mut path_len = 0.0_f64;
    let mut accel_e = 0.0_f64;
    let mut prev_speed = None::<f64>;
    for w in samples.windows(2) {
        let a = w[0];
        let b = w[1];
        let seg_dt = (b.t - a.t).max(1e-6);
        let dist = (b.x - a.x).hypot(b.y - a.y).max(0.0);
        path_len += dist;
        let speed = dist / seg_dt;
        if let Some(ps) = prev_speed {
            // |Δspeed| / dt ≈ |acceleration|; tremor → high.
            accel_e += ((speed - ps).abs() / seg_dt).min(500.0);
        }
        prev_speed = Some(speed);
    }
    let n_seg = (samples.len() - 1) as f64;
    let chord = (last.x - first.x).hypot(last.y - first.y);
    // Extra path length beyond pure translation (screen-normalized units / sec).
    let residual_rate = ((path_len - chord).max(0.0)) / dt;
    let mean_accel = accel_e / n_seg.max(1.0);

    // Blend residual + acceleration energy; soft-knee via 1-exp(-k·x).
    let g = gain.max(0.0);
    // Empirical scale: residual_rate ~0.05–0.3 for visible tremor, mean_accel larger.
    let energy = residual_rate * 1.15 + mean_accel * 0.00035;
    let raw = 1.0 - (-g * energy * 0.55).exp();
    raw.clamp(0.0, 1.0)
}

/// Resolve video path relative to septic file directory.
pub fn resolve_video_path(septic_path: &Path, meta: &SepticMeta) -> Option<PathBuf> {
    if meta.video_path.is_empty() {
        // Convention: sibling `.mp4` / same stem
        let stem = septic_path.file_stem()?.to_str()?;
        let parent = septic_path.parent().unwrap_or_else(|| Path::new("."));
        for ext in ["mp4", "webm", "mkv", "mov"] {
            let p = parent.join(format!("{stem}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        return None;
    }
    let p = PathBuf::from(&meta.video_path);
    if p.is_file() {
        return Some(p);
    }
    if let Some(parent) = septic_path.parent() {
        let joined = parent.join(&meta.video_path);
        if joined.is_file() {
            return Some(joined);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_jitter() -> SepticSession {
        let mut s = SepticSession::new_empty();
        s.meta.duration_sec = 1.0;
        s.meta.fps = 60.0;
        // Steady then shaky
        for i in 0..10 {
            s.mouse.push(MouseSample {
                t: i as f64 * 0.02,
                x: 0.1 + i as f64 * 0.01,
                y: 0.5,
                button_down: false,
            });
        }
        // Tremor
        for i in 0..10 {
            let j = i as f64;
            s.mouse.push(MouseSample {
                t: 0.2 + j * 0.02,
                x: 0.2 + if i % 2 == 0 { 0.02 } else { -0.02 },
                y: 0.5 + if i % 2 == 0 { 0.015 } else { -0.015 },
                button_down: i == 5,
            });
        }
        // click edge at sample where button goes true
        s
    }

    #[test]
    fn shakiness_higher_on_jitter() {
        let s = session_with_jitter();
        let steady = encode_mouse(
            &s,
            0.1,
            MouseEncoderParams {
                time_threshold: 0.20,
                gain: 6.0,
            },
        );
        let shaky = encode_mouse(
            &s,
            0.35,
            MouseEncoderParams {
                time_threshold: 0.20,
                gain: 6.0,
            },
        );
        assert!(
            shaky.shakiness > steady.shakiness,
            "steady={} shaky={}",
            steady.shakiness,
            shaky.shakiness
        );
        // Soft metric: pure pan should stay clearly below mid-range.
        assert!(
            steady.shakiness < 0.35,
            "steady pan should not look very shaky: {}",
            steady.shakiness
        );
    }

    #[test]
    fn click_event_code() {
        let mut s = SepticSession::new_empty();
        s.meta.duration_sec = 1.0;
        s.meta.fps = 60.0;
        s.mouse = vec![
            MouseSample {
                t: 0.0,
                x: 0.5,
                y: 0.5,
                button_down: false,
            },
            MouseSample {
                t: 0.1,
                x: 0.5,
                y: 0.5,
                button_down: true,
            },
            MouseSample {
                t: 0.2,
                x: 0.5,
                y: 0.5,
                button_down: true,
            },
        ];
        assert_eq!(s.sample_event(0.05), EVENT_NOTHING);
        assert_eq!(s.sample_event(0.1), EVENT_CLICKED);
    }

    #[test]
    fn roundtrip_json() {
        let mut s = SepticSession::new_empty();
        s.meta.duration_sec = 2.0;
        s.mouse.push(MouseSample {
            t: 0.5,
            x: 0.25,
            y: 0.75,
            button_down: true,
        });
        let dir = std::env::temp_dir().join(format!("septic_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.sepscrr");
        s.save_path(&path).unwrap();
        let loaded = SepticSession::load_path(&path).unwrap();
        assert_eq!(loaded.mouse.len(), 1);
        assert!((loaded.mouse[0].x - 0.25).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
