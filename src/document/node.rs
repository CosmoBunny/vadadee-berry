use std::collections::HashMap;

use kurbo::{BezPath, Rect, Shape};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Fill, NodeStyle, Paint, Stroke};

pub type NodeId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathEditTarget {
    Anchor(usize),
    HandleOut(usize),
    HandleIn(usize),
}

impl PathEditTarget {
    pub fn anchor_index(self) -> usize {
        match self {
            Self::Anchor(i) | Self::HandleOut(i) | Self::HandleIn(i) => i,
        }
    }
}

/// How paired bezier handles behave when one is dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BezierHandleMode {
    /// Opposite direction, equal length (Inkscape symmetric).
    #[default]
    Symmetric,
    /// Each handle moves independently.
    Asymmetric,
    /// Opposite direction with equal length (alias for symmetric, shown in UI).
    EqualLength,
    /// Single incoming handle.
    LeftOnly,
    /// Single outgoing handle.
    RightOnly,
    /// Both handles independent.
    Both,
}

impl BezierHandleMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Symmetric => "Symmetric",
            Self::Asymmetric => "Asymmetric",
            Self::EqualLength => "Equal Length",
            Self::LeftOnly => "Left Only",
            Self::RightOnly => "Right Only",
            Self::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeometryProfile {
    Rect {
        origin_x: f64,
        origin_y: f64,
        width: f64,
        height: f64,
        corner_radius: f64,
    },
    Circle {
        origin_x: f64,
        origin_y: f64,
        radius: f64,
    },
    Ellipse {
        origin_x: f64,
        origin_y: f64,
        radius_x: f64,
        radius_y: f64,
    },
    Line {
        origin_x: f64,
        origin_y: f64,
        end_x: f64,
        end_y: f64,
        length: f64,
        angle_deg: f64,
    },
    ClosedPath {
        vertices: usize,
        cyclic: bool,
    },
    OpenPath {
        vertices: usize,
        cyclic: bool,
    },
    /// Arc / chord / pie slice.
    Arc {
        origin_x: f64,
        origin_y: f64,
        radius: f64,
        start_angle_deg: f64,
        sweep_angle_deg: f64,
        join: ArcJoin,
    },
    Polygon {
        origin_x: f64,
        origin_y: f64,
        radius: f64,
        sides: u32,
        rotation_deg: f64,
    },
    Text {
        origin_x: f64,
        origin_y: f64,
        width: f64,
        height: f64,
        content: String,
        font_size: f32,
        font_family: String,
        bold: bool,
        italic: bool,
    },
    Unsupported,
}

fn default_font_family() -> String {
    "Noto Sans".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub content: String,
    pub font_size: f32,
    #[serde(default = "default_font_family", alias = "family")]
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            content: "Text".into(),
            font_size: 24.0,
            font_family: default_font_family(),
            bold: false,
            italic: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Rect { x: f64, y: f64, w: f64, h: f64, rx: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Polygon {
        cx: f64,
        cy: f64,
        r: f64,
        sides: u32,
        rotation_rad: f64,
    },
    Path { path: PathData },
    Text { x: f64, y: f64, style: TextStyle },
    Group { children: Vec<NodeId> },
    Image {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        /// Embedded original bytes (PNG or JPEG) for fidelity and save/load.
        bytes: Vec<u8>,
    },
    Arc {
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle_rad: f64,
        sweep_angle_rad: f64,
        /// Determines the closed shape for filling (if fill visible):
        /// - NoJoin: open arc (stroke only typically)
        /// - Chord: arc + straight line from end back to start (filled "chord"/segment)
        /// - ToOrigin: pie slice (arc + lines from center to start and end)
        join: ArcJoin,
    },
    BrushStroke {
        points: Vec<([f64; 2], f32)>, // pos, width
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArcJoin {
    #[default]
    NoJoin,
    Chord,    // "end to start point"
    ToOrigin, // "to origin" / pie
}

pub fn build_arc_bez(
    cx: f64,
    cy: f64,
    radius: f64,
    start: f64,
    sweep: f64,
    join: ArcJoin,
) -> BezPath {
    let mut path = BezPath::new();
    let p0 = (cx + radius * start.cos(), cy + radius * start.sin());
    path.move_to(p0);

    // Approximate arc with a cubic or use kurbo Arc if possible; for simplicity use to_path on ellipse sector.
    // kurbo Ellipse + arc_to is limited; we use a simple multi-line approx or kurbo's Arc.
    // kurbo 0.13 has Arc:
    let arc = kurbo::Arc::new(
        (cx, cy),
        (radius, radius),
        start,
        sweep,
        0.0,
    );
    // Append the arc segments (to_path gives the curve)
    let arc_path = arc.to_path(0.1);
    // Skip the initial move of the arc_path since we already moved
    for el in arc_path.elements().iter().skip(1) {
        path.push(*el);
    }

    match join {
        ArcJoin::NoJoin => {
            // pure arc, leave open
        }
        ArcJoin::Chord => {
            path.line_to(p0); // close with chord
            path.close_path();
        }
        ArcJoin::ToOrigin => {
            // line to center then back to start
            path.line_to((cx, cy));
            path.line_to(p0);
            path.close_path();
        }
    }
    path
}

pub fn regular_polygon_vertices(
    cx: f64,
    cy: f64,
    r: f64,
    sides: u32,
    rotation_rad: f64,
) -> Vec<(f64, f64)> {
    let n = sides.max(3) as f64;
    (0..sides.max(3))
        .map(|i| {
            let a = rotation_rad - std::f64::consts::FRAC_PI_2 + i as f64 * 2.0 * std::f64::consts::PI / n;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathData {
    pub verbs: Vec<u8>,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    /// Anchor indices (see `path_anchor_point_indices`) with smooth/bezier corners.
    #[serde(default)]
    pub smooth_anchors: Vec<usize>,
    /// Outgoing control offsets from each smooth anchor (doc space).
    #[serde(default)]
    pub handle_out_offset: HashMap<usize, [f64; 2]>,
    /// Incoming control offsets from each smooth anchor (doc space).
    #[serde(default)]
    pub handle_in_offset: HashMap<usize, [f64; 2]>,
    /// Per-anchor handle coupling mode.
    #[serde(default)]
    pub handle_modes: HashMap<usize, BezierHandleMode>,
}

impl PathData {
    pub fn from_bez(path: &BezPath) -> Self {
        let mut verbs = Vec::new();
        let mut points = Vec::new();
        for el in path.elements() {
            use kurbo::PathEl;
            match el {
                PathEl::MoveTo(p) => {
                    verbs.push(0);
                    points.push([p.x, p.y]);
                }
                PathEl::LineTo(p) => {
                    verbs.push(1);
                    points.push([p.x, p.y]);
                }
                PathEl::QuadTo(p1, p2) => {
                    verbs.push(2);
                    points.push([p1.x, p1.y]);
                    points.push([p2.x, p2.y]);
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    verbs.push(3);
                    points.push([p1.x, p1.y]);
                    points.push([p2.x, p2.y]);
                    points.push([p3.x, p3.y]);
                }
                PathEl::ClosePath => verbs.push(4),
            }
        }
        Self {
            verbs,
            points,
            closed: path.elements().last().is_some_and(|e| matches!(e, kurbo::PathEl::ClosePath)),
            smooth_anchors: Vec::new(),
            handle_out_offset: HashMap::new(),
            handle_in_offset: HashMap::new(),
            handle_modes: HashMap::new(),
        }
    }

    pub fn handle_mode(&self, anchor_idx: usize) -> BezierHandleMode {
        self.handle_modes
            .get(&anchor_idx)
            .copied()
            .unwrap_or(BezierHandleMode::Symmetric)
    }

    pub fn set_handle_mode(&mut self, anchor_idx: usize, mode: BezierHandleMode) {
        self.handle_modes.insert(anchor_idx, mode);
        if matches!(mode, BezierHandleMode::Symmetric | BezierHandleMode::Asymmetric | BezierHandleMode::EqualLength | BezierHandleMode::Both) {
            // Ensure both handles exist (mirror the one we have) so the canvas draws
            // the "second line" for the independent handle immediately.
            let has_out = self.handle_out_offset.contains_key(&anchor_idx);
            let has_in = self.handle_in_offset.contains_key(&anchor_idx);
            if has_out && !has_in {
                if let Some(&off) = self.handle_out_offset.get(&anchor_idx) {
                    self.handle_in_offset.insert(anchor_idx, [-off[0], -off[1]]);
                }
            } else if has_in && !has_out {
                if let Some(&off) = self.handle_in_offset.get(&anchor_idx) {
                    self.handle_out_offset.insert(anchor_idx, [-off[0], -off[1]]);
                }
            }
        } else if mode == BezierHandleMode::LeftOnly {
            self.handle_out_offset.remove(&anchor_idx);
            if !self.handle_in_offset.contains_key(&anchor_idx) {
                let anchors = self.anchor_positions();
                let closed = self.is_closed();
                let tan = anchor_tangent(&anchors, anchor_idx, closed);
                let dist = 32.0;
                self.handle_in_offset.insert(anchor_idx, [-tan.0 * dist, -tan.1 * dist]);
            }
        } else if mode == BezierHandleMode::RightOnly {
            self.handle_in_offset.remove(&anchor_idx);
            if !self.handle_out_offset.contains_key(&anchor_idx) {
                let anchors = self.anchor_positions();
                let closed = self.is_closed();
                let tan = anchor_tangent(&anchors, anchor_idx, closed);
                let dist = 32.0;
                self.handle_out_offset.insert(anchor_idx, [tan.0 * dist, tan.1 * dist]);
            }
        }
        // Rebuild so the baked curve points reflect any newly initialized opposite handle.
        let anchors = self.anchor_positions();
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn from_anchor_data(
        anchors: &[(f64, f64)],
        smooth_anchors: &[usize],
        handle_out_offset: HashMap<usize, [f64; 2]>,
        handle_in_offset: HashMap<usize, [f64; 2]>,
        closed: bool,
    ) -> Self {
        let mut path = Self {
            verbs: Vec::new(),
            points: Vec::new(),
            closed,
            smooth_anchors: smooth_anchors.to_vec(),
            handle_out_offset,
            handle_in_offset,
            handle_modes: HashMap::new(),
        };
        path.rebuild_with_smooth_anchors(anchors);
        path
    }

    pub fn anchor_positions(&self) -> Vec<(f64, f64)> {
        path_anchor_positions(self)
    }

    pub fn is_anchor_smooth(&self, anchor_idx: usize) -> bool {
        self.smooth_anchors.contains(&anchor_idx)
    }

    pub fn toggle_anchor_bezier(&mut self, anchor_idx: usize) {
        let smooth = !self.is_anchor_smooth(anchor_idx);
        self.set_anchor_smooth(anchor_idx, smooth);
    }

    pub fn set_anchor_smooth(&mut self, anchor_idx: usize, smooth: bool) {
        let anchors = self.anchor_positions();
        if anchor_idx >= anchors.len() {
            return;
        }
        if smooth {
            if !self.smooth_anchors.contains(&anchor_idx) {
                self.smooth_anchors.push(anchor_idx);
                self.smooth_anchors.sort_unstable();
                self.smooth_anchors.dedup();
            }
            let tan = anchor_tangent(&anchors, anchor_idx, self.is_closed());
            let dist = if anchors.len() > 1 {
                let prev_idx = if anchor_idx > 0 { anchor_idx - 1 } else { anchors.len() - 1 };
                let next_idx = (anchor_idx + 1) % anchors.len();
                let d1 = (anchors[anchor_idx].0 - anchors[prev_idx].0).hypot(anchors[anchor_idx].1 - anchors[prev_idx].1);
                let d2 = (anchors[next_idx].0 - anchors[anchor_idx].0).hypot(anchors[next_idx].1 - anchors[anchor_idx].1);
                (d1 + d2) * 0.25
            } else {
                30.0
            }.max(1.0);
            self.handle_out_offset.entry(anchor_idx).or_insert([tan.0 * dist, tan.1 * dist]);
            self.handle_in_offset.entry(anchor_idx).or_insert([-tan.0 * dist, -tan.1 * dist]);
        } else {
            self.smooth_anchors.retain(|&i| i != anchor_idx);
            self.handle_out_offset.remove(&anchor_idx);
            self.handle_in_offset.remove(&anchor_idx);
            self.handle_modes.remove(&anchor_idx);
        }
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn set_handle_out(&mut self, anchor_idx: usize, x: f64, y: f64) {
        let anchors = self.anchor_positions();
        let Some(&(ax, ay)) = anchors.get(anchor_idx) else {
            return;
        };
        if !self.is_anchor_smooth(anchor_idx) {
            self.smooth_anchors.push(anchor_idx);
            self.smooth_anchors.sort_unstable();
            self.smooth_anchors.dedup();
        }
        let offset = [x - ax, y - ay];
        self.apply_handle_drag(anchor_idx, true, offset);
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn set_handle_in(&mut self, anchor_idx: usize, x: f64, y: f64) {
        let anchors = self.anchor_positions();
        let Some(&(ax, ay)) = anchors.get(anchor_idx) else {
            return;
        };
        if !self.is_anchor_smooth(anchor_idx) {
            self.smooth_anchors.push(anchor_idx);
            self.smooth_anchors.sort_unstable();
            self.smooth_anchors.dedup();
        }
        let offset = [x - ax, y - ay];
        self.apply_handle_drag(anchor_idx, false, offset);
        self.rebuild_with_smooth_anchors(&anchors);
    }

    fn apply_handle_drag(&mut self, anchor_idx: usize, outgoing: bool, offset: [f64; 2]) {
        let len = (offset[0] * offset[0] + offset[1] * offset[1]).sqrt();
        match self.handle_mode(anchor_idx) {
            BezierHandleMode::Asymmetric | BezierHandleMode::Both => {
                if outgoing {
                    self.handle_out_offset.insert(anchor_idx, offset);
                } else {
                    self.handle_in_offset.insert(anchor_idx, offset);
                }
            }
            BezierHandleMode::LeftOnly => {
                if !outgoing {
                    self.handle_in_offset.insert(anchor_idx, offset);
                }
            }
            BezierHandleMode::RightOnly => {
                if outgoing {
                    self.handle_out_offset.insert(anchor_idx, offset);
                }
            }
            BezierHandleMode::Symmetric => {
                if outgoing {
                    self.handle_out_offset.insert(anchor_idx, offset);
                    let other_len = self
                        .handle_in_offset
                        .get(&anchor_idx)
                        .map(|o| (o[0] * o[0] + o[1] * o[1]).sqrt())
                        .unwrap_or(len);
                    if len > 1e-9 {
                        let scale = other_len / len;
                        self.handle_in_offset
                            .insert(anchor_idx, [-offset[0] * scale, -offset[1] * scale]);
                    }
                } else {
                    self.handle_in_offset.insert(anchor_idx, offset);
                    let other_len = self
                        .handle_out_offset
                        .get(&anchor_idx)
                        .map(|o| (o[0] * o[0] + o[1] * o[1]).sqrt())
                        .unwrap_or(len);
                    if len > 1e-9 {
                        let scale = other_len / len;
                        self.handle_out_offset
                            .insert(anchor_idx, [-offset[0] * scale, -offset[1] * scale]);
                    }
                }
            }
            BezierHandleMode::EqualLength => {
                if outgoing {
                    self.handle_out_offset.insert(anchor_idx, offset);
                    if len > 1e-9 {
                        self.handle_in_offset
                            .insert(anchor_idx, [-offset[0], -offset[1]]);
                    }
                } else {
                    self.handle_in_offset.insert(anchor_idx, offset);
                    if len > 1e-9 {
                        self.handle_out_offset
                            .insert(anchor_idx, [-offset[0], -offset[1]]);
                    }
                }
            }
        }
    }

    /// Endpoint-aware handles for UI: optional incoming/outgoing control points.
    pub fn bezier_handles_at(
        &self,
        anchor_idx: usize,
    ) -> Option<((f64, f64), Option<(f64, f64)>, Option<(f64, f64)>)> {
        if !self.is_anchor_smooth(anchor_idx) {
            return None;
        }
        let anchors = self.anchor_positions();
        let anchor = anchors.get(anchor_idx).copied()?;
        let closed = self.is_closed();
        let n = anchors.len();

        let mut incoming = self
            .handle_in_offset
            .get(&anchor_idx)
            .map(|o| (anchor.0 + o[0], anchor.1 + o[1]));
        let mut outgoing = self
            .handle_out_offset
            .get(&anchor_idx)
            .map(|o| (anchor.0 + o[0], anchor.1 + o[1]));

        // Walk baked verbs for segments that store control points explicitly.
        let mut pi = 0usize;
        let mut seg_end_anchor = 0usize;
        for &v in &self.verbs {
            match v {
                0 => pi += 1,
                1 => {
                    seg_end_anchor += 1;
                    pi += 1;
                }
                3 => {
                    if pi + 1 < self.points.len() {
                        let c1 = (self.points[pi][0], self.points[pi][1]);
                        let c2 = (self.points[pi + 1][0], self.points[pi + 1][1]);
                        let depart_anchor = seg_end_anchor;
                        let arrive_anchor = if pi + 2 < self.points.len() {
                            seg_end_anchor + 1
                        } else if closed {
                            0
                        } else {
                            seg_end_anchor + 1
                        };
                        if depart_anchor == anchor_idx && outgoing.is_none() {
                            outgoing = Some(c1);
                        }
                        if arrive_anchor == anchor_idx && incoming.is_none() {
                            incoming = Some(c2);
                        }
                        seg_end_anchor += 1;
                        pi += if pi + 2 < self.points.len() { 3 } else { 2 };
                    }
                }
                4 => {}
                _ => {}
            }
        }

        // Closed-path closing segment: last anchor → first (not always in verb walk).
        if closed && n > 2 {
            if anchor_idx == n - 1 && outgoing.is_none() {
                let (c1, _) = segment_controls(
                    &anchors,
                    n - 1,
                    0,
                    true,
                    self.is_anchor_smooth(n - 1),
                    self.is_anchor_smooth(0),
                    &self.handle_out_offset,
                    &self.handle_in_offset,
                    &self.handle_modes,
                );
                if self.is_anchor_smooth(n - 1) {
                    outgoing = Some(c1);
                }
            }
            if anchor_idx == 0 && incoming.is_none() {
                let (_, c2) = segment_controls(
                    &anchors,
                    n - 1,
                    0,
                    true,
                    self.is_anchor_smooth(n - 1),
                    self.is_anchor_smooth(0),
                    &self.handle_out_offset,
                    &self.handle_in_offset,
                    &self.handle_modes,
                );
                if self.is_anchor_smooth(0) || self.is_anchor_smooth(n - 1) {
                    incoming = Some(c2);
                }
            }
        }

        let mode = self.handle_mode(anchor_idx);
        if mode == BezierHandleMode::LeftOnly {
            outgoing = None;
        } else if mode == BezierHandleMode::RightOnly {
            incoming = None;
        }

        if incoming.is_none() && outgoing.is_none() {
            return None;
        }
        Some((anchor, incoming, outgoing))
    }

    /// Hit-test path segments; returns anchor indices at segment ends and nearest point on curve.
    pub fn hit_segment(
        &self,
        x: f64,
        y: f64,
        threshold: f64,
    ) -> Option<(usize, usize, f64, f64)> {
        let anchors = self.anchor_positions();
        if anchors.len() < 2 {
            return None;
        }
        let n = anchors.len();
        let seg_count = if self.is_closed() { n } else { n - 1 };
        let mut best: Option<(f64, usize, usize, f64, f64)> = None;
        for i in 0..seg_count {
            let j = (i + 1) % n;
            let (dist, px, py) = self.segment_nearest_point(i, j, x, y, &anchors);
            if dist <= threshold {
                let replace = best.as_ref().map_or(true, |(bd, ..)| dist < *bd);
                if replace {
                    best = Some((dist, i, j, px, py));
                }
            }
        }
        best.map(|(_, from, to, px, py)| (from, to, px, py))
    }

    fn segment_nearest_point(
        &self,
        from: usize,
        to: usize,
        x: f64,
        y: f64,
        anchors: &[(f64, f64)],
    ) -> (f64, f64, f64) {
        let closed = self.is_closed();
        let smooth_from = self.is_anchor_smooth(from);
        let smooth_to = self.is_anchor_smooth(to);
        let p0 = anchors[from];
        let p3 = anchors[to];
        if !smooth_from && !smooth_to {
            return Self::nearest_on_line_segment(x, y, p0, p3);
        }
        let (c1, c2) = segment_controls(
            anchors,
            from,
            to,
            closed,
            smooth_from,
            smooth_to,
            &self.handle_out_offset,
            &self.handle_in_offset,
            &self.handle_modes,
        );
        let samples = 24usize;
        let mut best_dist = f64::MAX;
        let mut best_pt = p0;
        for s in 0..=samples {
            let t = s as f64 / samples as f64;
            let pt = cubic_at(p0, c1, c2, p3, t);
            let d = (pt.0 - x).hypot(pt.1 - y);
            if d < best_dist {
                best_dist = d;
                best_pt = pt;
            }
        }
        (best_dist, best_pt.0, best_pt.1)
    }

    fn nearest_on_line_segment(
        x: f64,
        y: f64,
        p0: (f64, f64),
        p1: (f64, f64),
    ) -> (f64, f64, f64) {
        let dx = p1.0 - p0.0;
        let dy = p1.1 - p0.1;
        let len_sq = dx * dx + dy * dy;
        let (cx, cy) = if len_sq < 1e-12 {
            p0
        } else {
            let t = ((x - p0.0) * dx + (y - p0.1) * dy) / len_sq;
            let t = t.clamp(0.0, 1.0);
            (p0.0 + dx * t, p0.1 + dy * t)
        };
        ((x - cx).hypot(y - cy), cx, cy)
    }

    /// Insert a new anchor on the segment between `from` and `to`.
    pub fn insert_anchor_on_segment(&mut self, from: usize, to: usize, x: f64, y: f64) {
        let mut anchors = self.anchor_positions();
        let insert_idx = if to > from { to } else { anchors.len() };
        anchors.insert(insert_idx, (x, y));
        let smooth: Vec<usize> = self
            .smooth_anchors
            .iter()
            .map(|&i| if i >= insert_idx { i + 1 } else { i })
            .collect();
        let mut out = HashMap::new();
        let mut inn = HashMap::new();
        let mut modes = HashMap::new();
        for (k, v) in &self.handle_out_offset {
            let nk = if *k >= insert_idx { k + 1 } else { *k };
            out.insert(nk, *v);
        }
        for (k, v) in &self.handle_in_offset {
            let nk = if *k >= insert_idx { k + 1 } else { *k };
            inn.insert(nk, *v);
        }
        for (k, v) in &self.handle_modes {
            let nk = if *k >= insert_idx { k + 1 } else { *k };
            modes.insert(nk, *v);
        }
        self.smooth_anchors = smooth;
        self.handle_out_offset = out;
        self.handle_in_offset = inn;
        self.handle_modes = modes;
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn reverse(&mut self) {
        let anchors = self.anchor_positions();
        if anchors.len() < 2 {
            return;
        }
        let n = anchors.len();
        let mut rev: Vec<(f64, f64)> = anchors.into_iter().rev().collect();
        let mut smooth: Vec<usize> = self
            .smooth_anchors
            .iter()
            .map(|&i| n - 1 - i)
            .collect();
        smooth.sort_unstable();
        let mut out = HashMap::new();
        let mut inn = HashMap::new();
        let mut modes = HashMap::new();
        for (k, v) in &self.handle_out_offset {
            let nk = n - 1 - *k;
            inn.insert(nk, [-v[0], -v[1]]);
        }
        for (k, v) in &self.handle_in_offset {
            let nk = n - 1 - *k;
            out.insert(nk, [-v[0], -v[1]]);
        }
        for (k, v) in &self.handle_modes {
            modes.insert(n - 1 - *k, *v);
        }
        self.smooth_anchors = smooth;
        self.handle_out_offset = out;
        self.handle_in_offset = inn;
        self.handle_modes = modes;
        self.rebuild_with_smooth_anchors(&rev);
        let _ = &mut rev;
    }

    /// Mirror geometry across a vertical axis at `cx` (flip horizontal).
    pub fn mirror_horizontal(&mut self, cx: f64) {
        let anchors: Vec<(f64, f64)> = self
            .anchor_positions()
            .into_iter()
            .map(|(x, y)| (2.0 * cx - x, y))
            .collect();
        let mut out = HashMap::new();
        let mut inn = HashMap::new();
        for (k, v) in &self.handle_out_offset {
            out.insert(*k, [-v[0], v[1]]);
        }
        for (k, v) in &self.handle_in_offset {
            inn.insert(*k, [-v[0], v[1]]);
        }
        self.handle_out_offset = out;
        self.handle_in_offset = inn;
        self.rebuild_with_smooth_anchors(&anchors);
    }

    /// Mirror geometry across a horizontal axis at `cy` (flip vertical).
    pub fn mirror_vertical(&mut self, cy: f64) {
        let anchors: Vec<(f64, f64)> = self
            .anchor_positions()
            .into_iter()
            .map(|(x, y)| (x, 2.0 * cy - y))
            .collect();
        let mut out = HashMap::new();
        let mut inn = HashMap::new();
        for (k, v) in &self.handle_out_offset {
            out.insert(*k, [v[0], -v[1]]);
        }
        for (k, v) in &self.handle_in_offset {
            inn.insert(*k, [v[0], -v[1]]);
        }
        self.handle_out_offset = out;
        self.handle_in_offset = inn;
        self.rebuild_with_smooth_anchors(&anchors);
    }

    /// Remove anchors by index; returns false if fewer than the minimum would remain.
    pub fn remove_anchors(&mut self, indices: &[usize]) -> bool {
        let anchors = self.anchor_positions();
        let old_len = anchors.len();
        let min = if self.is_closed() { 3 } else { 2 };
        let remove: std::collections::HashSet<usize> = indices.iter().copied().collect();
        let kept: Vec<(usize, (f64, f64))> = anchors
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !remove.contains(i))
            .collect();
        if kept.len() < min || kept.len() == old_len {
            return false;
        }
        let new_anchors: Vec<(f64, f64)> = kept.iter().map(|(_, a)| *a).collect();
        let mut smooth = Vec::new();
        let mut out = HashMap::new();
        let mut inn = HashMap::new();
        let mut modes = HashMap::new();
        for (new_i, (old_i, _)) in kept.iter().enumerate() {
            if self.is_anchor_smooth(*old_i) {
                smooth.push(new_i);
            }
            if let Some(v) = self.handle_out_offset.get(old_i) {
                out.insert(new_i, *v);
            }
            if let Some(v) = self.handle_in_offset.get(old_i) {
                inn.insert(new_i, *v);
            }
            if let Some(v) = self.handle_modes.get(old_i) {
                modes.insert(new_i, *v);
            }
        }
        self.smooth_anchors = smooth;
        self.handle_out_offset = out;
        self.handle_in_offset = inn;
        self.handle_modes = modes;
        self.rebuild_with_smooth_anchors(&new_anchors);
        true
    }

    pub fn set_all_anchors_smooth(&mut self, smooth: bool) {
        let anchors = self.anchor_positions();
        if smooth {
            self.smooth_anchors = (0..anchors.len()).collect();
        } else {
            self.smooth_anchors.clear();
            self.handle_out_offset.clear();
            self.handle_in_offset.clear();
        }
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn simplify_collinear(&mut self, tolerance: f64) {
        let anchors = self.anchor_positions();
        if anchors.len() < 3 {
            return;
        }
        let mut kept = vec![0usize];
        for i in 1..anchors.len() - 1 {
            let a = anchors[kept[kept.len() - 1]];
            let b = anchors[i];
            let c = anchors[i + 1];
            let v1 = (b.0 - a.0, b.1 - a.1);
            let v2 = (c.0 - b.0, c.1 - b.1);
            let cross = (v1.0 * v2.1 - v1.1 * v2.0).abs();
            let len = v1.0.hypot(v1.1).max(v2.0.hypot(v2.1));
            if cross / len.max(1e-6) > tolerance {
                kept.push(i);
            }
        }
        kept.push(anchors.len() - 1);
        let new_anchors: Vec<_> = kept.iter().map(|&i| anchors[i]).collect();
        let mut smooth: Vec<usize> = self
            .smooth_anchors
            .iter()
            .filter_map(|&old| kept.iter().position(|&k| k == old))
            .collect();
        smooth.sort_unstable();
        self.smooth_anchors = smooth;
        self.handle_out_offset.clear();
        self.handle_in_offset.clear();
        self.rebuild_with_smooth_anchors(&new_anchors);
    }

    /// Control handles for smooth anchors: (anchor, incoming ctrl, outgoing ctrl).
    pub fn bezier_handles(&self) -> Vec<((f64, f64), (f64, f64), (f64, f64))> {
        let mut out = Vec::new();
        for &ai in &self.smooth_anchors {
            let Some((anchor, ctrl_in, ctrl_out)) = self.bezier_handles_at(ai) else {
                continue;
            };
            if let (Some(ci), Some(co)) = (ctrl_in, ctrl_out) {
                out.push((anchor, ci, co));
            }
        }
        out
    }

    pub fn set_anchor_position(&mut self, anchor_idx: usize, x: f64, y: f64) {
        let mut anchors = self.anchor_positions();
        if anchor_idx >= anchors.len() {
            return;
        }
        anchors[anchor_idx] = (x, y);
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn move_anchors_by(&mut self, indices: &[usize], dx: f64, dy: f64) {
        if dx.abs() < 1e-12 && dy.abs() < 1e-12 {
            return;
        }
        let mut anchors = self.anchor_positions();
        for &idx in indices {
            if let Some(a) = anchors.get_mut(idx) {
                a.0 += dx;
                a.1 += dy;
            }
        }
        self.rebuild_with_smooth_anchors(&anchors);
    }

    pub fn replace_anchors(&mut self, anchors: &[(f64, f64)]) {
        self.rebuild_with_smooth_anchors(anchors);
    }

    fn rebuild_with_smooth_anchors(&mut self, anchors: &[(f64, f64)]) {
        if anchors.is_empty() {
            return;
        }
        let closed = self.is_closed();
        let smooth: Vec<bool> = (0..anchors.len())
            .map(|i| self.is_anchor_smooth(i))
            .collect();
        let mut verbs = vec![0u8];
        let mut points = vec![[anchors[0].0, anchors[0].1]];
        let seg_count = if closed {
            anchors.len()
        } else if anchors.len() > 1 {
            anchors.len() - 1
        } else {
            0
        };
        for i in 0..seg_count {
            let j = (i + 1) % anchors.len();
            let p3 = anchors[j];
            let closing_seg = closed && j == 0;
            if smooth[i] || smooth[j] {
                let (c1, c2) = segment_controls(
                    anchors,
                    i,
                    j,
                    closed,
                    smooth[i],
                    smooth[j],
                    &self.handle_out_offset,
                    &self.handle_in_offset,
                    &self.handle_modes,
                );
                verbs.push(3);
                points.push([c1.0, c1.1]);
                points.push([c2.0, c2.1]);
                if !closing_seg {
                    points.push([p3.0, p3.1]);
                }
            } else if closing_seg {
                // Implicit line back to subpath start via ClosePath.
            } else {
                verbs.push(1);
                points.push([p3.0, p3.1]);
            }
        }
        if closed {
            verbs.push(4);
        }
        self.verbs = verbs;
        self.points = points;
    }

    pub fn to_bez(&self) -> BezPath {
        let mut path = BezPath::new();
        let mut pi = 0;
        for v in &self.verbs {
            match v {
                0 => {
                    if pi < self.points.len() {
                        let p = self.points[pi];
                        path.move_to((p[0], p[1]));
                        pi += 1;
                    }
                }
                1 => {
                    if pi < self.points.len() {
                        let p = self.points[pi];
                        path.line_to((p[0], p[1]));
                        pi += 1;
                    }
                }
                2 => {
                    if pi + 1 < self.points.len() {
                        let p1 = self.points[pi];
                        let p2 = self.points[pi + 1];
                        path.quad_to((p1[0], p1[1]), (p2[0], p2[1]));
                        pi += 2;
                    }
                }
                3 => {
                    if pi + 1 < self.points.len() {
                        let p1 = self.points[pi];
                        let p2 = self.points[pi + 1];
                        if pi + 2 < self.points.len() {
                            let p3 = self.points[pi + 2];
                            path.curve_to((p1[0], p1[1]), (p2[0], p2[1]), (p3[0], p3[1]));
                            pi += 3;
                        } else if let Some(kurbo::PathEl::MoveTo(start)) =
                            path.elements().first()
                        {
                            path.curve_to((p1[0], p1[1]), (p2[0], p2[1]), (start.x, start.y));
                            pi += 2;
                        }
                    }
                }
                4 => path.close_path(),
                _ => {}
            }
        }
        // Verb 4 already emits close_path(); avoid a second ClosePath (breaks Lyon tessellation).
        if self.closed && !self.verbs.contains(&4) {
            path.close_path();
        }
        path
    }

    pub fn is_closed(&self) -> bool {
        self.closed || self.verbs.contains(&4)
    }

    pub fn set_closed(&mut self, closed: bool) {
        self.closed = closed;
        if closed && !self.verbs.contains(&4) && self.points.len() >= 2 {
            self.verbs.push(4);
        }
        if !closed {
            self.verbs.retain(|v| *v != 4);
        }
    }

    /// Public arc-length sampling entry points (used by PathMagic + effect code).
    pub fn approximate_length(&self, tolerance: f64) -> f64 {
        self._approx_len(tolerance)
    }
    pub fn sample_point_and_angle(&self, dist: f64, tolerance: f64) -> (f64, f64, f64) {
        self._sample(dist, tolerance)
    }

    // --- arc-length sampling support for PathMagic (moved/adapted from path_effects for encapsulation)
    fn _flatten(&self, tolerance: f64) -> Vec<(f64, f64)> {
        let bez = self.to_bez();
        let mut pts = Vec::new();
        let els: Vec<kurbo::PathEl> = bez.elements().to_vec();
        let mut i = 0usize;
        while i < els.len() {
            match els[i] {
                kurbo::PathEl::MoveTo(p) => { pts.push((p.x, p.y)); i += 1; }
                kurbo::PathEl::LineTo(p) => { pts.push((p.x, p.y)); i += 1; }
                kurbo::PathEl::QuadTo(_, p2) => {
                    let p0 = pts.last().copied().unwrap_or((p2.x, p2.y));
                    let p1 = match els.get(i) {
                        Some(kurbo::PathEl::QuadTo(p1, _)) => (p1.x, p1.y),
                        _ => p0,
                    };
                    let steps = ((p0.0 - p2.x).hypot(p0.1 - p2.y) / tolerance).ceil() as usize;
                    let steps = steps.clamp(2, 32);
                    for s in 1..=steps {
                        let t = s as f64 / steps as f64;
                        let u = 1.0 - t;
                        let x = u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.x;
                        let y = u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.y;
                        pts.push((x, y));
                    }
                    i += 1;
                }
                kurbo::PathEl::CurveTo(_, _, p3) => {
                    let p0 = pts.last().copied().unwrap_or((p3.x, p3.y));
                    let (p1, p2) = match els.get(i) {
                        Some(kurbo::PathEl::CurveTo(p1, p2, _)) => ((p1.x, p1.y), (p2.x, p2.y)),
                        _ => (p0, (p3.x, p3.y)),
                    };
                    let steps = ((p0.0 - p3.x).hypot(p0.1 - p3.y) / tolerance).ceil() as usize;
                    let steps = steps.clamp(2, 48);
                    for s in 1..=steps {
                        let t = s as f64 / steps as f64;
                        let u = 1.0 - t;
                        let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.x;
                        let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.y;
                        pts.push((x, y));
                    }
                    i += 1;
                }
                kurbo::PathEl::ClosePath => {
                    if let Some(start) = pts.first().copied() {
                        if pts.last().map_or(true, |p| (p.0 - start.0).hypot(p.1 - start.1) > 1e-4) {
                            pts.push(start);
                        }
                    }
                    i += 1;
                }
            }
        }
        pts
    }

    fn _approx_len(&self, tolerance: f64) -> f64 {
        let pts = self._flatten(tolerance);
        if pts.len() < 2 { return 0.0; }
        let mut len = 0.0;
        for w in pts.windows(2) {
            len += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        }
        len.max(1e-6)
    }

    fn _sample(&self, dist: f64, tolerance: f64) -> (f64, f64, f64) {
        let mut pts = self._flatten(tolerance);
        if pts.len() < 2 {
            return (0.0, 0.0, 0.0);
        }
        let closed = self.is_closed();
        if closed && pts.len() >= 2 {
            let first = pts[0];
            let last = pts[pts.len()-1];
            if (first.0 - last.0).hypot(first.1 - last.1) > 1e-4 { pts.push(first); }
        }
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            let seg = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            cum.push(cum.last().copied().unwrap_or(0.0) + seg);
        }
        let total = (*cum.last().unwrap_or(&0.0)).max(1e-6);
        let mut d = dist;
        if closed && total > 1e-6 {
            d = d.rem_euclid(total);
        } else {
            d = d.clamp(0.0, total);
        }
        if d <= 0.0 {
            let (x0, y0) = pts[0];
            let (x1, y1) = pts.get(1).copied().unwrap_or((x0+1.0, y0));
            return (x0, y0, (y1-y0).atan2(x1-x0));
        }
        for i in 1..cum.len() {
            if cum[i] >= d {
                let d0 = cum[i-1];
                let d1 = cum[i];
                let t = if (d1-d0).abs() < 1e-9 { 0.0 } else { (d - d0) / (d1 - d0) };
                let (x0, y0) = pts[i-1];
                let (x1, y1) = pts[i];
                let x = x0 + (x1-x0)*t;
                let y = y0 + (y1-y0)*t;
                let ang = (y1 - y0).atan2(x1 - x0);
                return (x, y, ang);
            }
        }
        let (x, y) = *pts.last().unwrap();
        (x, y, 0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub style: NodeStyle,
    pub transform: Transform2D,
    /// Shared path-effect ids linked to this node (stored on both path and object).
    #[serde(default)]
    pub path_effect_links: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Transform2D {
    pub translation: [f64; 2],
    pub scale: [f64; 2],
    pub rotation_rad: f64,
}

impl Transform2D {
    pub fn apply_point(&self, x: f64, y: f64) -> (f64, f64) {
        let (tx, ty) = (self.translation[0], self.translation[1]);
        let (sx, sy) = (self.scale[0], self.scale[1]);
        let c = self.rotation_rad.cos();
        let s = self.rotation_rad.sin();
        let x = x * sx;
        let y = y * sy;
        let xr = x * c - y * s;
        let yr = x * s + y * c;
        (xr + tx, yr + ty)
    }
}

/// Marker trait for entities that can participate in object-on-path effects.
pub trait ObjectOnPath {}

/// FaceRenderable: any "facial" / profile object (rect, ellipse, polygon, closed path, arc...).
/// Implemented by Node (light delegation approach). Used for sources in ObjectOnPath / Loft.
pub trait FaceRenderable: ObjectOnPath {
    fn bounds(&self) -> kurbo::Rect;
    fn bez_path(&self) -> kurbo::BezPath;
    fn fill(&self) -> &Fill;
    fn stroke(&self) -> &Stroke;
    fn opacity(&self) -> f32;
    fn set_opacity(&mut self, opacity: f32);
    fn translate(&mut self, dx: f64, dy: f64);
    fn scale_about_center(&mut self, scale: f64);
    fn rotate_about_center(&mut self, angle_rad: f64);
    /// For dense placement in effects without losing full Node data (we downcast in practice).
    fn clone_renderable(&self) -> Box<dyn FaceRenderable>;
    /// Support for recovering concrete when needed.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// PathMagic: ONLY for real path spines (NodeKind::Path).
/// Super-trait relation via ObjectOnPath. Provides arc-length sampling for on-path placement.
pub trait PathMagic: ObjectOnPath {
    fn to_bez(&self) -> kurbo::BezPath;
    fn is_closed(&self) -> bool;
    /// Approximate total length (uses internal flattening with tolerance).
    fn total_length(&self, tolerance: f64) -> f64;
    /// Sample (x, y, angle_rad) at arc distance along the path.
    fn sample_at(&self, dist: f64, tolerance: f64) -> (f64, f64, f64);
    fn clone_path(&self) -> Box<dyn PathMagic>;
}

/// Tiling is a *separate* trait of PathMagic (not part of ObjectOnPath).
/// First determine the facial object's size (via FaceRenderable), then use that
/// bounding size to determine the repeat gap for tiling/cloning.
pub trait Tiling: PathMagic {
    /// 2D gaps (gap_x, gap_y) from the "object"'s bounds (width for x, height for y).
    fn gaps_for_object(&self, object: &dyn FaceRenderable) -> (f64, f64);
}

/// CircularClone is a *separate* trait of PathMagic (not part of ObjectOnPath).
/// Clones around a specific editable origin point (6 sides default).
/// The implementing "path" can serve as the editable center.
pub trait CircularClone: PathMagic {
    fn origin(&self) -> (f64, f64);
    fn set_origin(&mut self, x: f64, y: f64);
    fn radius(&self) -> f64;
    fn set_radius(&mut self, r: f64);
    fn sides(&self) -> usize;
    fn set_sides(&mut self, n: usize);
    /// Generate placement params for N circular copies.
    /// Returns (x, y, angle) for each.
    fn circular_placements(&self) -> Vec<(f64, f64, f64)>;
}

impl ObjectOnPath for Node {}
impl ObjectOnPath for PathData {}

impl FaceRenderable for Node {
    fn bounds(&self) -> kurbo::Rect { self.bounds() }
    fn bez_path(&self) -> kurbo::BezPath { self.bez_path() }
    fn fill(&self) -> &Fill { &self.style.fill }
    fn stroke(&self) -> &Stroke { &self.style.stroke }
    fn opacity(&self) -> f32 { self.style.opacity }
    fn set_opacity(&mut self, opacity: f32) { self.style.opacity = opacity; }
    fn translate(&mut self, dx: f64, dy: f64) { Node::translate(self, dx, dy); }
    fn scale_about_center(&mut self, scale: f64) { Node::scale_about_center(self, scale); }
    fn rotate_about_center(&mut self, angle_rad: f64) { Node::rotate_about_center(self, angle_rad); }
    fn clone_renderable(&self) -> Box<dyn FaceRenderable> { Box::new(self.clone()) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl PathMagic for Node {
    fn to_bez(&self) -> kurbo::BezPath {
        if let NodeKind::Path { path } = &self.kind {
            path.to_bez()
        } else {
            kurbo::BezPath::new()
        }
    }
    fn is_closed(&self) -> bool {
        if let NodeKind::Path { path } = &self.kind {
            path.is_closed()
        } else {
            false
        }
    }
    fn total_length(&self, tolerance: f64) -> f64 {
        if let NodeKind::Path { path } = &self.kind {
            path.total_length(tolerance)
        } else {
            0.0
        }
    }
    fn sample_at(&self, dist: f64, tolerance: f64) -> (f64, f64, f64) {
        if let NodeKind::Path { path } = &self.kind {
            path.sample_at(dist, tolerance)
        } else {
            (0.0, 0.0, 0.0)
        }
    }
    fn clone_path(&self) -> Box<dyn PathMagic> { Box::new(self.clone()) }
}

impl PathMagic for PathData {
    fn to_bez(&self) -> kurbo::BezPath { self.to_bez() }
    fn is_closed(&self) -> bool { self.is_closed() }
    fn total_length(&self, tolerance: f64) -> f64 { self._approx_len(tolerance) }
    fn sample_at(&self, dist: f64, tolerance: f64) -> (f64, f64, f64) { self._sample(dist, tolerance) }
    fn clone_path(&self) -> Box<dyn PathMagic> { Box::new(self.clone()) }
}

impl Tiling for Node {
    fn gaps_for_object(&self, object: &dyn FaceRenderable) -> (f64, f64) {
        let b = object.bounds();
        let w = (b.x1 - b.x0).abs().max(1.0);
        let h = (b.y1 - b.y0).abs().max(1.0);
        (w, h)
    }
}

impl Tiling for PathData {
    fn gaps_for_object(&self, object: &dyn FaceRenderable) -> (f64, f64) {
        let b = object.bounds();
        let w = (b.x1 - b.x0).abs().max(1.0);
        let h = (b.y1 - b.y0).abs().max(1.0);
        (w, h)
    }
}

impl CircularClone for Node {
    fn origin(&self) -> (f64, f64) {
        let b = self.bounds();
        ((b.x0 + b.x1) * 0.5, (b.y0 + b.y1) * 0.5)
    }
    fn set_origin(&mut self, x: f64, y: f64) {
        let b = self.bounds();
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        let dx = x - cx;
        let dy = y - cy;
        self.translate(dx, dy);
    }
    fn radius(&self) -> f64 {
        let b = self.bounds();
        let w = (b.x1 - b.x0).abs();
        let h = (b.y1 - b.y0).abs();
        (w.max(h) * 1.5).max(10.0)
    }
    fn set_radius(&mut self, _r: f64) {}
    fn sides(&self) -> usize { 6 }
    fn set_sides(&mut self, _n: usize) {}
    fn circular_placements(&self) -> Vec<(f64, f64, f64)> {
        let (cx, cy) = self.origin();
        let r = self.radius();
        let n = self.sides().max(3);
        (0..n).map(|i| {
            let ang = (i as f64 / n as f64) * std::f64::consts::TAU;
            let x = cx + r * ang.cos();
            let y = cy + r * ang.sin();
            (x, y, ang)
        }).collect()
    }
}

impl CircularClone for PathData {
    fn origin(&self) -> (f64, f64) {
        let b = self.to_bez().bounding_box();
        ((b.x0 + b.x1) * 0.5, (b.y0 + b.y1) * 0.5)
    }
    fn set_origin(&mut self, x: f64, y: f64) {
        let b = self.to_bez().bounding_box();
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        let dx = x - cx;
        let dy = y - cy;
        for p in &mut self.points {
            p[0] += dx;
            p[1] += dy;
        }
    }
    fn radius(&self) -> f64 { 48.0 }
    fn set_radius(&mut self, _r: f64) {}
    fn sides(&self) -> usize { 6 }
    fn set_sides(&mut self, _n: usize) {}
    fn circular_placements(&self) -> Vec<(f64, f64, f64)> {
        let (cx, cy) = self.origin();
        let r = self.radius();
        let n = self.sides().max(3);
        (0..n).map(|i| {
            let ang = (i as f64 / n as f64) * std::f64::consts::TAU;
            let x = cx + r * ang.cos();
            let y = cy + r * ang.sin();
            (x, y, ang)
        }).collect()
    }
}

impl Node {
    pub fn get_rotation(&self) -> f64 {
        self.transform.rotation_rad
    }

    pub fn set_rotation(&mut self, rad: f64) {
        let delta = rad - self.transform.rotation_rad;
        if delta.abs() > 1e-9 {
            self.rotate_about_center(delta);
        }
        self.transform.rotation_rad = rad;
    }

    pub fn get_opacity(&self) -> f32 {
        self.style.opacity
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.style.opacity = opacity;
    }

    pub fn get_color(&self) -> [f32; 4] {
        match &self.style.fill {
            Fill::Solid(paint) => paint.rgba,
            _ => [1.0, 1.0, 1.0, 1.0],
        }
    }

    pub fn set_color(&mut self, rgba: [f32; 4]) {
        self.style.fill = Fill::Solid(Paint { rgba });
    }

    pub fn get_pos(&self) -> (f64, f64) {
        match &self.kind {
            NodeKind::Rect { x, y, .. } => (*x, *y),
            NodeKind::Ellipse { cx, cy, .. } => (*cx, *cy),
            NodeKind::Polygon { cx, cy, .. } => (*cx, *cy),
            NodeKind::Path { path } => {
                if path.points.is_empty() {
                    (0.0, 0.0)
                } else {
                    (path.points[0][0], path.points[0][1])
                }
            }
            NodeKind::Text { x, y, .. } => (*x, *y),
            NodeKind::Group { .. } => (0.0, 0.0),
            NodeKind::Image { x, y, .. } => (*x, *y),
            NodeKind::Arc { cx, cy, .. } => (*cx, *cy),
            NodeKind::BrushStroke { points } => {
                if points.is_empty() {
                    (0.0, 0.0)
                } else {
                    (points[0].0[0], points[0].0[1])
                }
            }
        }
    }

    pub fn new(kind: NodeKind, name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            style: NodeStyle::default(),
            transform: Transform2D::default(),
            path_effect_links: Vec::new(),
        }
    }

    pub fn rect(x: f64, y: f64, w: f64, h: f64, fill: Fill) -> Self {
        let mut n = Self::new(NodeKind::Rect { x, y, w, h, rx: 0.0 }, "Rectangle");
        n.style.fill = fill;
        n
    }

    pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64, fill: Fill) -> Self {
        let mut n = Self::new(
            NodeKind::Ellipse { cx, cy, rx, ry },
            "Ellipse",
        );
        n.style.fill = fill;
        n
    }

    pub fn polygon(cx: f64, cy: f64, r: f64, sides: u32, fill: Fill) -> Self {
        let mut n = Self::new(
            NodeKind::Polygon {
                cx,
                cy,
                r,
                sides: sides.max(3),
                rotation_rad: 0.0,
            },
            format!("Polygon ({})", sides.max(3)),
        );
        n.style.fill = fill;
        n
    }

    pub fn path_from_bez(path: BezPath, name: impl Into<String>) -> Self {
        Self::new(NodeKind::Path { path: PathData::from_bez(&path) }, name)
    }

    pub fn group(children: Vec<NodeId>, name: impl Into<String>) -> Self {
        Self::new(NodeKind::Group { children }, name)
    }

    pub fn bounds_with_store(&self, store: &super::NodeStore) -> Rect {
        match &self.kind {
            NodeKind::Group { children } => {
                let mut acc: Option<Rect> = None;
                for id in children {
                    let Some(child) = store.get(*id) else {
                        continue;
                    };
                    let b = child.bounds_with_store(store);
                    if b.width() < 1e-9 && b.height() < 1e-9 {
                        continue;
                    }
                    acc = Some(match acc {
                        Some(r) => r.union(b),
                        None => b,
                    });
                }
                acc.unwrap_or(Rect::ZERO)
            }
            _ => self.bounds(),
        }
    }

    pub fn is_circle(&self) -> bool {
        if let NodeKind::Ellipse { rx, ry, .. } = &self.kind {
            self.name == "Circle" || (*rx - *ry).abs() < 0.01
        } else {
            false
        }
    }

    /// Constraint-oriented parameters for the geometry action tab.
    pub fn geometry_profile(&self) -> GeometryProfile {
        match &self.kind {
            NodeKind::Rect { x, y, w, h, rx } => GeometryProfile::Rect {
                origin_x: *x,
                origin_y: *y,
                width: *w,
                height: *h,
                corner_radius: *rx,
            },
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                if self.is_circle() || (*rx - *ry).abs() < 0.01 {
                    GeometryProfile::Circle {
                        origin_x: *cx,
                        origin_y: *cy,
                        radius: *rx,
                    }
                } else {
                    GeometryProfile::Ellipse {
                        origin_x: *cx,
                        origin_y: *cy,
                        radius_x: *rx,
                        radius_y: *ry,
                    }
                }
            }
            NodeKind::Path { path } => {
                if path.points.len() == 2 && path.verbs == [0, 1] && !path.is_closed() {
                    let (x0, y0) = (path.points[0][0], path.points[0][1]);
                    let (x1, y1) = (path.points[1][0], path.points[1][1]);
                    let dx = x1 - x0;
                    let dy = y1 - y0;
                    GeometryProfile::Line {
                        origin_x: x0,
                        origin_y: y0,
                        end_x: x1,
                        end_y: y1,
                        length: dx.hypot(dy),
                        angle_deg: dy.atan2(dx).to_degrees(),
                    }
                } else if path.is_closed() {
                    GeometryProfile::ClosedPath {
                        vertices: path_anchor_point_indices(path).len(),
                        cyclic: true,
                    }
                } else {
                    GeometryProfile::OpenPath {
                        vertices: path_anchor_point_indices(path).len(),
                        cyclic: false,
                    }
                }
            }
            NodeKind::Polygon {
                cx,
                cy,
                r,
                sides,
                rotation_rad,
            } => GeometryProfile::Polygon {
                origin_x: *cx,
                origin_y: *cy,
                radius: *r,
                sides: *sides,
                rotation_deg: rotation_rad.to_degrees(),
            },
            NodeKind::Text { x, y, style } => {
                let bounds = text_bounds(*x, *y, style);
                GeometryProfile::Text {
                    origin_x: *x,
                    origin_y: *y,
                    width: bounds.width(),
                    height: bounds.height(),
                    content: style.content.clone(),
                    font_size: style.font_size,
                    font_family: style.font_family.clone(),
                    bold: style.bold,
                    italic: style.italic,
                }
            }
            NodeKind::Group { .. } => GeometryProfile::Unsupported,
            NodeKind::Image { .. } => GeometryProfile::Unsupported,
            NodeKind::Arc { cx, cy, radius, start_angle_rad, sweep_angle_rad, join } => {
                GeometryProfile::Arc {
                    origin_x: *cx,
                    origin_y: *cy,
                    radius: *radius,
                    start_angle_deg: start_angle_rad.to_degrees(),
                    sweep_angle_deg: sweep_angle_rad.to_degrees(),
                    join: *join,
                }
            }
            NodeKind::BrushStroke { .. } => GeometryProfile::Unsupported,
        }
    }

    pub fn text(x: f64, y: f64, style: TextStyle) -> Self {
        Self::new(NodeKind::Text { x, y, style }, "Text")
    }

    pub fn image(x: f64, y: f64, width: f64, height: f64, bytes: Vec<u8>) -> Self {
        Self::new(
            NodeKind::Image { x, y, width, height, bytes },
            "Image",
        )
    }

    pub fn arc(
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle_rad: f64,
        sweep_angle_rad: f64,
        join: ArcJoin,
        fill: Fill,
    ) -> Self {
        let mut n = Self::new(
            NodeKind::Arc {
                cx,
                cy,
                radius,
                start_angle_rad,
                sweep_angle_rad,
                join,
            },
            "Arc",
        );
        n.style.fill = fill;
        n
    }

    pub fn line(x0: f64, y0: f64, x1: f64, y1: f64, stroke: Stroke) -> Self {
        let path = PathData {
            verbs: vec![0, 1],
            points: vec![[x0, y0], [x1, y1]],
            closed: false,
            smooth_anchors: Vec::new(),
            handle_out_offset: HashMap::new(),
            handle_in_offset: HashMap::new(),
            handle_modes: HashMap::new(),
        };
        let mut n = Self::new(NodeKind::Path { path }, "Line");
        n.style.fill = Fill::none();
        n.style.stroke = stroke;
        n
    }

    pub fn bounds(&self) -> Rect {
        match &self.kind {
            NodeKind::Rect { x, y, w, h, .. } => Rect::new(*x, *y, *x + *w, *y + *h),
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                Rect::new(cx - rx, cy - ry, cx + rx, cy + ry)
            }
            NodeKind::Polygon { cx, cy, r, sides, rotation_rad } => {
                let verts = regular_polygon_vertices(*cx, *cy, *r, *sides, *rotation_rad);
                if verts.is_empty() {
                    Rect::new(cx - r, cy - r, cx + r, cy + r)
                } else {
                    let mut min_x = f64::MAX;
                    let mut min_y = f64::MAX;
                    let mut max_x = f64::MIN;
                    let mut max_y = f64::MIN;
                    for &(px, py) in &verts {
                        min_x = min_x.min(px);
                        min_y = min_y.min(py);
                        max_x = max_x.max(px);
                        max_y = max_y.max(py);
                    }
                    Rect::new(min_x, min_y, max_x, max_y)
                }
            }
            NodeKind::Path { path } => path.to_bez().bounding_box(),
            NodeKind::Text { x, y, style } => text_bounds(*x, *y, style),
            NodeKind::Group { .. } => Rect::ZERO,
            NodeKind::Image { x, y, width, height, .. } => Rect::new(*x, *y, *x + *width, *y + *height),
            NodeKind::Arc { cx, cy, radius, .. } => Rect::new(cx - radius, cy - radius, cx + radius, cy + radius),
            NodeKind::BrushStroke { points } => {
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                let mut max_y = f64::MIN;
                for (pos, width) in points {
                    let r = (*width as f64) / 2.0;
                    min_x = min_x.min(pos[0] - r);
                    min_y = min_y.min(pos[1] - r);
                    max_x = max_x.max(pos[0] + r);
                    max_y = max_y.max(pos[1] + r);
                }
                if min_x <= max_x {
                    Rect::new(min_x, min_y, max_x, max_y)
                } else {
                    Rect::ZERO
                }
            }
        }
    }

    pub fn bez_path(&self) -> BezPath {
        match &self.kind {
            NodeKind::Rect { x, y, w, h, rx } => {
                let r = Rect::new(*x, *y, *x + *w, *y + *h);
                if *rx > 0.0 {
                    r.to_rounded_rect(*rx).to_path(0.1)
                } else {
                    r.to_path(0.1)
                }
            }
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                kurbo::Ellipse::new((*cx, *cy), (*rx, *ry), 0.0).to_path(0.1)
            }
            NodeKind::Polygon {
                cx,
                cy,
                r,
                sides,
                rotation_rad,
            } => {
                let mut path = BezPath::new();
                let verts = regular_polygon_vertices(*cx, *cy, *r, *sides, *rotation_rad);
                if let Some((x0, y0)) = verts.first() {
                    path.move_to((*x0, *y0));
                    for (x, y) in verts.iter().skip(1) {
                        path.line_to((*x, *y));
                    }
                    path.close_path();
                }
                path
            }
            NodeKind::Path { path } => path.to_bez(),
            NodeKind::Text { .. } => BezPath::new(),
            NodeKind::Group { .. } => BezPath::new(),
            NodeKind::Image { .. } => BezPath::new(),
            NodeKind::Arc { cx, cy, radius, start_angle_rad, sweep_angle_rad, join } => {
                build_arc_bez(*cx, *cy, *radius, *start_angle_rad, *sweep_angle_rad, *join)
            }
            NodeKind::BrushStroke { points } => {
                let mut path = BezPath::new();
                if let Some(&(first_pos, _)) = points.first() {
                    path.move_to(kurbo::Point::new(first_pos[0], first_pos[1]));
                    for &(pos, _) in points.iter().skip(1) {
                        path.line_to(kurbo::Point::new(pos[0], pos[1]));
                    }
                }
                path
            }
        }
    }

    pub fn hit_test_with_store(
        &self,
        store: &super::NodeStore,
        doc_x: f64,
        doc_y: f64,
        stroke_slop: f64,
    ) -> bool {
        if let NodeKind::Group { children } = &self.kind {
            return children.iter().any(|id| {
                store
                    .get(*id)
                    .is_some_and(|c| c.hit_test_with_store(store, doc_x, doc_y, stroke_slop))
            });
        }
        self.hit_test(doc_x, doc_y, stroke_slop)
    }

    pub fn hit_test(&self, doc_x: f64, doc_y: f64, stroke_slop: f64) -> bool {
        use kurbo::Shape;
        let pt = kurbo::Point::new(doc_x, doc_y);
        if let NodeKind::BrushStroke { points } = &self.kind {
            let mut prev_pt: Option<([f64; 2], f64)> = None;
            let slop = stroke_slop.max(2.0);
            for &(pos, width) in points {
                let r = (width as f64 / 2.0) + slop;
                let dx = doc_x - pos[0];
                let dy = doc_y - pos[1];
                if dx * dx + dy * dy <= r * r {
                    return true;
                }
                if let Some((prev_pos, prev_r)) = prev_pt {
                    let segment_dx = pos[0] - prev_pos[0];
                    let segment_dy = pos[1] - prev_pos[1];
                    let len_sq = segment_dx * segment_dx + segment_dy * segment_dy;
                    if len_sq > 1e-8 {
                        let t = ((doc_x - prev_pos[0]) * segment_dx + (doc_y - prev_pos[1]) * segment_dy) / len_sq;
                        let t = t.clamp(0.0, 1.0);
                        let proj_x = prev_pos[0] + t * segment_dx;
                        let proj_y = prev_pos[1] + t * segment_dy;
                        let dist_sq = (doc_x - proj_x).powi(2) + (doc_y - proj_y).powi(2);
                        let interpolated_r = prev_r + t * (r - prev_r);
                        if dist_sq <= interpolated_r * interpolated_r {
                            return true;
                        }
                    }
                }
                prev_pt = Some((pos, r));
            }
            return false;
        }
        if let NodeKind::Text { x, y, style } = &self.kind {
            let tol = stroke_slop.max(2.0);
            return text_bounds(*x, *y, style).inflate(tol, tol).contains(pt);
        }
        if let NodeKind::Image { x, y, width, height, .. } = &self.kind {
            let tol = stroke_slop.max(1.0);
            return Rect::new(*x, *y, *x + *width, *y + *height).inflate(tol, tol).contains(pt);
        }
        let path = self.bez_path();
        if path.contains(pt) {
            return true;
        }
        let tol = stroke_slop.max(self.style.stroke.width as f64);
        path.bounding_box().inflate(tol, tol).contains(pt)
    }

    pub fn rotate_about_center(&mut self, angle_rad: f64) {
        match &mut self.kind {
            NodeKind::Polygon { rotation_rad, .. } => {
                *rotation_rad += angle_rad;
                return;
            }
            NodeKind::Arc {
                start_angle_rad,
                ..
            } => {
                *start_angle_rad += angle_rad;
                return;
            }
            _ => {}
        }
        let b = self.bounds();
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        let c = angle_rad.cos();
        let s = angle_rad.sin();
        let map = |x: f64, y: f64| {
            let dx = x - cx;
            let dy = y - cy;
            (cx + dx * c - dy * s, cy + dx * s + dy * c)
        };
        match &mut self.kind {
            NodeKind::Rect { x, y, w, h, .. } => {
                let corners = [
                    (*x, *y),
                    (*x + *w, *y),
                    (*x + *w, *y + *h),
                    (*x, *y + *h),
                ];
                let mapped: Vec<(f64, f64)> = corners.iter().map(|(px, py)| map(*px, *py)).collect();
                let path = PathData::from_anchor_data(
                    &mapped,
                    &[],
                    std::collections::HashMap::new(),
                    std::collections::HashMap::new(),
                    true,
                );
                self.kind = NodeKind::Path { path };
            }
            NodeKind::Ellipse { cx: ecx, cy: ecy, rx, ry } => {
                if ( *rx - *ry ).abs() < 0.01 {
                    // preserve circle size on rotation
                    let (nx, ny) = map(*ecx, *ecy);
                    *ecx = nx;
                    *ecy = ny;
                    return;
                }
                let (nx, ny) = map(*ecx, *ecy);
                *ecx = nx;
                *ecy = ny;
                let (rxp, _) = map(*ecx + *rx, *ecy);
                let (_, ryp) = map(*ecx, *ecy + *ry);
                *rx = (rxp - *ecx).abs().max(1e-3);
                *ry = (ryp - *ecy).abs().max(1e-3);
            }
            NodeKind::Path { path } => {
                let anchors = path.anchor_positions();
                let new_anchors: Vec<_> = anchors.iter().map(|&(px, py)| map(px, py)).collect();
                path.replace_anchors(&new_anchors);
            }
            NodeKind::Text { x, y, .. } => {
                let (nx, ny) = map(*x, *y);
                *x = nx;
                *y = ny;
            }
            NodeKind::Image { x, y, width, height, .. } => {
                let corners = [
                    (*x, *y),
                    (*x + *width, *y),
                    (*x + *width, *y + *height),
                    (*x, *y + *height),
                ];
                let mapped: Vec<_> = corners.iter().map(|(px, py)| map(*px, *py)).collect();
                let xs: Vec<_> = mapped.iter().map(|p| p.0).collect();
                let ys: Vec<_> = mapped.iter().map(|p| p.1).collect();
                *x = xs.iter().copied().fold(f64::INFINITY, f64::min);
                *y = ys.iter().copied().fold(f64::INFINITY, f64::min);
                *width = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - *x;
                *height = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - *y;
            }
            NodeKind::Arc { cx: acx, cy: acy, .. } => {
                let (nx, ny) = map(*acx, *acy);
                *acx = nx;
                *acy = ny;
            }
            NodeKind::BrushStroke { points } => {
                for (pos, _) in points {
                    let (nx, ny) = map(pos[0], pos[1]);
                    pos[0] = nx;
                    pos[1] = ny;
                }
            }
            NodeKind::Polygon { .. } | NodeKind::Group { .. } => {}
        }
    }

    pub fn scale_about_center(&mut self, scale: f64) {
        match &mut self.kind {
            NodeKind::Polygon { r, .. } => {
                *r *= scale;
                return;
            }
            NodeKind::Ellipse { rx, ry, .. } => {
                *rx *= scale;
                *ry *= scale;
                return;
            }
            NodeKind::Arc { radius, .. } => {
                *radius *= scale;
                return;
            }
            _ => {}
        }
        let b = self.bounds();
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        let map = |x: f64, y: f64| (cx + (x - cx) * scale, cy + (y - cy) * scale);
        match &mut self.kind {
            NodeKind::Rect { x, y, w, h, .. } => {
                *w *= scale;
                *h *= scale;
                *x = cx - *w * 0.5;
                *y = cy - *h * 0.5;
            }
            NodeKind::Path { path } => {
                let anchors = path.anchor_positions();
                let new_anchors: Vec<_> = anchors.iter().map(|&(px, py)| map(px, py)).collect();
                path.replace_anchors(&new_anchors);
            }
            NodeKind::Text { x, y, style, .. } => {
                style.font_size *= scale as f32;
                let (nx, ny) = map(*x, *y);
                *x = nx;
                *y = ny;
            }
            NodeKind::Image { x, y, width, height, .. } => {
                *width *= scale;
                *height *= scale;
                *x = cx - *width * 0.5;
                *y = cy - *height * 0.5;
            }
            NodeKind::Polygon { .. }
            | NodeKind::Ellipse { .. }
            | NodeKind::Arc { .. }
            | NodeKind::Group { .. } => {}
            NodeKind::BrushStroke { points } => {
                for pt in points.iter_mut() {
                    pt.0[0] = cx + (pt.0[0] - cx) * scale;
                    pt.0[1] = cy + (pt.0[1] - cy) * scale;
                    pt.1 *= scale as f32;
                }
            }
        }
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        match &mut self.kind {
            NodeKind::Rect { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            NodeKind::Ellipse { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            NodeKind::Polygon { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            NodeKind::Path { path } => {
                for p in &mut path.points {
                    p[0] += dx;
                    p[1] += dy;
                }
            }
            NodeKind::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            NodeKind::Group { .. } => {}
            NodeKind::Image { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            NodeKind::Arc { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            NodeKind::BrushStroke { points } => {
                for pt in points.iter_mut() {
                    pt.0[0] += dx;
                    pt.0[1] += dy;
                }
            }
        }
    }

    pub fn translate_children(&mut self, store: &mut super::NodeStore, dx: f64, dy: f64) {
        let NodeKind::Group { children } = &self.kind else {
            return;
        };
        for id in children.clone() {
            if let Some(child) = store.get_mut(id) {
                child.translate(dx, dy);
            }
        }
    }

    pub fn set_bounds(&mut self, bounds: Rect) {
        let w = (bounds.x1 - bounds.x0).max(1.0);
        let h = (bounds.y1 - bounds.y0).max(1.0);
        match &mut self.kind {
            NodeKind::Rect { x, y, w: rw, h: rh, .. } => {
                *x = bounds.x0;
                *y = bounds.y0;
                *rw = w;
                *rh = h;
            }
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                *cx = bounds.x0 + w / 2.0;
                *cy = bounds.y0 + h / 2.0;
                *rx = w / 2.0;
                *ry = h / 2.0;
            }
            NodeKind::Polygon { cx, cy, r, .. } => {
                *cx = bounds.x0 + w / 2.0;
                *cy = bounds.y0 + h / 2.0;
                *r = w.min(h) / 2.0;
            }
            NodeKind::Path { path } => {
                let old = path.to_bez().bounding_box();
                if old.width() < 1e-6 || old.height() < 1e-6 {
                    return;
                }
                let sx = w / old.width();
                let sy = h / old.height();
                for p in &mut path.points {
                    p[0] = bounds.x0 + (p[0] - old.x0) * sx;
                    p[1] = bounds.y0 + (p[1] - old.y0) * sy;
                }
            }
            NodeKind::Text { x, y, .. } => {
                *x = bounds.x0;
                *y = bounds.y0;
            }
            NodeKind::Group { .. } => {}
            NodeKind::Image { x, y, width, height, .. } => {
                *x = bounds.x0;
                *y = bounds.y0;
                *width = w;
                *height = h;
            }
            NodeKind::Arc { cx, cy, radius, .. } => {
                *cx = bounds.x0 + w / 2.0;
                *cy = bounds.y0 + h / 2.0;
                *radius = (w.min(h) / 2.0).max(1.0);
            }
            NodeKind::BrushStroke { .. } => {}
        }
    }

    pub fn duplicate(&self) -> Self {
        let mut n = self.clone();
        n.id = Uuid::new_v4();
        n.name = format!("{} copy", self.name);
        n
    }

    /// Flip the node horizontally (mirror across vertical centre axis).
    pub fn flip_h(&mut self) {
        let b = self.bounds();
        let cx = (b.x0 + b.x1) * 0.5;
        match &mut self.kind {
            NodeKind::Path { path } => {
                path.mirror_horizontal(cx);
            }
            NodeKind::BrushStroke { points } => {
                for pt in points.iter_mut() {
                    pt.0[0] = 2.0 * cx - pt.0[0];
                }
                points.reverse();
            }
            NodeKind::Image { x, width, .. } => {
                *x = 2.0 * cx - *x - *width;
            }
            // Ellipse/Polygon/Rect/Arc are symmetric — flip is a no-op for shape
            // but we still need to adjust position for Rect
            NodeKind::Rect { x, w, .. } => {
                *x = 2.0 * cx - *x - *w;
            }
            _ => {}
        }
    }

    /// Flip the node vertically (mirror across horizontal centre axis).
    pub fn flip_v(&mut self) {
        let b = self.bounds();
        let cy = (b.y0 + b.y1) * 0.5;
        match &mut self.kind {
            NodeKind::Path { path } => {
                path.mirror_vertical(cy);
            }
            NodeKind::BrushStroke { points } => {
                for pt in points.iter_mut() {
                    pt.0[1] = 2.0 * cy - pt.0[1];
                }
                points.reverse();
            }
            NodeKind::Image { y, height, .. } => {
                *y = 2.0 * cy - *y - *height;
            }
            NodeKind::Rect { y, h, .. } => {
                *y = 2.0 * cy - *y - *h;
            }
            _ => {}
        }
    }

    pub fn node_points(&self) -> Vec<(f64, f64)> {
        self.edit_handles()
    }

    /// Draggable handles for the node tool (corners, endpoints, path anchors).
    pub fn is_center_edit_handle(&self, index: usize) -> bool {
        matches!(
            (&self.kind, index),
            (NodeKind::Rect { .. }, 0)
                | (NodeKind::Ellipse { .. }, 0)
                | (NodeKind::Polygon { .. }, 0)
                | (NodeKind::Image { .. }, 0)
                | (NodeKind::Arc { .. }, 0)
        )
    }

    pub fn is_text_origin_handle(&self, index: usize) -> bool {
        matches!(&self.kind, NodeKind::Text { .. } if index == 0)
    }

    pub fn path_edit_targets(&self) -> Vec<(PathEditTarget, (f64, f64))> {
        match &self.kind {
            NodeKind::Path { path } => {
                let mut hits = Vec::new();
                for (i, p) in path.anchor_positions().into_iter().enumerate() {
                    hits.push((PathEditTarget::Anchor(i), p));
                }
                for &ai in &path.smooth_anchors {
                    let Some((_, ctrl_in, ctrl_out)) = path.bezier_handles_at(ai) else {
                        continue;
                    };
                    if let Some(co) = ctrl_out {
                        hits.push((PathEditTarget::HandleOut(ai), co));
                    }
                    if let Some(ci) = ctrl_in {
                        hits.push((PathEditTarget::HandleIn(ai), ci));
                    }
                }
                hits
            }
            _ => self
                .edit_handles()
                .into_iter()
                .enumerate()
                .map(|(i, p)| (PathEditTarget::Anchor(i), p))
                .collect(),
        }
    }

    pub fn apply_path_edit_target(&mut self, target: PathEditTarget, x: f64, y: f64) {
        match target {
            PathEditTarget::Anchor(i) => self.set_edit_handle(i, x, y),
            PathEditTarget::HandleOut(i) => {
                if let NodeKind::Path { path } = &mut self.kind {
                    path.set_handle_out(i, x, y);
                }
            }
            PathEditTarget::HandleIn(i) => {
                if let NodeKind::Path { path } = &mut self.kind {
                    path.set_handle_in(i, x, y);
                }
            }
        }
    }

    pub fn edit_handles(&self) -> Vec<(f64, f64)> {
        match &self.kind {
            NodeKind::Rect { x, y, w, h, .. } => vec![
                (*x + *w * 0.5, *y + *h * 0.5),
                (*x, *y),
                (*x + *w, *y),
                (*x + *w, *y + *h),
                (*x, *y + *h),
            ],
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                if self.is_circle() {
                    vec![(*cx, *cy), (*cx + *rx, *cy)]
                } else {
                    vec![
                        (*cx, *cy),
                        (*cx - *rx, *cy),
                        (*cx, *cy - *ry),
                        (*cx + *rx, *cy),
                        (*cx, *cy + *ry),
                    ]
                }
            }
            NodeKind::Polygon {
                cx,
                cy,
                r,
                sides,
                rotation_rad,
            } => {
                let verts = regular_polygon_vertices(*cx, *cy, *r, *sides, *rotation_rad);
                let radius_pt = verts.first().copied().unwrap_or((*cx + *r, *cy));
                vec![(*cx, *cy), radius_pt]
            }
            NodeKind::Path { path } => path_anchor_positions(path),
            NodeKind::Text { x, y, .. } => vec![(*x, *y)],
            NodeKind::Group { .. } => vec![],
            NodeKind::BrushStroke { .. } => vec![],
            NodeKind::Image { x, y, width, height, .. } => vec![
                (*x + *width * 0.5, *y + *height * 0.5), // center
                (*x, *y),
                (*x + *width, *y),
                (*x + *width, *y + *height),
                (*x, *y + *height),
            ],
            NodeKind::Arc { cx, cy, radius, start_angle_rad, sweep_angle_rad, .. } => {
                let mid = *start_angle_rad + *sweep_angle_rad * 0.5;
                let p_start = (*cx + *radius * start_angle_rad.cos(), *cy + *radius * start_angle_rad.sin());
                let p_end = (*cx + *radius * (start_angle_rad + sweep_angle_rad).cos(), *cy + *radius * (start_angle_rad + sweep_angle_rad).sin());
                let p_rim = (*cx + *radius * mid.cos(), *cy + *radius * mid.sin());
                vec![(*cx, *cy), p_rim, p_start, p_end]
            }
        }
    }

    pub fn set_edit_handle(&mut self, index: usize, x: f64, y: f64) {
        let circle = self.is_circle();
        match &mut self.kind {
            NodeKind::Rect { x: rx, y: ry, w, h, .. } => match index {
                0 => {
                    *rx = x - *w * 0.5;
                    *ry = y - *h * 0.5;
                }
                1 => {
                    let x1 = *rx + *w;
                    let y1 = *ry + *h;
                    *rx = x.min(x1 - 1.0);
                    *ry = y.min(y1 - 1.0);
                    *w = (x1 - *rx).max(1.0);
                    *h = (y1 - *ry).max(1.0);
                }
                2 => {
                    let y1 = *ry + *h;
                    *w = (x - *rx).max(1.0);
                    *ry = y.min(y1 - 1.0);
                    *h = (y1 - *ry).max(1.0);
                }
                3 => {
                    *w = (x - *rx).max(1.0);
                    *h = (y - *ry).max(1.0);
                }
                4 => {
                    let x1 = *rx + *w;
                    *rx = x.min(x1 - 1.0);
                    *w = (x1 - *rx).max(1.0);
                    *h = (y - *ry).max(1.0);
                }
                _ => {}
            },
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                if circle {
                    match index {
                        0 => {
                            *cx = x;
                            *cy = y;
                        }
                        1 => {
                            let r = (x - *cx).hypot(y - *cy).max(1.0);
                            *rx = r;
                            *ry = r;
                        }
                        _ => {}
                    }
                } else {
                    match index {
                        0 => {
                            *cx = x;
                            *cy = y;
                        }
                        1 => *rx = (*cx - x).max(1.0),
                        2 => *ry = (*cy - y).max(1.0),
                        3 => *rx = (x - *cx).max(1.0),
                        4 => *ry = (y - *cy).max(1.0),
                        _ => {}
                    }
                }
            },
            NodeKind::Polygon { cx, cy, r, .. } => match index {
                0 => {
                    *cx = x;
                    *cy = y;
                }
                1 => *r = (x - *cx).hypot(y - *cy).max(1.0),
                _ => {}
            },
            NodeKind::Path { path } => {
                path.set_anchor_position(index, x, y);
            }
            NodeKind::Text { x: tx, y: ty, .. } => {
                if index == 0 {
                    *tx = x;
                    *ty = y;
                }
            }
            NodeKind::Group { .. } => {}
            NodeKind::Image { x: ix, y: iy, width: iw, height: ih, .. } => match index {
                0 => { // center
                    *ix = x - *iw * 0.5;
                    *iy = y - *ih * 0.5;
                }
                1 => { let x1 = *ix + *iw; let y1 = *iy + *ih; *ix = x.min(x1-1.); *iy = y.min(y1-1.); *iw = (x1-*ix).max(1.); *ih=(y1-*iy).max(1.); }
                2 => { let y1 = *iy + *ih; *iw = (x - *ix).max(1.); *iy = y.min(y1-1.); *ih = (y1-*iy).max(1.); }
                3 => { *iw = (x - *ix).max(1.); *ih = (y - *iy).max(1.); }
                4 => { let x1 = *ix + *iw; *ix = x.min(x1-1.); *ih = (y-*iy).max(1.); *iw = (x1-*ix).max(1.); }
                _ => {}
            },
            NodeKind::Arc { cx, cy, radius, start_angle_rad, sweep_angle_rad, .. } => match index {
                0 => { *cx = x; *cy = y; }
                1 => { // rim midpoint -> adjust radius only
                    *radius = ((x - *cx).hypot(y - *cy)).max(1.0);
                }
                2 => { // start angle point
                    let angle = (y - *cy).atan2(x - *cx);
                    let end_angle = *start_angle_rad + *sweep_angle_rad;
                    *start_angle_rad = angle;
                    let mut new_sweep = end_angle - angle;
                    // Keep sweep angle in the same visual range (-2PI to 2PI)
                    while new_sweep > std::f64::consts::PI * 2.0 {
                        new_sweep -= std::f64::consts::PI * 2.0;
                    }
                    while new_sweep < -std::f64::consts::PI * 2.0 {
                        new_sweep += std::f64::consts::PI * 2.0;
                    }
                    *sweep_angle_rad = new_sweep;
                    *radius = ((x - *cx).hypot(y - *cy)).max(1.0);
                }
                3 => { // end angle point
                    let angle = (y - *cy).atan2(x - *cx);
                    let mut new_sweep = angle - *start_angle_rad;
                    while new_sweep > std::f64::consts::PI * 2.0 {
                        new_sweep -= std::f64::consts::PI * 2.0;
                    }
                    while new_sweep < -std::f64::consts::PI * 2.0 {
                        new_sweep += std::f64::consts::PI * 2.0;
                    }
                    *sweep_angle_rad = new_sweep;
                    *radius = ((x - *cx).hypot(y - *cy)).max(1.0);
                }
                _ => {}
            }
            NodeKind::BrushStroke { .. } => {}
        }
    }

    pub fn get_geom_floats(&self) -> Vec<f64> {
        let mut v = match &self.kind {
            NodeKind::Rect { w, h, rx, .. } => vec![*w, *h, *rx],
            NodeKind::Ellipse { rx, ry, .. } => vec![*rx, *ry],
            NodeKind::Polygon { r, sides, .. } => vec![*r, *sides as f64],
            NodeKind::Arc { radius, start_angle_rad, sweep_angle_rad, .. } => vec![*radius, *start_angle_rad, *sweep_angle_rad],
            NodeKind::Path { path } => {
                let mut pv = Vec::new();
                let anchors = path.anchor_positions();
                for (i, p) in anchors.iter().enumerate() {
                    pv.push(p.0);
                    pv.push(p.1);
                    let out_off = path.handle_out_offset.get(&i).copied().unwrap_or([0.0, 0.0]);
                    pv.push(out_off[0]);
                    pv.push(out_off[1]);
                    let in_off = path.handle_in_offset.get(&i).copied().unwrap_or([0.0, 0.0]);
                    pv.push(in_off[0]);
                    pv.push(in_off[1]);
                }
                pv
            }
            NodeKind::BrushStroke { points } => {
                let mut pv = Vec::new();
                for (pos, w) in points {
                    pv.push(pos[0]);
                    pv.push(pos[1]);
                    pv.push(*w as f64);
                }
                pv
            }
            _ => Vec::new(),
        };

        // Append fill gradient stops and properties
        match &self.style.fill {
            Fill::LinearGradient { angle_deg, line_x0, line_y0, line_x1, line_y1, stops } => {
                v.push(1.0); // Marker for LinearGradient
                v.push(*angle_deg as f64);
                v.push(*line_x0 as f64);
                v.push(*line_y0 as f64);
                v.push(*line_x1 as f64);
                v.push(*line_y1 as f64);
                v.push(stops.len() as f64);
                for stop in stops {
                    v.push(stop.pos as f64);
                    v.push(stop.color.rgba[0] as f64);
                    v.push(stop.color.rgba[1] as f64);
                    v.push(stop.color.rgba[2] as f64);
                    v.push(stop.color.rgba[3] as f64);
                }
            }
            Fill::RadialGradient { center_x, center_y, stops } => {
                v.push(2.0); // Marker for RadialGradient
                v.push(*center_x as f64);
                v.push(*center_y as f64);
                v.push(stops.len() as f64);
                for stop in stops {
                    v.push(stop.pos as f64);
                    v.push(stop.color.rgba[0] as f64);
                    v.push(stop.color.rgba[1] as f64);
                    v.push(stop.color.rgba[2] as f64);
                    v.push(stop.color.rgba[3] as f64);
                }
            }
            _ => {
                v.push(0.0); // Solid or None marker
            }
        }

        v
    }

    pub fn set_geom_floats(&mut self, floats: &[f64]) {
        if floats.is_empty() {
            return;
        }
        let base_len = match &self.kind {
            NodeKind::Rect { .. } => 3,
            NodeKind::Ellipse { .. } => 2,
            NodeKind::Polygon { .. } => 2,
            NodeKind::Arc { .. } => 3,
            NodeKind::Path { path } => path.anchor_positions().len() * 6,
            NodeKind::BrushStroke { points } => points.len() * 3,
            _ => 0,
        };
        match &mut self.kind {
            NodeKind::Rect { w, h, rx, .. } => {
                if floats.len() >= 3 {
                    *w = floats[0];
                    *h = floats[1];
                    *rx = floats[2];
                }
            }
            NodeKind::Ellipse { rx, ry, .. } => {
                if floats.len() >= 2 {
                    *rx = floats[0];
                    *ry = floats[1];
                }
            }
            NodeKind::Polygon { r, sides, .. } => {
                if floats.len() >= 2 {
                    *r = floats[0];
                    *sides = (floats[1].round() as u32).max(3);
                }
            }
            NodeKind::Arc { radius, start_angle_rad, sweep_angle_rad, .. } => {
                if floats.len() >= 3 {
                    *radius = floats[0];
                    *start_angle_rad = floats[1];
                    *sweep_angle_rad = floats[2];
                }
            }
            NodeKind::Path { path } => {
                let num_anchors = floats.len().min(base_len) / 6;
                let mut anchors = path.anchor_positions();
                if num_anchors == anchors.len() {
                    for i in 0..num_anchors {
                        let base = i * 6;
                        anchors[i] = (floats[base], floats[base + 1]);
                        let out_off = [floats[base + 2], floats[base + 3]];
                        let in_off = [floats[base + 4], floats[base + 5]];
                        path.handle_out_offset.insert(i, out_off);
                        path.handle_in_offset.insert(i, in_off);
                    }
                    path.rebuild_with_smooth_anchors(&anchors);
                }
            }
            NodeKind::BrushStroke { points } => {
                let num_points = floats.len().min(base_len) / 3;
                if num_points == points.len() {
                    for i in 0..num_points {
                        points[i].0[0] = floats[i * 3];
                        points[i].0[1] = floats[i * 3 + 1];
                        points[i].1 = floats[i * 3 + 2] as f32;
                    }
                }
            }
            _ => {}
        }

        // Parse appended gradient floats if present
        if floats.len() > base_len {
            let marker = floats[base_len];
            if marker == 1.0 {
                // LinearGradient
                if floats.len() >= base_len + 7 {
                    let angle_deg = floats[base_len + 1] as f32;
                    let line_x0 = floats[base_len + 2] as f32;
                    let line_y0 = floats[base_len + 3] as f32;
                    let line_x1 = floats[base_len + 4] as f32;
                    let line_y1 = floats[base_len + 5] as f32;
                    let stops_len = floats[base_len + 6].round() as usize;
                    let mut stops = Vec::new();
                    for i in 0..stops_len {
                        let offset = base_len + 7 + i * 5;
                        if offset + 4 < floats.len() {
                            let pos = floats[offset] as f32;
                            let r = floats[offset + 1] as f32;
                            let g = floats[offset + 2] as f32;
                            let b = floats[offset + 3] as f32;
                            let a = floats[offset + 4] as f32;
                            stops.push(crate::document::GradientStop {
                                pos,
                                color: crate::document::Paint { rgba: [r, g, b, a] },
                            });
                        }
                    }
                    self.style.fill = Fill::LinearGradient {
                        angle_deg,
                        line_x0,
                        line_y0,
                        line_x1,
                        line_y1,
                        stops,
                    };
                }
            } else if marker == 2.0 {
                // RadialGradient
                if floats.len() >= base_len + 4 {
                    let center_x = floats[base_len + 1] as f32;
                    let center_y = floats[base_len + 2] as f32;
                    let stops_len = floats[base_len + 3].round() as usize;
                    let mut stops = Vec::new();
                    for i in 0..stops_len {
                        let offset = base_len + 4 + i * 5;
                        if offset + 4 < floats.len() {
                            let pos = floats[offset] as f32;
                            let r = floats[offset + 1] as f32;
                            let g = floats[offset + 2] as f32;
                            let b = floats[offset + 3] as f32;
                            let a = floats[offset + 4] as f32;
                            stops.push(crate::document::GradientStop {
                                pos,
                                color: crate::document::Paint { rgba: [r, g, b, a] },
                            });
                        }
                    }
                    self.style.fill = Fill::RadialGradient {
                        center_x,
                        center_y,
                        stops,
                    };
                }
            }
        }
    }
}

pub fn text_display_name(content: &str) -> String {
    let line = content.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return "Text".into();
    }
    let max = 40;
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(max).collect::<String>())
    }
}

pub fn text_bounds(x: f64, y: f64, style: &TextStyle) -> Rect {
    let line_count = style.content.lines().count().max(1) as f64;
    let max_chars = style
        .content
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1) as f64;
    let size = style.font_size as f64;
    let w = max_chars * size * 0.55 + size * 0.25;
    let h = line_count * size * 1.25 + size * 0.15;
    Rect::new(x, y, x + w.max(size), y + h.max(size))
}

fn path_anchor_positions(path: &PathData) -> Vec<(f64, f64)> {
    let mut positions: Vec<(f64, f64)> = path_anchor_point_indices(path)
        .into_iter()
        .filter_map(|pi| {
            path.points
                .get(pi)
                .map(|p| (p[0], p[1]))
        })
        .collect();
    if path.is_closed() && positions.len() > 1 {
        let first = positions[0];
        let last = positions[positions.len() - 1];
        if (first.0 - last.0).hypot(first.1 - last.1) < 1e-6 {
            positions.pop();
        }
    }
    positions
}

fn cubic_at(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t: f64,
) -> (f64, f64) {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let uuu = uu * u;
    let ttt = tt * t;
    (
        uuu * p0.0 + 3.0 * uu * t * p1.0 + 3.0 * u * tt * p2.0 + ttt * p3.0,
        uuu * p0.1 + 3.0 * uu * t * p1.1 + 3.0 * u * tt * p2.1 + ttt * p3.1,
    )
}

fn unit_vec(dx: f64, dy: f64) -> (f64, f64) {
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        (1.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

fn anchor_tangent(anchors: &[(f64, f64)], idx: usize, closed: bool) -> (f64, f64) {
    let n = anchors.len();
    if n < 2 {
        return (1.0, 0.0);
    }
    let prev = if idx > 0 {
        Some(idx - 1)
    } else if closed && n > 2 {
        Some(n - 1)
    } else {
        None
    };
    let next = if idx + 1 < n {
        Some(idx + 1)
    } else if closed && n > 2 {
        Some(0)
    } else {
        None
    };
    match (prev, next) {
        (Some(p), Some(ni)) => {
            let v1 = unit_vec(
                anchors[idx].0 - anchors[p].0,
                anchors[idx].1 - anchors[p].1,
            );
            let v2 = unit_vec(
                anchors[ni].0 - anchors[idx].0,
                anchors[ni].1 - anchors[idx].1,
            );
            unit_vec(v1.0 + v2.0, v1.1 + v2.1)
        }
        (None, Some(ni)) => unit_vec(
            anchors[ni].0 - anchors[idx].0,
            anchors[ni].1 - anchors[idx].1,
        ),
        (Some(p), None) => unit_vec(
            anchors[idx].0 - anchors[p].0,
            anchors[idx].1 - anchors[p].1,
        ),
        (None, None) => (1.0, 0.0),
    }
}

fn segment_controls(
    anchors: &[(f64, f64)],
    i: usize,
    j: usize,
    closed: bool,
    smooth_i: bool,
    smooth_j: bool,
    handle_out: &HashMap<usize, [f64; 2]>,
    handle_in: &HashMap<usize, [f64; 2]>,
    handle_modes: &HashMap<usize, BezierHandleMode>,
) -> ((f64, f64), (f64, f64)) {
    let p0 = anchors[i];
    let p3 = anchors[j];
    let dist = (p3.0 - p0.0).hypot(p3.1 - p0.1).max(1e-6);
    let t_len = dist / 3.0;

    let mode_i = handle_modes.get(&i).copied().unwrap_or(BezierHandleMode::Symmetric);
    let smooth_i_eff = smooth_i && mode_i != BezierHandleMode::LeftOnly;

    let c1 = if smooth_i_eff {
        if let Some(off) = handle_out.get(&i) {
            (p0.0 + off[0], p0.1 + off[1])
        } else {
            let tan = anchor_tangent(anchors, i, closed);
            (p0.0 + tan.0 * t_len, p0.1 + tan.1 * t_len)
        }
    } else {
        p0
    };

    let mode_j = handle_modes.get(&j).copied().unwrap_or(BezierHandleMode::Symmetric);
    let smooth_j_eff = smooth_j && mode_j != BezierHandleMode::RightOnly;

    let c2 = if smooth_j_eff {
        if let Some(off) = handle_in.get(&j) {
            (p3.0 + off[0], p3.1 + off[1])
        } else {
            let tan = anchor_tangent(anchors, j, closed);
            (p3.0 - tan.0 * t_len, p3.1 - tan.1 * t_len)
        }
    } else {
        p3
    };

    (c1, c2)
}

fn path_anchor_point_indices(path: &PathData) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut pi = 0usize;
    for v in &path.verbs {
        match v {
            0 | 1 => {
                if pi < path.points.len() {
                    indices.push(pi);
                    pi += 1;
                }
            }
            2 => {
                if pi + 1 < path.points.len() {
                    indices.push(pi + 1);
                    pi += 2;
                }
            }
            3 => {
                if pi + 2 < path.points.len() {
                    indices.push(pi + 2);
                    pi += 3;
                } else if pi + 1 < path.points.len() {
                    pi += 2;
                }
            }
            4 => {}
            _ => {}
        }
    }
    indices
}
#[cfg(test)]
mod bezier_tests {
    use super::*;
    use kurbo::PathEl;

    fn flatten_path_points(path: &BezPath, tolerance: f64) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        let els: Vec<PathEl> = path.elements().iter().copied().collect();
        kurbo::flatten(els, tolerance, |el| {
            match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push((p.x, p.y)),
                _ => {}
            }
        });
        pts
    }

    #[test]
    fn closed_path_anchor_count_stable() {
        let anchors = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let path = PathData::from_anchor_data(
            &anchors,
            &[],
            HashMap::new(),
            HashMap::new(),
            true,
        );
        assert_eq!(path.anchor_positions().len(), 4);
        let mut path = path;
        path.set_anchor_position(1, 120.0, 10.0);
        assert_eq!(path.anchor_positions().len(), 4);
    }

    #[test]
    fn smooth_anchor_rebuilds_cubic() {
        let mut path = PathData {
            verbs: vec![0, 1, 1],
            points: vec![[0.0, 0.0], [100.0, 0.0], [200.0, 100.0]],
            closed: false,
            smooth_anchors: Vec::new(),
            handle_out_offset: HashMap::new(),
            handle_in_offset: HashMap::new(),
            handle_modes: HashMap::new(),
        };
        path.set_anchor_smooth(1, true);
        assert!(path.verbs.contains(&3), "verbs: {:?}", path.verbs);
        let bez = path.to_bez();
        assert!(bez.elements().iter().any(|e| matches!(e, PathEl::CurveTo(_, _, _))));
        let flat = flatten_path_points(&bez, 0.5);
        assert!(flat.len() > 3, "flat len {}", flat.len());
        let has_bow = flat.iter().any(|p| p.0 > 10.0 && p.0 < 90.0 && p.1.abs() > 1.0);
        assert!(
            has_bow,
            "curve should bow away from chord, flat={flat:?}"
        );
    }
}
