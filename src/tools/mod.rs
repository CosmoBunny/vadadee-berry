use std::collections::HashMap;

use egui::{Key, Pos2, Ui, Vec2};

use crate::document::{Node, NodeId, PathData, PathEditTarget, FillKind, GradientStop, Paint};

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
    Brush,
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
            Self::Brush => "Brush",
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
            Self::Brush => Some(Key::B),
            Self::Eyedropper => Some(Key::I),
        }
    }

    pub fn is_shape_drag(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Circle | Self::Ellipse | Self::Line | Self::Polygon | Self::Arc
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrushType {
    #[default]
    Standard,
    Pen,
    Calligraphy,
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
        }
    }
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
    TilingGizmo(usize),   // 0 = origin (first), 1 = col end, 2 = row end
    CircularGizmo(usize), // 0 = first/base pos, 1 = origin
}

/// Drag on empty canvas to select all objects intersecting the rectangle.
#[derive(Debug, Clone, Copy)]
pub struct MarqueeSelect {
    pub origin_doc: (f64, f64),
    pub current_doc: (f64, f64),
    pub shift: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SelectSession {
    pub drag_mode: Option<SelectDrag>,
    pub marquee: Option<MarqueeSelect>,
    pub last_doc: (f64, f64),
    pub resize_anchor: kurbo::Rect,
    pub drag_snapshot: Vec<(NodeId, Node)>,
    pub node_edit_target: Option<PathEditTarget>,
    /// Path anchors selected in node edit mode (ctrl+click toggles).
    pub selected_path_points: Vec<(NodeId, usize)>,
    /// Two anchors spanning a selected path segment (node edit).
    pub selected_path_segment: Option<(NodeId, usize, usize)>,
    pub node_drag_origin: Option<(f64, f64)>,
    pub node_drag_active: bool,
    pub select_rotation_mode: bool,
    pub rotate_center: Option<(f64, f64)>,
    pub rotate_start_angle: f64,
    pub drag_start_doc: Option<(f64, f64)>,
    pub clicked_already_selected: bool,
}

#[derive(Debug, Default)]
pub struct ToolState {
    pub active: ToolKind,
    pub last_active_tool: ToolKind,
    pub drag_shape: Option<DragNewShape>,
    pub pen: PenSession,
    pub select: SelectSession,
    pub brush: BrushSession,
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
        if ui.ctx().wants_keyboard_input() {
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
                ToolKind::Brush,
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
    match handle {
        ResizeHandle::Nw => {
            x0 = px;
            y0 = py;
        }
        ResizeHandle::N => y0 = py,
        ResizeHandle::Ne => {
            x1 = px;
            y0 = py;
        }
        ResizeHandle::E => x1 = px,
        ResizeHandle::Se => {
            x1 = px;
            y1 = py;
        }
        ResizeHandle::S => y1 = py,
        ResizeHandle::Sw => {
            x0 = px;
            y1 = py;
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