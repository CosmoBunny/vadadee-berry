use egui::{Align2, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use egui::epaint::{
    CubicBezierShape, EllipseShape, PathShape, PathStroke, QuadraticBezierShape,
};
use kurbo::{BezPath, Ellipse, PathEl, Rect as KurboRect, Shape as KurboShape};
use lyon::math::Point;
use lyon::path::Path;
use lyon::tessellation::{BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};

use crate::canvas::Viewport;
use std::collections::HashSet;

use crate::document::{
    ArcJoin, FaceRenderable, Fill, LineCap, LineJoin, Node, NodeId, NodeKind, NodeStore, Paint, PathMagic, TextStyle,
    regular_polygon_vertices,
};
use crate::theme::colors;
use crate::gradient_ui::GradientLineHandle;
use crate::tools::ResizeHandle;

fn path_flatten_tolerance(viewport: &Viewport) -> f64 {
    // Tighter base tolerance so curved boundaries (rounded rect, ellipse) are followed more
    // closely by the gradient mesh. This improves visual clipping of the gradient to the true
    // curve instead of coarse chords.
    (0.15 / (viewport.zoom as f64).max(0.2)).clamp(0.02, 0.15)
}

/// Finer flattening when a filled region must be approximated as a polygon.
fn fill_flatten_tolerance(viewport: &Viewport) -> f64 {
    (0.06 / (viewport.zoom as f64).max(0.25)).clamp(0.004, 0.06)
}

pub fn draw_grid(painter: &Painter, viewport: &Viewport, _origin: Pos2, page: Rect) {
    if !viewport.show_grid {
        return;
    }
    let step = viewport.grid_step * viewport.zoom;
    if step < 4.0 {
        return;
    }
    let clip = page.intersect(painter.clip_rect());
    let mut x = (clip.left() / step).floor() * step;
    while x < clip.right() {
        let color = if (x / step).rem_euclid(5.0) < 0.5 {
            Color32::from_gray(55)
        } else {
            Color32::from_gray(40)
        };
        painter.line_segment(
            [Pos2::new(x, clip.top()), Pos2::new(x, clip.bottom())],
            Stroke::new(1.0, color),
        );
        x += step;
    }
    let mut y = (clip.top() / step).floor() * step;
    while y < clip.bottom() {
        let color = if (y / step).rem_euclid(5.0) < 0.5 {
            Color32::from_gray(55)
        } else {
            Color32::from_gray(40)
        };
        painter.line_segment(
            [Pos2::new(clip.left(), y), Pos2::new(clip.right(), y)],
            Stroke::new(1.0, color),
        );
        y += step;
    }
}

pub fn draw_page_shadow(painter: &Painter, page: Rect) {
    let shadow = page.expand(6.0);
    painter.rect_filled(shadow, 4.0, Color32::from_black_alpha(80));
    painter.rect_filled(page, 0.0, Color32::WHITE);
    painter.rect_stroke(page, 0.0, Stroke::new(1.0, Color32::from_gray(120)), egui::StrokeKind::Inside);
}

fn paint_to_color(p: Paint, opacity: f32) -> Color32 {
    let mut c = p.to_egui();
    c = Color32::from_rgba_premultiplied(
        c.r(),
        c.g(),
        c.b(),
        (c.a() as f32 * opacity) as u8,
    );
    c
}

fn stroke_width(node: &Node, viewport: &Viewport) -> Option<f32> {
    if node.style.stroke.width <= 0.0 || !node.style.stroke.style.is_visible() {
        return None;
    }
    Some((node.style.stroke.width * viewport.zoom).max(0.01))
}

fn sample_fill_at(fill: &Fill, opacity: f32, nx: f32, ny: f32) -> Color32 {
    if !fill.is_visible() {
        return Color32::TRANSPARENT;
    }
    paint_to_color(fill.sample_at(nx, ny), opacity)
}

pub fn sample_paint_fill(fill: &Fill, opacity: f32, nx: f32, ny: f32) -> Color32 {
    sample_fill_at(fill, opacity, nx, ny)
}

fn draw_gradient_line(
    painter: &Painter,
    p0: Pos2,
    p1: Pos2,
    c0: Color32,
    c1: Color32,
    width: f32,
) {
    const SEGS: usize = 12;
    for i in 0..SEGS {
        let t0 = i as f32 / SEGS as f32;
        let t1 = (i + 1) as f32 / SEGS as f32;
        let a = p0.lerp(p1, t0);
        let b = p0.lerp(p1, t1);
        let c = Color32::from_rgba_premultiplied(
            lerp_u8(c0.r(), c1.r(), t0),
            lerp_u8(c0.g(), c1.g(), t0),
            lerp_u8(c0.b(), c1.b(), t0),
            lerp_u8(c0.a(), c1.a(), t0),
        );
        painter.line_segment([a, b], Stroke::new(width, c));
    }
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn draw_stroke_closed_ring(
    painter: &Painter,
    _viewport: &Viewport,
    screen_pts: &[Pos2],
    doc_pts: &[(f64, f64)],
    style: &Fill,
    opacity: f32,
    width: f32,
    join: LineJoin,
) {
    if screen_pts.len() < 2 || doc_pts.len() < 2 {
        return;
    }
    if screen_pts.len() < 3 {
        return;
    }
    if matches!(style, Fill::Solid(_)) {
        let c = sample_fill_at(style, opacity, 0.5, 0.5);
        draw_feathered_polyline_stroke(painter, screen_pts, true, width, c);
        if join == LineJoin::Round {
            stroke_join_dots(painter, screen_pts, width, c, join);
        }
        return;
    }
    let screen_pts = screen_pts;
    let doc_pts = doc_pts;
    let (x0, y0, x1, y1) = doc_pts.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(x0, y0, x1, y1), (x, y)| (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)),
    );
    let n = screen_pts.len();
    let half = width * 0.5;
    for i in 0..n {
        let j = (i + 1) % n;
        let (nx0, ny0) = doc_norm(doc_pts[i].0, doc_pts[i].1, x0, y0, x1, y1);
        let (nx1, ny1) = doc_norm(doc_pts[j].0, doc_pts[j].1, x0, y0, x1, y1);
        let c0 = sample_fill_at(style, opacity, nx0, ny0);
        let c1 = sample_fill_at(style, opacity, nx1, ny1);
        let (seg_a, seg_b) =
            segment_endpoints_for_join(screen_pts, i, true, join, half);
        if matches!(style, Fill::Solid(_)) {
            painter.line_segment([seg_a, seg_b], Stroke::new(width, c0));
        } else {
            draw_gradient_line(painter, seg_a, seg_b, c0, c1, width);
        }
    }
    if join == LineJoin::Round {
        let r = width * 0.5;
        for i in 0..n {
            let (nx, ny) = doc_norm(doc_pts[i].0, doc_pts[i].1, x0, y0, x1, y1);
            let c = sample_fill_at(style, opacity, nx, ny);
            painter.circle_filled(screen_pts[i], r, c);
        }
    }
}

fn stroke_cap_circles(painter: &Painter, pts: &[Pos2], width: f32, color: Color32, cap: LineCap) {
    if cap != LineCap::Round || pts.len() < 2 {
        return;
    }
    let r = width * 0.5;
    painter.circle_filled(pts[0], r, color);
    painter.circle_filled(pts[pts.len() - 1], r, color);
}

fn stroke_join_dots(
    painter: &Painter,
    screen_pts: &[Pos2],
    width: f32,
    color: Color32,
    join: LineJoin,
) {
    if join != LineJoin::Round || screen_pts.len() < 3 {
        return;
    }
    let r = width * 0.5;
    for p in &screen_pts[1..screen_pts.len() - 1] {
        painter.circle_filled(*p, r, color);
    }
}

fn segment_endpoints_for_join(
    screen_pts: &[Pos2],
    seg_idx: usize,
    _closed: bool,
    _join: LineJoin,
    _half_width: f32,
) -> (Pos2, Pos2) {
    let n = screen_pts.len();
    (screen_pts[seg_idx], screen_pts[(seg_idx + 1) % n])
}

fn draw_stroke_open_polyline(
    painter: &Painter,
    _viewport: &Viewport,
    screen_pts: &[Pos2],
    doc_pts: &[(f64, f64)],
    style: &Fill,
    opacity: f32,
    width: f32,
    join: LineJoin,
    cap: LineCap,
) {
    if screen_pts.len() < 2 || doc_pts.len() < 2 {
        return;
    }
    if matches!(style, Fill::Solid(_)) {
        let c = sample_fill_at(style, opacity, 0.5, 0.5);
        draw_feathered_polyline_stroke(painter, screen_pts, false, width, c);
        stroke_cap_circles(painter, screen_pts, width, c, cap);
        if join == LineJoin::Round {
            stroke_join_dots(painter, screen_pts, width, c, join);
        }
        return;
    }
    let screen_pts = screen_pts;
    let doc_pts = doc_pts;
    let (x0, y0, x1, y1) = doc_pts.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(x0, y0, x1, y1), (x, y)| (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)),
    );
    let half = width * 0.5;
    for i in 0..screen_pts.len() - 1 {
        let (nx0, ny0) = doc_norm(doc_pts[i].0, doc_pts[i].1, x0, y0, x1, y1);
        let (nx1, ny1) = doc_norm(doc_pts[i + 1].0, doc_pts[i + 1].1, x0, y0, x1, y1);
        let c0 = sample_fill_at(style, opacity, nx0, ny0);
        let c1 = sample_fill_at(style, opacity, nx1, ny1);
        let (seg_a, seg_b) =
            segment_endpoints_for_join(screen_pts, i, false, join, half);
        if matches!(style, Fill::Solid(_)) {
            painter.line_segment([seg_a, seg_b], Stroke::new(width, c0));
        } else {
            draw_gradient_line(painter, seg_a, seg_b, c0, c1, width);
        }
    }
    if join == LineJoin::Round {
        let c = sample_fill_at(style, opacity, 0.5, 0.5);
        stroke_cap_circles(painter, screen_pts, width, c, cap);
        stroke_join_dots(painter, screen_pts, width, c, join);
    }
}

fn rounded_rect_path_points(
    viewport: &Viewport,
    origin: Pos2,
    doc: (f64, f64, f64, f64),
    rx: f64,
) -> (Vec<Pos2>, Vec<(f64, f64)>) {
    let (x0, y0, x1, y1) = doc;
    let r = KurboRect::new(x0, y0, x1, y1);
    let path = if rx > 0.0 {
        KurboShape::to_path(&r.to_rounded_rect(rx), 0.05)
    } else {
        KurboShape::to_path(&r, 0.05)
    };
    let tol = path_flatten_tolerance(viewport);
    let doc_pts = flatten_path_points(&path, tol);
    let screen_pts: Vec<Pos2> = doc_pts
        .iter()
        .map(|p| viewport.doc_to_screen(*p, origin))
        .collect();
    (screen_pts, doc_pts)
}

fn draw_rect_stroke(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    screen: Rect,
    doc: (f64, f64, f64, f64),
    rx_doc: f64,
    style: &Fill,
    opacity: f32,
    width: f32,
    corner_screen: f32,
    join: LineJoin,
    _cap: LineCap,
) {
    let use_rounded = rx_doc > 0.0 || corner_screen > 0.5;
    if use_rounded {
        let (screen_pts, doc_pts) = rounded_rect_path_points(viewport, origin, doc, rx_doc);
        if screen_pts.len() >= 3 {
            let bez = {
                let (x0, y0, x1, y1) = doc;
                let r = KurboRect::new(x0, y0, x1, y1);
                if rx_doc > 0.0 {
                    KurboShape::to_path(&r.to_rounded_rect(rx_doc), 0.05)
                } else {
                    KurboShape::to_path(&r, 0.05)
                }
            };
            if matches!(style, Fill::Solid(_)) {
                let c = sample_fill_at(style, opacity, 0.5, 0.5);
                for s in bez_to_feathered_stroke_shapes(&bez, viewport, origin, width, c) {
                    painter.add(s);
                }
                if join == LineJoin::Round {
                    stroke_join_dots(painter, &screen_pts, width, c, join);
                }
            } else {
                draw_stroke_closed_ring(
                    painter,
                    viewport,
                    &screen_pts,
                    &doc_pts,
                    style,
                    opacity,
                    width,
                    join,
                );
            }
            return;
        }
    }
    if matches!(style, Fill::Solid(_)) {
        let c = sample_fill_at(style, opacity, 0.0, 0.0);
        painter.rect_stroke(
            screen,
            corner_screen,
            Stroke::new(width, c),
            egui::StrokeKind::Outside,
        );
        return;
    }
    let (x0, y0, x1, y1) = doc;
    let corners_doc = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let corners_screen = [
        screen.left_top(),
        screen.right_top(),
        screen.right_bottom(),
        screen.left_bottom(),
    ];
    draw_stroke_closed_ring(
        painter,
        viewport,
        &corners_screen,
        &corners_doc,
        style,
        opacity,
        width,
        join,
    );
}

fn draw_ellipse_stroke(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    center: Pos2,
    radius: egui::Vec2,
    doc_bounds: (f64, f64, f64, f64),
    style: &Fill,
    opacity: f32,
    width: f32,
    join: LineJoin,
) {
    if matches!(style, Fill::Solid(_)) {
        let c = sample_fill_at(style, opacity, 0.5, 0.5);
        painter.add(Shape::ellipse_stroke(center, radius, Stroke::new(width, c)));
        return;
    }
    let (x0, y0, x1, y1) = doc_bounds;
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let rx = (x1 - x0) / 2.0;
    let ry = (y1 - y0) / 2.0;
    let (screen_pts, doc_pts) = ellipse_ring_points(cx, cy, rx, ry, viewport, origin);
    draw_stroke_closed_ring(
        painter,
        viewport,
        &screen_pts,
        &doc_pts,
        style,
        opacity,
        width,
        join,
    );
}

fn doc_norm(x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> (f32, f32) {
    let w = (x1 - x0).max(1e-6);
    let h = (y1 - y0).max(1e-6);
    (((x - x0) / w) as f32, ((y - y0) / h) as f32)
}

fn screen_norm(p: Pos2, bbox: Rect) -> (f32, f32) {
    let w = bbox.width().max(1e-6);
    let h = bbox.height().max(1e-6);
    (((p.x - bbox.left()) / w), ((p.y - bbox.top()) / h))
}

fn lyon_fill_options(viewport: &Viewport) -> FillOptions {
    let tolerance = (0.12 / viewport.zoom).clamp(0.02, 0.12);
    FillOptions::default()
        .with_tolerance(tolerance)
        .with_fill_rule(lyon::tessellation::FillRule::NonZero)
}

/// Stroke a kurbo path with egui feathering (anti-aliased curves and line joins).
fn draw_solid_bez_stroke(
    painter: &Painter,
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    width: f32,
    color: Color32,
    join: LineJoin,
    cap: LineCap,
    closed: bool,
) {
    for s in bez_to_feathered_stroke_shapes(bez, viewport, origin, width, color) {
        painter.add(s);
    }
    let (screen_pts, _) = polyline_from_bez(bez, viewport, origin, closed);
    if screen_pts.len() >= 2 {
        if !closed {
            stroke_cap_circles(painter, &screen_pts, width, color, cap);
        }
        if join == LineJoin::Round {
            stroke_join_dots(painter, &screen_pts, width, color, join);
        }
    }
}

fn draw_feathered_polyline_stroke(
    painter: &Painter,
    screen_pts: &[Pos2],
    closed: bool,
    width: f32,
    color: Color32,
) {
    if screen_pts.len() < 2 || (closed && screen_pts.len() < 3) {
        return;
    }
    painter.add(Shape::Path(PathShape {
        points: screen_pts.to_vec(),
        closed,
        fill: Color32::TRANSPARENT,
        stroke: PathStroke::new(width, color),
    }));
}

fn bez_to_lyon_path(bez: &BezPath, viewport: &Viewport, origin: Pos2) -> Path {
    let mut builder = Path::builder();
    let mut open = false;
    let map = |x: f64, y: f64| {
        let s = viewport.doc_to_screen((x, y), origin);
        Point::new(s.x, s.y)
    };
    let begin_at = |builder: &mut lyon::path::Builder, open: &mut bool, x: f64, y: f64| {
        if *open {
            builder.end(false);
        }
        builder.begin(map(x, y));
        *open = true;
    };
    for el in bez.elements() {
        match el {
            PathEl::MoveTo(p) => begin_at(&mut builder, &mut open, p.x, p.y),
            PathEl::LineTo(p) => {
                if !open {
                    begin_at(&mut builder, &mut open, p.x, p.y);
                } else {
                    builder.line_to(map(p.x, p.y));
                }
            }
            PathEl::QuadTo(p1, p2) => {
                if !open {
                    begin_at(&mut builder, &mut open, p1.x, p1.y);
                }
                builder.quadratic_bezier_to(map(p1.x, p1.y), map(p2.x, p2.y));
            }
            PathEl::CurveTo(p1, p2, p3) => {
                if !open {
                    begin_at(&mut builder, &mut open, p1.x, p1.y);
                }
                builder.cubic_bezier_to(
                    map(p1.x, p1.y),
                    map(p2.x, p2.y),
                    map(p3.x, p3.y),
                );
            }
            PathEl::ClosePath => {
                if open {
                    // close() is end(true) — never call end() again for this subpath.
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

fn ellipse_bez_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(0.01)
}

fn ellipse_ring_points(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    viewport: &Viewport,
    origin: Pos2,
) -> (Vec<Pos2>, Vec<(f64, f64)>) {
    let bez = ellipse_bez_path(cx, cy, rx, ry);
    let tol = path_flatten_tolerance(viewport);
    let doc_pts = flatten_path_points(&bez, tol);
    let screen_pts = doc_pts
        .iter()
        .map(|p| viewport.doc_to_screen(*p, origin))
        .collect();
    (screen_pts, doc_pts)
}

fn flatten_path_points(path: &BezPath, tolerance: f64) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    let els: Vec<PathEl> = path.elements().iter().copied().collect();
    kurbo::flatten(els, tolerance, |el| {
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push((p.x, p.y)),
            _ => {}
        }
    });
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

fn doc_bounds_screen_rect(viewport: &Viewport, origin: Pos2, doc: (f64, f64, f64, f64)) -> Rect {
    let (x0, y0, x1, y1) = doc;
    let tl = viewport.doc_to_screen((x0, y0), origin);
    let br = viewport.doc_to_screen((x1, y1), origin);
    Rect::from_min_max(tl, br)
}

/// Gradient field over object **doc bbox** (0..1), clipped to shape via Lyon (beziers included).
fn clipped_gradient_mesh_from_bez(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    doc_bounds: (f64, f64, f64, f64),
    fill: &Fill,
    opacity: f32,
) -> Mesh {
    let mut mesh = Mesh::default();
    let bbox_screen = doc_bounds_screen_rect(viewport, origin, doc_bounds);
    let lyon_path = bez_to_lyon_path(bez, viewport, origin);
    tessellate_clipped_gradient(
        &mut mesh,
        &lyon_path,
        bbox_screen,
        fill,
        opacity,
        &lyon_fill_options(viewport),
    );
    mesh
}

fn polygon_bez_path(verts: &[(f64, f64)]) -> BezPath {
    let mut path = BezPath::new();
    if verts.is_empty() {
        return path;
    }
    path.move_to((verts[0].0, verts[0].1));
    for (x, y) in &verts[1..] {
        path.line_to((*x, *y));
    }
    path.close_path();
    path
}

fn rounded_rect_gradient_mesh(
    viewport: &Viewport,
    origin: Pos2,
    doc: (f64, f64, f64, f64),
    rx: f64,
    fill: &Fill,
    opacity: f32,
) -> Mesh {
    let (x0, y0, x1, y1) = doc;
    let r = KurboRect::new(x0, y0, x1, y1);
    let path = if rx > 0.0 {
        KurboShape::to_path(&r.to_rounded_rect(rx), 0.05)
    } else {
        KurboShape::to_path(&r, 0.05)
    };
    clipped_gradient_mesh_from_bez(&path, viewport, origin, doc, fill, opacity)
}

fn rect_gradient_mesh(
    screen: Rect,
    doc: (f64, f64, f64, f64),
    fill: &Fill,
    opacity: f32,
) -> Mesh {
    // For LinearGradient we use a band-based tessellation with iso-lines at the stop positions.
    // This guarantees that color bands are straight lines perpendicular to the gradient line
    // and the transitions are exactly at the stop positions along the line (linear spread w.r.t. the line).
    if let Fill::LinearGradient { line_x0: lx0, line_y0: ly0, line_x1: lx1, line_y1: ly1, stops, .. } = fill {
        return linear_gradient_rect_bands(screen, *lx0, *ly0, *lx1, *ly1, stops, opacity);
    }

    // Fallback for Radial (and future) on rects: center + fan gives good results.
    let (x0, y0, x1, y1) = doc;
    let corners = [
        (screen.left_top(), doc_norm(x0, y0, x0, y0, x1, y1)),
        (screen.right_top(), doc_norm(x1, y0, x0, y0, x1, y1)),
        (screen.right_bottom(), doc_norm(x1, y1, x0, y0, x1, y1)),
        (screen.left_bottom(), doc_norm(x0, y1, x0, y0, x1, y1)),
    ];
    let mut mesh = Mesh::default();
    let center_screen = screen.center();
    let (cnx, cny) = (0.5f32, 0.5f32);
    let cidx = mesh.vertices.len() as u32;
    mesh.colored_vertex(center_screen, sample_fill_at(fill, opacity, cnx, cny));

    let mut corner_v = [0u32; 4];
    for (i, (pos, (nx, ny))) in corners.iter().enumerate() {
        let v = mesh.vertices.len() as u32;
        mesh.colored_vertex(*pos, sample_fill_at(fill, opacity, *nx, *ny));
        corner_v[i] = v;
    }
    for i in 0..4 {
        let a = corner_v[i];
        let b = corner_v[(i + 1) % 4];
        mesh.add_triangle(cidx, a, b);
    }
    mesh
}

/// Create a mesh for a linear gradient on an axis-aligned rect using explicit bands
/// between consecutive stop positions. Each band is a (possibly degenerate) trapezoid/quad
/// whose two parallel sides lie on iso-t lines, with constant color on each iso side.
/// This makes the spread perfectly linear w.r.t. the gradient line.
fn linear_gradient_rect_bands(
    screen: Rect,
    lx0: f32,
    ly0: f32,
    lx1: f32,
    ly1: f32,
    stops: &[crate::document::GradientStop],
    opacity: f32,
) -> Mesh {
    let mut mesh = Mesh::default();

    if stops.len() < 2 {
        // degenerate, just a solid-ish quad using first color
        let c = sample_fill_at(&crate::document::Fill::Solid(stops.first().map(|s| s.color).unwrap_or(crate::document::Paint::none())), opacity, 0.0, 0.0);
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(screen.left_top(), c);
        mesh.colored_vertex(screen.right_top(), c);
        mesh.colored_vertex(screen.right_bottom(), c);
        mesh.colored_vertex(screen.left_bottom(), c);
        mesh.add_triangle(base, base+1, base+2);
        mesh.add_triangle(base, base+2, base+3);
        return mesh;
    }

    // Collect critical levels: all stop positions + the projected t at the 4 corners (for caps)
    let mut levels: Vec<f32> = stops.iter().map(|s| s.pos).collect();
    let corner_norms = [(0f32,0f32), (1f32,0f32), (1f32,1f32), (0f32,1f32)];
    for (nx, ny) in corner_norms {
        let tt = crate::document::project_onto_linear_line(nx, ny, lx0, ly0, lx1, ly1);
        levels.push(tt);
    }
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap());
    levels.dedup_by(|a, b| (*a - *b).abs() < 1e-5);

    // Helper: intersections of iso-t with the normalized unit rect, returned as screen positions
    let iso_hits = |t: f32| -> Vec<Pos2> {
        let mut raw: Vec<(f32, f32)> = vec![];
        let vx = lx1 - lx0;
        let vy = ly1 - ly0;
        let l2 = vx * vx + vy * vy;
        if l2 < 1e-12 {
            return vec![];
        }

        // left (nx=0)
        {
            let nx = 0.0;
            if vy.abs() > 1e-9 {
                let ny = ly0 + ((t * l2) - (nx - lx0) * vx) / vy;
                if ny >= -1e-4 && ny <= 1.0004 {
                    raw.push((nx, ny.clamp(0.0, 1.0)));
                }
            } else if vx.abs() > 1e-9 {
                let tt = ((nx - lx0) * vx + (0.5 - ly0) * vy) / l2;
                if (tt - t).abs() < 1e-3 {
                    raw.push((nx, 0.0));
                    raw.push((nx, 1.0));
                }
            }
        }
        // right (nx=1)
        {
            let nx = 1.0;
            if vy.abs() > 1e-9 {
                let ny = ly0 + ((t * l2) - (nx - lx0) * vx) / vy;
                if ny >= -1e-4 && ny <= 1.0004 {
                    raw.push((nx, ny.clamp(0.0, 1.0)));
                }
            } else if vx.abs() > 1e-9 {
                let tt = ((nx - lx0) * vx + (0.5 - ly0) * vy) / l2;
                if (tt - t).abs() < 1e-3 {
                    raw.push((nx, 0.0));
                    raw.push((nx, 1.0));
                }
            }
        }
        // bottom (ny=0)
        {
            let ny = 0.0;
            if vx.abs() > 1e-9 {
                let nx = lx0 + ((t * l2) - (ny - ly0) * vy) / vx;
                if nx >= -1e-4 && nx <= 1.0004 {
                    raw.push((nx.clamp(0.0, 1.0), ny));
                }
            } else if vy.abs() > 1e-9 {
                let tt = ((0.5 - lx0) * vx + (ny - ly0) * vy) / l2;
                if (tt - t).abs() < 1e-3 {
                    raw.push((0.0, ny));
                    raw.push((1.0, ny));
                }
            }
        }
        // top (ny=1)
        {
            let ny = 1.0;
            if vx.abs() > 1e-9 {
                let nx = lx0 + ((t * l2) - (ny - ly0) * vy) / vx;
                if nx >= -1e-4 && nx <= 1.0004 {
                    raw.push((nx.clamp(0.0, 1.0), ny));
                }
            } else if vy.abs() > 1e-9 {
                let tt = ((0.5 - lx0) * vx + (ny - ly0) * vy) / l2;
                if (tt - t).abs() < 1e-3 {
                    raw.push((0.0, ny));
                    raw.push((1.0, ny));
                }
            }
        }

        // map to screen (axis aligned rect)
        let mut out: Vec<Pos2> = raw
            .into_iter()
            .map(|(nx, ny)| {
                Pos2::new(
                    screen.left() + nx * screen.width(),
                    screen.top() + ny * screen.height(),
                )
            })
            .collect();

        // sort the (usually 2) hits along the perpendicular so we have consistent "side0 / side1"
        if out.len() >= 2 {
            let dx = (lx1 - lx0) as f32 * screen.width();
            let dy = (ly1 - ly0) as f32 * screen.height();
            let perp_x = -dy;
            let perp_y = dx;
            out.sort_by(|a, b| {
                let da = (a.x - screen.center().x) * perp_x + (a.y - screen.center().y) * perp_y;
                let db = (b.x - screen.center().x) * perp_x + (b.y - screen.center().y) * perp_y;
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        // dedup very close
        out.dedup_by(|a, b| a.distance(*b) < 0.1);
        out
    };

    // Compute the portion of the axis-aligned screen rect that lies on one side of the iso-t line
    // defined by the gradient line at `target_t`. Returns ordered vertices for a convex polygon.
    // `low=true` means the half-plane project(nx,ny) <= target_t (the "start" side of the line).
    let rect_halfplane_polygon = |target_t: f32, low: bool| -> Vec<Pos2> {
        let vx = lx1 - lx0;
        let vy = ly1 - ly0;
        let l2 = vx * vx + vy * vy;
        if l2 < 1e-12 {
            return vec![screen.left_top(), screen.right_top(), screen.right_bottom(), screen.left_bottom()];
        }
        // Walk the unit rect boundary in order and clip to the half-plane
        let uc = [(0f32, 0f32), (1., 0.), (1., 1.), (0., 1.)];
        let mut clipped_norm: Vec<(f32, f32)> = vec![];
        for i in 0..4 {
            let (x1, y1) = uc[i];
            let (x2, y2) = uc[(i + 1) % 4];
            let t1 = ((x1 - lx0) * vx + (y1 - ly0) * vy) / l2;
            let t2 = ((x2 - lx0) * vx + (y2 - ly0) * vy) / l2;
            let side1 = if low { t1 <= target_t } else { t1 >= target_t };
            let side2 = if low { t2 <= target_t } else { t2 >= target_t };
            if side1 {
                clipped_norm.push((x1, y1));
            }
            if side1 != side2 {
                let f = (target_t - t1) / (t2 - t1 + 1e-12);
                let ix = (x1 + (x2 - x1) * f).clamp(0.0, 1.0);
                let iy = (y1 + (y2 - y1) * f).clamp(0.0, 1.0);
                clipped_norm.push((ix, iy));
            }
        }
        // Map to screen (axis-aligned)
        clipped_norm
            .into_iter()
            .map(|(nx, ny)| {
                Pos2::new(
                    screen.left() + nx * screen.width(),
                    screen.top() + ny * screen.height(),
                )
            })
            .collect()
    };

    // --- Explicit caps for areas beyond the gradient line ends (t <= 0 and t >= 1) ---
    // This ensures the remaining space is always filled with the end stop colors,
    // even when gradient line endpoints are inside the shape or dragged outside.
    // Low side (t <= 0): flat first stop color
    {
        let cap = rect_halfplane_polygon(0.0, true);
        if cap.len() >= 3 {
            let paint = crate::document::sample_stops(stops, 0.0);
            let c = egui::Color32::from_rgba_premultiplied(
                (paint.rgba[0] * 255.0 * opacity) as u8,
                (paint.rgba[1] * 255.0 * opacity) as u8,
                (paint.rgba[2] * 255.0 * opacity) as u8,
                (paint.rgba[3] * 255.0 * opacity) as u8,
            );
            let base = mesh.vertices.len() as u32;
            for p in &cap {
                mesh.colored_vertex(*p, c);
            }
            for i in 1..cap.len() - 1 {
                mesh.add_triangle(base, base + i as u32, base + (i + 1) as u32);
            }
        }
    }
    // High side (t >= 1): flat last stop color
    {
        let cap = rect_halfplane_polygon(1.0, false);
        if cap.len() >= 3 {
            let paint = crate::document::sample_stops(stops, 1.0);
            let c = egui::Color32::from_rgba_premultiplied(
                (paint.rgba[0] * 255.0 * opacity) as u8,
                (paint.rgba[1] * 255.0 * opacity) as u8,
                (paint.rgba[2] * 255.0 * opacity) as u8,
                (paint.rgba[3] * 255.0 * opacity) as u8,
            );
            let base = mesh.vertices.len() as u32;
            for p in &cap {
                mesh.colored_vertex(*p, c);
            }
            for i in 1..cap.len() - 1 {
                mesh.add_triangle(base, base + i as u32, base + (i + 1) as u32);
            }
        }
    }

    for i in 0..levels.len().saturating_sub(1) {
        let ta = levels[i];
        let tb = levels[i + 1];

        let ha = iso_hits(ta);
        let hb = iso_hits(tb);
        if ha.len() < 2 || hb.len() < 2 {
            continue;
        }

        // colors on the iso sides (using a point on the iso gives the clamped sample)
        let pa = (lx0 + (lx1 - lx0) * ta, ly0 + (ly1 - ly0) * ta);
        let pb = (lx0 + (lx1 - lx0) * tb, ly0 + (ly1 - ly0) * tb);
        // Compute color by sampling the stops at the (clamped) parameter t for the iso line
        let paint_a = crate::document::sample_stops(stops, ta.clamp(0.0, 1.0));
        let paint_b = crate::document::sample_stops(stops, tb.clamp(0.0, 1.0));
        let ca = egui::Color32::from_rgba_premultiplied(
            (paint_a.rgba[0] * 255.0 * opacity) as u8,
            (paint_a.rgba[1] * 255.0 * opacity) as u8,
            (paint_a.rgba[2] * 255.0 * opacity) as u8,
            (paint_a.rgba[3] * 255.0 * opacity) as u8,
        );
        let cb = egui::Color32::from_rgba_premultiplied(
            (paint_b.rgba[0] * 255.0 * opacity) as u8,
            (paint_b.rgba[1] * 255.0 * opacity) as u8,
            (paint_b.rgba[2] * 255.0 * opacity) as u8,
            (paint_b.rgba[3] * 255.0 * opacity) as u8,
        );

        // vertices
        let v0 = mesh.vertices.len() as u32; // low side0
        mesh.colored_vertex(ha[0], ca);
        let v1 = mesh.vertices.len() as u32; // low side1
        mesh.colored_vertex(ha[1], ca);
        let v2 = mesh.vertices.len() as u32; // high side1
        mesh.colored_vertex(hb[1], cb);
        let v3 = mesh.vertices.len() as u32; // high side0
        mesh.colored_vertex(hb[0], cb);

        // two triangles for the band quad (ha0, ha1, hb1, hb0)
        mesh.add_triangle(v0, v1, v2);
        mesh.add_triangle(v0, v2, v3);
    }

    // If for some reason no bands were added (degenerate line), fall back to a simple colored rect
    if mesh.vertices.is_empty() {
        let paint = crate::document::sample_stops(stops, 0.5);
        let c = egui::Color32::from_rgba_premultiplied(
            (paint.rgba[0] * 255.0 * opacity) as u8,
            (paint.rgba[1] * 255.0 * opacity) as u8,
            (paint.rgba[2] * 255.0 * opacity) as u8,
            (paint.rgba[3] * 255.0 * opacity) as u8,
        );
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(screen.left_top(), c);
        mesh.colored_vertex(screen.right_top(), c);
        mesh.colored_vertex(screen.right_bottom(), c);
        mesh.colored_vertex(screen.left_bottom(), c);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }

    mesh
}

// --- bbox-sized gradient map + clip mask on closed path ---
// The gradient is defined over the object's bounding box (normalized 0..1 in x/y).
// Lyon tessellates the closed path outline as a clip mask; each interior vertex samples
// the bbox gradient field at its position. Works for concave paths and arbitrary winding.

fn tessellate_clipped_gradient(
    mesh: &mut Mesh,
    path: &Path,
    bbox_screen: Rect,
    fill: &Fill,
    opacity: f32,
    fill_options: &FillOptions,
) {
    if bbox_screen.width() < 1e-6 || bbox_screen.height() < 1e-6 {
        return;
    }

    let mut tessellator = FillTessellator::new();
    let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
    if tessellator
        .tessellate_path(
            path,
            fill_options,
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| v.position()),
        )
        .is_err()
        || buffers.indices.is_empty()
    {
        return;
    }

    for chunk in buffers.indices.chunks_exact(3) {
        let v0 = buffers.vertices[chunk[0] as usize];
        let v1 = buffers.vertices[chunk[1] as usize];
        let v2 = buffers.vertices[chunk[2] as usize];
        let p0 = Pos2::new(v0.x, v0.y);
        let p1 = Pos2::new(v1.x, v1.y);
        let p2 = Pos2::new(v2.x, v2.y);

        let (nx0, ny0) = screen_norm(p0, bbox_screen);
        let (nx1, ny1) = screen_norm(p1, bbox_screen);
        let (nx2, ny2) = screen_norm(p2, bbox_screen);

        let i0 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p0, sample_fill_at(fill, opacity, nx0, ny0));
        let i1 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p1, sample_fill_at(fill, opacity, nx1, ny1));
        let i2 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p2, sample_fill_at(fill, opacity, nx2, ny2));
        mesh.add_triangle(i0, i1, i2);
    }
}

fn add_clipped_gradient_mesh(
    mesh: &mut Mesh,
    screen_pts: &[Pos2],
    _doc_pts: &[(f64, f64)],
    fill: &Fill,
    opacity: f32,
    viewport: &Viewport,
) {
    if screen_pts.len() < 3 {
        return;
    }

    let mut min_sx = f32::MAX;
    let mut min_sy = f32::MAX;
    let mut max_sx = f32::MIN;
    let mut max_sy = f32::MIN;
    for p in screen_pts {
        min_sx = min_sx.min(p.x);
        min_sy = min_sy.min(p.y);
        max_sx = max_sx.max(p.x);
        max_sy = max_sy.max(p.y);
    }
    let bbox_screen = Rect::from_min_max(Pos2::new(min_sx, min_sy), Pos2::new(max_sx, max_sy));

    let mut builder = Path::builder();
    builder.begin(Point::new(screen_pts[0].x, screen_pts[0].y));
    for p in &screen_pts[1..] {
        builder.line_to(Point::new(p.x, p.y));
    }
    builder.close();
    let path = builder.build();

    tessellate_clipped_gradient(
        mesh,
        &path,
        bbox_screen,
        fill,
        opacity,
        &lyon_fill_options(viewport),
    );
}

fn draw_shape_fill(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    fill: &Fill,
    opacity: f32,
    screen_rect: Rect,
    doc_bounds: (f64, f64, f64, f64),
    rx_doc: f64,
    corner_screen: f32,
) {
    if !fill.is_visible() {
        return;
    }
    match fill {
        Fill::Solid(p) => {
            let c = paint_to_color(*p, opacity);
            if corner_screen > 0.0 {
                painter.rect_filled(
                    screen_rect,
                    corner_screen.min(screen_rect.width() / 2.0),
                    c,
                );
            } else {
                painter.rect_filled(screen_rect, 0.0, c);
            }
        }
        _ => {
            let (x0, y0, x1, y1) = doc_bounds;
            let r = KurboRect::new(x0, y0, x1, y1);
            let path = if rx_doc > 0.0 {
                KurboShape::to_path(&r.to_rounded_rect(rx_doc), 0.05)
            } else {
                KurboShape::to_path(&r, 0.05)
            };
            let mesh =
                clipped_gradient_mesh_from_bez(&path, viewport, origin, doc_bounds, fill, opacity);
            painter.add(Shape::mesh(mesh));
        }
    }
}

fn doc_to_screen_pos(viewport: &Viewport, origin: Pos2, x: f64, y: f64) -> Pos2 {
    viewport.doc_to_screen((x, y), origin)
}

/// Stroke each kurbo segment as a native egui bezier/line shape so epaint feathering (AA) applies.
fn bez_to_feathered_stroke_shapes(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    width: f32,
    color: Color32,
) -> Vec<Shape> {
    let path_stroke = PathStroke::new(width, color);
    let line_stroke = Stroke::new(width, color);
    let map = |x: f64, y: f64| doc_to_screen_pos(viewport, origin, x, y);
    let mut shapes = Vec::new();
    let mut subpath_start = Pos2::ZERO;
    let mut pen: Option<Pos2> = None;

    for el in bez.elements() {
        match el {
            PathEl::MoveTo(p) => {
                let pt = map(p.x, p.y);
                subpath_start = pt;
                pen = Some(pt);
            }
            PathEl::LineTo(p) => {
                let to = map(p.x, p.y);
                if let Some(from) = pen {
                    if from.distance(to) > 1e-4 {
                        shapes.push(Shape::line_segment([from, to], line_stroke));
                    }
                }
                pen = Some(to);
            }
            PathEl::QuadTo(p1, p2) => {
                let from = pen.unwrap_or_else(|| map(p1.x, p1.y));
                shapes.push(Shape::QuadraticBezier(
                    QuadraticBezierShape::from_points_stroke(
                        [from, map(p1.x, p1.y), map(p2.x, p2.y)],
                        false,
                        Color32::TRANSPARENT,
                        path_stroke.clone(),
                    ),
                ));
                pen = Some(map(p2.x, p2.y));
            }
            PathEl::CurveTo(p1, p2, p3) => {
                let from = pen.unwrap_or_else(|| map(p1.x, p1.y));
                shapes.push(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    [from, map(p1.x, p1.y), map(p2.x, p2.y), map(p3.x, p3.y)],
                    false,
                    Color32::TRANSPARENT,
                    path_stroke.clone(),
                )));
                pen = Some(map(p3.x, p3.y));
            }
            PathEl::ClosePath => {
                if let Some(from) = pen {
                    if from.distance(subpath_start) > 1e-4 {
                        shapes.push(Shape::line_segment([from, subpath_start], line_stroke));
                    }
                }
                pen = Some(subpath_start);
            }
        }
    }
    shapes
}

fn bez_to_fill_shapes(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    fill: Color32,
    treat_as_closed: bool,
) -> Vec<Shape> {
    let tol = fill_flatten_tolerance(viewport);
    let mut shapes = Vec::new();
    let mut subpath = BezPath::new();
    let mut closed = treat_as_closed;

    let flush_subpath = |shapes: &mut Vec<Shape>, subpath: &BezPath, closed: bool| {
        if subpath.elements().is_empty() || !closed {
            return;
        }
        let doc_pts = flatten_path_points(subpath, tol);
        if doc_pts.len() < 3 {
            return;
        }
        let pts: Vec<Pos2> = doc_pts
            .iter()
            .map(|p| viewport.doc_to_screen(*p, origin))
            .collect();
        shapes.push(Shape::Path(PathShape {
            points: pts,
            closed: true,
            fill,
            stroke: PathStroke::NONE,
        }));
    };

    for el in bez.elements() {
        match el {
            PathEl::MoveTo(_) => {
                if !subpath.elements().is_empty() {
                    flush_subpath(&mut shapes, &subpath, closed);
                    subpath = BezPath::new();
                }
                subpath.push(*el);
                closed = treat_as_closed;
            }
            PathEl::ClosePath => {
                subpath.push(*el);
                closed = true;
            }
            _ => subpath.push(*el),
        }
    }
    flush_subpath(&mut shapes, &subpath, closed);
    shapes
}

pub fn bez_to_egui_shapes(
    path: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    fill: Option<Color32>,
    stroke: Option<(f32, Color32)>,
    treat_as_closed: bool,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let fill_color = fill.unwrap_or(Color32::TRANSPARENT);
    if treat_as_closed && fill_color.a() > 0 {
        shapes.extend(bez_to_fill_shapes(
            path,
            viewport,
            origin,
            fill_color,
            treat_as_closed,
        ));
    }
    if let Some((w, sc)) = stroke {
        shapes.extend(bez_to_feathered_stroke_shapes(
            path, viewport, origin, w, sc,
        ));
    }
    shapes
}

fn polyline_from_bez(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    closed: bool,
) -> (Vec<Pos2>, Vec<(f64, f64)>) {
    let tol = path_flatten_tolerance(viewport);
    let doc = flatten_path_points(bez, tol);
    let screen: Vec<Pos2> = doc
        .iter()
        .map(|p| viewport.doc_to_screen(*p, origin))
        .collect();
    let _ = closed;
    (screen, doc)
}

pub fn draw_node(
    painter: &Painter,
    node: &Node,
    viewport: &Viewport,
    origin: Pos2,
    selected: bool,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
) {
    let opacity = node.style.opacity;
    let fill = &node.style.fill;
    let stroke_style = &node.style.stroke.style;
    let stroke_join = node.style.stroke.line_join;
    let stroke_cap = node.style.stroke.line_cap;
    let stroke_w = stroke_width(node, viewport);

    match &node.kind {
        NodeKind::Rect { x, y, w, h, rx } => {
            let tl = viewport.doc_to_screen((*x, *y), origin);
            let br = viewport.doc_to_screen((x + w, y + h), origin);
            let r = Rect::from_min_max(tl, br);
            let corner_screen = ((*rx as f32) * viewport.zoom)
                .min(r.width() / 2.0)
                .min(r.height() / 2.0);
            let has_fill = fill.is_visible();
            if let Some(sw) = stroke_w {
                if has_fill {
                    draw_rect_stroke(
                        painter,
                        viewport,
                        origin,
                        r,
                        (*x, *y, x + w, y + h),
                        *rx,
                        stroke_style,
                        opacity,
                        sw,
                        corner_screen,
                        stroke_join,
                        stroke_cap,
                    );
                }
            }
            if has_fill {
                draw_shape_fill(
                    painter,
                    viewport,
                    origin,
                    fill,
                    opacity,
                    r,
                    (*x, *y, x + w, y + h),
                    *rx,
                    corner_screen,
                );
            }
            if let Some(sw) = stroke_w {
                if !has_fill {
                    draw_rect_stroke(
                        painter,
                        viewport,
                        origin,
                        r,
                        (*x, *y, x + w, y + h),
                        *rx,
                        stroke_style,
                        opacity,
                        sw,
                        corner_screen,
                        stroke_join,
                        stroke_cap,
                    );
                }
            }
        }
        NodeKind::Ellipse { cx, cy, rx, ry } => {
            let tl = viewport.doc_to_screen((cx - rx, cy - ry), origin);
            let br = viewport.doc_to_screen((cx + rx, cy + ry), origin);
            let r = Rect::from_min_max(tl, br);
            let center = r.center();
            let radius = r.size() * 0.5;
            let doc_bounds = (cx - rx, cy - ry, cx + rx, cy + ry);
            let bbox_screen = Rect::from_center_size(center, radius * 2.0);
            let stroke_after_fill = stroke_w.filter(|_| {
                !fill.is_visible() || !matches!(fill, Fill::Solid(_))
            });
            if fill.is_visible() {
                match fill {
                    Fill::Solid(p) => {
                        let fc = paint_to_color(*p, opacity);
                        if let Some(sw) = stroke_w {
                            let sc = sample_fill_at(stroke_style, opacity, 0.5, 0.5);
                            painter.add(Shape::Ellipse(EllipseShape {
                                center,
                                radius,
                                fill: fc,
                                stroke: Stroke::new(sw, sc),
                                angle: 0.0,
                            }));
                        } else {
                            painter.add(Shape::ellipse_filled(center, radius, fc));
                        }
                    }
                    _ => {
                        let bez = ellipse_bez_path(*cx, *cy, *rx, *ry);
                        let mesh = clipped_gradient_mesh_from_bez(
                            &bez,
                            viewport,
                            origin,
                            doc_bounds,
                            fill,
                            opacity,
                        );
                        painter.add(Shape::mesh(mesh));
                    }
                }
            }
            if let Some(sw) = stroke_after_fill {
                draw_ellipse_stroke(
                    painter,
                    viewport,
                    origin,
                    center,
                    radius,
                    doc_bounds,
                    stroke_style,
                    opacity,
                    sw,
                    stroke_join,
                );
            }
        }
        NodeKind::Polygon {
            cx,
            cy,
            r: pr,
            sides,
            rotation_rad,
        } => {
            let verts = regular_polygon_vertices(*cx, *cy, *pr, *sides, *rotation_rad);
            let screen: Vec<Pos2> = verts
                .iter()
                .map(|p| viewport.doc_to_screen(*p, origin))
                .collect();
            let bounds = node.bounds();
            let doc_bounds = (bounds.x0, bounds.y0, bounds.x1, bounds.y1);
            let has_fill = fill.is_visible();
            if let Some(sw) = stroke_w {
                if has_fill {
                    draw_stroke_closed_ring(
                        painter,
                        viewport,
                        &screen,
                        &verts,
                        stroke_style,
                        opacity,
                        sw,
                        stroke_join,
                    );
                }
            }
            if has_fill {
                match fill {
                    Fill::Solid(p) => {
                        let fc = paint_to_color(*p, opacity);
                        painter.add(Shape::Path(PathShape::convex_polygon(
                            screen.clone(),
                            fc,
                            Stroke::NONE,
                        )));
                    }
                    _ => {
                        let bez = polygon_bez_path(&verts);
                        let mesh = clipped_gradient_mesh_from_bez(
                            &bez,
                            viewport,
                            origin,
                            doc_bounds,
                            fill,
                            opacity,
                        );
                        painter.add(Shape::mesh(mesh));
                    }
                }
            }
            if let Some(sw) = stroke_w {
                if !has_fill {
                    draw_stroke_closed_ring(
                        painter,
                        viewport,
                        &screen,
                        &verts,
                        stroke_style,
                        opacity,
                        sw,
                        stroke_join,
                    );
                }
            }
            let _ = doc_bounds;
        }
        NodeKind::Path { path } => {
            let bez = path.to_bez();
            let closed = path.is_closed();
            let has_fill = fill.is_visible();

            let draw_path_stroke = |painter: &Painter| {
                let Some(sw) = stroke_w else {
                    return;
                };
                if matches!(stroke_style, Fill::Solid(_)) {
                    let c = sample_fill_at(stroke_style, opacity, 0.5, 0.5);
                    draw_solid_bez_stroke(
                        painter,
                        &bez,
                        viewport,
                        origin,
                        sw,
                        c,
                        stroke_join,
                        stroke_cap,
                        closed,
                    );
                } else {
                    let (screen_pts, doc_pts) =
                        polyline_from_bez(&bez, viewport, origin, closed);
                    if closed && screen_pts.len() >= 3 {
                        draw_stroke_closed_ring(
                            painter,
                            viewport,
                            &screen_pts,
                            &doc_pts,
                            stroke_style,
                            opacity,
                            sw,
                            stroke_join,
                        );
                    } else if screen_pts.len() >= 2 {
                        draw_stroke_open_polyline(
                            painter,
                            viewport,
                            &screen_pts,
                            &doc_pts,
                            stroke_style,
                            opacity,
                            sw,
                            stroke_join,
                            stroke_cap,
                        );
                    }
                }
            };

            // Stroke under fill on closed shapes.
            if closed && has_fill {
                draw_path_stroke(painter);
            }

            if closed && has_fill {
                let bounds = node.bounds();
                let doc_bounds = (bounds.x0, bounds.y0, bounds.x1, bounds.y1);
                let mesh = clipped_gradient_mesh_from_bez(
                    &bez,
                    viewport,
                    origin,
                    doc_bounds,
                    fill,
                    opacity,
                );
                if !mesh.vertices.is_empty() {
                    painter.add(Shape::mesh(mesh));
                }
            }

            // Skip fill contribution from here (we emitted accurate mesh above for solid
            // and the gradient branch for others). Stroke contribution is no-op here.
            let shapes = bez_to_egui_shapes(
                &bez,
                viewport,
                origin,
                None,
                None,
                closed,
            );
            for s in shapes {
                painter.add(s);
            }

            if !has_fill || !closed {
                draw_path_stroke(painter);
            }
        }
        NodeKind::Text { x, y, style } => {
            draw_text_node(
                painter,
                fonts,
                viewport,
                origin,
                *x,
                *y,
                style,
                fill,
                stroke_style,
                stroke_w,
                stroke_join,
                stroke_cap,
                opacity,
                node.get_rotation(),
            );
        }
        NodeKind::Image { x, y, width, height, .. } => {
            if let Some(tex) = image_textures.get(&node.id) {
                let tl = viewport.doc_to_screen((*x, *y), origin);
                let br = viewport.doc_to_screen((*x + *width, *y + *height), origin);
                let rect = Rect::from_min_max(tl, br);
                painter.image(
                    tex.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        NodeKind::Arc { .. } => {
            let bez = node.bez_path();
            let is_closed_fill = matches!(
                &node.kind,
                NodeKind::Arc { join: ArcJoin::Chord | ArcJoin::ToOrigin, .. }
            );
            let has_fill = fill.is_visible();

            if is_closed_fill && has_fill {
                if let Fill::Solid(p) = fill {
                    let c = paint_to_color(*p, opacity);
                    for s in bez_to_fill_shapes(&bez, viewport, origin, c, true) {
                        painter.add(s);
                    }
                } else {
                    let bounds = node.bounds();
                    let docb = (bounds.x0, bounds.y0, bounds.x1, bounds.y1);
                    let mesh = clipped_gradient_mesh_from_bez(&bez, viewport, origin, docb, fill, opacity);
                    if !mesh.vertices.is_empty() {
                        painter.add(Shape::mesh(mesh));
                    }
                }
            }

            // Stroke the (parts of) the arc
            if let Some(sw) = stroke_w {
                if matches!(stroke_style, Fill::Solid(_)) {
                    let c = sample_fill_at(stroke_style, opacity, 0.5, 0.5);
                    draw_solid_bez_stroke(
                        painter,
                        &bez,
                        viewport,
                        origin,
                        sw,
                        c,
                        stroke_join,
                        stroke_cap,
                        false,
                    );
                } else {
                    // fallback simple for gradient stroke on arc
                    let (screen_pts, doc_pts) = polyline_from_bez(&bez, viewport, origin, false);
                    if screen_pts.len() >= 2 {
                        draw_stroke_open_polyline(
                            painter,
                            viewport,
                            &screen_pts,
                            &doc_pts,
                            stroke_style,
                            opacity,
                            sw,
                            stroke_join,
                            stroke_cap,
                        );
                    }
                }
            }
        }
        NodeKind::Group { .. } => {}
        NodeKind::BrushStroke { points } => {
            let color = match fill {
                Fill::Solid(p) => paint_to_color(*p, opacity),
                _ => paint_to_color(Paint::from_hex(0x000000, 1.0), opacity),
            };

            let mut prev_pt: Option<([f64; 2], f32)> = None;
            for &(pos, width) in points {
                if let Some((prev_pos, prev_width)) = prev_pt {
                    let dx = pos[0] - prev_pos[0];
                    let dy = pos[1] - prev_pos[1];
                    let dist = dx.hypot(dy);
                    let step = (1.0 / (viewport.zoom as f64)).max(0.5).min(width as f64 / 8.0).max(0.1);
                    if dist > step {
                        let steps = (dist / step).ceil() as usize;
                        for s in 1..steps {
                            let t = s as f64 / steps as f64;
                            let ix = prev_pos[0] + dx * t;
                            let iy = prev_pos[1] + dy * t;
                            let iw = prev_width + (width - prev_width) * (t as f32);
                            let center = viewport.doc_to_screen((ix, iy), origin);
                            let radius = (iw / 2.0) * viewport.zoom;
                            if radius > 0.0 {
                                painter.circle_filled(center, radius, color);
                            }
                        }
                    }
                }
                let center = viewport.doc_to_screen((pos[0], pos[1]), origin);
                let radius = (width / 2.0) * viewport.zoom;
                if radius > 0.0 {
                    painter.circle_filled(center, radius, color);
                }
                prev_pt = Some((pos, width));
            }

            if selected {
                let stroke_pts: Vec<Pos2> = points
                    .iter()
                    .map(|&(pos, _)| viewport.doc_to_screen((pos[0], pos[1]), origin))
                    .collect();
                if stroke_pts.len() >= 2 {
                    painter.add(Shape::line(
                        stroke_pts,
                        Stroke::new(1.0, Color32::from_rgb(0, 120, 215)),
                    ));
                }
            }
        }
    }

    if selected {
        let bounds = node.bounds();
        let tl = viewport.doc_to_screen((bounds.x0, bounds.y0), origin);
        let br = viewport.doc_to_screen((bounds.x1, bounds.y1), origin);
        let r = Rect::from_min_max(tl, br);
        painter.rect_stroke(
            r.expand(2.0),
            0.0,
            Stroke::new(1.0, colors::SELECTION),
            egui::StrokeKind::Outside,
        );
    }
}

pub fn selection_screen_rect(
    node: &Node,
    nodes: &NodeStore,
    viewport: &Viewport,
    origin: Pos2,
) -> Rect {
    let bounds = node.bounds_with_store(nodes);
    let tl = viewport.doc_to_screen((bounds.x0, bounds.y0), origin);
    let br = viewport.doc_to_screen((bounds.x1, bounds.y1), origin);
    let mut r = Rect::from_min_max(tl, br);
    if r.width() < 16.0 {
        r.min.x -= 8.0;
        r.max.x += 8.0;
    }
    if r.height() < 16.0 {
        r.min.y -= 8.0;
        r.max.y += 8.0;
    }
    r
}

pub fn selection_union_screen_rect(
    nodes: &crate::document::NodeStore,
    selection: &[crate::document::NodeId],
    viewport: &Viewport,
    origin: Pos2,
    tiling_effects: &indexmap::IndexMap<uuid::Uuid, crate::document::TilingEffect>,
    circular_effects: &indexmap::IndexMap<uuid::Uuid, crate::document::CircularCloneEffect>,
) -> Option<Rect> {
    let mut union: Option<kurbo::Rect> = None;
    for id in selection {
        let Some(node) = nodes.get(*id) else { continue };
        let mut b = node.bounds_with_store(nodes);
        if let Some(e) = tiling_effects.values().find(|e| e.source_id == *id) {
            b = b.union(crate::document::compute_tiling_whole_bounds(node, e));
        }
        if let Some(e) = circular_effects.values().find(|e| e.source_id == *id) {
            b = b.union(crate::document::compute_circular_whole_bounds(node, e));
        }
        union = Some(match union {
            None => b,
            Some(u) => u.union(b),
        });
    }
    union.map(|b| {
        let tl = viewport.doc_to_screen((b.x0, b.y0), origin);
        let br = viewport.doc_to_screen((b.x1, b.y1), origin);
        Rect::from_min_max(tl, br)
    })
}

pub fn draw_group_selection_bounds(painter: &Painter, screen_rect: Rect) {
    painter.rect_stroke(
        screen_rect.expand(2.0),
        0.0,
        Stroke::new(1.5, colors::SELECTION),
        egui::StrokeKind::Outside,
    );
}

pub fn draw_transform_handles(painter: &Painter, screen_rect: Rect, rotation_mode: bool) {
    let r = screen_rect;
    let stroke_color = if rotation_mode { colors::POWERLINE_C } else { colors::SELECTION };
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, stroke_color), egui::StrokeKind::Outside);
    
    let positions = handle_positions(r);
    for (i, c) in positions.into_iter().enumerate() {
        let is_corner = i == 0 || i == 2 || i == 4 || i == 6; // Nw, Ne, Se, Sw
        if rotation_mode {
            if is_corner {
                painter.circle_filled(c, 5.0, colors::POWERLINE_C);
                painter.circle_stroke(c, 5.0, Stroke::new(1.0, Color32::WHITE));
            } else {
                painter.circle_filled(c, 3.0, colors::BG_DEEP);
                painter.circle_stroke(c, 3.0, Stroke::new(1.0, colors::BORDER));
            }
        } else {
            painter.circle_filled(c, 5.0, Color32::WHITE);
            painter.circle_stroke(c, 5.0, Stroke::new(1.5, colors::SELECTION));
        }
    }
}

fn handle_positions(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        Pos2::new(r.center().x, r.top()),
        r.right_top(),
        Pos2::new(r.right(), r.center().y),
        r.right_bottom(),
        Pos2::new(r.center().x, r.bottom()),
        r.left_bottom(),
        Pos2::new(r.left(), r.center().y),
    ]
}

pub fn hit_resize_handle(
    screen_rect: Rect,
    pointer: Pos2,
    zoom: f32,
) -> Option<ResizeHandle> {
    let slop = 10.0 / zoom.max(0.1);
    let handles = [
        (ResizeHandle::Nw, screen_rect.left_top()),
        (ResizeHandle::N, Pos2::new(screen_rect.center().x, screen_rect.top())),
        (ResizeHandle::Ne, screen_rect.right_top()),
        (ResizeHandle::E, Pos2::new(screen_rect.right(), screen_rect.center().y)),
        (ResizeHandle::Se, screen_rect.right_bottom()),
        (ResizeHandle::S, Pos2::new(screen_rect.center().x, screen_rect.bottom())),
        (ResizeHandle::Sw, screen_rect.left_bottom()),
        (ResizeHandle::W, Pos2::new(screen_rect.left(), screen_rect.center().y)),
    ];
    for (h, pos) in handles {
        if pointer.distance(pos) <= slop {
            return Some(h);
        }
    }
    None
}

pub fn draw_nodes(
    painter: &Painter,
    nodes: &NodeStore,
    order: &[NodeId],
    viewport: &Viewport,
    origin: Pos2,
    selection: &[NodeId],
    hidden: &HashSet<NodeId>,
    loft_paths: &HashSet<NodeId>,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
) {
    for id in order {
        if hidden.contains(id) {
            continue;
        }
        let Some(raw_node) = nodes.get(*id) else {
            continue;
        };
        let node = if loft_paths.contains(id) {
            let mut n = raw_node.clone();
            if matches!(n.kind, NodeKind::Path { .. }) {
                if !selection.contains(id) {
                    n.style.stroke.width = 0.0;
                }
                n.style.fill = Fill::None; // never fill the path itself for loft to avoid between region shading
            }
            n
        } else {
            raw_node.clone()
        };
        if let NodeKind::Group { children } = &node.kind {
            for cid in children {
                if let Some(child) = nodes.get(*cid) {
                    draw_node(
                        painter,
                        child,
                        viewport,
                        origin,
                        selection.contains(id),
                        fonts,
                        image_textures,
                    );
                }
            }
            continue;
        }
        let sel = selection.contains(id);
        draw_node(painter, &node, viewport, origin, sel, fonts, image_textures);
    }
}

pub fn draw_tiling_effects(
    painter: &Painter,
    nodes: &NodeStore,
    effects: &indexmap::IndexMap<uuid::Uuid, crate::document::TilingEffect>,
    viewport: &Viewport,
    origin: Pos2,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
    selection: &[NodeId],
) {
    use crate::document::{FaceRenderable, node_at_placement};
    for effect in effects.values() {
        let Some(source) = nodes.get(effect.source_id) else { continue; };
        let src_face: &dyn FaceRenderable = source;
        let b = source.bounds();
        let w = b.x1 - b.x0;
        let h = b.y1 - b.y0;
        let first_left = b.x0 + effect.offset_x;
        let first_top = b.y0 + effect.offset_y;
        for ix in 0..effect.count_x {
            for iy in 0..effect.count_y {
                let left = first_left + ix as f64 * effect.gap_x;
                let top = first_top + iy as f64 * effect.gap_y;
                let cx = left + w / 2.0;
                let cy = top + h / 2.0;
                let rot = (ix as f64 * effect.row_rotation + iy as f64 * effect.col_rotation).to_radians();
                let sc = 1.0 + (ix as f64 * effect.row_scale + iy as f64 * effect.col_scale);
                let pl = crate::document::PathPlacement {
                    x: cx,
                    y: cy,
                    angle_rad: rot,
                    scale: sc as f32,
                    opacity_mul: 1.0,
                };
                let inst = node_at_placement(src_face, &pl);
                draw_node(painter, &inst, viewport, origin, false, fonts, image_textures);
            }
        }
        if selection.contains(&effect.source_id) {
            let b = source.bounds();
            let first_x = b.x0 + effect.offset_x;
            let first_y = b.y0 + effect.offset_y;
            let col_end_x = first_x + effect.gap_x;
            let col_end_y = first_y;
            let row_end_x = first_x;
            let row_end_y = first_y + effect.gap_y;
            let p0 = viewport.doc_to_screen((first_x, first_y), origin);
            let p_col = viewport.doc_to_screen((col_end_x, col_end_y), origin);
            let p_row = viewport.doc_to_screen((row_end_x, row_end_y), origin);
            let col = Color32::from_rgb(255, 165, 0);
            painter.line_segment([p0, p_col], Stroke::new(2.0, col));
            painter.line_segment([p0, p_row], Stroke::new(2.0, col));
            painter.circle_filled(p_col, 4.0, Color32::WHITE);
            painter.circle_filled(p_row, 4.0, Color32::WHITE);
        }
    }
}

pub fn draw_circular_effects(
    painter: &Painter,
    nodes: &NodeStore,
    effects: &indexmap::IndexMap<uuid::Uuid, crate::document::CircularCloneEffect>,
    viewport: &Viewport,
    origin: Pos2,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
    selection: &[NodeId],
) {
    use crate::document::{FaceRenderable, node_at_placement};
    for effect in effects.values() {
        let Some(source) = nodes.get(effect.source_id) else { continue; };
        let src_face: &dyn FaceRenderable = source;
        let dx = effect.base_x - effect.origin_x;
        let dy = effect.base_y - effect.origin_y;
        let r = dx.hypot(dy).max(1.0);
        let base_ang = dy.atan2(dx);
        let n = effect.copies.max(3);
        for i in 0..n {
            let ang = base_ang + (i as f64 / n as f64) * std::f64::consts::TAU + effect.angle_offset.to_radians();
            let x = effect.origin_x + r * ang.cos();
            let y = effect.origin_y + r * ang.sin();
            let pl = crate::document::PathPlacement {
                x,
                y,
                angle_rad: ang,
                scale: 1.0,
                opacity_mul: 1.0,
            };
            let inst = node_at_placement(src_face, &pl);
            draw_node(painter, &inst, viewport, origin, false, fonts, image_textures);
        }
        if selection.contains(&effect.source_id) {
            let p0 = viewport.doc_to_screen((effect.base_x, effect.base_y), origin);
            let p1 = viewport.doc_to_screen((effect.origin_x, effect.origin_y), origin);
            // compute p2 as one step
            let dx = effect.base_x - effect.origin_x;
            let dy = effect.base_y - effect.origin_y;
            let r = dx.hypot(dy).max(1.0);
            let base_ang = dy.atan2(dx);
            let ang1 = base_ang + (std::f64::consts::TAU / effect.copies.max(3) as f64) + effect.angle_offset.to_radians();
            let p2x = effect.origin_x + r * ang1.cos();
            let p2y = effect.origin_y + r * ang1.sin();
            let p2 = viewport.doc_to_screen((p2x, p2y), origin);
            let col = Color32::from_rgb(255, 165, 0);
            painter.line_segment([p0, p1], Stroke::new(2.0, col));
            painter.line_segment([p1, p2], Stroke::new(2.0, col));
            painter.circle_filled(p0, 5.0, Color32::WHITE);
            painter.circle_stroke(p0, 5.0, Stroke::new(1.5, col));
            painter.circle_filled(p1, 5.0, Color32::WHITE);
            painter.circle_stroke(p1, 5.0, Stroke::new(1.5, col));
        }
    }
}

pub fn draw_path_effects(
    painter: &Painter,
    nodes: &NodeStore,
    effects: &indexmap::IndexMap<uuid::Uuid, crate::document::ObjectOnPathEffect>,
    viewport: &Viewport,
    origin: Pos2,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
    selection: &[NodeId],
) {
    use crate::document::{
        effect_placements, node_at_placement, Fill, NodeKind, OnPathMode,
    };
    let tol = 0.5 / viewport.zoom as f64;
    for effect in effects.values() {
        let Some(source) = nodes.get(effect.source_id) else {
            continue;
        };
        let Some(path_node) = nodes.get(effect.path_id) else {
            continue;
        };
        let NodeKind::Path { path } = &path_node.kind else {
            continue;
        };
        if effect.mode == OnPathMode::Loft {
            // Very old dense method: plot the source object densely along the path
            // with stroke=0 so fills merge into continuous integral shade.
            // No union/contour in live render (avoids CPU and stale outlines).
            // The "edge" is the natural boundary of the merged shade.
            // Path line shown in edit.
            for placement in effect_placements(effect, path as &dyn PathMagic, tol) {
                let mut instance = node_at_placement(source as &dyn FaceRenderable, &placement);
                instance.style.stroke.width = 0.0;
                draw_node(
                    painter,
                    &instance,
                    viewport,
                    origin,
                    false,
                    fonts,
                    image_textures,
                );
            }

            // Show the original path line on top ONLY in edit mode (when path is selected).
            if selection.contains(&effect.path_id) {
                if let Some(path_node) = nodes.get(effect.path_id) {
                    let mut p = path_node.clone();
                    p.style.fill = Fill::None;
                    draw_node(
                        painter,
                        &p,
                        viewport,
                        origin,
                        false,
                        fonts,
                        image_textures,
                    );
                }
            }
            continue;
        }

        // non-Loft modes
        for placement in effect_placements(effect, path as &dyn PathMagic, tol) {
            let instance = node_at_placement(source as &dyn FaceRenderable, &placement);
            draw_node(
                painter,
                &instance,
                viewport,
                origin,
                false,
                fonts,
                image_textures,
            );
        }
    }
}

pub fn draw_preview_rect(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    let tl = viewport.doc_to_screen((x, y), origin);
    let br = viewport.doc_to_screen((x + w, y + h), origin);
    let r = Rect::from_min_max(tl, br);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.5, Color32::from_rgb(0, 120, 215)),
        egui::StrokeKind::Outside,
    );
    painter.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(0, 120, 215, 40));
}

pub fn draw_marquee_rect(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    a: (f64, f64),
    b: (f64, f64),
) {
    let (x, y, w, h) = crate::tools::normalize_rect(a, b);
    let tl = viewport.doc_to_screen((x, y), origin);
    let br = viewport.doc_to_screen((x + w, y + h), origin);
    let r = Rect::from_min_max(tl, br);
    painter.rect_filled(r, 0.0, colors::ACCENT.gamma_multiply(0.15));
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0, colors::SELECTION),
        egui::StrokeKind::Outside,
    );
}

pub fn draw_preview_ellipse(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
) {
    let tl = viewport.doc_to_screen((cx - rx, cy - ry), origin);
    let br = viewport.doc_to_screen((cx + rx, cy + ry), origin);
    let r = Rect::from_min_max(tl, br);
    painter.add(Shape::Ellipse(EllipseShape {
        center: r.center(),
        radius: r.size() * 0.5,
        fill: Color32::from_rgba_premultiplied(0, 120, 215, 40),
        stroke: Stroke::new(1.5, Color32::from_rgb(0, 120, 215)),
        angle: 0.0,
    }));
}

pub fn draw_preview_polygon(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    cx: f64,
    cy: f64,
    r: f64,
    sides: u32,
) {
    let verts = regular_polygon_vertices(cx, cy, r, sides, 0.0);
    let pts: Vec<Pos2> = verts
        .iter()
        .map(|p| viewport.doc_to_screen(*p, origin))
        .collect();
    painter.add(Shape::closed_line(
        pts,
        Stroke::new(2.0, Color32::from_rgb(0, 120, 215)),
    ));
}

pub fn append_smoothed_points(path: &mut kurbo::BezPath, pts: &[[f64; 2]], smoothness: f32, is_first: bool) {
    if pts.is_empty() {
        return;
    }
    if is_first {
        path.move_to(kurbo::Point::new(pts[0][0], pts[0][1]));
    } else {
        path.line_to(kurbo::Point::new(pts[0][0], pts[0][1]));
    }
    
    let n = pts.len();
    if n < 3 {
        for pt in pts.iter().skip(1) {
            path.line_to(kurbo::Point::new(pt[0], pt[1]));
        }
        return;
    }

    for i in 1..(n - 1) {
        let p_curr = pts[i];
        let p_next = pts[i + 1];
        let mx = (p_curr[0] + p_next[0]) / 2.0;
        let my = (p_curr[1] + p_next[1]) / 2.0;
        
        let end_x = p_curr[0] * (1.0 - smoothness as f64) + mx * smoothness as f64;
        let end_y = p_curr[1] * (1.0 - smoothness as f64) + my * smoothness as f64;
        
        path.quad_to(
            kurbo::Point::new(p_curr[0], p_curr[1]),
            kurbo::Point::new(end_x, end_y),
        );
    }
    path.line_to(kurbo::Point::new(pts[n - 1][0], pts[n - 1][1]));
}

pub fn draw_brush_preview(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    points: &[([f64; 2], f64, f32)],
    stroke_color: Color32,
    smoothness: f32,
    heavy: f32,
    cursor_doc: Option<(f64, f64)>,
    brush_type: crate::tools::BrushType,
) {
    if points.is_empty() {
        return;
    }

    if brush_type == crate::tools::BrushType::Pen {
        let mut prev_pt: Option<([f64; 2], f32)> = None;
        for &(pos, _, width) in points {
            if let Some((prev_pos, prev_width)) = prev_pt {
                let dx = pos[0] - prev_pos[0];
                let dy = pos[1] - prev_pos[1];
                let dist = dx.hypot(dy);
                let step = (1.0 / (viewport.zoom as f64)).max(0.5).min(width as f64 / 8.0).max(0.1);
                if dist > step {
                    let steps = (dist / step).ceil() as usize;
                    for s in 1..steps {
                        let t = s as f64 / steps as f64;
                        let ix = prev_pos[0] + dx * t;
                        let iy = prev_pos[1] + dy * t;
                        let iw = prev_width + (width - prev_width) * (t as f32);
                        let center = viewport.doc_to_screen((ix, iy), origin);
                        let radius = (iw / 2.0) * viewport.zoom;
                        if radius > 0.0 {
                            painter.circle_filled(center, radius, stroke_color);
                        }
                    }
                }
            }
            let center = viewport.doc_to_screen((pos[0], pos[1]), origin);
            let radius = (width / 2.0) * viewport.zoom;
            if radius > 0.0 {
                painter.circle_filled(center, radius, stroke_color);
            }
            prev_pt = Some((pos, width));
        }

        // Draw guide if heavy is active
        if heavy > 0.001 {
            if let Some(cursor) = cursor_doc {
                let cursor_screen = viewport.doc_to_screen(cursor, origin);
                let r_screen = (heavy * 60.0) as f32 * viewport.zoom;
                painter.circle_stroke(
                    cursor_screen,
                    r_screen,
                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 200, 80)),
                );
                if let Some(&(last_pos, _, _)) = points.last() {
                    let last_screen = viewport.doc_to_screen((last_pos[0], last_pos[1]), origin);
                    painter.line_segment(
                        [cursor_screen, last_screen],
                        egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(200, 200, 200, 120)),
                    );
                }
            }
        }
        return;
    }

    if points.len() < 2 {
        return;
    }
    let mut pts = points.to_vec();
    if brush_type != crate::tools::BrushType::Calligraphy {
        pts[0].2 = 0.0;
        if let Some(last) = pts.last_mut() {
            last.2 = 0.0;
        }
    }
    let n = pts.len();
    let mut left_pts = Vec::with_capacity(n);
    let mut right_pts = Vec::with_capacity(n);

    for i in 0..n {
        let (pos, _, w) = pts[i];
        let half_w = (w / 2.0) as f64;

        let normal = if brush_type == crate::tools::BrushType::Calligraphy {
            [0.7071067811865476, 0.7071067811865476]
        } else if i == 0 {
            let next_pos = pts[1].0;
            let dx = next_pos[0] - pos[0];
            let dy = next_pos[1] - pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else if i == n - 1 {
            let prev_pos = pts[n - 2].0;
            let dx = pos[0] - prev_pos[0];
            let dy = pos[1] - prev_pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else {
            let prev_pos = pts[i - 1].0;
            let next_pos = pts[i + 1].0;
            let dx1 = pos[0] - prev_pos[0];
            let dy1 = pos[1] - prev_pos[1];
            let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();

            let dx2 = next_pos[0] - pos[0];
            let dy2 = next_pos[1] - pos[1];
            let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();

            let nx1 = if len1 > 0.0001 { -dy1 / len1 } else { 0.0 };
            let ny1 = if len1 > 0.0001 { dx1 / len1 } else { 1.0 };

            let nx2 = if len2 > 0.0001 { -dy2 / len2 } else { 0.0 };
            let ny2 = if len2 > 0.0001 { dx2 / len2 } else { 1.0 };

            let nx = (nx1 + nx2) / 2.0;
            let ny = (ny1 + ny2) / 2.0;
            let nlen = (nx * nx + ny * ny).sqrt();
            if nlen > 0.0001 {
                [nx / nlen, ny / nlen]
            } else {
                [0.0, 1.0]
            }
        };

        left_pts.push([pos[0] + normal[0] * half_w, pos[1] + normal[1] * half_w]);
        right_pts.push([pos[0] - normal[0] * half_w, pos[1] - normal[1] * half_w]);
    }

    let mut path = kurbo::BezPath::new();
    let mut right_pts_rev = right_pts.clone();
    right_pts_rev.reverse();

    append_smoothed_points(&mut path, &left_pts, smoothness, true);

    if brush_type == crate::tools::BrushType::Pen && n > 0 {
        let end_idx = n - 1;
        let c = pts[end_idx].0;
        let r = (pts[end_idx].2 as f64) / 2.0;
        if r > 0.1 {
            let dx = left_pts[end_idx][0] - c[0];
            let dy = left_pts[end_idx][1] - c[1];
            let start_angle = dy.atan2(dx);
            let sweep = std::f64::consts::PI;
            let arc = kurbo::Arc::new((c[0], c[1]), (r, r), start_angle, sweep, 0.0);
            for el in arc.to_path(0.1).elements().iter().skip(1) {
                path.push(*el);
            }
        }
    }

    append_smoothed_points(&mut path, &right_pts_rev, smoothness, false);

    if brush_type == crate::tools::BrushType::Pen && n > 0 {
        let c = pts[0].0;
        let r = (pts[0].2 as f64) / 2.0;
        if r > 0.1 {
            let dx = right_pts[0][0] - c[0];
            let dy = right_pts[0][1] - c[1];
            let start_angle = dy.atan2(dx);
            let sweep = std::f64::consts::PI;
            let arc = kurbo::Arc::new((c[0], c[1]), (r, r), start_angle, sweep, 0.0);
            for el in arc.to_path(0.1).elements().iter().skip(1) {
                path.push(*el);
            }
        }
    }

    path.close_path();

    let (screen_pts, _) = polyline_from_bez(&path, viewport, origin, true);

    painter.add(Shape::closed_line(
        screen_pts,
        Stroke::new(2.0, stroke_color),
    ));

    // Draw pull-string / joystick guide if heavy is active
    if heavy > 0.001 {
        if let Some(cursor) = cursor_doc {
            let cursor_screen = viewport.doc_to_screen(cursor, origin);
            let r_screen = (heavy * 60.0) as f32 * viewport.zoom;
            
            // Draw stabilizer circle (faint semi-transparent gray)
            painter.circle_stroke(
                cursor_screen,
                r_screen,
                egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 200, 80)),
            );
            
            // Draw pull string line from cursor to last stabilized point
            if let Some(&(last_pos, _, _)) = points.last() {
                let last_screen = viewport.doc_to_screen((last_pos[0], last_pos[1]), origin);
                painter.line_segment(
                    [cursor_screen, last_screen],
                    egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(200, 200, 200, 120)),
                );
            }
        }
    }
}

pub fn draw_preview_line(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    a: (f64, f64),
    b: (f64, f64),
) {
    let pts = [
        viewport.doc_to_screen(a, origin),
        viewport.doc_to_screen(b, origin),
    ];
    painter.add(Shape::line(pts.to_vec(), Stroke::new(2.0, Color32::from_rgb(0, 120, 215))));
    for p in pts {
        painter.circle_filled(p, 4.0, Color32::from_rgb(0, 120, 215));
    }
}

pub fn draw_pen_preview(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    pen: &crate::tools::PenSession,
    cursor_doc: Option<(f64, f64)>,
) {
    if pen.is_empty() {
        return;
    }

    if pen.len() >= 2 {
        let path = pen.to_path_data();
        let bez = path.to_bez();
        for s in bez_to_feathered_stroke_shapes(&bez, viewport, origin, 2.0, Color32::LIGHT_BLUE) {
            painter.add(s);
        }
    }

    for (i, anchor) in pen.anchors.iter().enumerate() {
        let s = viewport.doc_to_screen(*anchor, origin);
        painter.circle_filled(s, 4.0, Color32::LIGHT_BLUE);
        if pen.smooth_anchors.contains(&i) {
            if let Some(off) = pen.handle_out_offset.get(&i) {
                let handle = (anchor.0 + off[0], anchor.1 + off[1]);
                let hs = viewport.doc_to_screen(handle, origin);
                painter.add(Shape::line(
                    vec![s, hs],
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 180, 80, 180)),
                ));
                painter.circle_filled(hs, 3.0, Color32::from_rgb(255, 140, 40));
            }
            if let Some(off) = pen.handle_in_offset.get(&i) {
                let handle = (anchor.0 + off[0], anchor.1 + off[1]);
                let hs = viewport.doc_to_screen(handle, origin);
                painter.add(Shape::line(
                    vec![s, hs],
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 180, 80, 180)),
                ));
                painter.circle_filled(hs, 3.0, Color32::from_rgb(255, 140, 40));
            }
        }
    }

    if pen.curve_adjust.is_none() {
        let anchor = if pen.extend_from_start {
            pen.anchors.first()
        } else {
            pen.anchors.last()
        };
        if let (Some(last), Some(cursor)) = (anchor, cursor_doc) {
            let a = viewport.doc_to_screen(*last, origin);
            let b = viewport.doc_to_screen(cursor, origin);
            painter.add(Shape::line(
                vec![a, b],
                Stroke::new(
                    1.5,
                    Color32::from_rgba_unmultiplied(120, 180, 255, 120),
                ),
            ));
        }
    }
}

fn text_font_id(style: &TextStyle, zoom: f32) -> FontId {
    let size = (style.font_size * zoom).max(6.0);
    FontId::new(
        size,
        FontFamily::Name(style.font_family.as_str().into()),
    )
}

fn draw_text_node(
    painter: &Painter,
    fonts: &crate::fonts::FontRegistry,
    viewport: &Viewport,
    origin: Pos2,
    x: f64,
    y: f64,
    style: &TextStyle,
    fill: &Fill,
    stroke_style: &Fill,
    stroke_w: Option<f32>,
    stroke_join: LineJoin,
    stroke_cap: LineCap,
    opacity: f32,
    rotation_rad: f64,
) {
    if crate::text_glyph::draw_text_glyphs(
        painter,
        fonts,
        viewport,
        origin,
        x,
        y,
        style,
        fill,
        stroke_style,
        stroke_w,
        stroke_join,
        stroke_cap,
        opacity,
        rotation_rad,
    ) {
        return;
    }

    // Fallback when the font file cannot be parsed (fill only).
    if !fill.is_visible() {
        return;
    }
    let pos = viewport.doc_to_screen((x, y), origin);
    let fill_color = sample_fill_at(fill, opacity, 0.5, 0.5);
    let font_id = text_font_id(style, viewport.zoom);
    if style.bold || style.italic {
        let mut job = egui::text::LayoutJob::default();
        let mut fmt = egui::TextFormat::simple(font_id, fill_color);
        fmt.italics = style.italic;
        job.append(&style.content, 0.0, fmt);
        let galley = painter.layout_job(job);
        painter.galley(pos, galley, fill_color);
    } else {
        painter.text(pos, Align2::LEFT_TOP, style.content.as_str(), font_id, fill_color);
    }
}

pub fn draw_node_handles(
    painter: &Painter,
    node: &Node,
    viewport: &Viewport,
    origin: Pos2,
    selected_path_points: &[(NodeId, usize)],
    selected_path_segment: Option<(NodeId, usize, usize)>,
) {
    let selected_on_path: Vec<usize> = selected_path_points
        .iter()
        .filter(|(sid, _)| sid == &node.id)
        .map(|(_, pi)| *pi)
        .collect();
    let segment_endpoints = selected_path_segment
        .filter(|(sid, ..)| sid == &node.id)
        .map(|(_, from, to)| (from, to));

    if let (NodeKind::Path { path }, Some((_, from, to))) =
        (&node.kind, selected_path_segment.filter(|(s, ..)| s == &node.id))
    {
        let anchors = path.anchor_positions();
        if let (Some(&a), Some(&b)) = (anchors.get(from), anchors.get(to)) {
            let sa = viewport.doc_to_screen(a, origin);
            let sb = viewport.doc_to_screen(b, origin);
            painter.line_segment(
                [sa, sb],
                Stroke::new(3.0, Color32::from_rgb(80, 200, 255)),
            );
        }
    }

    if let NodeKind::Path { path } = &node.kind {
        let show_all_handles = selected_on_path.is_empty();
        let handle_indices: Vec<usize> = if show_all_handles {
            path.smooth_anchors.clone()
        } else {
            selected_on_path
                .iter()
                .filter(|pi| path.is_anchor_smooth(**pi))
                .copied()
                .collect()
        };
        for pi in handle_indices {
            if let Some((anchor, ctrl_in, ctrl_out)) = path.bezier_handles_at(pi) {
                let a = viewport.doc_to_screen(anchor, origin);
                if let Some(ci) = ctrl_in {
                    let cin = viewport.doc_to_screen(ci, origin);
                    painter.line_segment(
                        [a, cin],
                        Stroke::new(1.5, Color32::from_rgb(255, 180, 60)),
                    );
                    painter.rect_filled(
                        Rect::from_center_size(cin, Vec2::splat(6.0)),
                        0.0,
                        Color32::from_rgb(255, 180, 60),
                    );
                }
                if let Some(co) = ctrl_out {
                    let cout = viewport.doc_to_screen(co, origin);
                    painter.line_segment(
                        [a, cout],
                        Stroke::new(1.5, Color32::from_rgb(255, 180, 60)),
                    );
                    painter.rect_filled(
                        Rect::from_center_size(cout, Vec2::splat(6.0)),
                        0.0,
                        Color32::from_rgb(255, 180, 60),
                    );
                }
            }
        }
    }

    for (i, p) in node.edit_handles().into_iter().enumerate() {
        let s = viewport.doc_to_screen(p, origin);
        let is_selected = selected_on_path.contains(&i)
            || segment_endpoints
                .map(|(from, to)| i == from || i == to)
                .unwrap_or(false);
        if node.is_text_origin_handle(i) {
            painter.circle_filled(s, 6.0, colors::ACCENT);
            painter.circle_stroke(s, 6.0, Stroke::new(2.0, Color32::WHITE));
            let r = 4.0;
            painter.line_segment(
                [s + Vec2::new(-r, 0.0), s + Vec2::new(r, 0.0)],
                Stroke::new(1.5, Color32::WHITE),
            );
            painter.line_segment(
                [s + Vec2::new(0.0, -r), s + Vec2::new(0.0, r)],
                Stroke::new(1.5, Color32::WHITE),
            );
        } else if node.is_center_edit_handle(i) {
            painter.circle_filled(s, 6.0, colors::ACCENT);
            painter.circle_stroke(s, 6.0, Stroke::new(2.0, Color32::WHITE));
            let r = 4.0;
            painter.line_segment(
                [s + Vec2::new(-r, 0.0), s + Vec2::new(r, 0.0)],
                Stroke::new(1.5, Color32::WHITE),
            );
            painter.line_segment(
                [s + Vec2::new(0.0, -r), s + Vec2::new(0.0, r)],
                Stroke::new(1.5, Color32::WHITE),
            );
        } else {
            let smooth = matches!(
                &node.kind,
                NodeKind::Path { path } if path.is_anchor_smooth(i)
            );
            let radius = if is_selected { 7.0 } else { 5.0 };
            let fill = if is_selected {
                colors::ACCENT
            } else if smooth {
                Color32::from_rgb(255, 180, 60)
            } else {
                Color32::WHITE
            };
            painter.circle_filled(s, radius, fill);
            painter.circle_stroke(
                s,
                radius,
                Stroke::new(1.5, Color32::from_rgb(0, 120, 215)),
            );
        }
    }
}

fn gradient_line_screen(
    r: Rect,
    line: (f32, f32, f32, f32),
) -> (Pos2, Pos2, Pos2) {
    let a = Pos2::new(
        r.left() + r.width() * line.0,
        r.top() + r.height() * line.1,
    );
    let b = Pos2::new(
        r.left() + r.width() * line.2,
        r.top() + r.height() * line.3,
    );
    let mid = Pos2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    (a, b, mid)
}

pub fn draw_gradient_flow_overlay(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    bounds: kurbo::Rect,
    kind: crate::document::FillKind,
    line: (f32, f32, f32, f32),
    radial_cx: f32,
    radial_cy: f32,
) {
    let tl = viewport.doc_to_screen((bounds.x0, bounds.y0), origin);
    let br = viewport.doc_to_screen((bounds.x1, bounds.y1), origin);
    let r = Rect::from_min_max(tl, br);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0, colors::SELECTION),
        egui::StrokeKind::Outside,
    );
    match kind {
        crate::document::FillKind::LinearGradient => {
            let (a, b, mid) = gradient_line_screen(r, line);
            painter.line_segment([a, b], Stroke::new(3.0, colors::ACCENT));
            painter.circle_filled(a, 6.0, Color32::WHITE);
            painter.circle_filled(b, 6.0, Color32::WHITE);
            painter.circle_stroke(a, 6.0, Stroke::new(1.5, colors::ACCENT));
            painter.circle_stroke(b, 6.0, Stroke::new(1.5, colors::ACCENT));
            painter.circle_filled(mid, 6.0, colors::ACCENT);
            painter.circle_stroke(mid, 6.0, Stroke::new(1.5, Color32::WHITE));
        }
        crate::document::FillKind::RadialGradient => {
            let focal = Pos2::new(
                r.left() + r.width() * radial_cx,
                r.top() + r.height() * radial_cy,
            );
            painter.circle_filled(focal, 7.0, colors::ACCENT);
            painter.circle_stroke(focal, 7.0, Stroke::new(2.0, Color32::WHITE));
        }
        crate::document::FillKind::Solid => {}
    }
}

pub fn pick_gradient_flow_handle(
    viewport: &Viewport,
    origin: Pos2,
    bounds: kurbo::Rect,
    kind: crate::document::FillKind,
    line: (f32, f32, f32, f32),
    radial_cx: f32,
    radial_cy: f32,
    screen: Pos2,
    slop: f32,
) -> Option<GradientLineHandle> {
    let tl = viewport.doc_to_screen((bounds.x0, bounds.y0), origin);
    let br = viewport.doc_to_screen((bounds.x1, bounds.y1), origin);
    let r = Rect::from_min_max(tl, br);
    match kind {
        crate::document::FillKind::LinearGradient => {
            let (a, b, mid) = gradient_line_screen(r, line);
            if screen.distance(a) < slop {
                return Some(GradientLineHandle::LinearEnd0);
            }
            if screen.distance(b) < slop {
                return Some(GradientLineHandle::LinearEnd1);
            }
            if screen.distance(mid) < slop {
                return Some(GradientLineHandle::LinearMid);
            }
            None
        }
        crate::document::FillKind::RadialGradient => {
            let focal = Pos2::new(
                r.left() + r.width() * radial_cx,
                r.top() + r.height() * radial_cy,
            );
            if screen.distance(focal) < slop * 1.5 {
                return Some(GradientLineHandle::RadialFocal);
            }
            None
        }
        crate::document::FillKind::Solid => None,
    }
}

pub fn radial_from_bounds_drag(bounds: kurbo::Rect, doc: (f64, f64)) -> (f32, f32) {
    let w = (bounds.x1 - bounds.x0).max(1e-6);
    let h = (bounds.y1 - bounds.y0).max(1e-6);
    (
        ((doc.0 - bounds.x0) / w) as f32,
        ((doc.1 - bounds.y0) / h) as f32,
    )
}

pub fn linear_norm_from_bounds_drag(bounds: kurbo::Rect, doc: (f64, f64)) -> (f32, f32) {
    radial_from_bounds_drag(bounds, doc)
}
pub fn draw_eyedropper_magnifier(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    target_doc: (f64, f64),
    t: f32, // progress 0.0 ..= 1.0
    hovered_color: Color32,
) {
    if t <= 0.001 {
        return;
    }
    let cubic_out = |x: f32| {
        let f = x - 1.0;
        f * f * f + 1.0
    };
    let scale = cubic_out(t);
    let radius = 64.0 * scale;

    let center = viewport.doc_to_screen(target_doc, origin);

    // 1. Draw outer drop shadow / glow
    painter.circle_filled(
        center,
        radius + 6.0,
        Color32::from_black_alpha(40),
    );

    // 2. Draw zoomed-in grid pattern inside the glass
    let glass_bg = Color32::from_black_alpha(180);
    painter.circle_filled(center, radius, glass_bg);

    // Draw grid lines inside the circle
    let r_grid = radius - 3.0; // grid bounds
    let step = 8.0;
    let mut x_offset = -r_grid;
    while x_offset <= r_grid {
        if x_offset.abs() > 0.01 { // skip center line
            let h = (r_grid * r_grid - x_offset * x_offset).sqrt();
            painter.line_segment(
                [
                    Pos2::new(center.x + x_offset, center.y - h),
                    Pos2::new(center.x + x_offset, center.y + h),
                ],
                Stroke::new(1.0, Color32::from_white_alpha(30)),
            );
        }
        x_offset += step;
    }
    let mut y_offset = -r_grid;
    while y_offset <= r_grid {
        if y_offset.abs() > 0.01 { // skip center line
            let w = (r_grid * r_grid - y_offset * y_offset).sqrt();
            painter.line_segment(
                [
                    Pos2::new(center.x - w, center.y + y_offset),
                    Pos2::new(center.x + w, center.y + y_offset),
                ],
                Stroke::new(1.0, Color32::from_white_alpha(30)),
            );
        }
        y_offset += step;
    }

    // 3. Draw central crosshair
    painter.circle_filled(center, 3.0, Color32::WHITE);
    painter.circle_stroke(center, 3.0, Stroke::new(1.0, Color32::BLACK));

    // 4. Draw outer preview ring showing the hovered color
    let ring_thickness = 6.0;
    let ring_radius = radius - ring_thickness / 2.0;
    painter.circle_stroke(
        center,
        ring_radius,
        Stroke::new(ring_thickness, hovered_color),
    );

    // Draw thin white border around the outer edge, and thin dark border inside
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, Color32::WHITE),
    );
    painter.circle_stroke(
        center,
        radius - ring_thickness,
        Stroke::new(1.0, Color32::BLACK),
    );

    // 5. Draw hex color text below the circle
    let font_id = FontId::new(10.0, FontFamily::Monospace);
    let hex_str = format!(
        "#{:02X}{:02X}{:02X}",
        hovered_color.r(),
        hovered_color.g(),
        hovered_color.b()
    );
    let text_pos = Pos2::new(center.x, center.y + radius + 15.0);
    let text_galley = painter.layout_no_wrap(hex_str, font_id, Color32::WHITE);
    let rect_w = text_galley.size().x + 8.0;
    let rect_h = text_galley.size().y + 4.0;
    let text_rect = Rect::from_center_size(text_pos, Vec2::new(rect_w, rect_h));
    
    painter.rect_filled(
        text_rect,
        4.0,
        Color32::from_black_alpha(200),
    );
    painter.rect_stroke(
        text_rect,
        4.0,
        Stroke::new(1.0, Color32::from_white_alpha(50)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        Pos2::new(text_rect.left() + 4.0, text_rect.top() + 2.0),
        text_galley,
        Color32::WHITE,
    );
}

#[cfg(test)]
mod lyon_path_tests {
    use super::*;
    use crate::document::PathData;
    use lyon::tessellation::{
        BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
    };
    use std::collections::HashMap;

    fn screen_polyline_to_lyon_path(pts: &[Pos2], closed: bool) -> Path {
        let mut builder = Path::builder();
        if pts.len() < 2 {
            return builder.build();
        }
        builder.begin(Point::new(pts[0].x, pts[0].y));
        for p in &pts[1..] {
            builder.line_to(Point::new(p.x, p.y));
        }
        if closed {
            builder.close();
        } else {
            builder.end(false);
        }
        builder.build()
    }

    fn assert_stroke_tessellates(bez: &BezPath) {
        let viewport = Viewport::default();
        let path = bez_to_lyon_path(bez, &viewport, Pos2::ZERO);
        let mut tessellator = StrokeTessellator::new();
        let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
        let options = StrokeOptions::default().with_line_width(2.0);
        tessellator
            .tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex<'_, '_>| v.position()),
            )
            .expect("stroke tessellation should not fail");
    }

    #[test]
    fn open_pen_path_stroke() {
        let path = PathData::from_anchor_data(
            &[(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)],
            &[],
            HashMap::new(),
            HashMap::new(),
            false,
        );
        assert_stroke_tessellates(&path.to_bez());
    }

    #[test]
    fn closed_path_stroke_no_duplicate_close() {
        let path = PathData::from_anchor_data(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            &[],
            HashMap::new(),
            HashMap::new(),
            true,
        );
        assert!(path.verbs.contains(&4));
        let bez = path.to_bez();
        let close_count = bez
            .elements()
            .iter()
            .filter(|e| matches!(e, PathEl::ClosePath))
            .count();
        assert_eq!(close_count, 1, "to_bez must emit a single ClosePath");
        assert_stroke_tessellates(&bez);
    }

    #[test]
    fn closed_path_via_set_closed_stroke() {
        let mut path = PathData::from_anchor_data(
            &[(0.0, 0.0), (120.0, 0.0), (60.0, 90.0)],
            &[],
            HashMap::new(),
            HashMap::new(),
            false,
        );
        path.set_closed(true);
        assert_stroke_tessellates(&path.to_bez());
    }

    #[test]
    fn smooth_closed_path_stroke() {
        let path = PathData::from_anchor_data(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            &[1],
            HashMap::new(),
            HashMap::new(),
            true,
        );
        assert_stroke_tessellates(&path.to_bez());
    }

    #[test]
    fn bez_with_consecutive_close_paths() {
        let mut bez = BezPath::new();
        bez.move_to((0.0, 0.0));
        bez.line_to((50.0, 0.0));
        bez.line_to((25.0, 40.0));
        bez.close_path();
        bez.close_path();
        assert_stroke_tessellates(&bez);
    }

    #[test]
    fn screen_polyline_closed_ring() {
        let pts = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(40.0, 0.0),
            Pos2::new(40.0, 30.0),
        ];
        let path = screen_polyline_to_lyon_path(&pts, true);
        let mut tessellator = StrokeTessellator::new();
        let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
        let options = StrokeOptions::default().with_line_width(2.0);
        tessellator
            .tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex<'_, '_>| v.position()),
            )
            .expect("closed polyline stroke should tessellate");
    }
}