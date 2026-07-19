use std::collections::HashMap;

use egui::{Key, Pos2, Ui, Vec2};

use crate::document::{Node, NodeId, PathData, PathEditTarget, FillKind, GradientStop, Paint};

pub mod weight_flow;
pub use weight_flow::{
    Falloff, MagneticPole, WeightFlowBrush, WeightFlowConfig, WeightFlowMode, WeightFlowStroke,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToolKind {
    #[default]
    Select,
    Node,
    Rectangle,
    Circle,
    Ellipse,
    Line,
    Polygon,
    Pen,
    Text,
    Arc,
    Plotter,
    Brush,
    /// Paint into Image RGBA (soft/hard continuous brush).
    RasterBrush,
    /// Erase alpha on Image RGBA.
    Eraser,
    /// Flood-fill Image RGBA (contiguous color).
    BucketFill,
    Eyedropper,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Node => "Edit",
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Polygon => "Polygon",
            Self::Pen => "Pen",
            Self::Text => "Text",
            Self::Arc => "Arc",
            Self::Plotter => "Plotter",
            Self::Brush => "Brush",
            Self::RasterBrush => "Paint",
            Self::Eraser => "Eraser",
            Self::BucketFill => "Fill",
            Self::Eyedropper => "Eyedropper",
        }
    }

    pub fn shortcut(self) -> Option<Key> {
        match self {
            Self::Select => Some(Key::V),
            Self::Node => Some(Key::N),
            Self::Rectangle => Some(Key::R),
            Self::Circle => Some(Key::C),
            Self::Ellipse => Some(Key::E),
            Self::Line => Some(Key::L),
            Self::Polygon => Some(Key::G),
            Self::Pen => Some(Key::P),
            Self::Text => Some(Key::T),
            Self::Arc => Some(Key::A),
            Self::Plotter => Some(Key::M),
            Self::Brush => Some(Key::B),
            // E = Ellipse already; use K (paint) / X eraser / F fill.
            Self::RasterBrush => Some(Key::K),
            Self::Eraser => Some(Key::X),
            Self::BucketFill => Some(Key::F),
            Self::Eyedropper => Some(Key::I),
        }
    }

    pub fn is_shape_drag(self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::Circle
                | Self::Ellipse
                | Self::Line
                | Self::Polygon
                | Self::Arc
                | Self::Plotter
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrushType {
    #[default]
    Standard,
    Pen,
    Calligraphy,
    /// Grid-aligned pixel stamps (size in grid cells).
    Pixel,
}

/// Which input device mode controls brush behaviour
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrushInputMode {
    #[default]
    Mouse,
    Stylus,
}

#[derive(Debug, Clone)]
pub struct BrushSession {
    pub brush_type: BrushType,
    pub points: Vec<([f64; 2], f64, f32)>, // pos, time, width
    pub size: f32,
    pub smoothness: f32,
    pub heavy: f32,
    pub fill_kind: FillKind,
    pub fill_stops: Vec<GradientStop>,
    pub fill_stop_sel: usize,
    pub gradient_angle: f32,
    pub fill_line_x0: f32,
    pub fill_line_y0: f32,
    pub fill_line_x1: f32,
    pub fill_line_y1: f32,
    pub radial_cx: f32,
    pub radial_cy: f32,
    pub fill_edit_gradient_line: bool,

    // --- Input mode ---
    pub input_mode: BrushInputMode,

    // Mouse mode settings
    pub mouse_pressure_sensitivity: f32,  // 0..2
    pub mouse_speed_sensitivity: f32,     // 0..2
    pub mouse_rotate_by_direction: bool,

    // Stylus mode settings
    pub stylus_tilt_angle: f32,           // degrees 0..90
    pub stylus_pen_angle: f32,            // degrees 0..360
    pub stylus_pressure: f32,             // 0..1

    // Pen type settings
    pub pen_roundness: f32,               // 0..1  (1 = fully round, 0 = flat)
    pub pen_press_on_paper: f32,          // 0..1  (how hard the pen presses)

    // Calligraphy type settings
    pub calli_rotate_tip: bool,
    pub calli_fountain_size: f32,         // nib width multiplier 0.1..3.0
    pub calli_dynamic: bool,

    /// Pixel brush: stamp size in grid cells (1 = one box).
    pub pixel_cells: u32,
    /// Ctrl+drag line: document point at press (line origin).
    pub pixel_line_anchor: Option<(f64, f64)>,
    /// Shift+erase: node snapshots before this erase stroke (one undo on release).
    pub pixel_erase_before: Vec<(crate::document::NodeId, crate::document::Node)>,
}

impl Default for BrushSession {
    fn default() -> Self {
        Self {
            brush_type: BrushType::Standard,
            points: Vec::new(),
            size: 16.0,
            smoothness: 0.5,
            heavy: 0.2,
            fill_kind: FillKind::Solid,
            fill_stops: vec![
                GradientStop { pos: 0.0, color: Paint::from_hex(0x000000, 1.0) },
                GradientStop { pos: 1.0, color: Paint::from_hex(0x000000, 1.0) },
            ],
            fill_stop_sel: 0,
            gradient_angle: 0.0,
            fill_line_x0: 0.0,
            fill_line_y0: 0.0,
            fill_line_x1: 1.0,
            fill_line_y1: 0.0,
            radial_cx: 0.5,
            radial_cy: 0.5,
            fill_edit_gradient_line: false,
            input_mode: BrushInputMode::Mouse,
            mouse_pressure_sensitivity: 1.0,
            mouse_speed_sensitivity: 1.0,
            mouse_rotate_by_direction: false,
            stylus_tilt_angle: 0.0,
            stylus_pen_angle: 0.0,
            stylus_pressure: 1.0,
            pen_roundness: 1.0,
            pen_press_on_paper: 0.5,
            calli_rotate_tip: false,
            calli_fountain_size: 1.0,
            calli_dynamic: false,
            pixel_cells: 1,
            pixel_line_anchor: None,
            pixel_erase_before: Vec::new(),
        }
    }
}

/// Snap a document point to a pixel-brush stamp (grid-aligned).
/// Returns `(center_x, center_y, width_doc, height_doc)`.
pub fn pixel_stamp_at(
    doc: (f64, f64),
    step_x: f64,
    step_y: f64,
    cells: u32,
) -> (f64, f64, f64, f64) {
    let gx = step_x.max(0.5);
    let gy = step_y.max(0.5);
    let n = cells.max(1) as f64;
    let i0 = (doc.0 / gx).floor();
    let j0 = (doc.1 / gy).floor();
    let w = n * gx;
    let h = n * gy;
    // Align stamp so the cell under the cursor is the top-left of the n×n block
    // for n==1 that is the single cell; for n>1 expands right/down.
    let cx = i0 * gx + w * 0.5;
    let cy = j0 * gy + h * 0.5;
    (cx, cy, w, h)
}

/// Cell index under document position (top-left of stamp block).
pub fn pixel_cell_index(doc: (f64, f64), step_x: f64, step_y: f64) -> (i64, i64) {
    let gx = step_x.max(0.5);
    let gy = step_y.max(0.5);
    (
        (doc.0 / gx).floor() as i64,
        (doc.1 / gy).floor() as i64,
    )
}

/// Fill every grid stamp from `from` → `to` (inclusive).
/// Dense sampling along the segment so fast pointer motion never skips cells.
pub fn pixel_stamps_along(
    from: (f64, f64),
    to: (f64, f64),
    step_x: f64,
    step_y: f64,
    cells: u32,
) -> Vec<(f64, f64, f64, f64)> {
    let gx = step_x.max(0.5);
    let gy = step_y.max(0.5);
    let (i0, j0) = pixel_cell_index(from, gx, gy);
    let (i1, j1) = pixel_cell_index(to, gx, gy);

    let di = (i1 - i0).unsigned_abs();
    let dj = (j1 - j0).unsigned_abs();
    // At least one sample per crossed cell, extra density on long diagonals.
    let steps = (di.max(dj).max(1) as usize).saturating_mul(2).max(1);

    let mut out = Vec::with_capacity(steps + 1);
    let mut seen = std::collections::HashSet::new();
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        let x = from.0 + (to.0 - from.0) * t;
        let y = from.1 + (to.1 - from.1) * t;
        let (cx, cy, w, h) = pixel_stamp_at((x, y), gx, gy, cells);
        let key = (
            (cx * 1000.0).round() as i64,
            (cy * 1000.0).round() as i64,
        );
        if seen.insert(key) {
            out.push((cx, cy, w, h));
        }
    }
    out
}


#[derive(Debug, Clone, Default)]
pub struct DragNewShape {
    pub origin_doc: (f64, f64),
    pub current_doc: (f64, f64),
    pub kind: Option<ToolKind>,
}

#[derive(Debug, Clone, Default)]
pub struct PenSession {
    pub anchors: Vec<(f64, f64)>,
    pub smooth_anchors: Vec<usize>,
    pub handle_out_offset: HashMap<usize, [f64; 2]>,
    pub handle_in_offset: HashMap<usize, [f64; 2]>,
    /// Anchor being shaped with ctrl+click-drag.
    pub curve_adjust: Option<usize>,
    /// Extending an existing open path in-place (node updated on finish).
    pub continue_node: Option<crate::document::NodeId>,
    /// When true, new points are prepended at the start anchor.
    pub extend_from_start: bool,
    /// Join endpoint marked smooth when continuing from that end.
    pub join_anchor: Option<usize>,
    /// Path was closed when pen continuation started (preserve on finish).
    pub was_closed: bool,
}

impl PenSession {
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    pub fn pop_anchor(&mut self) {
        if self.anchors.is_empty() {
            return;
        }
        let removed = self.anchors.len() - 1;
        self.anchors.pop();
        self.smooth_anchors.retain(|&i| i != removed);
        self.handle_out_offset.remove(&removed);
        self.handle_in_offset.remove(&removed);
        self.curve_adjust = None;
    }

    pub fn to_path_data(&self) -> PathData {
        PathData::from_anchor_data(
            &self.anchors,
            &self.smooth_anchors,
            self.handle_out_offset.clone(),
            self.handle_in_offset.clone(),
            false,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectDrag {
    Move,
    Resize(ResizeHandle),
    Rotate,
    TilingGizmo(usize), // 0 = origin (first), 1 = col end, 2 = row end
    /// CircularClone gizmo: 0 = base (ring / object), 1 = origin (center), 2 = angle handle.
    CircularGizmo(usize),
}

/// Drag on empty canvas to select all objects intersecting the rectangle.
#[derive(Debug, Clone, Copy)]
pub struct MarqueeSelect {
    pub origin_doc: (f64, f64),
    pub current_doc: (f64, f64),
    pub shift: bool,
}

/// Lightweight bulk move — stores origins only (no full node clones).
#[derive(Debug, Clone, Default)]
pub struct BulkDrag {
    pub ids: Vec<NodeId>,
    pub origins: Vec<(f64, f64)>,
    pub preview_dx: f64,
    pub preview_dy: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SelectSession {
    pub drag_mode: Option<SelectDrag>,
    pub marquee: Option<MarqueeSelect>,
    pub last_doc: (f64, f64),
    pub resize_anchor: kurbo::Rect,
    pub drag_snapshot: Vec<(NodeId, Node)>,
    pub bulk_drag: Option<BulkDrag>,
    pub node_edit_target: Option<PathEditTarget>,
    /// Path anchors selected in node edit mode (ctrl+click toggles).
    pub selected_path_points: Vec<(NodeId, usize)>,
    /// Two anchors spanning a selected path segment (node edit).
    pub selected_path_segment: Option<(NodeId, usize, usize)>,
    /// Dragging one of the yellow corner curve controls (LPE).
    pub mid_curve_drag: Option<(NodeId, usize, bool)>, // (id, seg_start, is_first_ctrl)
    pub node_drag_origin: Option<(f64, f64)>,
    pub node_drag_active: bool,
    pub select_rotation_mode: bool,
    pub rotate_center: Option<(f64, f64)>,
    pub rotate_start_angle: f64,
    pub rotate_start_layer_rotation: f32,
    pub drag_start_doc: Option<(f64, f64)>,
    pub clicked_already_selected: bool,
    /// True once pointer moved past click-threshold during SelectDrag::Move.
    pub move_drag_engaged: bool,
    /// CircularClone ring pose at move-drag start: (source_id, base_x, base_y, origin_x, origin_y).
    pub circular_ring_drag_start: Vec<(NodeId, f64, f64, f64, f64)>,
    /// Document snapshot before Tiling/Circular gizmo drag (for undo).
    pub effect_drag_doc_before: Option<crate::document::Document>,
}

/// Raster paint / eraser session (targets `NodeKind::Image` pixels).
#[derive(Debug, Clone)]
pub struct RasterSession {
    /// Brush diameter in document units.
    pub size: f32,
    /// 0 = fully soft, 1 = hard edge.
    pub hardness: f32,
    /// 0..1 stamp opacity / flow.
    pub opacity: f32,
    /// Stamp spacing as fraction of radius (0.05..1.0). Lower = denser continuous stroke.
    pub spacing: f32,
    /// Streamline / stabilizer 0..1 (CSP-style pull). 0 = raw pointer, 1 = strong lag.
    pub stabilizer: f32,
    /// Stabilized tip in image-pixel space (while stroke active).
    pub stable_px: Option<(f32, f32)>,
    /// Flood fill color tolerance 0..255 (RGB distance).
    pub fill_tolerance: u8,
    /// Active paint target (Image node).
    pub target: Option<crate::document::NodeId>,
    /// Snapshot of Image.bytes before the stroke (undo).
    pub before_bytes: Option<Vec<u8>>,
    pub before_w: f64,
    pub before_h: f64,
    pub before_x: f64,
    pub before_y: f64,
    /// Last pointer position in **image pixel** space.
    pub last_px: Option<(f32, f32)>,
    /// Recent samples (oldest → newest) for Catmull-Rom freehand smoothing.
    pub sample_hist: Vec<(f32, f32)>,
    /// Carry for continuous spacing.
    pub spacing_carry: f32,
    pub painting: bool,
    /// True if any stamp landed this stroke.
    pub dirty: bool,
    /// Live paint buffer (RGBA8) for the active stroke — avoids Color32 round-trips.
    pub live_w: u32,
    pub live_h: u32,
    pub live_rgba: Option<Vec<u8>>,
    /// GPU texture needs re-upload from `live_rgba`.
    pub tex_dirty: bool,
    /// `ctx.input.time` of last GPU upload (throttle mid-stroke).
    pub last_tex_upload: f64,
}

impl Default for RasterSession {
    fn default() -> Self {
        Self {
            size: 24.0,
            hardness: 0.85,
            opacity: 1.0,
            // Dense stamps for continuous freehand.
            spacing: 0.08,
            stabilizer: 0.35,
            stable_px: None,
            fill_tolerance: 24,
            target: None,
            before_bytes: None,
            before_w: 0.0,
            before_h: 0.0,
            before_x: 0.0,
            before_y: 0.0,
            last_px: None,
            sample_hist: Vec::new(),
            spacing_carry: 0.0,
            painting: false,
            dirty: false,
            live_w: 0,
            live_h: 0,
            live_rgba: None,
            tex_dirty: false,
            last_tex_upload: 0.0,
        }
    }
}

#[derive(Debug, Default)]
pub struct ToolState {
    pub active: ToolKind,
    pub last_active_tool: ToolKind,
    pub drag_shape: Option<DragNewShape>,
    pub pen: PenSession,
    pub select: SelectSession,
    pub brush: BrushSession,
    pub raster: RasterSession,
    /// Path weight-flow sculpt (Geometry tab; Select/Node + path only).
    pub weight_flow: WeightFlowBrush,
    pub space_pan: bool,
    /// Middle/right-button canvas pan in progress.
    pub canvas_pan_drag: bool,
}

impl SelectSession {
    pub fn clear_path_point_selection(&mut self) {
        self.selected_path_points.clear();
        self.selected_path_segment = None;
    }

    pub fn set_single_path_point(&mut self, id: NodeId, idx: usize) {
        self.selected_path_points = vec![(id, idx)];
        self.selected_path_segment = None;
    }

    pub fn set_path_segment(&mut self, id: NodeId, from: usize, to: usize) {
        self.selected_path_points = vec![(id, from), (id, to)];
        self.selected_path_segment = Some((id, from, to));
    }

    pub fn toggle_path_point(&mut self, id: NodeId, idx: usize, ctrl: bool) {
        if ctrl {
            if let Some(pos) = self
                .selected_path_points
                .iter()
                .position(|&(sid, pi)| sid == id && pi == idx)
            {
                self.selected_path_points.remove(pos);
            } else if self.selected_path_points.is_empty()
                || self.selected_path_points.iter().all(|(sid, _)| *sid == id)
            {
                self.selected_path_points.push((id, idx));
            } else {
                self.selected_path_points = vec![(id, idx)];
            }
            self.selected_path_segment = None;
        } else {
            self.set_single_path_point(id, idx);
        }
    }

    pub fn primary_path_point(&self) -> Option<(NodeId, usize)> {
        self.selected_path_points.first().copied()
    }

    pub fn points_on_path(&self, id: NodeId) -> Vec<usize> {
        self.selected_path_points
            .iter()
            .filter(|(sid, _)| *sid == id)
            .map(|(_, i)| *i)
            .collect()
    }

    pub fn is_path_point_selected(&self, id: NodeId, idx: usize) -> bool {
        self.selected_path_points
            .iter()
            .any(|&(sid, pi)| sid == id && pi == idx)
    }
}

impl ToolState {
    pub fn handle_shortcuts(&mut self, ui: &Ui) {
        if ui.ctx().text_edit_focused() {
            return;
        }
        if ui.input(|i| i.modifiers.command_only()) {
            return;
        }
        let tools: &[ToolKind] = if self.active == ToolKind::Node {
            &[ToolKind::Select, ToolKind::Node]
        } else {
            &[
                ToolKind::Select,
                ToolKind::Node,
                ToolKind::Rectangle,
                ToolKind::Circle,
                ToolKind::Ellipse,
                ToolKind::Line,
                ToolKind::Polygon,
                ToolKind::Pen,
                ToolKind::Text,
                ToolKind::Arc,
                ToolKind::Plotter,
                ToolKind::Brush,
                ToolKind::RasterBrush,
                ToolKind::Eraser,
                ToolKind::BucketFill,
                ToolKind::Eyedropper,
            ]
        };
        for tool in tools {
            if let Some(key) = tool.shortcut() {
                if ui.input(|i| i.key_pressed(key)) {
                    if self.active != ToolKind::Eyedropper {
                        self.last_active_tool = self.active;
                    }
                    self.active = *tool;
                }
            }
        }
        self.space_pan = ui.input(|i| i.key_down(Key::Space));
    }
}

pub fn doc_point_from_screen(
    screen: Pos2,
    canvas_origin: Pos2,
    pan: Vec2,
    zoom: f32,
) -> (f64, f64) {
    let x = (screen.x - canvas_origin.x - pan.x) as f64 / zoom as f64;
    let y = (screen.y - canvas_origin.y - pan.y) as f64 / zoom as f64;
    (x, y)
}

pub fn screen_from_doc(
    doc: (f64, f64),
    canvas_origin: Pos2,
    pan: Vec2,
    zoom: f32,
) -> Pos2 {
    Pos2::new(
        canvas_origin.x + pan.x + doc.0 as f32 * zoom,
        canvas_origin.y + pan.y + doc.1 as f32 * zoom,
    )
}

/// Lock `point` to a 15° multiple relative to `origin` (…, -30, -15, 0, 15, 30, 45, …).
/// Keeps distance from origin; used for line/2-pt path + Ctrl angle constrain.
pub fn snap_angle_15deg(origin: (f64, f64), point: (f64, f64)) -> (f64, f64) {
    let dx = point.0 - origin.0;
    let dy = point.1 - origin.1;
    let len = dx.hypot(dy);
    if len < 1e-12 {
        return point;
    }
    let ang = dy.atan2(dx);
    let step = std::f64::consts::PI / 12.0; // 15°
    let snapped = (ang / step).round() * step;
    (
        origin.0 + len * snapped.cos(),
        origin.1 + len * snapped.sin(),
    )
}

/// Screen-pixel threshold before a Select click becomes a move drag.
pub const SELECT_MOVE_THRESHOLD_PX: f64 = 5.0;

pub struct ToolAction {
    pub new_nodes: Vec<Node>,
    pub delete_ids: Vec<NodeId>,
    pub patch: Vec<(NodeId, Node)>,
    pub finish_pen: bool,
    pub clear_pen: bool,
}

impl Default for ToolAction {
    fn default() -> Self {
        Self {
            new_nodes: vec![],
            delete_ids: vec![],
            patch: vec![],
            finish_pen: false,
            clear_pen: false,
        }
    }
}

pub fn resize_bounds(
    anchor: kurbo::Rect,
    handle: ResizeHandle,
    doc: (f64, f64),
) -> kurbo::Rect {
    let mut x0 = anchor.x0;
    let mut y0 = anchor.y0;
    let mut x1 = anchor.x1;
    let mut y1 = anchor.y1;
    let (px, py) = doc;
    
    let orig_w = anchor.width();
    let orig_h = anchor.height();
    
    match handle {
        ResizeHandle::Nw => {
            if orig_w > 0.0 && orig_h > 0.0 {
                let w = x1 - px;
                let h = y1 - py;
                let scale = ((w / orig_w) + (h / orig_h)) / 2.0;
                x0 = x1 - orig_w * scale;
                y0 = y1 - orig_h * scale;
            } else {
                x0 = px;
                y0 = py;
            }
        }
        ResizeHandle::N => y0 = py,
        ResizeHandle::Ne => {
            if orig_w > 0.0 && orig_h > 0.0 {
                let w = px - x0;
                let h = y1 - py;
                let scale = ((w / orig_w) + (h / orig_h)) / 2.0;
                x1 = x0 + orig_w * scale;
                y0 = y1 - orig_h * scale;
            } else {
                x1 = px;
                y0 = py;
            }
        }
        ResizeHandle::E => x1 = px,
        ResizeHandle::Se => {
            if orig_w > 0.0 && orig_h > 0.0 {
                let w = px - x0;
                let h = py - y0;
                let scale = ((w / orig_w) + (h / orig_h)) / 2.0;
                x1 = x0 + orig_w * scale;
                y1 = y0 + orig_h * scale;
            } else {
                x1 = px;
                y1 = py;
            }
        }
        ResizeHandle::S => y1 = py,
        ResizeHandle::Sw => {
            if orig_w > 0.0 && orig_h > 0.0 {
                let w = x1 - px;
                let h = py - y0;
                let scale = ((w / orig_w) + (h / orig_h)) / 2.0;
                x0 = x1 - orig_w * scale;
                y1 = y0 + orig_h * scale;
            } else {
                x0 = px;
                y1 = py;
            }
        }
        ResizeHandle::W => x0 = px,
    }
    if x1 < x0 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if y1 < y0 {
        std::mem::swap(&mut y0, &mut y1);
    }
    kurbo::Rect::new(x0, y0, x1, y1)
}

pub fn normalize_rect(a: (f64, f64), b: (f64, f64)) -> (f64, f64, f64, f64) {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = a.0.max(b.0);
    let y1 = a.1.max(b.1);
    (x0, y0, x1 - x0, y1 - y0)
}

pub fn marquee_rect(origin: (f64, f64), current: (f64, f64)) -> kurbo::Rect {
    let (x, y, w, h) = normalize_rect(origin, current);
    kurbo::Rect::new(x, y, x + w, y + h)
}

pub fn marquee_is_drag(origin: (f64, f64), current: (f64, f64)) -> bool {
    (origin.0 - current.0).abs() > 1.5 || (origin.1 - current.1).abs() > 1.5
}

pub fn node_bounds_intersects_marquee(node: &Node, marquee: kurbo::Rect) -> bool {
    let b = node.bounds();
    let hit = b.intersect(marquee);
    hit.width() > 1e-6 && hit.height() > 1e-6
}