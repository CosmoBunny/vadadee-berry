//! Dynamic flowchart layer: nodes (rounded rects) and orthogonal connector paths.

use std::collections::HashMap;

use kurbo::{BezPath, PathEl, Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

use super::{Node, NodeId, NodeKind, NodeStore};

/// Minimum straight segment leaving a shape anchor (perpendicular stub).
pub const FLOWCHART_STUB_LEN: f64 = 20.0;
/// Routing treats nodes as impassable with this margin (doc units).
pub const FLOWCHART_OBSTACLE_HALO: f64 = 15.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlowchartEdgeSide {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FlowchartAnchor {
    Center,
    Edge {
        side: FlowchartEdgeSide,
        slot: u32,
        slots: u32,
        /// When set (e.g. snap at connect), position along the straight edge in 0..1.
        #[serde(default)]
        edge_t: Option<f64>,
    },
}

impl FlowchartAnchor {
    pub fn edge(side: FlowchartEdgeSide, slot: u32, slots: u32) -> Self {
        let slots = slots.max(1);
        Self::Edge {
            side,
            slot: slot.min(slots.saturating_sub(1)),
            slots,
            edge_t: None,
        }
    }

    pub fn edge_at_doc_t(side: FlowchartEdgeSide, slot: u32, slots: u32, edge_t: f64) -> Self {
        let slots = slots.max(1);
        Self::Edge {
            side,
            slot: slot.min(slots.saturating_sub(1)),
            slots,
            edge_t: Some(edge_t.clamp(0.0, 1.0)),
        }
    }

    pub fn edge_side(&self) -> Option<FlowchartEdgeSide> {
        match self {
            Self::Center => None,
            Self::Edge { side, .. } => Some(*side),
        }
    }
}

/// Along-edge parameter for `n` connections: divide into `n + 2` segments; anchors at interior points.
pub fn edge_anchor_t(slot: u32, connection_count: u32) -> f64 {
    let n = connection_count.max(1) as f64;
    let s = slot.min(connection_count.saturating_sub(1)) as f64;
    (s + 1.0) / (n + 2.0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowchartNodeGeom {
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    pub corner_rx: f64,
}

impl FlowchartNodeGeom {
    pub fn doc_rect(&self) -> Rect {
        Rect::new(
            self.cx - self.w * 0.5,
            self.cy - self.h * 0.5,
            self.cx + self.w * 0.5,
            self.cy + self.h * 0.5,
        )
    }

    pub fn anchor_position(&self, anchor: FlowchartAnchor) -> (f64, f64) {
        let r = self.doc_rect();
        match anchor {
            FlowchartAnchor::Center => (self.cx, self.cy),
            FlowchartAnchor::Edge {
                side,
                slot,
                slots,
                edge_t,
            } => {
                let t = edge_t.unwrap_or_else(|| edge_anchor_t(slot, slots));
                let inset = self.corner_rx.min(self.w * 0.45).min(self.h * 0.45);
                match side {
                    FlowchartEdgeSide::Top => {
                        let x = r.x0 + inset + t * (r.width() - 2.0 * inset);
                        (x, r.y0)
                    }
                    FlowchartEdgeSide::Bottom => {
                        let x = r.x0 + inset + t * (r.width() - 2.0 * inset);
                        (x, r.y1)
                    }
                    FlowchartEdgeSide::Left => {
                        let y = r.y0 + inset + t * (r.height() - 2.0 * inset);
                        (r.x0, y)
                    }
                    FlowchartEdgeSide::Right => {
                        let y = r.y0 + inset + t * (r.height() - 2.0 * inset);
                        (r.x1, y)
                    }
                }
            }
        }
    }

    pub fn contains_doc(&self, doc: (f64, f64)) -> bool {
        self.doc_rect().contains(Point::new(doc.0, doc.1))
    }
}

fn default_endpoint_marker() -> f32 {
    12.0
}

fn default_path_corner() -> f64 {
    12.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowchartPathData {
    pub points: Vec<(f64, f64)>,
    #[serde(default)]
    pub start_node: Option<NodeId>,
    #[serde(default)]
    pub start_anchor: Option<FlowchartAnchor>,
    #[serde(default)]
    pub end_node: Option<NodeId>,
    #[serde(default)]
    pub end_anchor: Option<FlowchartAnchor>,
    /// Hollow/filled square size at path ends (doc-independent; screen px scaled in render).
    #[serde(default = "default_endpoint_marker")]
    pub endpoint_marker_size: f32,
    /// Rounded corner radius for orthogonal bends (doc units).
    #[serde(default = "default_path_corner")]
    pub corner_radius: f64,
}

pub fn edge_inset(geom: &FlowchartNodeGeom) -> f64 {
    geom.corner_rx.min(geom.w * 0.45).min(geom.h * 0.45)
}

/// Straight edge length excluding rounded corners.
pub fn straight_edge_length(geom: &FlowchartNodeGeom, side: FlowchartEdgeSide) -> f64 {
    let inset = edge_inset(geom);
    match side {
        FlowchartEdgeSide::Top | FlowchartEdgeSide::Bottom => (geom.w - 2.0 * inset).max(1.0),
        FlowchartEdgeSide::Left | FlowchartEdgeSide::Right => (geom.h - 2.0 * inset).max(1.0),
    }
}

pub fn anchor_positions_on_side(
    geom: &FlowchartNodeGeom,
    side: FlowchartEdgeSide,
    connection_count: u32,
) -> Vec<(f64, f64)> {
    let n = connection_count.max(1);
    (0..n)
        .map(|slot| geom.anchor_position(FlowchartAnchor::edge(side, slot, n)))
        .collect()
}

/// Parameter `t` in 0..1 along the straight edge (excluding corners).
pub fn edge_doc_parameter(geom: &FlowchartNodeGeom, side: FlowchartEdgeSide, doc: (f64, f64)) -> f64 {
    let r = geom.doc_rect();
    let inset = edge_inset(geom);
    match side {
        FlowchartEdgeSide::Top | FlowchartEdgeSide::Bottom => {
            let x0 = r.x0 + inset;
            let len = straight_edge_length(geom, side);
            ((doc.0 - x0) / len).clamp(0.0, 1.0)
        }
        FlowchartEdgeSide::Left | FlowchartEdgeSide::Right => {
            let y0 = r.y0 + inset;
            let len = straight_edge_length(geom, side);
            ((doc.1 - y0) / len).clamp(0.0, 1.0)
        }
    }
}

pub fn slot_for_edge_doc(
    geom: &FlowchartNodeGeom,
    side: FlowchartEdgeSide,
    doc: (f64, f64),
    connection_count: u32,
) -> u32 {
    let n = connection_count.max(1);
    let divisions = n + 2;
    let t = edge_doc_parameter(geom, side, doc);
    let mut best = 0u32;
    let mut best_d = f64::INFINITY;
    for slot in 0..n {
        let center_t = (slot as f64 + 1.0) / divisions as f64;
        let d = (t - center_t).abs();
        if d < best_d {
            best_d = d;
            best = slot;
        }
    }
    best
}

/// Perpendicular distance from `doc` to the straight edge segment (excluding corners).
pub fn distance_to_edge(geom: &FlowchartNodeGeom, side: FlowchartEdgeSide, doc: (f64, f64)) -> f64 {
    let r = geom.doc_rect();
    let inset = edge_inset(geom);
    match side {
        FlowchartEdgeSide::Top => {
            let x0 = r.x0 + inset;
            let x1 = r.x1 - inset;
            let x = doc.0.clamp(x0, x1);
            (doc.0 - x).hypot(doc.1 - r.y0)
        }
        FlowchartEdgeSide::Bottom => {
            let x0 = r.x0 + inset;
            let x1 = r.x1 - inset;
            let x = doc.0.clamp(x0, x1);
            (doc.0 - x).hypot(doc.1 - r.y1)
        }
        FlowchartEdgeSide::Left => {
            let y0 = r.y0 + inset;
            let y1 = r.y1 - inset;
            let y = doc.1.clamp(y0, y1);
            (r.x0 - doc.0).hypot(doc.1 - y)
        }
        FlowchartEdgeSide::Right => {
            let y0 = r.y0 + inset;
            let y1 = r.y1 - inset;
            let y = doc.1.clamp(y0, y1);
            (doc.0 - r.x1).hypot(doc.1 - y)
        }
    }
}

pub fn nearest_edge_side(geom: &FlowchartNodeGeom, doc: (f64, f64)) -> FlowchartEdgeSide {
    use FlowchartEdgeSide::{Bottom, Left, Right, Top};
    let sides = [Top, Right, Bottom, Left];
    let mut best = Top;
    let mut best_d = f64::INFINITY;
    for side in sides {
        let d = distance_to_edge(geom, side, doc);
        if d < best_d {
            best_d = d;
            best = side;
        }
    }
    best
}

/// Nearest edge band the cursor is approaching (if within slop of that side).
pub fn nearest_edge_approach(
    geom: &FlowchartNodeGeom,
    doc: (f64, f64),
    slop: f64,
) -> Option<(FlowchartEdgeSide, f64)> {
    let side = nearest_edge_side(geom, doc);
    let dist = distance_to_edge(geom, side, doc);
    if dist > slop {
        return None;
    }
    let t = edge_doc_parameter(geom, side, doc);
    Some((side, t))
}

/// Port normal (outward from shape) for routing stubs.
pub fn anchor_port_normal(
    anchor: FlowchartAnchor,
    anchor_pos: (f64, f64),
    toward: (f64, f64),
) -> (f64, f64) {
    match anchor {
        FlowchartAnchor::Center => {
            let dx = toward.0 - anchor_pos.0;
            let dy = toward.1 - anchor_pos.1;
            if dx.abs() >= dy.abs() {
                if dx >= 0.0 {
                    (1.0, 0.0)
                } else {
                    (-1.0, 0.0)
                }
            } else if dy >= 0.0 {
                (0.0, 1.0)
            } else {
                (0.0, -1.0)
            }
        }
        FlowchartAnchor::Edge { side, .. } => match side {
            FlowchartEdgeSide::Top => (0.0, -1.0),
            FlowchartEdgeSide::Bottom => (0.0, 1.0),
            FlowchartEdgeSide::Left => (-1.0, 0.0),
            FlowchartEdgeSide::Right => (1.0, 0.0),
        },
    }
}

pub fn snap_anchor_for_point(geom: &FlowchartNodeGeom, doc: (f64, f64)) -> FlowchartAnchor {
    let (px, py) = doc;
    let r = geom.doc_rect();
    let inside = r.contains(Point::new(px, py));
    let dx = px - geom.cx;
    let dy = py - geom.cy;

    if inside {
        let inner_w = geom.w * 0.35;
        let inner_h = geom.h * 0.35;
        if dx.abs() <= inner_w * 0.5 && dy.abs() <= inner_h * 0.5 {
            return FlowchartAnchor::Center;
        }
    }
    let side = nearest_edge_side(geom, doc);
    FlowchartAnchor::edge(side, 0, 1)
}

pub fn estimated_port_normals(start: (f64, f64), end: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let start_n = if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            (1.0, 0.0)
        } else {
            (-1.0, 0.0)
        }
    } else if dy >= 0.0 {
        (0.0, 1.0)
    } else {
        (0.0, -1.0)
    };
    let end_n = if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            (-1.0, 0.0)
        } else {
            (1.0, 0.0)
        }
    } else if dy >= 0.0 {
        (0.0, -1.0)
    } else {
        (0.0, 1.0)
    };
    (start_n, end_n)
}

pub fn route_orthogonal(
    start: (f64, f64),
    end: (f64, f64),
    obstacles: &[Rect],
) -> Vec<(f64, f64)> {
    let (sn, en) = estimated_port_normals(start, end);
    route_orthogonal_with_normals(start, end, sn, en, obstacles)
}

pub fn route_orthogonal_with_normals(
    start: (f64, f64),
    end: (f64, f64),
    start_normal: (f64, f64),
    end_normal: (f64, f64),
    obstacles: &[Rect],
) -> Vec<(f64, f64)> {
    let stub = FLOWCHART_STUB_LEN;
    let stub_a = (
        start.0 + start_normal.0 * stub,
        start.1 + start_normal.1 * stub,
    );
    let stub_b = (
        end.0 + end_normal.0 * stub,
        end.1 + end_normal.1 * stub,
    );
    // Pathfind only between stub waypoints (outside node halos), then attach anchor segments.
    let mut mid = route_orthogonal_mid(stub_a, stub_b, obstacles);
    if mid.is_empty() {
        mid = vec![stub_a, stub_b];
    }
    let mut pts = vec![start];
    if (start.0 - stub_a.0).hypot(start.1 - stub_a.1) > 1e-6 {
        pts.push(stub_a);
    }
    for p in mid.iter().copied().skip(1) {
        if pts.last().copied().map_or(true, |q| (q.0 - p.0).hypot(q.1 - p.1) > 1e-6) {
            pts.push(p);
        }
    }
    if pts.last().copied() != Some(stub_b) {
        pts.push(stub_b);
    }
    if (end.0 - stub_b.0).hypot(end.1 - stub_b.1) > 1e-6 {
        pts.push(end);
    }
    let mut pts = dedupe_points(pts);
    pts = orthogonalize_polyline(pts);
    pts[0] = start;
    if let Some(last) = pts.last_mut() {
        *last = end;
    }

    // Enforce explicit outward first and last stub lines (opposite to the node at the anchor)
    // so that 1st and last are the perpendicular out/in, and 2nd/4th start from the cleared position.
    if pts.len() >= 2 {
        if (pts[1].0 - stub_a.0).hypot(pts[1].1 - stub_a.1) > 1e-6 {
            pts.insert(1, stub_a);
        }
        let n = pts.len();
        if (pts[n-2].0 - stub_b.0).hypot(pts[n-2].1 - stub_b.1) > 1e-6 {
            pts.insert(n-1, stub_b);
        }
    }

    let mut pts = orthogonalize_polyline(pts);

    // Keep the explicit stubs and cross points (do not canonicalize away) so we get 5 lines
    // (start-stub, 2nd away, 3rd cross at margin, 4th away, last-stub) instead of collapsing to 3.

    pts
}

/// Obstacle rects for routing; endpoint nodes are excluded so connectors can leave/arrive on their ports.
pub fn flowchart_routing_obstacles(
    nodes: &super::NodeStore,
    layer_node_ids: &[super::NodeId],
    exclude_node_ids: &[super::NodeId],
) -> Vec<Rect> {
    layer_node_ids
        .iter()
        .filter(|id| !exclude_node_ids.contains(id))
        .filter_map(|id| nodes.get(*id))
        .filter_map(|n| node_as_flowchart_geom(&n.kind))
        .map(|g| {
            g.doc_rect()
                .inflate(FLOWCHART_OBSTACLE_HALO, FLOWCHART_OBSTACLE_HALO)
        })
        .collect()
}

pub fn flowchart_bend_point_indices(points: &[(f64, f64)]) -> Vec<usize> {
    if points.len() <= 2 {
        return Vec::new();
    }
    if points.len() == 3 {
        return vec![1];
    }
    let mut bends = Vec::new();
    for i in 1..points.len() - 1 {
        let prev = points[i - 1];
        let cur = points[i];
        let next = points[i + 1];
        let h1 = segment_is_horizontal(prev, cur);
        let v1 = segment_is_vertical(prev, cur);
        let h2 = segment_is_horizontal(cur, next);
        let v2 = segment_is_vertical(cur, next);
        if (h1 && v2) || (v1 && h2) {
            bends.push(i);
        }
    }
    if bends.is_empty() && points.len() >= 3 {
        bends.push(points.len() / 2);
    }
    bends
}

/// Keep endpoints plus bend corners only (typically 3 points for one elbow).
fn ensure_three_points(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if pts.len() != 2 {
        return pts;
    }
    let a = pts[0];
    let b = pts[1];
    let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    vec![a, mid, b]
}

pub fn canonicalize_flowchart_path_points(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let pts = ensure_three_points(pts);
    if pts.len() <= 3 {
        return orthogonalize_polyline(pts);
    }
    let first = pts[0];
    let last = *pts.last().unwrap();
    let mut out = vec![first];
    for i in flowchart_bend_point_indices(&pts) {
        if i > 0 && i + 1 < pts.len() {
            out.push(pts[i]);
        }
    }
    out.push(last);
    orthogonalize_polyline(dedupe_points(out))
}

pub fn orthogonalize_flowchart_path(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    orthogonalize_polyline(pts)
}

pub fn fix_flowchart_path_anchor_endpoints(
    path: &mut FlowchartPathData,
    nodes: &super::NodeStore,
) {
    if let (Some(nid), Some(anc)) = (path.start_node, path.start_anchor) {
        if let Some(g) = nodes
            .get(nid)
            .and_then(|n| node_as_flowchart_geom(&n.kind))
        {
            if let Some(p) = path.points.first_mut() {
                *p = g.anchor_position(anc);
            }
        }
    }
    if let (Some(nid), Some(anc)) = (path.end_node, path.end_anchor) {
        if let Some(g) = nodes
            .get(nid)
            .and_then(|n| node_as_flowchart_geom(&n.kind))
        {
            if let Some(p) = path.points.last_mut() {
                *p = g.anchor_position(anc);
            }
        }
    }
}

fn route_orthogonal_mid(
    start: (f64, f64),
    end: (f64, f64),
    obstacles: &[Rect],
) -> Vec<(f64, f64)> {
    let mut candidates: Vec<Vec<(f64, f64)>> = vec![
        orthogonal_l_route(start, end, (end.0, start.1)),
        orthogonal_l_route(start, end, (start.0, end.1)),
        orthogonal_elbow_route(start, end),
    ];
    if let Some(detour) = bbox_detour_route(start, end, obstacles) {
        candidates.push(detour);
    }

    // Extra "go high / low first" candidates so opposing ports (left<->right on stacked nodes)
    // produce a route whose horizontal crossing happens outside the vertical spans.
    if !obstacles.is_empty() {
        let pad = FLOWCHART_OBSTACLE_HALO + 10.0;
        let top = obstacles.iter().map(|r| r.y0).fold(f64::INFINITY, f64::min) - pad;
        let bot = obstacles.iter().map(|r| r.y1).fold(f64::NEG_INFINITY, f64::max) + pad;
        let around_top = vec![start, (start.0, top), (end.0, top), end];
        let around_bot = vec![start, (start.0, bot), (end.0, bot), end];
        candidates.push(around_top);
        candidates.push(around_bot);
    }

    // Add candidates for crossing in the gaps between nodes (with margin from nearest)
    // This prevents going far away high/low when there is space between stacked nodes.
    if obstacles.len() >= 2 {
        let mut rects: Vec<Rect> = obstacles.iter().map(|&r| r.inflate(-FLOWCHART_OBSTACLE_HALO, -FLOWCHART_OBSTACLE_HALO)).collect();
        rects.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        for i in 0..rects.len()-1 {
            let higher = &rects[i]; // smaller y0
            let lower = &rects[i+1];
            let gap_top = higher.y1 + FLOWCHART_OBSTACLE_HALO;
            let gap_bot = lower.y0 - FLOWCHART_OBSTACLE_HALO;
            if gap_bot > gap_top {
                let cross_y = (gap_top + gap_bot) * 0.5;
                let c = vec![start, (start.0, cross_y), (end.0, cross_y), end];
                candidates.push(c);
            }
        }
    }
    let mut best: Option<Vec<(f64, f64)>> = None;
    let mut best_score = f64::INFINITY;
    for base in &candidates {
        if route_hits_any_obstacle(base, obstacles) {
            continue;
        }
        let len = route_polyline_length(base);
        let penalty = if route_has_inner_crossing(base, obstacles) { 10000.0 } else { 0.0 };
        let score = len + penalty;
        if score < best_score {
            best_score = score;
            best = Some(base.clone());
        }
    }
    if let Some(route) = best {
        return orthogonalize_polyline(route);
    }
    let pushed = segment_push_route(orthogonal_elbow_route(start, end), obstacles, 0, 0);
    orthogonalize_polyline(pushed)
}

fn orthogonal_l_route(
    start: (f64, f64),
    end: (f64, f64),
    corner: (f64, f64),
) -> Vec<(f64, f64)> {
    let mut pts = vec![start];
    for p in [corner, end] {
        if pts
            .last()
            .copied()
            .map_or(true, |last| (last.0 - p.0).hypot(last.1 - p.1) > 1e-6)
        {
            pts.push(p);
        }
    }
    dedupe_points(pts)
}

fn orthogonal_elbow_route(start: (f64, f64), end: (f64, f64)) -> Vec<(f64, f64)> {
    let mut pts = vec![start];
    let mid_x = (start.0 + end.0) * 0.5;
    for p in [(mid_x, start.1), (mid_x, end.1), end] {
        if pts.last().copied().map_or(true, |last| (last.0 - p.0).hypot(last.1 - p.1) > 1e-6) {
            pts.push(p);
        }
    }
    dedupe_points(pts)
}

fn route_polyline_length(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

fn route_has_inner_crossing(pts: &[(f64, f64)], obstacles: &[Rect]) -> bool {
    if obstacles.is_empty() || pts.len() < 2 {
        return false;
    }
    let min_y = obstacles.iter().map(|r| r.y0).fold(f64::INFINITY, f64::min);
    let max_y = obstacles.iter().map(|r| r.y1).fold(f64::NEG_INFINITY, f64::max);
    let min_x = obstacles.iter().map(|r| r.x0).fold(f64::INFINITY, f64::min);
    let max_x = obstacles.iter().map(|r| r.x1).fold(f64::NEG_INFINITY, f64::max);
    for w in pts.windows(2) {
        let a = w[0];
        let b = w[1];
        let is_h = (a.1 - b.1).abs() < 1e-6 && (a.0 - b.0).abs() > 1e-6;
        let is_v = (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() > 1e-6;
        if is_h {
            let y = a.1;
            if y > min_y && y < max_y {
                // check if x span overlaps some obstacle that contains this y
                let x0 = a.0.min(b.0);
                let x1 = a.0.max(b.0);
                for r in obstacles {
                    if y >= r.y0 && y <= r.y1 && x1 >= r.x0 && x0 <= r.x1 {
                        return true;
                    }
                }
            }
        } else if is_v {
            let x = a.0;
            if x > min_x && x < max_x {
                let y0 = a.1.min(b.1);
                let y1 = a.1.max(b.1);
                for r in obstacles {
                    if x >= r.x0 && x <= r.x1 && y1 >= r.y0 && y0 <= r.y1 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn route_hits_any_obstacle(pts: &[(f64, f64)], obstacles: &[Rect]) -> bool {
    if obstacles.is_empty() || pts.len() < 2 {
        return false;
    }
    for w in pts.windows(2) {
        for &r in obstacles {
            if segment_intersects_rect_interior(w[0], w[1], r) {
                return true;
            }
        }
    }
    false
}

/// Route that goes around the union of obstacles blocking the straight L-shapes between stubs.
fn bbox_detour_route(
    start: (f64, f64),
    end: (f64, f64),
    obstacles: &[Rect],
) -> Option<Vec<(f64, f64)>> {
    if obstacles.is_empty() {
        return None;
    }
    let probes = [
        orthogonal_l_route(start, end, (end.0, start.1)),
        orthogonal_l_route(start, end, (start.0, end.1)),
        orthogonal_elbow_route(start, end),
    ];
    let mut blocking: Vec<Rect> = Vec::new();
    for probe in &probes {
        for w in probe.windows(2) {
            for &r in obstacles {
                if segment_intersects_rect_interior(w[0], w[1], r)
                    && !blocking.iter().any(|b| rects_near_equal(*b, r))
                {
                    blocking.push(r);
                }
            }
        }
    }
    if blocking.is_empty() {
        return None;
    }
    let mut union = blocking[0];
    for r in blocking.iter().skip(1) {
        union = union.union(*r);
    }
    let pad = FLOWCHART_OBSTACLE_HALO + 6.0;
    let u = union.inflate(pad, pad);
    let variants = [
        vec![start, (u.x0, start.1), (u.x0, end.1), end],
        vec![start, (u.x1, start.1), (u.x1, end.1), end],
        vec![start, (start.0, u.y0), (end.0, u.y0), end],
        vec![start, (start.0, u.y1), (end.0, u.y1), end],
    ];
    variants
        .into_iter()
        .map(|v| dedupe_points(v))
        .filter(|r| !route_hits_any_obstacle(r, obstacles))
        .min_by(|a, b| {
            route_polyline_length(a)
                .partial_cmp(&route_polyline_length(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn rects_near_equal(a: Rect, b: Rect) -> bool {
    (a.x0 - b.x0).abs() < 0.5
        && (a.y0 - b.y0).abs() < 0.5
        && (a.x1 - b.x1).abs() < 0.5
        && (a.y1 - b.y1).abs() < 0.5
}

pub(crate) fn segment_is_horizontal(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.1 - b.1).abs() < 1e-6 && (a.0 - b.0).abs() > 1e-6
}

pub(crate) fn segment_is_vertical(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() > 1e-6
}

/// Force every leg to be purely horizontal or vertical (no diagonals / rope tangles).
fn orthogonalize_polyline(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if pts.len() < 2 {
        return pts;
    }
    let mut out = vec![pts[0]];
    for i in 1..pts.len() {
        let prev = *out.last().unwrap();
        let target = pts[i];
        if (target.0 - prev.0).hypot(target.1 - prev.1) < 1e-6 {
            continue;
        }
        if segment_is_horizontal(prev, target) || segment_is_vertical(prev, target) {
            out.push(target);
            continue;
        }
        let incoming_h = out.len() >= 2 && segment_is_horizontal(out[out.len() - 2], prev);
        let corner = if incoming_h {
            (target.0, prev.1)
        } else {
            (prev.0, target.1)
        };
        if (corner.0 - prev.0).hypot(corner.1 - prev.1) > 1e-6 {
            out.push(corner);
        }
        if (target.0 - corner.0).hypot(target.1 - corner.1) > 1e-6 {
            out.push(target);
        }
    }
    collapse_collinear(dedupe_points(out))
}

fn collapse_collinear(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts;
    }
    let mut out = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let a = *out.last().unwrap();
        let b = pts[i];
        let c = pts[i + 1];
        let collinear_h = (a.1 - b.1).abs() < 1e-6
            && (b.1 - c.1).abs() < 1e-6
            && b.0 >= a.0.min(c.0) - 1e-6
            && b.0 <= a.0.max(c.0) + 1e-6;
        let collinear_v = (a.0 - b.0).abs() < 1e-6
            && (b.0 - c.0).abs() < 1e-6
            && b.1 >= a.1.min(c.1) - 1e-6
            && b.1 <= a.1.max(c.1) + 1e-6;
        if !collinear_h && !collinear_v {
            out.push(b);
        }
    }
    out.push(*pts.last().unwrap());
    out
}

/// Push axis-aligned segments off avoidance rects; re-orthogonalize after each pass.
fn segment_push_route(
    mut pts: Vec<(f64, f64)>,
    obstacles: &[Rect],
    protect_leading_segments: usize,
    protect_trailing_segments: usize,
) -> Vec<(f64, f64)> {
    if pts.len() < 2 || obstacles.is_empty() {
        return pts;
    }
    pts = orthogonalize_polyline(pts);

    for _ in 0..4 {
        let seg_count = pts.len().saturating_sub(1);
        if seg_count == 0 {
            break;
        }
        let first_pushable = protect_leading_segments.min(seg_count);
        let last_pushable = seg_count.saturating_sub(protect_trailing_segments);
        if first_pushable >= last_pushable {
            break;
        }
        let mut changed = false;
        for i in first_pushable..last_pushable {
            if i + 1 >= pts.len() {
                break;
            }
            let a = pts[i];
            let b = pts[i + 1];
            let is_h = segment_is_horizontal(a, b);
            let is_v = segment_is_vertical(a, b);
            if !is_h && !is_v {
                continue;
            }
            let Some(avoid) = obstacles
                .iter()
                .copied()
                .find(|&r| segment_intersects_avoid_rect(a, b, r))
            else {
                continue;
            };
            if is_v {
                let x = a.0;
                let push_x = if (x - avoid.x0).abs() <= (avoid.x1 - x).abs() {
                    avoid.x0
                } else {
                    avoid.x1
                };
                pts[i].0 = push_x;
                pts[i + 1].0 = push_x;
                changed = true;
            } else {
                let y = a.1;
                let push_y = if (y - avoid.y0).abs() <= (avoid.y1 - y).abs() {
                    avoid.y0
                } else {
                    avoid.y1
                };
                pts[i].1 = push_y;
                pts[i + 1].1 = push_y;
                changed = true;
            }
        }
        pts = orthogonalize_polyline(pts);
        if !changed {
            break;
        }
    }
    pts
}

fn segment_intersects_avoid_rect(a: (f64, f64), b: (f64, f64), avoid: Rect) -> bool {
    if avoid.contains(Point::new(a.0, a.1)) || avoid.contains(Point::new(b.0, b.1)) {
        return true;
    }
    let dx = (b.0 - a.0).abs();
    let dy = (b.1 - a.1).abs();
    if dy < 1e-6 {
        let y = a.1;
        if y >= avoid.y0 && y <= avoid.y1 {
            let x0 = a.0.min(b.0);
            let x1 = a.0.max(b.0);
            return x1 >= avoid.x0 && x0 <= avoid.x1;
        }
    }
    if dx < 1e-6 {
        let x = a.0;
        if x >= avoid.x0 && x <= avoid.x1 {
            let y0 = a.1.min(b.1);
            let y1 = a.1.max(b.1);
            return y1 >= avoid.y0 && y0 <= avoid.y1;
        }
    }
    false
}

fn segment_intersects_rect_interior(a: (f64, f64), b: (f64, f64), r: Rect) -> bool {
    let pad = 6.0;
    let inner = r.inflate(-pad, -pad);
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return false;
    }
    if inner.contains(Point::new(a.0, a.1)) || inner.contains(Point::new(b.0, b.1)) {
        return true;
    }
    segment_hits_obstacles(a, b, &[inner])
}

fn dedupe_points(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for p in pts {
        if out.last().copied().map_or(true, |q: (f64, f64)| (q.0 - p.0).hypot(q.1 - p.1) > 0.5) {
            out.push(p);
        }
    }
    out
}

fn segment_hits_obstacles(a: (f64, f64), b: (f64, f64), obstacles: &[Rect]) -> bool {
    let seg = Rect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
    obstacles.iter().any(|r| {
        let inflated = r.inflate(4.0, 4.0);
        inflated.intersect(seg).width() > 0.0 && inflated.intersect(seg).height() > 0.0
    })
}

pub fn polyline_segments(points: &[(f64, f64)]) -> Vec<((f64, f64), (f64, f64))> {
    points.windows(2).map(|w| (w[0], w[1])).collect()
}

pub fn flatten_flowchart_stroke(points: &[(f64, f64)], corner_r: f64) -> Vec<(f64, f64)> {
    let bez = rounded_orthogonal_bez(points, corner_r);
    let mut out = Vec::new();
    let els: Vec<PathEl> = bez.elements().iter().copied().collect();
    kurbo::flatten(els, 0.35, |el| {
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => out.push((p.x, p.y)),
            _ => {}
        }
    });
    out
}

pub fn flowchart_stroke_hit_with_corner(
    points: &[(f64, f64)],
    doc_x: f64,
    doc_y: f64,
    stroke_slop: f64,
    stroke_width: f64,
    corner_r: f64,
) -> bool {
    let tol = stroke_slop
        .max(stroke_width * 0.5 + 4.0)
        .max(8.0);
    let flat = flatten_flowchart_stroke(points, corner_r.max(2.0));
    for w in flat.windows(2) {
        if point_near_segment(doc_x, doc_y, w[0].0, w[0].1, w[1].0, w[1].1, tol) {
            return true;
        }
    }
    false
}

fn point_near_segment(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64, tol: f64) -> bool {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return (px - x0).hypot(py - y0) <= tol;
    }
    let t = ((px - x0) * dx + (py - y0) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let qx = x0 + t * dx;
    let qy = y0 + t * dy;
    (px - qx).hypot(py - qy) <= tol
}

#[derive(Clone)]
struct EdgeSortEntry {
    path_id: NodeId,
    is_start: bool,
    sort_key: f64,
}

const PREVIEW_PATH_SENTINEL: uuid::Uuid = uuid::Uuid::from_u128(0);

fn edge_sort_key(_geom: &FlowchartNodeGeom, side: FlowchartEdgeSide, doc: (f64, f64)) -> f64 {
    match side {
        FlowchartEdgeSide::Top | FlowchartEdgeSide::Bottom => doc.0,
        FlowchartEdgeSide::Left | FlowchartEdgeSide::Right => doc.1,
    }
}

/// Reassign edge slots on each node side using `(2 + n)` subdivision (`n` = connection count).
pub fn rebalance_flowchart_edge_anchors(store: &mut NodeStore, layer_node_ids: &[NodeId]) {
    rebalance_flowchart_edge_anchors_with_pending(store, layer_node_ids, &[]);
}

/// `pending` adds preview endpoints (e.g. connector drag) for slot layout without writing them.
pub fn rebalance_flowchart_edge_anchors_with_pending(
    store: &mut NodeStore,
    layer_node_ids: &[NodeId],
    pending: &[(NodeId, FlowchartAnchor, (f64, f64))],
) {
    let mut by_edge: HashMap<(NodeId, FlowchartEdgeSide), Vec<EdgeSortEntry>> = HashMap::new();

    for &pid in layer_node_ids {
        let Some(node) = store.get(pid) else {
            continue;
        };
        let NodeKind::FlowchartPath { path } = &node.kind else {
            continue;
        };
        if let (Some(nid), Some(anc)) = (path.start_node, path.start_anchor) {
            if let FlowchartAnchor::Edge { side, .. } = anc {
                if let Some(geom) = store
                    .get(nid)
                    .and_then(|n| node_as_flowchart_geom(&n.kind))
                {
                    let doc = path.points.first().copied().unwrap_or((0.0, 0.0));
                    by_edge.entry((nid, side)).or_default().push(EdgeSortEntry {
                        path_id: pid,
                        is_start: true,
                        sort_key: edge_sort_key(&geom, side, doc),
                    });
                }
            }
        }
        if let (Some(nid), Some(anc)) = (path.end_node, path.end_anchor) {
            if let FlowchartAnchor::Edge { side, .. } = anc {
                if let Some(geom) = store
                    .get(nid)
                    .and_then(|n| node_as_flowchart_geom(&n.kind))
                {
                    let doc = path.points.last().copied().unwrap_or((0.0, 0.0));
                    by_edge.entry((nid, side)).or_default().push(EdgeSortEntry {
                        path_id: pid,
                        is_start: false,
                        sort_key: edge_sort_key(&geom, side, doc),
                    });
                }
            }
        }
    }

    for (nid, anc, doc) in pending {
        if let FlowchartAnchor::Edge { side, .. } = anc {
            if let Some(geom) = store
                .get(*nid)
                .and_then(|n| node_as_flowchart_geom(&n.kind))
            {
                by_edge.entry((*nid, *side)).or_default().push(EdgeSortEntry {
                    path_id: PREVIEW_PATH_SENTINEL,
                    is_start: false,
                    sort_key: edge_sort_key(&geom, *side, *doc),
                });
            }
        }
    }

    let mut updates: Vec<(NodeId, bool, FlowchartAnchor)> = Vec::new();

    for ((nid, side), refs) in &by_edge {
        let Some(node) = store.get(*nid) else {
            continue;
        };
        let Some(geom) = node_as_flowchart_geom(&node.kind) else {
            continue;
        };
        let n = refs.len() as u32;
        if n == 0 {
            continue;
        }
        let mut ordered = refs.clone();
        ordered.sort_by(|a, b| {
            a.sort_key
                .partial_cmp(&b.sort_key)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (slot, entry) in ordered.into_iter().enumerate() {
            if entry.path_id == PREVIEW_PATH_SENTINEL {
                continue;
            }
            let anc = FlowchartAnchor::edge(*side, slot as u32, n);
            updates.push((entry.path_id, entry.is_start, anc));
        }
        let _ = geom;
    }

    for (path_id, is_start, anc) in updates {
        let Some(node) = store.get_mut(path_id) else {
            continue;
        };
        let NodeKind::FlowchartPath { path } = &mut node.kind else {
            continue;
        };
        let preserved = if is_start {
            path.start_anchor
        } else {
            path.end_anchor
        };
        let final_anc = match (preserved, &anc) {
            (
                Some(FlowchartAnchor::Edge {
                    edge_t: Some(t),
                    side: old_side,
                    ..
                }),
                FlowchartAnchor::Edge { side, slot, slots, .. },
            ) if old_side == *side => FlowchartAnchor::edge_at_doc_t(*side, *slot, *slots, t),
            _ => anc,
        };
        if is_start {
            path.start_anchor = Some(final_anc);
        } else {
            path.end_anchor = Some(final_anc);
        }
    }
}

pub fn node_as_flowchart_geom(kind: &NodeKind) -> Option<FlowchartNodeGeom> {
    match kind {
        NodeKind::FlowchartNode {
            cx,
            cy,
            w,
            h,
            corner_rx,
            ..
        } => Some(FlowchartNodeGeom {
            cx: *cx,
            cy: *cy,
            w: *w,
            h: *h,
            corner_rx: *corner_rx,
        }),
        _ => None,
    }
}

pub fn new_flowchart_node(cx: f64, cy: f64) -> Node {
    Node::new(
        NodeKind::FlowchartNode {
            cx,
            cy,
            w: 160.0,
            h: 72.0,
            corner_rx: 24.0,
            label: String::new(),
            label_font_size: 14.0,
            label_align: super::TextAlign::Center,
            label_font_family: "Noto Sans".to_string(),
            label_bold: false,
            label_italic: false,
        },
        "Flowchart node",
    )
}

pub fn new_flowchart_node_from_rect(x: f64, y: f64, w: f64, h: f64) -> Node {
    let corner_rx = (w.min(h) * 0.22).clamp(8.0, 48.0);
    Node::new(
        NodeKind::FlowchartNode {
            cx: x + w * 0.5,
            cy: y + h * 0.5,
            w,
            h,
            corner_rx,
            label: String::new(),
            label_font_size: 14.0,
            label_align: super::TextAlign::Center,
            label_font_family: "Noto Sans".to_string(),
            label_bold: false,
            label_italic: false,
        },
        "Flowchart node",
    )
}

/// Re-route connector when anchored endpoints or obstacles change.
pub fn sync_flowchart_path_endpoints(
    path: &mut FlowchartPathData,
    nodes: &super::NodeStore,
    obstacles: &[Rect],
) {
    let mut start = path.points.first().copied();
    let mut end = path.points.last().copied();
    let mut start_anc = path.start_anchor;
    let mut end_anc = path.end_anchor;
    if let (Some(nid), Some(anc)) = (path.start_node, path.start_anchor) {
        if let Some(n) = nodes.get(nid) {
            if let Some(g) = node_as_flowchart_geom(&n.kind) {
                start = Some(g.anchor_position(anc));
                start_anc = Some(anc);
            }
        }
    }
    if let (Some(nid), Some(anc)) = (path.end_node, path.end_anchor) {
        if let Some(n) = nodes.get(nid) {
            if let Some(g) = node_as_flowchart_geom(&n.kind) {
                end = Some(g.anchor_position(anc));
                end_anc = Some(anc);
            }
        }
    }
    let Some(s) = start else {
        return;
    };
    let Some(e) = end else {
        return;
    };

    let (sn, en) = match (start_anc, end_anc) {
        (Some(sa), Some(ea)) => (
            anchor_port_normal(sa, s, e),
            anchor_port_normal(ea, e, s),
        ),
        (Some(sa), None) => (
            anchor_port_normal(sa, s, e),
            estimated_port_normals(s, e).1,
        ),
        (None, Some(ea)) => (
            estimated_port_normals(s, e).0,
            anchor_port_normal(ea, e, s),
        ),
        (None, None) => estimated_port_normals(s, e),
    };
    path.points = route_orthogonal_with_normals(s, e, sn, en, obstacles);
    path.points = canonicalize_flowchart_path_points(path.points.clone());
    fix_flowchart_path_anchor_endpoints(path, nodes);
}

/// Doc-space intersection points between this path and other flowchart paths (for line jumps).
pub fn flowchart_path_jump_points(
    points: &[(f64, f64)],
    others: &[&[(f64, f64)]],
) -> Vec<(f64, f64)> {
    let segs_a = polyline_segments(points);
    let mut jumps: Vec<(f64, f64)> = Vec::new();
    for other in others {
        for seg_b in polyline_segments(other) {
            for seg_a in &segs_a {
                if let Some(p) = segment_intersection(*seg_a, seg_b) {
                    if !jumps.iter().any(|q: &(f64, f64)| (q.0 - p.0).hypot(q.1 - p.1) < 3.0) {
                        jumps.push(p);
                    }
                }
            }
        }
    }
    jumps
}

fn segment_intersection(
    a: ((f64, f64), (f64, f64)),
    b: ((f64, f64), (f64, f64)),
) -> Option<(f64, f64)> {
    let (x1, y1) = a.0;
    let (x2, y2) = a.1;
    let (x3, y3) = b.0;
    let (x4, y4) = b.1;
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;
    if t > 0.05 && t < 0.95 && u > 0.05 && u < 0.95 {
        Some((x1 + t * (x2 - x1), y1 + t * (y2 - y1)))
    } else {
        None
    }
}

/// Orthogonal polyline with rounded corners (doc space).
pub fn rounded_orthogonal_bez(points: &[(f64, f64)], radius: f64) -> BezPath {
    let mut bez = BezPath::new();
    if points.len() < 2 {
        return bez;
    }
    if points.len() == 2 {
        bez.push(PathEl::MoveTo(Point::new(points[0].0, points[0].1)));
        bez.push(PathEl::LineTo(Point::new(points[1].0, points[1].1)));
        return bez;
    }
    let r = radius.max(2.0);
    bez.push(PathEl::MoveTo(Point::new(points[0].0, points[0].1)));
    for i in 1..points.len() - 1 {
        let prev = points[i - 1];
        let cur = points[i];
        let next = points[i + 1];
        let v1 = Vec2::new(cur.0 - prev.0, cur.1 - prev.1);
        let v2 = Vec2::new(next.0 - cur.0, next.1 - cur.1);
        let l1 = v1.hypot().max(1e-6);
        let l2 = v2.hypot().max(1e-6);
        let trim = r.min(l1 * 0.45).min(l2 * 0.45);
        let u1 = Vec2::new(v1.x / l1, v1.y / l1);
        let u2 = Vec2::new(v2.x / l2, v2.y / l2);
        let before = Point::new(cur.0 - u1.x * trim, cur.1 - u1.y * trim);
        let after = Point::new(cur.0 + u2.x * trim, cur.1 + u2.y * trim);
        bez.push(PathEl::LineTo(before));
        bez.push(PathEl::QuadTo(
            Point::new(cur.0, cur.1),
            after,
        ));
    }
    let last = points[points.len() - 1];
    bez.push(PathEl::LineTo(Point::new(last.0, last.1)));
    bez
}

pub fn new_flowchart_path(points: Vec<(f64, f64)>) -> Node {
    Node::new(
        NodeKind::FlowchartPath {
            path: FlowchartPathData {
                points,
                start_node: None,
                start_anchor: None,
                end_node: None,
                end_anchor: None,
                endpoint_marker_size: default_endpoint_marker(),
                corner_radius: default_path_corner(),
            },
        },
        "Flowchart path",
    )
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use kurbo::Rect;

    #[test]
    fn stacked_nodes_left_right_stubs_avoid_bodies() {
        let top = FlowchartNodeGeom {
            cx: 200.0,
            cy: 100.0,
            w: 120.0,
            h: 60.0,
            corner_rx: 12.0,
        };
        let bottom = FlowchartNodeGeom {
            cx: 200.0,
            cy: 220.0,
            w: 120.0,
            h: 60.0,
            corner_rx: 12.0,
        };
        let start = top.anchor_position(FlowchartAnchor::edge(
            FlowchartEdgeSide::Left,
            0,
            1,
        ));
        let end = bottom.anchor_position(FlowchartAnchor::edge(
            FlowchartEdgeSide::Right,
            0,
            1,
        ));
        let obstacles = [
            top.doc_rect().inflate(FLOWCHART_OBSTACLE_HALO, FLOWCHART_OBSTACLE_HALO),
            bottom.doc_rect().inflate(FLOWCHART_OBSTACLE_HALO, FLOWCHART_OBSTACLE_HALO),
        ];
        let sn = anchor_port_normal(
            FlowchartAnchor::edge(FlowchartEdgeSide::Left, 0, 1),
            start,
            end,
        );
        let en = anchor_port_normal(
            FlowchartAnchor::edge(FlowchartEdgeSide::Right, 0, 1),
            end,
            start,
        );
        let pts = route_orthogonal_with_normals(start, end, sn, en, &obstacles);
        assert!(pts.len() >= 3);
        // Only check non-stub segments (stubs legitimately start/end on/near node boundary)
        let internal_windows: Vec<_> = if pts.len() > 2 {
            pts.windows(2).skip(1).take(pts.len().saturating_sub(3)).collect()
        } else { vec![] };
        for w in internal_windows {
            for &r in &obstacles {
                assert!(
                    !segment_intersects_rect_interior(w[0], w[1], r),
                    "segment {:?} {:?} crosses obstacle",
                    w[0],
                    w[1]
                );
            }
        }
    }

    #[test]
    fn same_node_left_to_top_avoid_body() {
        let geom = FlowchartNodeGeom {
            cx: 100.0,
            cy: 100.0,
            w: 100.0,
            h: 80.0,
            corner_rx: 10.0,
        };
        let start = geom.anchor_position(FlowchartAnchor::edge(
            FlowchartEdgeSide::Left,
            0,
            1,
        ));
        let end = geom.anchor_position(FlowchartAnchor::edge(
            FlowchartEdgeSide::Top,
            0,
            1,
        ));
        let obstacle = [geom
            .doc_rect()
            .inflate(FLOWCHART_OBSTACLE_HALO, FLOWCHART_OBSTACLE_HALO)];
        let sn = anchor_port_normal(
            FlowchartAnchor::edge(FlowchartEdgeSide::Left, 0, 1),
            start,
            end,
        );
        let en = anchor_port_normal(
            FlowchartAnchor::edge(FlowchartEdgeSide::Top, 0, 1),
            end,
            start,
        );
        let pts = route_orthogonal_with_normals(start, end, sn, en, &obstacle);
        // Only check non-stub internal segments for self-connect on same node
        let internal: Vec<_> = if pts.len() > 2 {
            pts.windows(2).skip(1).take(pts.len().saturating_sub(3)).collect()
        } else { vec![] };
        for w in internal {
            assert!(!segment_intersects_rect_interior(w[0], w[1], obstacle[0]));
        }
    }
}