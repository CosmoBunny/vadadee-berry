//! Background CV jobs so node-editor resolve never blocks the UI thread.
//!
//! Live preview: cache hit → ready; miss → spawn worker, return Pending.
//! Export: [`get_or_run_blocking`].
//!
//! **Sticky last-good:** when params change, preview keeps the previous bake key
//! until the new job finishes. First bake (no previous) uses a black placeholder.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::{global_cv_cache, CvCacheValue};
use uuid::Uuid;

static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
/// node_id → last successful bake/regions cache key (for sticky preview).
static LAST_GOOD: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();
/// Set when a background job finishes — UI should request repaint.
static DIRTY: AtomicBool = AtomicBool::new(false);
/// Export / tests: run CV inline instead of scheduling.
static FORCE_SYNC: AtomicBool = AtomicBool::new(false);

/// Synthetic bake key for first-time pending preview (1×1 black RGBA in cache).
pub const BLACK_PLACEHOLDER_KEY: &str = "cv|placeholder|black";

/// Run `f` with CV jobs forced synchronous (export path).
pub fn with_sync_cv<R>(f: impl FnOnce() -> R) -> R {
    FORCE_SYNC.store(true, Ordering::SeqCst);
    let r = f();
    FORCE_SYNC.store(false, Ordering::SeqCst);
    r
}

fn pending() -> &'static Mutex<HashSet<String>> {
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn last_good() -> &'static Mutex<HashMap<Uuid, String>> {
    LAST_GOOD.get_or_init(|| Mutex::new(HashMap::new()))
}

/// True if any CV job completed since last call (clears the flag).
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::SeqCst)
}

pub fn is_pending(key: &str) -> bool {
    pending()
        .lock()
        .ok()
        .map(|g| g.contains(key))
        .unwrap_or(false)
}

pub fn is_pending_any() -> bool {
    pending()
        .lock()
        .ok()
        .map(|g| !g.is_empty())
        .unwrap_or(false)
}

/// Remember a successful cache key for sticky preview on later param changes.
pub fn remember_last_good(node_id: Uuid, key: impl Into<String>) {
    if let Ok(mut g) = last_good().lock() {
        g.insert(node_id, key.into());
    }
}

pub fn last_good_key(node_id: Uuid) -> Option<String> {
    last_good().lock().ok().and_then(|g| g.get(&node_id).cloned())
}

pub fn clear_last_good() {
    if let Ok(mut g) = last_good().lock() {
        g.clear();
    }
}

/// Ensure 1×1 opaque black RGBA exists under [`BLACK_PLACEHOLDER_KEY`].
pub fn ensure_black_placeholder() {
    if global_cv_cache().contains(BLACK_PLACEHOLDER_KEY) {
        return;
    }
    global_cv_cache().insert(
        BLACK_PLACEHOLDER_KEY,
        CvCacheValue::Rgba {
            width: 1,
            height: 1,
            data: std::sync::Arc::new(vec![0, 0, 0, 255]),
        },
    );
}

pub enum JobOutcome {
    Ready(CvCacheValue),
    /// Work scheduled or already running.
    ///
    /// For **image bakes**: show last-good if any, else black placeholder.
    /// For **regions**: use last-good regions if any, else empty.
    Pending,
}

/// Cache lookup or schedule `work` on a background thread (never runs heavy work inline).
///
/// On success, updates sticky last-good for `node_id` when provided.
pub fn get_or_schedule(
    key: String,
    node_id: Option<Uuid>,
    work: impl FnOnce() -> CvCacheValue + Send + 'static,
) -> JobOutcome {
    if let Some(v) = global_cv_cache().get(&key) {
        if let Some(nid) = node_id {
            remember_last_good(nid, key.clone());
        }
        return JobOutcome::Ready(v);
    }
    if FORCE_SYNC.load(Ordering::SeqCst) {
        let v = get_or_run_blocking(key.clone(), work);
        if let Some(nid) = node_id {
            remember_last_good(nid, key);
        }
        return JobOutcome::Ready(v);
    }
    {
        let mut g = match pending().lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if g.contains(&key) {
            return JobOutcome::Pending;
        }
        g.insert(key.clone());
    }
    let key_bg = key.clone();
    let _ = std::thread::Builder::new()
        .name("vadadee-cv".into())
        .spawn(move || {
            let v = work();
            global_cv_cache().insert(key_bg.clone(), v);
            if let Some(nid) = node_id {
                remember_last_good(nid, key_bg.clone());
            }
            if let Ok(mut g) = pending().lock() {
                g.remove(&key_bg);
            }
            DIRTY.store(true, Ordering::SeqCst);
        });
    JobOutcome::Pending
}

/// Resolve a bake key for preview: exact hit, or sticky last-good, or black.
pub fn preview_bake_key(node_id: Uuid, desired_key: &str, outcome: &JobOutcome) -> String {
    match outcome {
        JobOutcome::Ready(_) => {
            remember_last_good(node_id, desired_key);
            desired_key.to_string()
        }
        JobOutcome::Pending => {
            if let Some(prev) = last_good_key(node_id) {
                if global_cv_cache().contains(&prev) {
                    return prev;
                }
            }
            ensure_black_placeholder();
            BLACK_PLACEHOLDER_KEY.to_string()
        }
    }
}

/// For export / tests: run inline if missing (still uses cache).
pub fn get_or_run_blocking(
    key: String,
    work: impl FnOnce() -> CvCacheValue,
) -> CvCacheValue {
    if let Some(v) = global_cv_cache().get(&key) {
        return v;
    }
    // Wait briefly if another thread is computing the same key.
    for _ in 0..200 {
        if !is_pending(&key) {
            break;
        }
        if let Some(v) = global_cv_cache().get(&key) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if let Some(v) = global_cv_cache().get(&key) {
        return v;
    }
    let v = work();
    global_cv_cache().insert(key, v.clone());
    v
}

/// Max long-side for live CV previews (full-res only on export if needed).
pub const PREVIEW_MAX_SIDE: u32 = 640;
/// Face detect always runs on a small proxy.
pub const FACE_MAX_SIDE: u32 = 320;
