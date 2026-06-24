use egui::{Pos2, Rect, Vec2};

#[derive(Debug, Clone)]
pub struct Viewport {
    pub pan: Vec2,
    pub zoom: f32,
    pub show_grid: bool,
    pub snap_grid: bool,
    pub grid_step: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
            show_grid: true,
            snap_grid: true,
            grid_step: 20.0,
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

    pub fn snap(&self, doc: (f64, f64)) -> (f64, f64) {
        if !self.snap_grid {
            return doc;
        }
        let g = self.grid_step as f64;
        if g <= 0.0 {
            return doc;
        }
        (
            (doc.0 / g).round() * g,
            (doc.1 / g).round() * g,
        )
    }

    pub fn page_rect(&self, origin: Pos2, width: f32, height: f32) -> Rect {
        let tl = self.doc_to_screen((0.0, 0.0), origin);
        Rect::from_min_size(tl, Vec2::new(width * self.zoom, height * self.zoom))
    }
}