use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub rgba: [f32; 4],
}

impl Paint {
    pub fn from_hex(rgb: u32, a: f32) -> Self {
        let r = ((rgb >> 16) & 0xff) as f32 / 255.0;
        let g = ((rgb >> 8) & 0xff) as f32 / 255.0;
        let b = (rgb & 0xff) as f32 / 255.0;
        Self {
            rgba: [r, g, b, a],
        }
    }

    pub fn none() -> Self {
        Self {
            rgba: [0.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn to_egui(&self) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(
            (self.rgba[0] * 255.0) as u8,
            (self.rgba[1] * 255.0) as u8,
            (self.rgba[2] * 255.0) as u8,
            (self.rgba[3] * 255.0) as u8,
        )
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            rgba: [
                a.rgba[0] + (b.rgba[0] - a.rgba[0]) * t,
                a.rgba[1] + (b.rgba[1] - a.rgba[1]) * t,
                a.rgba[2] + (b.rgba[2] - a.rgba[2]) * t,
                a.rgba[3] + (b.rgba[3] - a.rgba[3]) * t,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub pos: f32,
    pub color: Paint,
}

impl GradientStop {
    pub fn new(pos: f32, color: Paint) -> Self {
        Self {
            pos: pos.clamp(0.0, 1.0),
            color,
        }
    }
}

pub fn default_gradient_stops() -> Vec<GradientStop> {
    vec![
        GradientStop::new(0.0, Paint::from_hex(0x6382ff, 0.9)),
        GradientStop::new(1.0, Paint::from_hex(0xff6b9d, 0.9)),
    ]
}

fn default_line_x0() -> f32 {
    0.0
}
fn default_line_y0() -> f32 {
    0.5
}
fn default_line_x1() -> f32 {
    1.0
}
fn default_line_y1() -> f32 {
    0.5
}

/// Default linear gradient axis (left → right through vertical midline).
pub fn default_linear_line() -> (f32, f32, f32, f32) {
    (default_line_x0(), default_line_y0(), default_line_x1(), default_line_y1())
}

pub fn linear_angle_from_line(x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    (y1 - y0).atan2(x1 - x0).to_degrees()
}

pub fn project_onto_linear_line(
    nx: f32,
    ny: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
) -> f32 {
    let vx = x1 - x0;
    let vy = y1 - y0;
    let len_sq = vx * vx + vy * vy;
    if len_sq < 1e-8 {
        return 0.5;
    }
    let t = ((nx - x0) * vx + (ny - y0) * vy) / len_sq;
    t.clamp(0.0, 1.0)
}

/// Distance along `(dx,dy)` from `(cx,cy)` to the edge of the unit bbox `[0,1]²`.
fn positive_ray_to_bbox(cx: f32, cy: f32, dx: f32, dy: f32) -> f32 {
    let mut best = f32::MAX;
    if dx.abs() > 1e-6 {
        for x in [0.0f32, 1.0] {
            let t = (x - cx) / dx;
            if t > 1e-5 {
                let y = cy + t * dy;
                if y >= -1e-5 && y <= 1.0 + 1e-5 {
                    best = best.min(t);
                }
            }
        }
    }
    if dy.abs() > 1e-6 {
        for y in [0.0f32, 1.0] {
            let t = (y - cy) / dy;
            if t > 1e-5 {
                let x = cx + t * dx;
                if x >= -1e-5 && x <= 1.0 + 1e-5 {
                    best = best.min(t);
                }
            }
        }
    }
    best.max(1e-4)
}

/// Linear gradient axis through bbox center, spanning edge to edge along `angle_deg`.
pub fn linear_line_spanning_bbox(angle_deg: f32) -> (f32, f32, f32, f32) {
    let rad = angle_deg.to_radians();
    let ux = rad.cos();
    let uy = rad.sin();
    let cx = 0.5f32;
    let cy = 0.5f32;
    let t_pos = positive_ray_to_bbox(cx, cy, ux, uy);
    let t_neg = positive_ray_to_bbox(cx, cy, -ux, -uy);
    (
        cx - ux * t_neg,
        cy - uy * t_neg,
        cx + ux * t_pos,
        cy + uy * t_pos,
    )
}

/// Rotate the line around its midpoint to match `angle_deg`, spanning the bbox.
pub fn set_linear_line_angle(line: &mut (f32, f32, f32, f32), angle_deg: f32) {
    let (x0, y0, x1, y1) = *line;
    let mx = (x0 + x1) * 0.5;
    let my = (y0 + y1) * 0.5;
    let rad = angle_deg.to_radians();
    let ux = rad.cos();
    let uy = rad.sin();
    let t_pos = positive_ray_to_bbox(mx, my, ux, uy);
    let t_neg = positive_ray_to_bbox(mx, my, -ux, -uy);
    *line = (
        mx - ux * t_neg,
        my - uy * t_neg,
        mx + ux * t_pos,
        my + uy * t_pos,
    );
}

pub fn translate_linear_line(line: &mut (f32, f32, f32, f32), dx: f32, dy: f32) {
    let (x0, y0, x1, y1) = *line;
    *line = (x0 + dx, y0 + dy, x1 + dx, y1 + dy);
}

pub fn normalize_stops(stops: &mut Vec<GradientStop>) {
    if stops.len() < 2 {
        *stops = default_gradient_stops();
    }
    stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap());
    stops[0].pos = 0.0;
    if let Some(last) = stops.last_mut() {
        last.pos = 1.0;
    }
}

pub fn sample_stops(stops: &[GradientStop], t: f32) -> Paint {
    if stops.is_empty() {
        return Paint::none();
    }
    if stops.len() == 1 {
        return stops[0].color;
    }
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if t >= a.pos && t <= b.pos {
            let span = (b.pos - a.pos).max(1e-6);
            let local = (t - a.pos) / span;
            return Paint::lerp(a.color, b.color, local);
        }
    }
    stops.last().map(|s| s.color).unwrap_or(Paint::none())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillKind {
    #[default]
    Solid,
    LinearGradient,
    RadialGradient,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    None,
    Solid(Paint),
    LinearGradient {
        angle_deg: f32,
        #[serde(default = "default_line_x0")]
        line_x0: f32,
        #[serde(default = "default_line_y0")]
        line_y0: f32,
        #[serde(default = "default_line_x1")]
        line_x1: f32,
        #[serde(default = "default_line_y1")]
        line_y1: f32,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center_x: f32,
        center_y: f32,
        stops: Vec<GradientStop>,
    },
}

impl Default for Fill {
    fn default() -> Self {
        Self::Solid(Paint::from_hex(0x6c5ce7, 0.75))
    }
}

impl Fill {
    pub fn none() -> Self {
        Self::None
    }

    pub fn is_visible(&self) -> bool {
        match self {
            Self::None => false,
            Self::Solid(p) => p.rgba[3] > 0.01,
            Self::LinearGradient { stops, .. } | Self::RadialGradient { stops, .. } => stops
                .iter()
                .any(|s| s.color.rgba[3] > 0.01),
        }
    }

    pub fn kind(&self) -> FillKind {
        match self {
            Self::Solid(_) | Self::None => FillKind::Solid,
            Self::LinearGradient { .. } => FillKind::LinearGradient,
            Self::RadialGradient { .. } => FillKind::RadialGradient,
        }
    }

    pub fn stops(&self) -> Vec<GradientStop> {
        match self {
            Self::Solid(p) => vec![
                GradientStop::new(0.0, *p),
                GradientStop::new(1.0, *p),
            ],
            Self::LinearGradient { stops, .. } | Self::RadialGradient { stops, .. } => {
                if stops.len() >= 2 {
                    stops.clone()
                } else {
                    default_gradient_stops()
                }
            }
            Self::None => default_gradient_stops(),
        }
    }

    pub fn primary_paint(&self) -> Paint {
        self.stops().first().map(|s| s.color).unwrap_or(Paint::none())
    }

    pub fn secondary_paint(&self) -> Paint {
        self.stops().last().map(|s| s.color).unwrap_or(Paint::none())
    }

    pub fn linear_angle_deg(&self) -> f32 {
        match self {
            Self::LinearGradient { angle_deg, .. } => *angle_deg,
            _ => 0.0,
        }
    }

    pub fn radial_center(&self) -> (f32, f32) {
        match self {
            Self::RadialGradient {
                center_x,
                center_y,
                ..
            } => (*center_x, *center_y),
            _ => (0.5, 0.5),
        }
    }

    pub fn linear_line(&self) -> (f32, f32, f32, f32) {
        match self {
            Self::LinearGradient {
                line_x0,
                line_y0,
                line_x1,
                line_y1,
                ..
            } => (*line_x0, *line_y0, *line_x1, *line_y1),
            _ => default_linear_line(),
        }
    }

    pub fn sample_at(&self, nx: f32, ny: f32) -> Paint {
        match self {
            Self::None => Paint::none(),
            Self::Solid(p) => *p,
            Self::LinearGradient {
                line_x0,
                line_y0,
                line_x1,
                line_y1,
                stops,
                ..
            } => {
                let t = project_onto_linear_line(nx, ny, *line_x0, *line_y0, *line_x1, *line_y1);
                sample_stops(stops, t)
            }
            Self::RadialGradient {
                center_x,
                center_y,
                stops,
            } => {
                let dx = nx - center_x;
                let dy = ny - center_y;
                let t = (dx * dx + dy * dy).sqrt() * 1.25;
                sample_stops(stops, t.clamp(0.0, 1.0))
            }
        }
    }

    pub fn build(
        kind: FillKind,
        enabled: bool,
        stops: &[GradientStop],
        angle_deg: f32,
        line_x0: f32,
        line_y0: f32,
        line_x1: f32,
        line_y1: f32,
        center_x: f32,
        center_y: f32,
    ) -> Self {
        if !enabled {
            return Self::None;
        }
        let mut stops: Vec<GradientStop> = stops.to_vec();
        normalize_stops(&mut stops);
        match kind {
            FillKind::Solid => Self::Solid(stops.first().map(|s| s.color).unwrap_or(Paint::none())),
            FillKind::LinearGradient => Self::LinearGradient {
                angle_deg,
                line_x0,
                line_y0,
                line_x1,
                line_y1,
                stops,
            },
            FillKind::RadialGradient => Self::RadialGradient {
                center_x,
                center_y,
                stops,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub style: Fill,
    pub width: f32,
    #[serde(default)]
    pub line_join: LineJoin,
    #[serde(default)]
    pub line_cap: LineCap,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            style: Fill::Solid(Paint::from_hex(0x1a1a2e, 1.0)),
            width: 2.0,
            line_join: LineJoin::Miter,
            line_cap: LineCap::Butt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeStyle {
    pub fill: Fill,
    pub stroke: Stroke,
    pub opacity: f32,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            fill: Fill::default(),
            stroke: Stroke::default(),
            opacity: 1.0,
        }
    }
}