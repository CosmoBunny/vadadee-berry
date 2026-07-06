//! Performance thresholds for dense documents (pixel art, 10k+ rects).

/// Above this count, selection/drag uses bulk paths (no per-node overlay / single history).
pub const BULK_SELECTION_THRESHOLD: usize = 500;

/// Skip per-node vector selection overlay when layer raster cache is warm.
pub const BULK_OVERLAY_SKIP: usize = 150;