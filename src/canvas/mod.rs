use egui::{Pos2, Rect, Vec2};

#[derive(Debug, Clone)]
pub struct Viewport {
    pub pan: Vec2,
    pub zoom: f32,
    pub show_grid: bool,
    pub snap_grid: bool,
    pub grid_step: f32,
    /// Static grid: divide page width into this many columns (0 = use `grid_step`).
    pub grid_cols: u32,
    /// Static grid: divide page height into this many rows (0 = use `grid_step`).
    pub grid_rows: u32,
    /// Page size for static grid division (document units).
    pub page_width: f32,
    pub page_height: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
            show_grid: false,
            snap_grid: true,
            grid_step: 20.0,
            grid_cols: 0,
            grid_rows: 0,
            page_width: 794.0,
            page_height: 1123.0,
        }
    }
}

impl Viewport {
    pub fn zoom_at(&mut self, screen: Pos2, origin: Pos2, factor: f32) {
        let before = self.screen_to_doc(screen, origin);
        self.zoom = (self.zoom * factor).clamp(0.05, 32.0);
        let after = self.screen_to_doc(screen, origin);
        self.pan.x += (after.0 - before.0) as f32 * self.zoom;
        self.pan.y += (after.1 - before.1) as f32 * self.zoom;
    }

    pub fn screen_to_doc(&self, screen: Pos2, origin: Pos2) -> (f64, f64) {
        let x = (screen.x - origin.x - self.pan.x) as f64 / self.zoom as f64;
        let y = (screen.y - origin.y - self.pan.y) as f64 / self.zoom as f64;
        (x, y)
    }

    pub fn doc_to_screen(&self, doc: (f64, f64), origin: Pos2) -> Pos2 {
        Pos2::new(
            origin.x + self.pan.x + doc.0 as f32 * self.zoom,
            origin.y + self.pan.y + doc.1 as f32 * self.zoom,
        )
    }

    /// Zoom + pan framing the whole document in the available view with
    /// padding, preserving aspect ratio:
    /// `scale = min(view_w / doc_w, view_h / doc_h)`.
    /// Pan assumes `origin == view min` (as `canvas_ui` establishes), so the
    /// document centers. Returns `None` for degenerate inputs, in which case
    /// the caller keeps the current view.
    pub fn fit_document_view(
        doc_w: f32,
        doc_h: f32,
        view_w: f32,
        view_h: f32,
    ) -> Option<(f32, Vec2)> {
        if doc_w <= 1.0 || doc_h <= 1.0 || view_w <= 1.0 || view_h <= 1.0 {
            return None;
        }
        let pad = 0.92;
        let zoom = ((view_w * pad / doc_w).min(view_h * pad / doc_h)).clamp(0.05, 32.0);
        let pan = Vec2::new(
            view_w * 0.5 - doc_w * 0.5 * zoom,
            view_h * 0.5 - doc_h * 0.5 * zoom,
        );
        Some((zoom, pan))
    }

    /// Step in X for snap/draw (static columns or fixed step).
    pub fn step_x(&self) -> f64 {
        if self.grid_cols > 0 && self.page_width > 1.0 {
            (self.page_width as f64) / self.grid_cols as f64
        } else {
            self.grid_step.max(0.5) as f64
        }
    }

    /// Step in Y for snap/draw (static rows or fixed step).
    pub fn step_y(&self) -> f64 {
        if self.grid_rows > 0 && self.page_height > 1.0 {
            (self.page_height as f64) / self.grid_rows as f64
        } else {
            self.grid_step.max(0.5) as f64
        }
    }

    pub fn snap(&self, doc: (f64, f64)) -> (f64, f64) {
        if !self.snap_grid {
            return doc;
        }
        let gx = self.step_x();
        let gy = self.step_y();
        if gx <= 0.0 || gy <= 0.0 {
            return doc;
        }
        ((doc.0 / gx).round() * gx, (doc.1 / gy).round() * gy)
    }

    pub fn page_rect(&self, origin: Pos2, width: f32, height: f32) -> Rect {
        let tl = self.doc_to_screen((0.0, 0.0), origin);
        Rect::from_min_size(tl, Vec2::new(width * self.zoom, height * self.zoom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_frames_small_document_large() {
        // 200x100 logo in a 1000x800 view: width-bound, aspect kept 2:1.
        let (z, pan) = Viewport::fit_document_view(200.0, 100.0, 1000.0, 800.0).unwrap();
        assert!((z - 4.6).abs() < 1e-4, "zoom {z}");
        // Doc center maps to view center (origin == view min).
        let cx = pan.x + 100.0 * z;
        let cy = pan.y + 50.0 * z;
        assert!((cx - 500.0).abs() < 1e-3 && (cy - 400.0).abs() < 1e-3);
        // Whole document visible with padding.
        assert!(pan.x > 0.0 && pan.y > 0.0);
        assert!(pan.x + 200.0 * z < 1000.0 && pan.y + 100.0 * z < 800.0);
    }

    #[test]
    fn fit_frames_large_document_whole() {
        // Height-bound case.
        let (z, _) = Viewport::fit_document_view(400.0, 1200.0, 1000.0, 800.0).unwrap();
        assert!((z - 800.0 * 0.92 / 1200.0).abs() < 1e-4, "zoom {z}");
    }

    #[test]
    fn fit_rejects_degenerate_inputs() {
        assert!(Viewport::fit_document_view(0.0, 100.0, 800.0, 600.0).is_none());
        assert!(Viewport::fit_document_view(200.0, 100.0, 0.0, 600.0).is_none());
    }
}
