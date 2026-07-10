use std::collections::HashSet;

use indexmap::IndexMap;
use kurbo::{BezPath, PathEl, Shape};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{FaceRenderable, Node, NodeId, NodeKind, NodeStore, PathData, PathMagic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OnPathMode {
    /// Place copies every `gap` doc units along the path.
    #[default]
    GapDuplicate,
    /// Evenly spaced copies (`count` instances) along the path.
    EvenlySpaced,
    /// Dense slices along the path — continuous extrusion with soft shade (e.g. circle × line → cylinder).
    Loft,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectOnPathEffect {
    pub id: Uuid,
    pub source_id: NodeId,
    pub path_id: NodeId,
    pub mode: OnPathMode,
    pub gap: f64,
    pub count: usize,
    pub start_offset: f64,
    pub rotate_to_tangent: bool,
    pub cyclic: bool,
    pub loft_end_scale: f32,
    pub loft_end_opacity: f32,
    pub hide_source: bool,
    /// Pick/drag proxy: closed Faceable path matching the combined path-magic form (not drawn).
    #[serde(default)]
    pub form_node_id: Option<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TilingEffect {
    pub id: Uuid,
    pub source_id: NodeId,
    pub gap_x: f64,
    pub gap_y: f64,
    pub count_x: usize,
    pub count_y: usize,
    pub offset_x: f64,
    pub offset_y: f64,
    pub row_rotation: f64, // degrees
    pub col_rotation: f64, // degrees
    pub row_scale: f64,
    pub col_scale: f64,
    pub hide_source: bool,
}

impl Default for TilingEffect {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id: Uuid::nil(),
            gap_x: 48.0,
            gap_y: 48.0,
            count_x: 3,
            count_y: 3,
            offset_x: 0.0,
            offset_y: 0.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
            row_scale: 0.0,
            col_scale: 0.0,
            hide_source: false,
        }
    }
}

/// How each CircularClone instance is oriented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CircularRotateMode {
    /// Keep the source orientation for every copy (translate only around the ring).
    Static,
    /// Rotate each copy by its step around the origin, relative to the base instance
    /// (good for chord / fan layouts — base keeps its angle; others follow the ring).
    #[default]
    ReferenceOrigin,
}

impl CircularRotateMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::ReferenceOrigin => "Origin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularCloneEffect {
    pub id: Uuid,
    pub source_id: NodeId,
    pub origin_x: f64,
    pub origin_y: f64,
    pub radius: f64,
    pub copies: usize,
    pub angle_offset: f64, // degrees
    pub base_x: f64,
    pub base_y: f64,
    pub hide_source: bool,
    /// Instance orientation around the origin.
    #[serde(default)]
    pub rotate_mode: CircularRotateMode,
}

impl Default for CircularCloneEffect {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id: Uuid::nil(),
            origin_x: 0.0,
            origin_y: 0.0,
            radius: 48.0,
            copies: 6,
            angle_offset: 0.0,
            base_x: 0.0,
            base_y: 0.0,
            hide_source: false,
            rotate_mode: CircularRotateMode::ReferenceOrigin,
        }
    }
}

impl CircularCloneEffect {
    pub fn ring_radius(&self) -> f64 {
        let dx = self.base_x - self.origin_x;
        let dy = self.base_y - self.origin_y;
        dx.hypot(dy).max(1.0)
    }

    pub fn base_angle_rad(&self) -> f64 {
        let dx = self.base_x - self.origin_x;
        let dy = self.base_y - self.origin_y;
        dy.atan2(dx)
    }

    /// Absolute polar angle of copy `i` on the ring (includes angle_offset).
    pub fn copy_angle_rad(&self, i: usize) -> f64 {
        let n = self.copies.max(3) as f64;
        self.base_angle_rad()
            + (i as f64 / n) * std::f64::consts::TAU
            + self.angle_offset.to_radians()
    }

    pub fn placement_xy(&self, i: usize) -> (f64, f64) {
        let r = self.ring_radius();
        let ang = self.copy_angle_rad(i);
        (
            self.origin_x + r * ang.cos(),
            self.origin_y + r * ang.sin(),
        )
    }

    /// Rotation applied to the instance after placing at ring position.
    /// - Static: 0 (source orientation preserved)
    /// - ReferenceOrigin: delta from base ray so base stays unrotated and copies fan around origin
    pub fn instance_rotation_rad(&self, i: usize) -> f64 {
        match self.rotate_mode {
            CircularRotateMode::Static => 0.0,
            CircularRotateMode::ReferenceOrigin => {
                self.copy_angle_rad(i) - self.base_angle_rad()
            }
        }
    }

    pub fn path_placement(&self, i: usize) -> PathPlacement {
        let (x, y) = self.placement_xy(i);
        PathPlacement {
            x,
            y,
            angle_rad: self.instance_rotation_rad(i),
            scale: 1.0,
            opacity_mul: 1.0,
        }
    }
}

/// A Clip Mask effect: `source_id` (typically a raster image) is rendered clipped to the
/// **solid face** of `mask_id` (Path/Rect/Ellipse/Arc/Polygon) — not the bounding box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipMaskEffect {
    pub id: Uuid,
    /// The object being clipped (usually an Image).
    pub source_id: NodeId,
    /// The node whose filled geometry defines the clip region.
    pub mask_id: NodeId,
    /// When true, the mask node itself is hidden from normal rendering.
    pub hide_mask: bool,
}

impl Default for ClipMaskEffect {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id: Uuid::nil(),
            mask_id: Uuid::nil(),
            hide_mask: true,
        }
    }
}

/// Boolean op between two solid face shapes (Path / Rect / Ellipse / Arc / Polygon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BooleanOpKind {
    #[default]
    Union,
    Intersection,
    Difference,
    /// Symmetric difference (A △ B). Only valid for exactly two operands.
    #[serde(alias = "Xor", alias = "Exclude")]
    Exclude,
}

impl BooleanOpKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Union => "Union",
            Self::Intersection => "Intersection",
            Self::Difference => "Difference",
            Self::Exclude => "Exclude",
        }
    }

    /// Ops that fold cleanly over N≥2 operands.
    pub fn supports_multi(self) -> bool {
        matches!(self, Self::Union | Self::Intersection)
    }
}

/// Live boolean path effect: result is a path node; operands can be hidden until bake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BooleanEffect {
    pub id: Uuid,
    /// Operand A (left side of difference A − B).
    pub a_id: NodeId,
    /// Operand B.
    pub b_id: NodeId,
    pub op: BooleanOpKind,
    /// Hide A and B while the effect is active (result path stays visible).
    #[serde(default = "default_true")]
    pub hide_operands: bool,
    /// Generated path node showing the boolean result (updated live).
    #[serde(default)]
    pub result_node_id: Option<NodeId>,
}

fn default_true() -> bool {
    true
}

impl Default for BooleanEffect {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            a_id: Uuid::nil(),
            b_id: Uuid::nil(),
            op: BooleanOpKind::Union,
            hide_operands: true,
            result_node_id: None,
        }
    }
}

/// Shapes that support boolean solid-face ops.
pub fn is_booleanable_shape(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Path { path } => path.is_closed() || path.to_bez().area().abs() > 1e-3,
        NodeKind::Rect { .. }
        | NodeKind::Ellipse { .. }
        | NodeKind::Polygon { .. } => true,
        NodeKind::Arc { join, .. } => !matches!(join, super::ArcJoin::NoJoin),
        _ => false,
    }
}

pub fn is_raster_image(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Image { .. })
}

/// Flatten a node’s solid face to a geo MultiPolygon (doc space).
pub fn node_to_multipolygon(node: &Node, tolerance: f64) -> Option<geo::MultiPolygon<f64>> {
    use geo::{Coord, LineString, MultiPolygon, Polygon};

    let bez = node.bez_path();
    if bez.elements().is_empty() {
        return None;
    }
    // Flatten kurbo path to polylines (closed rings).
    let mut rings: Vec<Vec<Coord<f64>>> = Vec::new();
    let mut cur: Vec<Coord<f64>> = Vec::new();
    let mut start: Option<Coord<f64>> = None;
    let mut last = Coord { x: 0.0, y: 0.0 };

    let flush = |cur: &mut Vec<Coord<f64>>, rings: &mut Vec<Vec<Coord<f64>>>, start: Option<Coord<f64>>| {
        if cur.len() < 3 {
            cur.clear();
            return;
        }
        if let Some(s) = start {
            if cur.last().map(|c| (c.x - s.x).hypot(c.y - s.y)).unwrap_or(1.0) > 1e-6 {
                cur.push(s);
            }
        }
        if cur.len() >= 4 {
            rings.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };

    for el in bez.elements() {
        match el {
            PathEl::MoveTo(p) => {
                flush(&mut cur, &mut rings, start);
                let c = Coord { x: p.x, y: p.y };
                start = Some(c);
                last = c;
                cur.push(c);
            }
            PathEl::LineTo(p) => {
                let c = Coord { x: p.x, y: p.y };
                last = c;
                cur.push(c);
            }
            PathEl::QuadTo(p1, p2) => {
                let steps = (((last.x - p2.x).hypot(last.y - p2.y) / tolerance).ceil() as usize)
                    .clamp(2, 24);
                let p0 = last;
                for s in 1..=steps {
                    let t = s as f64 / steps as f64;
                    let u = 1.0 - t;
                    let x = u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x;
                    let y = u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y;
                    last = Coord { x, y };
                    cur.push(last);
                }
            }
            PathEl::CurveTo(p1, p2, p3) => {
                let steps = (((last.x - p3.x).hypot(last.y - p3.y) / tolerance).ceil() as usize)
                    .clamp(3, 32);
                let p0 = last;
                for s in 1..=steps {
                    let t = s as f64 / steps as f64;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.x
                        + 3.0 * u * u * t * p1.x
                        + 3.0 * u * t * t * p2.x
                        + t * t * t * p3.x;
                    let y = u * u * u * p0.y
                        + 3.0 * u * u * t * p1.y
                        + 3.0 * u * t * t * p2.y
                        + t * t * t * p3.y;
                    last = Coord { x, y };
                    cur.push(last);
                }
            }
            PathEl::ClosePath => {
                flush(&mut cur, &mut rings, start);
                start = None;
            }
        }
    }
    flush(&mut cur, &mut rings, start);

    if rings.is_empty() {
        return None;
    }
    use geo::orient::{Direction, Orient};
    let polys: Vec<Polygon<f64>> = rings
        .into_iter()
        .filter(|r| r.len() >= 4)
        .map(|r| Polygon::new(LineString::new(r), vec![]).orient(Direction::Default))
        .collect();
    if polys.is_empty() {
        None
    } else {
        Some(MultiPolygon::new(polys))
    }
}

/// Run boolean op on two solid-face nodes → closed BezPath (possibly multi-contour).
/// Empty intersection / difference returns `Some(empty path)` so the effect can still be
/// created and updated when operands later overlap.
pub fn compute_boolean_bez(
    a: &Node,
    b: &Node,
    op: BooleanOpKind,
    tolerance: f64,
) -> Option<BezPath> {
    use geo::BooleanOps;

    let ma = node_to_multipolygon(a, tolerance)?;
    let mb = node_to_multipolygon(b, tolerance)?;
    let result = match op {
        BooleanOpKind::Union => ma.union(&mb),
        BooleanOpKind::Intersection => ma.intersection(&mb),
        BooleanOpKind::Difference => ma.difference(&mb),
        BooleanOpKind::Exclude => ma.xor(&mb),
    };
    if result.0.is_empty() {
        // Valid empty result (e.g. far-apart intersection) — not a conversion failure.
        return Some(BezPath::new());
    }
    multipolygon_to_bez(&result)
}

fn multipolygon_to_bez(mp: &geo::MultiPolygon<f64>) -> Option<BezPath> {
    if mp.0.is_empty() {
        return None;
    }
    let mut bez = BezPath::new();
    for poly in &mp.0 {
        let ext: Vec<_> = poly.exterior().coords().map(|c| (c.x, c.y)).collect();
        if ext.len() < 3 {
            continue;
        }
        bez.move_to(ext[0]);
        for &(x, y) in &ext[1..] {
            bez.line_to((x, y));
        }
        bez.close_path();
        for hole in poly.interiors() {
            let ring: Vec<_> = hole.coords().map(|c| (c.x, c.y)).collect();
            if ring.len() < 3 {
                continue;
            }
            bez.move_to(ring[0]);
            for &(x, y) in &ring[1..] {
                bez.line_to((x, y));
            }
            bez.close_path();
        }
    }
    if bez.elements().is_empty() {
        None
    } else {
        Some(bez)
    }
}

impl Default for ObjectOnPathEffect {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id: Uuid::nil(),
            path_id: Uuid::nil(),
            mode: OnPathMode::GapDuplicate,
            gap: 48.0,
            count: 5,
            start_offset: 0.0,
            rotate_to_tangent: true,
            cyclic: true,
            loft_end_scale: 1.0,
            loft_end_opacity: 0.75,
            hide_source: false,
            form_node_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathPlacement {
    pub x: f64,
    pub y: f64,
    pub angle_rad: f64,
    pub scale: f32,
    pub opacity_mul: f32,
}

#[derive(Debug, Clone)]
struct PathSample {
    points: Vec<(f64, f64)>,
    cumulative: Vec<f64>,
    total_length: f64,
    closed: bool,
}

fn flatten_bez(bez: &BezPath, tolerance: f64) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    let els: Vec<PathEl> = bez.elements().to_vec();
    let mut i = 0usize;
    while i < els.len() {
        match els[i] {
            PathEl::MoveTo(p) => {
                pts.push((p.x, p.y));
                i += 1;
            }
            PathEl::LineTo(p) => {
                pts.push((p.x, p.y));
                i += 1;
            }
            PathEl::QuadTo(_, p2) => {
                let p0 = pts.last().copied().unwrap_or((p2.x, p2.y));
                let p1 = match els.get(i) {
                    Some(PathEl::QuadTo(p1, _)) => (p1.x, p1.y),
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
            PathEl::CurveTo(_, _, p3) => {
                let p0 = pts.last().copied().unwrap_or((p3.x, p3.y));
                let (p1, p2) = match els.get(i) {
                    Some(PathEl::CurveTo(p1, p2, _)) => ((p1.x, p1.y), (p2.x, p2.y)),
                    _ => (p0, (p3.x, p3.y)),
                };
                let steps = ((p0.0 - p3.x).hypot(p0.1 - p3.y) / tolerance).ceil() as usize;
                let steps = steps.clamp(2, 48);
                for s in 1..=steps {
                    let t = s as f64 / steps as f64;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.0
                        + 3.0 * u * u * t * p1.0
                        + 3.0 * u * t * t * p2.0
                        + t * t * t * p3.x;
                    let y = u * u * u * p0.1
                        + 3.0 * u * u * t * p1.1
                        + 3.0 * u * t * t * p2.1
                        + t * t * t * p3.y;
                    pts.push((x, y));
                }
                i += 1;
            }
            PathEl::ClosePath => {
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

fn build_path_samples(path: &PathData, tolerance: f64) -> PathSample {
    let closed = path.is_closed();
    let mut points = flatten_bez(&path.to_bez(), tolerance);
    if points.len() < 2 {
        return PathSample {
            points,
            cumulative: vec![0.0],
            total_length: 0.0,
            closed,
        };
    }
    if closed && points.len() >= 2 {
        let first = points[0];
        let last = points[points.len() - 1];
        if (first.0 - last.0).hypot(first.1 - last.1) > 1e-4 {
            points.push(first);
        }
    }
    let mut cumulative = vec![0.0];
    for w in points.windows(2) {
        let seg = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        cumulative.push(cumulative.last().copied().unwrap_or(0.0) + seg);
    }
    let total_length = *cumulative.last().unwrap_or(&0.0);
    PathSample {
        points,
        cumulative,
        total_length: total_length.max(1e-6),
        closed,
    }
}

fn sample_at(sample: &PathSample, dist: f64) -> (f64, f64, f64) {
    if sample.points.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let total = sample.total_length;
    let mut d = dist;
    if sample.closed && total > 1e-6 {
        d = d.rem_euclid(total);
    } else {
        d = d.clamp(0.0, total);
    }
    if d <= 0.0 {
        let (x0, y0) = sample.points[0];
        let (x1, y1) = sample.points.get(1).copied().unwrap_or((x0 + 1.0, y0));
        let ang = (y1 - y0).atan2(x1 - x0);
        return (x0, y0, ang);
    }
    for i in 1..sample.cumulative.len() {
        if sample.cumulative[i] >= d {
            let d0 = sample.cumulative[i - 1];
            let d1 = sample.cumulative[i];
            let t = if (d1 - d0).abs() < 1e-9 {
                0.0
            } else {
                (d - d0) / (d1 - d0)
            };
            let (x0, y0) = sample.points[i - 1];
            let (x1, y1) = sample.points[i];
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            let ang = (y1 - y0).atan2(x1 - x0);
            return (x, y, ang);
        }
    }
    let (x, y) = *sample.points.last().unwrap();
    (x, y, 0.0)
}

pub fn effect_placements(
    effect: &ObjectOnPathEffect,
    path: &dyn PathMagic,
    tolerance: f64,
) -> Vec<PathPlacement> {
    let total = path.total_length(tolerance);
    if total < 1e-6 {
        return Vec::new();
    }
    let closed = path.is_closed();
    let mut raw: Vec<(f64, f64, f64, f32, f32)> = Vec::new();
    match effect.mode {
        OnPathMode::GapDuplicate => {
            let gap = effect.gap.max(1.0);
            let mut dist = effect.start_offset.max(0.0);
            let limit = if effect.cyclic && closed { total } else { total + 1e-6 };
            while dist <= limit + 1e-6 {
                let (x, y, ang) = path.sample_at(dist, tolerance);
                raw.push((x, y, ang, 1.0, 1.0));
                dist += gap;
                if !effect.cyclic && dist > total { break; }
                if effect.cyclic && closed && dist >= total { break; }
                if raw.len() > 512 { break; }
            }
        }
        OnPathMode::Loft => {
            let desired = 300f64;
            let gap = (total / desired).clamp(0.05, 1.5);
            let mut dist = effect.start_offset.max(0.0);
            let limit = if effect.cyclic && closed { total } else { total + 1e-6 };
            while dist <= limit + 1e-6 {
                let t = (dist / total).clamp(0.0, 1.0) as f32;
                let (x, y, ang) = path.sample_at(dist, tolerance);
                let scale = 1.0 + (effect.loft_end_scale - 1.0) * t;
                let shade = 1.0 + (effect.loft_end_opacity - 1.0) * t;
                raw.push((x, y, ang, scale, shade));
                dist += gap;
                if !effect.cyclic && dist > total { break; }
                if effect.cyclic && closed && dist >= total { break; }
                if raw.len() > 4096 { break; }
            }
            // end point guarantee
            let (ex, ey, eang) = path.sample_at(total, tolerance);
            let et = 1.0f32;
            let escale = 1.0 + (effect.loft_end_scale - 1.0) * et;
            let eshade = 1.0 + (effect.loft_end_opacity - 1.0) * et;
            if let Some(last) = raw.last() {
                if (last.0 - ex).hypot(last.1 - ey) > 1e-3 {
                    raw.push((ex, ey, eang, escale, eshade));
                } else if let Some(last_mut) = raw.last_mut() {
                    *last_mut = (ex, ey, eang, escale, eshade);
                }
            } else {
                raw.push((ex, ey, eang, escale, eshade));
            }
        }
        OnPathMode::EvenlySpaced => {
            let n = effect.count.max(2);
            for i in 0..n {
                let t = if effect.cyclic && closed {
                    i as f64 / n as f64
                } else if n == 1 {
                    0.0
                } else {
                    i as f64 / (n - 1) as f64
                };
                let dist = effect.start_offset + t * total;
                let (x, y, ang) = path.sample_at(dist, tolerance);
                raw.push((x, y, ang, 1.0, 1.0));
            }
        }
    }
    raw.into_iter()
        .map(|(x, y, ang, scale, opacity_mul)| PathPlacement {
            x,
            y,
            angle_rad: if effect.rotate_to_tangent { ang } else { 0.0 },
            scale,
            opacity_mul,
        })
        .collect()
}

/// Slice spacing for loft mode from the source cross-section size.
pub fn default_loft_gap_for_node(source: &Node) -> f64 {
    let b = source.bounds();
    let w = (b.x1 - b.x0).abs().max(1.0);
    let h = (b.y1 - b.y0).abs().max(1.0);
    (w.min(h) * 0.35).clamp(2.0, 24.0)
}

/// For ObjectOnPath selections: compute the "whole Object" bounds (union of all placed instances).
/// This is so the inspector shows the full extent, not just the path spine.
pub fn compute_whole_object_bounds(
    source: &Node,
    effect: &ObjectOnPathEffect,
    path: &PathData,
    tolerance: f64,
) -> kurbo::Rect {
    let placements = effect_placements(effect, path as &dyn PathMagic, tolerance);
    if placements.is_empty() {
        return source.bounds();
    }
    let mut acc: Option<kurbo::Rect> = None;
    for pl in placements {
        let inst = node_at_placement(source as &dyn FaceRenderable, &pl);
        let b = inst.bounds();
        acc = Some(match acc {
            Some(r) => r.union(b),
            None => b,
        });
    }
    acc.unwrap_or_else(|| source.bounds())
}

pub fn compute_tiling_whole_bounds(source: &Node, effect: &TilingEffect) -> kurbo::Rect {
    let b = source.bounds();
    let w = b.x1 - b.x0;
    let h = b.y1 - b.y0;
    let mut acc: Option<kurbo::Rect> = None;
    let first_left = b.x0 + effect.offset_x;
    let first_top = b.y0 + effect.offset_y;
    for ix in 0..effect.count_x {
        for iy in 0..effect.count_y {
            let left = first_left + ix as f64 * effect.gap_x;
            let top = first_top + iy as f64 * effect.gap_y;
            let cx = left + w / 2.0;
            let cy = top + h / 2.0;
            let rot = (ix as f64 * effect.row_rotation + iy as f64 * effect.col_rotation).to_radians();
            let pl = PathPlacement {
                x: cx,
                y: cy,
                angle_rad: rot,
                scale: 1.0,
                opacity_mul: 1.0,
            };
            let inst = node_at_placement(source as &dyn FaceRenderable, &pl);
            let bb = inst.bounds();
            acc = Some(match acc {
                Some(r) => r.union(bb),
                None => bb,
            });
        }
    }
    acc.unwrap_or(b)
}

pub fn compute_circular_whole_bounds(source: &Node, effect: &CircularCloneEffect) -> kurbo::Rect {
    let mut acc: Option<kurbo::Rect> = None;
    let n = effect.copies.max(3);
    for i in 0..n {
        let pl = effect.path_placement(i);
        let inst = node_at_placement(source as &dyn FaceRenderable, &pl);
        let bb = inst.bounds();
        acc = Some(match acc {
            Some(r) => r.union(bb),
            None => bb,
        });
    }
    acc.unwrap_or_else(|| source.bounds())
}

/// True if `doc` hits any circular-clone instance (or the source bounds with slop).
pub fn hit_test_circular_clone(
    source: &Node,
    effect: &CircularCloneEffect,
    doc_x: f64,
    doc_y: f64,
    slop: f64,
) -> bool {
    let n = effect.copies.max(3);
    for i in 0..n {
        let pl = effect.path_placement(i);
        let inst = node_at_placement(source as &dyn FaceRenderable, &pl);
        if inst.hit_test(doc_x, doc_y, slop) {
            return true;
        }
    }
    // Also allow picking the gizmo segment origin↔base (small slop).
    let ox = effect.origin_x;
    let oy = effect.origin_y;
    let bx = effect.base_x;
    let by = effect.base_y;
    let dx = bx - ox;
    let dy = by - oy;
    let len_sq = dx * dx + dy * dy;
    if len_sq > 1e-12 {
        let t = ((doc_x - ox) * dx + (doc_y - oy) * dy) / len_sq;
        let t = t.clamp(0.0, 1.0);
        let px = ox + t * dx;
        let py = oy + t * dy;
        let dist = (doc_x - px).hypot(doc_y - py);
        if dist <= slop.max(4.0) {
            return true;
        }
    }
    false
}

/// Sources that are hidden from canvas draw but still pickable via effect footprints
/// (circular / tiling / object-on-path). Boolean/clip ghosts stay non-pickable.
pub fn is_pickable_effect_source(document: &super::Document, id: NodeId) -> bool {
    node_uses_extended_pick_bounds(document, id)
}

pub fn bez_path_from_rect(r: kurbo::Rect) -> BezPath {
    let mut bez = BezPath::new();
    bez.move_to((r.x0, r.y0));
    bez.line_to((r.x1, r.y0));
    bez.line_to((r.x1, r.y1));
    bez.line_to((r.x0, r.y1));
    bez.close_path();
    bez
}

/// Faceable proxy node for grabbing/moving an object-on-path result as one unit.
pub fn build_path_effect_form_node(
    source: &Node,
    effect: &ObjectOnPathEffect,
    path: &PathData,
    tolerance: f64,
) -> Option<Node> {
    let mut node = if effect.mode == OnPathMode::Loft {
        loft_sweep_node(source, effect, path, tolerance).or_else(|| {
            let b = compute_whole_object_bounds(source, effect, path, tolerance);
            if b.width() < 1e-6 && b.height() < 1e-6 {
                return None;
            }
            Some(Node::path_from_bez(
                bez_path_from_rect(b),
                format!("{} on path", source.name),
            ))
        })?
    } else {
        let b = compute_whole_object_bounds(source, effect, path, tolerance);
        if b.width() < 1e-6 && b.height() < 1e-6 {
            return None;
        }
        let mut n = Node::path_from_bez(
            bez_path_from_rect(b),
            format!("{} on path", source.name),
        );
        n.style = source.style.clone();
        n
    };
    node.style.fill = super::Fill::None;
    node.style.stroke.width = 0.0;
    Some(node)
}

pub fn sync_path_effect_form_geometry(
    form: &mut Node,
    source: &Node,
    effect: &ObjectOnPathEffect,
    path: &PathData,
    tolerance: f64,
) {
    let Some(fresh) = build_path_effect_form_node(source, effect, path, tolerance) else {
        return;
    };
    if let (NodeKind::Path { path: dst }, NodeKind::Path { path: src }) = (&mut form.kind, &fresh.kind)
    {
        *dst = src.clone();
    }
    form.name = fresh.name;
}

pub fn path_effect_by_form_node<'a>(
    effects: &'a IndexMap<Uuid, ObjectOnPathEffect>,
    form_id: NodeId,
) -> Option<&'a ObjectOnPathEffect> {
    effects.values().find(|e| e.form_node_id == Some(form_id))
}

/// Ids that should move together when dragging a path-magic selection (source, spine path, form).
pub fn path_effect_move_bundle(
    document: &super::Document,
    id: NodeId,
) -> Vec<NodeId> {
    if let Some(eff) = path_effect_by_form_node(&document.path_effects, id) {
        return vec![eff.form_node_id.unwrap_or(id), eff.source_id, eff.path_id];
    }
    if let Some(eff) = document
        .path_effects
        .values()
        .find(|e| e.source_id == id || e.path_id == id)
    {
        let mut v = vec![eff.source_id, eff.path_id];
        if let Some(fid) = eff.form_node_id {
            v.push(fid);
        }
        v.sort_by_key(|a| a.as_u128());
        v.dedup();
        return v;
    }
    // Boolean: moving the *result* moves A+B+result together.
    // Moving a ghost operand (A or B alone) must NOT drag the other operand.
    if let Some(eff) = document.boolean_effects.values().find(|e| {
        e.a_id == id || e.b_id == id || e.result_node_id == Some(id)
    }) {
        if eff.result_node_id == Some(id) {
            let mut v = vec![eff.a_id, eff.b_id, id];
            v.sort_by_key(|a| a.as_u128());
            v.dedup();
            return v;
        }
        return vec![id];
    }
    // Clip mask: image and mask move independently (re-clip is live via mesh).
    if document
        .clip_masks
        .values()
        .any(|cm| cm.source_id == id || cm.mask_id == id)
    {
        return vec![id];
    }
    vec![id]
}

pub fn path_effect_form_node_ids(effects: &IndexMap<Uuid, ObjectOnPathEffect>) -> HashSet<NodeId> {
    effects
        .values()
        .filter_map(|e| e.form_node_id)
        .collect()
}

pub fn node_uses_extended_pick_bounds(document: &super::Document, id: NodeId) -> bool {
    document
        .path_effects
        .values()
        .any(|e| e.source_id == id || e.path_id == id || e.form_node_id == Some(id))
        || document.tiling_effects.values().any(|e| e.source_id == id)
        || document
            .circular_effects
            .values()
            .any(|e| e.source_id == id)
}

fn path_data_for_id(nodes: &NodeStore, path_id: NodeId) -> Option<PathData> {
    nodes.get(path_id).and_then(|n| match &n.kind {
        NodeKind::Path { path } => Some(path.clone()),
        _ => None,
    })
}

pub fn spatial_index_bounds(
    node: &Node,
    document: &super::Document,
    nodes: &NodeStore,
) -> kurbo::Rect {
    if node_uses_extended_pick_bounds(document, node.id) {
        get_effective_bounds(node, document, nodes)
    } else {
        node.bounds_with_store(nodes)
    }
}

pub fn get_effective_bounds(
    node: &Node,
    document: &super::Document,
    nodes: &NodeStore,
) -> kurbo::Rect {
    // Groups have bounds() == ZERO; always walk children via store.
    let mut b = node.bounds_with_store(nodes);
    if let Some(e) = document.tiling_effects.values().find(|e| e.source_id == node.id) {
        let whole = compute_tiling_whole_bounds(node, e);
        // Hidden source must not keep a "stuck" original bbox edge in the selection box.
        b = if e.hide_source {
            whole
        } else {
            b.union(whole)
        };
    }
    if let Some(e) = document.circular_effects.values().find(|e| e.source_id == node.id) {
        let whole = compute_circular_whole_bounds(node, e);
        b = if e.hide_source {
            whole
        } else {
            b.union(whole)
        };
    }
    if let Some(e) = document.path_effects.values().find(|e| e.source_id == node.id) {
        if let Some(path) = path_data_for_id(nodes, e.path_id) {
            b = b.union(compute_whole_object_bounds(node, e, &path, 0.5));
        }
    }
    for e in document
        .path_effects
        .values()
        .filter(|e| e.path_id == node.id)
    {
        let Some(source) = nodes.get(e.source_id) else {
            continue;
        };
        let Some(path) = path_data_for_id(nodes, e.path_id) else {
            continue;
        };
        b = b.union(compute_whole_object_bounds(source, e, &path, 0.5));
    }
    if let Some(eff) = path_effect_by_form_node(&document.path_effects, node.id) {
        if let (Some(source), Some(path)) = (
            nodes.get(eff.source_id),
            path_data_for_id(nodes, eff.path_id),
        ) {
            b = b.union(compute_whole_object_bounds(source, eff, &path, 0.5));
        }
    }
    b
}

fn transform_profile_point(
    p: (f64, f64),
    cx: f64,
    cy: f64,
    ang: f64,
    scale: f32,
) -> (f64, f64) {
    let sx = p.0 * scale as f64;
    let sy = p.1 * scale as f64;
    let rx = sx * ang.cos() - sy * ang.sin();
    let ry = sx * ang.sin() + sy * ang.cos();
    (cx + rx, cy + ry)
}

fn profile_points_relative(source: &Node, tolerance: f64) -> Vec<(f64, f64)> {
    let bez = source.bez_path();
    let bb = bez.bounding_box();
    let cx = (bb.x0 + bb.x1) * 0.5;
    let cy = (bb.y0 + bb.y1) * 0.5;
    let mut pts = flatten_bez(&bez, tolerance);
    if pts.len() >= 2 {
        let first = pts[0];
        if let Some(last) = pts.last().copied() {
            if (first.0 - last.0).hypot(first.1 - last.1) < 1e-4 {
                pts.pop();
            }
        }
    }
    if pts.len() < 3 {
        let hx = ((bb.x1 - bb.x0) * 0.5).max(0.5);
        let hy = ((bb.y1 - bb.y0) * 0.5).max(0.5);
        return vec![(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)];
    }
    pts.into_iter().map(|(x, y)| (x - cx, y - cy)).collect()
}

fn loft_spine_samples(
    sample: &PathSample,
    effect: &ObjectOnPathEffect,
) -> Vec<(f64, f64, f64, f32, f32)> {
    let total = sample.total_length;
    let step = if effect.mode == OnPathMode::Loft {
        (total / 800.0).clamp(0.1, 2.0)
    } else {
        (effect.gap * 0.2).clamp(0.5, 3.0)
    };
    let mut dist = effect.start_offset.max(0.0);
    let mut out = Vec::new();
    while dist <= total + 1e-6 {
        let t = (dist / total).clamp(0.0, 1.0) as f32;
        let (x, y, ang) = sample_at(sample, dist);
        let scale = 1.0 + (effect.loft_end_scale - 1.0) * t;
        let shade = 1.0 + (effect.loft_end_opacity - 1.0) * t;
        out.push((x, y, ang, scale, shade));
        dist += step;
        if !effect.cyclic && dist > total + 1e-6 {
            break;
        }
        if effect.cyclic && sample.closed && dist >= total {
            break;
        }
        if out.len() > 2048 {
            break;
        }
    }
    if out.is_empty() {
        return out;
    }
    let (ex, ey, eang) = sample_at(&sample, total);
    let ea = if effect.rotate_to_tangent { eang } else { 0.0 };
    let end_scale = effect.loft_end_scale;
    let end_shade = effect.loft_end_opacity;
    let last = out.last().copied().unwrap();
    if (last.0 - ex).hypot(last.1 - ey) > 0.5 {
        out.push((ex, ey, ea, end_scale, end_shade));
    } else {
        let n = out.len();
        out[n - 1] = (ex, ey, ea, end_scale, end_shade);
    }
    out
}

/// CAD-style loft silhouette using discrete sampling + Boolean Union.
/// This avoids all naive offsetting self-intersections (swallowtails).
///
/// Pipeline (exactly as specified):
/// 1. Discretize the base path into finite sample points (via effect_placements for Loft: resolution via fixed small step).
/// 2. At each sample, generate a discrete closed polygon for the profile at the correctly interpolated scale (and rotation only if rotate_to_tangent).
/// 3. Perform continuous Boolean Union over all sample polygons using geo.
/// 4. Extract the final merged external boundary contour (largest exterior ring).
/// 5. Return as kurbo::BezPath ready to become epaint::PathShape or Shape::closed_line.
pub fn loft_sweep_bez(
    source: &Node,
    effect: &ObjectOnPathEffect,
    path: &PathData,
    tolerance: f64,
) -> Option<BezPath> {
    if effect.mode != OnPathMode::Loft {
        return None;
    }
    let profile = profile_points_relative(source, tolerance);
    if profile.len() < 3 {
        return None;
    }

    // 1. Discretize path curve using the same placement logic (Loft forces dense step internally for accurate union).
    let placements = effect_placements(effect, path as &dyn PathMagic, tolerance);
    if placements.len() < 2 {
        return None;
    }

    // 2. At each sampled point generate discrete closed polygon at interpolated scale (+ rot only when flag).
    use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon};

    let mut polys: Vec<Polygon<f64>> = Vec::new();
    for pl in &placements {
        // Respect rotate_to_tangent exactly as placements do for live instances.
        let rot = pl.angle_rad;
        let mut pts: Vec<Coord<f64>> = profile
            .iter()
            .map(|&(px, py)| {
                let (tx, ty) = transform_profile_point((px, py), pl.x, pl.y, rot, pl.scale);
                Coord { x: tx, y: ty }
            })
            .collect();

        if pts.len() >= 3 {
            // close ring
            if pts.first() != pts.last() {
                if let Some(&first) = pts.first() {
                    pts.push(first);
                }
            }
            let ls = LineString::new(pts);
            polys.push(Polygon::new(ls, vec![]));
        }
    }

    if polys.is_empty() {
        return None;
    }

    // 3. Continuous Boolean Union across all generated circle/profile polygons.
    let mut iter = polys.into_iter();
    let mut result = if let Some(p) = iter.next() {
        MultiPolygon::new(vec![p])
    } else {
        return None;
    };
    for p in iter {
        result = result.union(&p);
    }

    if result.0.is_empty() {
        return None;
    }

    // 4. Extract the final, merged external boundary contour.
    // Choose the single largest ring as the primary external boundary.
    let mut best: Option<Vec<(f64, f64)>> = None;
    let mut best_len = 0usize;
    for poly in &result.0 {
        let coords: Vec<_> = poly.exterior().coords().map(|c| (c.x, c.y)).collect();
        if coords.len() >= 3 && coords.len() > best_len {
            best_len = coords.len();
            best = Some(coords);
        }
    }
    let ring = best?;

    // 5. Convert to single clean contour BezPath for epaint::PathShape / Shape::closed_line.
    let mut bez = BezPath::new();
    if ring.len() < 3 {
        return None;
    }
    bez.move_to(ring[0]);
    for &(x, y) in &ring[1..] {
        bez.line_to((x, y));
    }
    bez.close_path();

    if bez.is_empty() || bez.area().abs() < 1e-3 {
        return None;
    }
    Some(bez)
}

/// Single path node for loft preview/bake — fill + one outer stroke only.
/// Uses the clean Boolean-union contour. Keeps source fill; modulates opacity by avg shade.
pub fn loft_sweep_node(
    source: &Node,
    effect: &ObjectOnPathEffect,
    path: &PathData,
    tolerance: f64,
) -> Option<Node> {
    let bez = loft_sweep_bez(source, effect, path, tolerance)?;
    let sample = build_path_samples(path, tolerance);
    let samples = loft_spine_samples(&sample, effect);
    let mut node = Node::path_from_bez(bez, format!("{} loft", source.name));
    node.style = source.style.clone();
    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let shade = ((first.4 + last.4) * 0.5).clamp(0.05, 1.0);
        node.style.opacity = (node.style.opacity * shade).clamp(0.0, 1.0);
    }
    Some(node)
}

pub fn node_at_placement(source: &dyn FaceRenderable, placement: &PathPlacement) -> Node {
    let mut inst: Box<dyn FaceRenderable> = source.clone_renderable();
    let b = inst.bounds();
    let cx = (b.x0 + b.x1) * 0.5;
    let cy = (b.y0 + b.y1) * 0.5;
    inst.translate(placement.x - cx, placement.y - cy);
    if placement.scale.abs() > 1e-4 && (placement.scale - 1.0).abs() > 1e-4 {
        inst.scale_about_center(placement.scale as f64);
    }
    if placement.angle_rad.abs() > 1e-6 {
        inst.rotate_about_center(placement.angle_rad);
    }
    let new_op = (inst.opacity() * placement.opacity_mul).clamp(0.0, 1.0);
    inst.set_opacity(new_op);

    // Recover concrete Node (all objects are Nodes in current light-A model)
    let mut n = if let Some(n) = inst.as_any().downcast_ref::<Node>() {
        n.clone()
    } else if let Some(orig) = source.as_any().downcast_ref::<Node>() {
        // Fallback: clone original source and re-apply (should not happen)
        let mut n = orig.clone();
        let b = n.bounds();
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        n.translate(placement.x - cx, placement.y - cy);
        if placement.scale.abs() > 1e-4 && (placement.scale - 1.0).abs() > 1e-4 {
            n.scale_about_center(placement.scale as f64);
        }
        if placement.angle_rad.abs() > 1e-6 {
            n.rotate_about_center(placement.angle_rad);
        }
        n.style.opacity = (n.style.opacity * placement.opacity_mul).clamp(0.0, 1.0);
        n
    } else {
        Node::new(
            NodeKind::Rect {
                x: 0.0,
                y: 0.0,
                w: 8.0,
                h: 8.0,
                rx: 0.0,
            },
            "dyn-fallback",
        )
    };
    // Always unique id — bake/tiling insert many instances; reusing source id overwrites.
    n.id = Uuid::new_v4();
    n
}

pub fn find_effect_for_pair<'a>(
    effects: &'a IndexMap<Uuid, ObjectOnPathEffect>,
    a: NodeId,
    b: NodeId,
) -> Option<&'a ObjectOnPathEffect> {
    effects.values().find(|e| {
        (e.source_id == a && e.path_id == b) || (e.source_id == b && e.path_id == a)
    })
}

/// Source objects replaced on canvas by an active object-on-path effect.
pub fn hidden_effect_sources(effects: &IndexMap<Uuid, ObjectOnPathEffect>) -> HashSet<NodeId> {
    effects
        .values()
        .filter(|e| e.hide_source)
        .map(|e| e.source_id)
        .collect()
}

pub fn has_effect_for_objects(
    effects: &IndexMap<Uuid, ObjectOnPathEffect>,
    objects: &[NodeId],
    path_id: NodeId,
) -> bool {
    objects
        .iter()
        .any(|oid| find_effect_for_pair(effects, *oid, path_id).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Fill, Node, NodeKind, Paint, Stroke};

    #[test]
    fn loft_dense_slices_along_open_path() {
        let line = Node::line(0.0, 0.0, 200.0, 0.0, Stroke::default());
        let NodeKind::Path { path } = &line.kind else {
            panic!("expected path node");
        };
        let path = path.clone();
        let effect = ObjectOnPathEffect {
            mode: OnPathMode::Loft,
            gap: 10.0,
            cyclic: false,
            loft_end_scale: 1.0,
            loft_end_opacity: 0.8,
            ..ObjectOnPathEffect::default()
        };
        let placements = effect_placements(&effect, &path as &dyn PathMagic, 0.5);
        assert!(placements.len() >= 18, "expected dense loft slices, got {}", placements.len());
        assert!((placements.last().unwrap().opacity_mul - 0.8).abs() < 0.05);
        assert!((placements.first().unwrap().opacity_mul - 1.0).abs() < 0.05);
    }

    #[test]
    fn loft_sweep_outline_is_single_closed_capsule() {
        let circle = Node::ellipse(
            0.0,
            0.0,
            20.0,
            20.0,
            Fill::Solid(Paint::from_hex(0xffffff, 1.0)),
        );
        let line = Node::line(0.0, 0.0, 200.0, 0.0, Stroke::default());
        let NodeKind::Path { path } = &line.kind else {
            panic!("expected path");
        };
        let path = path.clone();
        let effect = ObjectOnPathEffect {
            mode: OnPathMode::Loft,
            gap: 8.0,
            rotate_to_tangent: false,
            cyclic: false,
            loft_end_scale: 1.0,
            loft_end_opacity: 0.85,
            ..ObjectOnPathEffect::default()
        };
        let bez = loft_sweep_bez(&circle, &effect, &path, 0.5).expect("loft outline");
        let bb = bez.bounding_box();
        assert!(bb.width() > 180.0 && bb.height() > 35.0, "bbox {bb:?}");
        assert!(bez.area().abs() > 6_000.0, "capsule area {}", bez.area());
    }

    #[test]
    fn default_loft_gap_uses_smaller_cross_section() {
        let node = Node::ellipse(
            0.0,
            0.0,
            40.0,
            40.0,
            Fill::Solid(Paint::from_hex(0xffffff, 1.0)),
        );
        let gap = default_loft_gap_for_node(&node);
        assert!(gap >= 2.0 && gap <= 24.0);
    }
}

