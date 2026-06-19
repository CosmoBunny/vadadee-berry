use std::collections::HashSet;

use indexmap::IndexMap;
use kurbo::{BezPath, PathEl, Shape};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Node, NodeId, PathData};

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
    path: &PathData,
    tolerance: f64,
) -> Vec<PathPlacement> {
    let sample = build_path_samples(path, tolerance);
    if sample.points.len() < 2 || sample.total_length < 1e-6 {
        return Vec::new();
    }
    let mut raw: Vec<(f64, f64, f64, f32, f32)> = Vec::new();
    match effect.mode {
        OnPathMode::GapDuplicate => {
            let gap = effect.gap.max(1.0);
            let mut dist = effect.start_offset.max(0.0);
            let limit = if effect.cyclic && sample.closed {
                sample.total_length
            } else {
                sample.total_length + 1e-6
            };
            while dist <= limit + 1e-6 {
                let (x, y, ang) = sample_at(&sample, dist);
                raw.push((x, y, ang, 1.0, 1.0));
                dist += gap;
                if !effect.cyclic && dist > sample.total_length {
                    break;
                }
                if effect.cyclic && sample.closed && dist >= sample.total_length {
                    break;
                }
                if raw.len() > 512 {
                    break;
                }
            }
        }
        OnPathMode::Loft => {
            // Force dense step. Choose step so we get good coverage without millions of points
            // on very long paths, while still dense enough for smooth union silhouette.
            let total = sample.total_length;
            let desired = 300f64; // dense enough for smooth merge, but not thousands to keep FPS
            let gap = (total / desired).clamp(0.05, 1.5);
            let mut dist = effect.start_offset.max(0.0);
            let limit = if effect.cyclic && sample.closed {
                total
            } else {
                total + 1e-6
            };
            while dist <= limit + 1e-6 {
                let t = (dist / total).clamp(0.0, 1.0) as f32;
                let (x, y, ang) = sample_at(&sample, dist);
                let scale = 1.0 + (effect.loft_end_scale - 1.0) * t;
                let shade = 1.0 + (effect.loft_end_opacity - 1.0) * t;
                raw.push((x, y, ang, scale, shade));
                dist += gap;
                if !effect.cyclic && dist > total {
                    break;
                }
                if effect.cyclic && sample.closed && dist >= total {
                    break;
                }
                if raw.len() > 4096 {
                    break;
                }
            }
            // Guarantee the exact end point (for correct end scale/shade + full path coverage in union).
            let (ex, ey, eang) = sample_at(&sample, total);
            let et = 1.0f32;
            let escale = 1.0 + (effect.loft_end_scale - 1.0) * et;
            let eshade = 1.0 + (effect.loft_end_opacity - 1.0) * et;
            if let Some(last) = raw.last() {
                if (last.0 - ex).hypot(last.1 - ey) > 1e-3 {
                    raw.push((ex, ey, eang, escale, eshade));
                } else {
                    let n = raw.len();
                    raw[n - 1] = (ex, ey, eang, escale, eshade);
                }
            } else {
                raw.push((ex, ey, eang, escale, eshade));
            }
        }
        OnPathMode::EvenlySpaced => {
            let n = effect.count.max(2);
            for i in 0..n {
                let t = if effect.cyclic && sample.closed {
                    i as f64 / n as f64
                } else if n == 1 {
                    0.0
                } else {
                    i as f64 / (n - 1) as f64
                };
                let dist = effect.start_offset + t * sample.total_length;
                let (x, y, ang) = sample_at(&sample, dist);
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
    let placements = effect_placements(effect, path, tolerance);
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

pub fn node_at_placement(source: &Node, placement: &PathPlacement) -> Node {
    let mut n = source.clone();
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
        let placements = effect_placements(&effect, &path, 0.5);
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

