use egui::{Align2, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use egui::epaint::{
    CubicBezierShape, EllipseShape, PathShape, PathStroke, QuadraticBezierShape,
};
use kurbo::{BezPath, Ellipse, PathEl, Rect as KurboRect, Shape as KurboShape};
use lyon::math::Point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};

use crate::canvas::Viewport;
use std::collections::HashSet;

use crate::document::{
    ArcJoin, FaceRenderable, Fill, LineCap, LineJoin, MarkerKind, Node, NodeId, NodeKind, NodeStore,
    Paint, PathMagic, PathMarker, StrokePaintOrder, TextStyle, regular_polygon_vertices,
};
use crate::document::Stroke as DocStroke;
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

pub fn draw_page_shadow(painter: &Painter, page: Rect, page_color: Color32) {
    let shadow = page.expand(6.0);
    painter.rect_filled(shadow, 4.0, Color32::from_black_alpha(80));
    painter.rect_filled(page, 0.0, page_color);
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

fn to_lyon_line_join(join: LineJoin) -> lyon::path::LineJoin {
    match join {
        LineJoin::Miter => lyon::path::LineJoin::Miter,
        LineJoin::Round => lyon::path::LineJoin::Round,
        LineJoin::Bevel => lyon::path::LineJoin::Bevel,
    }
}

fn to_lyon_line_cap(cap: LineCap) -> lyon::path::LineCap {
    match cap {
        LineCap::Butt | LineCap::Square => lyon::path::LineCap::Butt,
        LineCap::Round => lyon::path::LineCap::Round,
    }
}

/// Continuous stroke with real miter/round/bevel joins (lyon tessellation).
/// Segment-by-segment egui strokes used to leave double edges at sharp corners.
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
    if width <= 0.0 || color.a() == 0 {
        return;
    }
    if let Some(mesh) =
        stroke_bez_lyon_mesh(bez, viewport, origin, width, color, join, cap, closed)
    {
        painter.add(Shape::mesh(mesh));
        return;
    }
    // Fallback: continuous PathShape polyline (still better joins than per-segment strokes).
    let (screen_pts, _) = polyline_from_bez(bez, viewport, origin, closed);
    draw_feathered_polyline_stroke(painter, &screen_pts, closed, width, color);
    if screen_pts.len() >= 2 {
        if !closed {
            stroke_cap_circles(painter, &screen_pts, width, color, cap);
        }
        if join == LineJoin::Round {
            stroke_join_dots(painter, &screen_pts, width, color, join);
        }
    }
}

fn stroke_bez_lyon_mesh(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    width: f32,
    color: Color32,
    join: LineJoin,
    cap: LineCap,
    closed: bool,
) -> Option<Mesh> {
    let lyon_path = bez_to_lyon_path_for_stroke(bez, viewport, origin, closed);
    let mut tessellator = StrokeTessellator::new();
    let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
    let tolerance = (0.25 / viewport.zoom).clamp(0.04, 0.35);
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_join(to_lyon_line_join(join))
        .with_line_cap(to_lyon_line_cap(cap))
        .with_miter_limit(4.0)
        .with_tolerance(tolerance);
    tessellator
        .tessellate_path(
            &lyon_path,
            &options,
            &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex<'_, '_>| v.position()),
        )
        .ok()?;
    if buffers.indices.is_empty() || buffers.vertices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::default();
    for chunk in buffers.indices.chunks_exact(3) {
        let v0 = buffers.vertices[chunk[0] as usize];
        let v1 = buffers.vertices[chunk[1] as usize];
        let v2 = buffers.vertices[chunk[2] as usize];
        let i0 = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(v0.x, v0.y), color);
        let i1 = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(v1.x, v1.y), color);
        let i2 = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(v2.x, v2.y), color);
        mesh.add_triangle(i0, i1, i2);
    }
    Some(mesh)
}

/// Paint a lyon stroke mesh plus a soft AA fringe via a wider, translucent second pass.
pub fn paint_stroke_mesh_with_aa(painter: &Painter, mesh: Mesh, fringe_color: Color32, fringe_width_boost: f32) {
    // Soft underlay first (slightly expanded visually via thicker alternative path is not available
    // on mesh; approximate with a second mesh draw at reduced alpha when caller passes fringe).
    if fringe_color.a() > 0 && fringe_width_boost > 0.0 {
        let mut soft = mesh.clone();
        for v in &mut soft.vertices {
            v.color = fringe_color;
        }
        painter.add(Shape::mesh(soft));
    }
    painter.add(Shape::mesh(mesh));
}

/// Like `bez_to_lyon_path`, but ensures closed subpaths call `close()` so joins form at the seam.
fn bez_to_lyon_path_for_stroke(
    bez: &BezPath,
    viewport: &Viewport,
    origin: Pos2,
    force_closed: bool,
) -> Path {
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
                builder.cubic_bezier_to(map(p1.x, p1.y), map(p2.x, p2.y), map(p3.x, p3.y));
            }
            PathEl::ClosePath => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        if force_closed {
            builder.close();
        } else {
            builder.end(false);
        }
    }
    builder.build()
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
    // Core stroke (egui PathShape applies edge feathering / AA).
    painter.add(Shape::Path(PathShape {
        points: screen_pts.to_vec(),
        closed,
        fill: Color32::TRANSPARENT,
        stroke: PathStroke::new(width, color),
    }));
    // Extra soft halo (~1px) reduces stairstepping on thin or diagonal lines.
    if width < 8.0 {
        let a = color.a() as f32 / 255.0;
        let soft = Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            ((a * 0.35) * 255.0) as u8,
        );
        painter.add(Shape::Path(PathShape {
            points: screen_pts.to_vec(),
            closed,
            fill: Color32::TRANSPARENT,
            stroke: PathStroke::new((width + 1.25).max(1.5), soft),
        }));
    }
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
        let _pa = (lx0 + (lx1 - lx0) * ta, ly0 + (ly1 - ly0) * ta);
        let _pb = (lx0 + (lx1 - lx0) * tb, ly0 + (ly1 - ly0) * tb);
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
//
// Multi-stop linear/radial gradients cannot rely on plain vertex-color interpolation: a
// triangle spanning t=0..1 would lerp first→last and skip intermediate stop colors. We
// therefore split triangles along iso-parameter lines at each stop position so every
// band only interpolates between two consecutive stops.

#[derive(Clone, Copy)]
struct GradVert {
    p: Pos2,
    /// Gradient parameter (linear projection t, or radial distance parameter).
    t: f32,
}

fn fill_param_at(fill: &Fill, nx: f32, ny: f32) -> f32 {
    match fill {
        Fill::LinearGradient {
            line_x0,
            line_y0,
            line_x1,
            line_y1,
            ..
        } => crate::document::project_onto_linear_line(nx, ny, *line_x0, *line_y0, *line_x1, *line_y1),
        Fill::RadialGradient {
            center_x,
            center_y,
            ..
        } => {
            let dx = nx - center_x;
            let dy = ny - center_y;
            ((dx * dx + dy * dy).sqrt() * 1.25).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

fn gradient_cut_levels(fill: &Fill) -> Vec<f32> {
    let stops = match fill {
        Fill::LinearGradient { stops, .. } | Fill::RadialGradient { stops, .. } => stops.as_slice(),
        _ => return Vec::new(),
    };
    if stops.len() <= 2 {
        return Vec::new();
    }
    let mut levels: Vec<f32> = stops
        .iter()
        .map(|s| s.pos.clamp(0.0, 1.0))
        .filter(|p| *p > 1e-4 && *p < 1.0 - 1e-4)
        .collect();
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    levels.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    levels
}

fn lerp_grad_vert(a: GradVert, b: GradVert, t_cut: f32) -> GradVert {
    let denom = b.t - a.t;
    let u = if denom.abs() < 1e-12 {
        0.5
    } else {
        ((t_cut - a.t) / denom).clamp(0.0, 1.0)
    };
    GradVert {
        p: a.p + (b.p - a.p) * u,
        t: t_cut,
    }
}

/// Split one triangle by the iso-line `t = cut`. Returns 1–3 triangles covering the same area.
fn split_triangle_at_t(v: [GradVert; 3], cut: f32) -> Vec<[GradVert; 3]> {
    let eps = 1e-5;
    let side = |t: f32| -> i8 {
        if t < cut - eps {
            -1
        } else if t > cut + eps {
            1
        } else {
            0
        }
    };
    let s = [side(v[0].t), side(v[1].t), side(v[2].t)];
    // No strict crossing → leave as-is.
    if !((s[0] < 0 || s[1] < 0 || s[2] < 0) && (s[0] > 0 || s[1] > 0 || s[2] > 0)) {
        return vec![v];
    }

    // Count strictly below / above; treat on-plane as neither for isolation pick.
    let mut below: Vec<usize> = Vec::new();
    let mut above: Vec<usize> = Vec::new();
    let mut on: Vec<usize> = Vec::new();
    for i in 0..3 {
        match s[i] {
            -1 => below.push(i),
            1 => above.push(i),
            _ => on.push(i),
        }
    }

    // One vertex alone on one side of the cut; the other two on the opposite side (or on-plane).
    let (alone_idx, alone_side, others) = if below.len() == 1 && above.len() + on.len() == 2 {
        (below[0], -1i8, {
            let mut o = above.clone();
            o.extend(on.iter().copied());
            o
        })
    } else if above.len() == 1 && below.len() + on.len() == 2 {
        (above[0], 1i8, {
            let mut o = below.clone();
            o.extend(on.iter().copied());
            o
        })
    } else if below.len() == 2 && above.len() == 1 {
        (above[0], 1i8, below.clone())
    } else if above.len() == 2 && below.len() == 1 {
        (below[0], -1i8, above.clone())
    } else {
        // Degenerate classification (e.g. two on plane) — skip split.
        return vec![v];
    };
    let _ = alone_side;
    if others.len() != 2 {
        return vec![v];
    }
    let a = v[alone_idx];
    let b = v[others[0]];
    let c = v[others[1]];
    let i_ab = if (a.t - cut).abs() < eps {
        a
    } else if (b.t - cut).abs() < eps {
        b
    } else {
        lerp_grad_vert(a, b, cut)
    };
    let i_ac = if (a.t - cut).abs() < eps {
        a
    } else if (c.t - cut).abs() < eps {
        c
    } else {
        lerp_grad_vert(a, c, cut)
    };
    // Alone side triangle + quad on the other side (two tris).
    vec![[a, i_ab, i_ac], [i_ab, b, c], [i_ab, c, i_ac]]
}

fn subdivide_triangle_for_stops(v: [GradVert; 3], cuts: &[f32]) -> Vec<[GradVert; 3]> {
    let mut tris = vec![v];
    for &cut in cuts {
        let mut next = Vec::with_capacity(tris.len() * 2);
        for tri in tris {
            let tmin = tri[0].t.min(tri[1].t).min(tri[2].t);
            let tmax = tri[0].t.max(tri[1].t).max(tri[2].t);
            if cut > tmin + 1e-5 && cut < tmax - 1e-5 {
                next.extend(split_triangle_at_t(tri, cut));
            } else {
                next.push(tri);
            }
        }
        tris = next;
    }
    tris
}

fn emit_grad_triangle(
    mesh: &mut Mesh,
    fill: &Fill,
    opacity: f32,
    bbox_screen: Rect,
    tri: [GradVert; 3],
) {
    let sample = |gv: GradVert| -> Color32 {
        let (nx, ny) = screen_norm(gv.p, bbox_screen);
        // Prefer exact sample at the gradient parameter when this vertex was placed on a cut.
        // Using geometric (nx,ny) keeps radial/linear consistent with the true field.
        sample_fill_at(fill, opacity, nx, ny)
    };
    let i0 = mesh.vertices.len() as u32;
    mesh.colored_vertex(tri[0].p, sample(tri[0]));
    let i1 = mesh.vertices.len() as u32;
    mesh.colored_vertex(tri[1].p, sample(tri[1]));
    let i2 = mesh.vertices.len() as u32;
    mesh.colored_vertex(tri[2].p, sample(tri[2]));
    mesh.add_triangle(i0, i1, i2);
}

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

    let cuts = gradient_cut_levels(fill);
    let need_subdiv = !cuts.is_empty()
        && matches!(
            fill,
            Fill::LinearGradient { .. } | Fill::RadialGradient { .. }
        );

    for chunk in buffers.indices.chunks_exact(3) {
        let v0 = buffers.vertices[chunk[0] as usize];
        let v1 = buffers.vertices[chunk[1] as usize];
        let v2 = buffers.vertices[chunk[2] as usize];
        let p0 = Pos2::new(v0.x, v0.y);
        let p1 = Pos2::new(v1.x, v1.y);
        let p2 = Pos2::new(v2.x, v2.y);

        if !need_subdiv {
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
            continue;
        }

        let (nx0, ny0) = screen_norm(p0, bbox_screen);
        let (nx1, ny1) = screen_norm(p1, bbox_screen);
        let (nx2, ny2) = screen_norm(p2, bbox_screen);
        let tri = [
            GradVert {
                p: p0,
                t: fill_param_at(fill, nx0, ny0),
            },
            GradVert {
                p: p1,
                t: fill_param_at(fill, nx1, ny1),
            },
            GradVert {
                p: p2,
                t: fill_param_at(fill, nx2, ny2),
            },
        ];
        for sub in subdivide_triangle_for_stops(tri, &cuts) {
            emit_grad_triangle(mesh, fill, opacity, bbox_screen, sub);
        }
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
            // Sharp axis-aligned rect + linear multi-stop: explicit iso-t bands (accurate stops).
            if rx_doc <= 0.0 {
                if matches!(fill, Fill::LinearGradient { .. }) {
                    let mesh = rect_gradient_mesh(screen_rect, doc_bounds, fill, opacity);
                    painter.add(Shape::mesh(mesh));
                    return;
                }
            }
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
    // Soft outer stroke first (underlay) for extra AA on diagonals.
    let soft_a = (color.a() as f32 / 255.0 * 0.32 * 255.0) as u8;
    let soft = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), soft_a);
    let soft_w = (width + 1.15).max(width * 1.15);
    let path_stroke_soft = PathStroke::new(soft_w, soft);
    let line_stroke_soft = Stroke::new(soft_w, soft);
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
                        shapes.push(Shape::line_segment([from, to], line_stroke_soft));
                        shapes.push(Shape::line_segment([from, to], line_stroke));
                    }
                }
                pen = Some(to);
            }
            PathEl::QuadTo(p1, p2) => {
                let from = pen.unwrap_or_else(|| map(p1.x, p1.y));
                let pts = [from, map(p1.x, p1.y), map(p2.x, p2.y)];
                shapes.push(Shape::QuadraticBezier(
                    QuadraticBezierShape::from_points_stroke(
                        pts,
                        false,
                        Color32::TRANSPARENT,
                        path_stroke_soft.clone(),
                    ),
                ));
                shapes.push(Shape::QuadraticBezier(
                    QuadraticBezierShape::from_points_stroke(
                        pts,
                        false,
                        Color32::TRANSPARENT,
                        path_stroke.clone(),
                    ),
                ));
                pen = Some(map(p2.x, p2.y));
            }
            PathEl::CurveTo(p1, p2, p3) => {
                let from = pen.unwrap_or_else(|| map(p1.x, p1.y));
                let pts = [from, map(p1.x, p1.y), map(p2.x, p2.y), map(p3.x, p3.y)];
                shapes.push(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    pts,
                    false,
                    Color32::TRANSPARENT,
                    path_stroke_soft.clone(),
                )));
                shapes.push(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    pts,
                    false,
                    Color32::TRANSPARENT,
                    path_stroke.clone(),
                )));
                pen = Some(map(p3.x, p3.y));
            }
            PathEl::ClosePath => {
                if let Some(from) = pen {
                    if from.distance(subpath_start) > 1e-4 {
                        shapes.push(Shape::line_segment([from, subpath_start], line_stroke_soft));
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
    let stroke_order = node.style.stroke.paint_order;
    let stroke_w = stroke_width(node, viewport);
    let stroke_behind = matches!(stroke_order, StrokePaintOrder::BehindFill);

    match &node.kind {
        NodeKind::Rect { x, y, w, h, rx } => {
            let tl = viewport.doc_to_screen((*x, *y), origin);
            let br = viewport.doc_to_screen((x + w, y + h), origin);
            let r = Rect::from_min_max(tl, br);
            let corner_screen = ((*rx as f32) * viewport.zoom)
                .min(r.width() / 2.0)
                .min(r.height() / 2.0);
            let has_fill = fill.is_visible();
            let draw_r_stroke = |painter: &Painter, sw: f32| {
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
            };
            if let Some(sw) = stroke_w {
                if has_fill && stroke_behind {
                    draw_r_stroke(painter, sw);
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
                if !has_fill || !stroke_behind {
                    draw_r_stroke(painter, sw);
                }
            }
        }
        NodeKind::Plotter {
            x,
            y,
            w,
            h,
            plot_stroke_width,
            plot_stroke_rgba,
            ..
        } => {
            let tl = viewport.doc_to_screen((*x, *y), origin);
            let br = viewport.doc_to_screen((x + w, y + h), origin);
            let r = Rect::from_min_max(tl, br);
            let has_fill = fill.is_visible();
            let draw_r_stroke = |painter: &Painter, sw: f32| {
                draw_rect_stroke(
                    painter,
                    viewport,
                    origin,
                    r,
                    (*x, *y, x + w, y + h),
                    0.0,
                    stroke_style,
                    opacity,
                    sw,
                    0.0,
                    stroke_join,
                    stroke_cap,
                );
            };
            if let Some(sw) = stroke_w {
                if has_fill && stroke_behind {
                    draw_r_stroke(painter, sw);
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
                    0.0,
                    0.0,
                );
            }
            if let Some(sw) = stroke_w {
                if !has_fill || !stroke_behind {
                    draw_r_stroke(painter, sw);
                }
            }
            // Plot curve clipped to region
            if let Some((pts, _, _)) = node.plotter_polyline() {
                if pts.len() >= 2 && *plot_stroke_width > 0.05 {
                    let mut screen_pts: Vec<Pos2> = pts
                        .iter()
                        .map(|&(px, py)| viewport.doc_to_screen((px, py), origin))
                        .collect();
                    // Clip soft: only draw segments that overlap the rect expanded a bit
                    let clip = r.expand(2.0);
                    let col = Color32::from_rgba_unmultiplied(
                        (plot_stroke_rgba[0] * 255.0).clamp(0.0, 255.0) as u8,
                        (plot_stroke_rgba[1] * 255.0).clamp(0.0, 255.0) as u8,
                        (plot_stroke_rgba[2] * 255.0).clamp(0.0, 255.0) as u8,
                        ((plot_stroke_rgba[3] * opacity).clamp(0.0, 1.0) * 255.0) as u8,
                    );
                    let sw = (*plot_stroke_width * viewport.zoom).max(0.5);
                    // Drop points far outside (keeps auto-range overflow from painting whole canvas)
                    screen_pts.retain(|p| {
                        p.x >= clip.left() - r.width()
                            && p.x <= clip.right() + r.width()
                            && p.y >= clip.top() - r.height()
                            && p.y <= clip.bottom() + r.height()
                    });
                    if screen_pts.len() >= 2 {
                        painter.add(egui::Shape::line(
                            screen_pts,
                            egui::Stroke::new(sw, col),
                        ));
                    }
                    // Region outline when no object stroke (subtle)
                    if stroke_w.is_none() {
                        painter.rect_stroke(
                            r,
                            0.0,
                            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
                            egui::StrokeKind::Inside,
                        );
                    }
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
            let has_fill = fill.is_visible();
            let draw_e_stroke = |painter: &Painter, sw: f32| {
                // Always draw stroke as separate mesh so paint-order is respected
                // (combined EllipseShape always puts stroke on top).
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
            };
            if let Some(sw) = stroke_w {
                if has_fill && stroke_behind {
                    draw_e_stroke(painter, sw);
                }
            }
            if has_fill {
                match fill {
                    Fill::Solid(p) => {
                        let fc = paint_to_color(*p, opacity);
                        painter.add(Shape::ellipse_filled(center, radius, fc));
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
            if let Some(sw) = stroke_w {
                if !has_fill || !stroke_behind {
                    draw_e_stroke(painter, sw);
                }
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
            let draw_p_stroke = |painter: &Painter, sw: f32| {
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
            };
            if let Some(sw) = stroke_w {
                if has_fill && stroke_behind {
                    draw_p_stroke(painter, sw);
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
                if !has_fill || !stroke_behind {
                    draw_p_stroke(painter, sw);
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

            // Stroke behind fill on closed shapes when paint_order is BehindFill.
            if closed && has_fill && stroke_behind {
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

            // Stroke above fill, or open / unfilled paths.
            if !closed || !has_fill || !stroke_behind {
                draw_path_stroke(painter);
            }

            // Draw start/mid/end point icons (arrows, rings etc) for Path (pen) geometry
            draw_path_markers(painter, viewport, origin, &bez, closed, &node.style.stroke);
        }
        NodeKind::FlowchartNode { cx, cy, w, h, corner_rx, .. } => {
            let x = cx - w / 2.0;
            let y = cy - h / 2.0;
            let rx = *corner_rx;
            let tl = viewport.doc_to_screen((x, y), origin);
            let br = viewport.doc_to_screen((x + w, y + h), origin);
            let r = Rect::from_min_max(tl, br);
            let corner_screen = ((rx as f32) * viewport.zoom)
                .min(r.width() / 2.0)
                .min(r.height() / 2.0);
            let has_fill = fill.is_visible();
            let c_stroke = sample_fill_at(stroke_style, opacity, 0.5, 0.5);
            let c_fill = sample_fill_at(fill, opacity, 0.5, 0.5);
            // Use kurbo for rounded rect
            let kurbo_r = kurbo::Rect::new(x, y, x + w, y + h);
            if has_fill {
                let egui_c = egui::Color32::from(c_fill);
                if corner_screen > 0.1 {
                    painter.rect_filled(r, corner_screen, egui_c);
                } else {
                    painter.rect_filled(r, 0.0, egui_c);
                }
            }
            if let Some(sw) = stroke_w {
                let egui_c = egui::Color32::from(c_stroke);
                if corner_screen > 0.1 {
                    painter.rect_stroke(r, corner_screen, egui::Stroke::new(sw, egui_c), egui::StrokeKind::Middle);
                } else {
                    painter.rect_stroke(r, 0.0, egui::Stroke::new(sw, egui_c), egui::StrokeKind::Middle);
                }
            }
            if let crate::document::NodeKind::FlowchartNode {
                label,
                label_font_size,
                label_align,
                label_font_family,
                label_bold,
                label_italic,
                ..
            } = &node.kind
            {
                if !label.is_empty() {
                    let size = (*label_font_size as f32 * viewport.zoom).max(6.0);
                    let family = egui::FontFamily::Name(label_font_family.as_str().into());
                    let font_id = egui::FontId::new(size, family);
                    let align2 = match label_align {
                        crate::document::TextAlign::Left => egui::Align2::LEFT_CENTER,
                        crate::document::TextAlign::Center => egui::Align2::CENTER_CENTER,
                        crate::document::TextAlign::Right => egui::Align2::RIGHT_CENTER,
                    };
                    let text_color = c_stroke; // use stroke color for label visibility on fill

                    if *label_bold || *label_italic {
                        let mut job = egui::text::LayoutJob::default();
                        let mut fmt = egui::TextFormat::simple(font_id, text_color);
                        fmt.italics = *label_italic;
                        // Bold relies on the font family registration / fallback in egui context
                        job.append(label, 0.0, fmt);
                        let galley = painter.layout_job(job);
                        let galley_rect = egui::Rect::from_center_size(r.center(), galley.size());
                        let pos = align2.align_size_within_rect(galley.size(), galley_rect).min;
                        painter.galley(pos, galley, text_color);
                    } else {
                        painter.text(r.center(), align2, label, font_id, text_color);
                    }
                }
            }
        }
        NodeKind::FlowchartPath { path: fp } => {
            if fp.points.len() < 2 { return; }
            let bez = crate::document::flowchart::rounded_orthogonal_bez(&fp.points, fp.corner_radius);
            let c = sample_fill_at(stroke_style, opacity, 0.5, 0.5);
            if let Some(sw) = stroke_w {
                draw_solid_bez_stroke(painter, &bez, viewport, origin, sw, c, stroke_join, stroke_cap, false);
            }
            let ms = fp.endpoint_marker_size as f32;
            if ms > 0.0 {
                // Markers are doc-independent screen px size; convert ends to screen space
                let start_screen = viewport.doc_to_screen(fp.points[0], origin);
                let last = *fp.points.last().unwrap_or(&fp.points[0]);
                let end_screen = viewport.doc_to_screen(last, origin);
                let size = egui::vec2(ms, ms);

                // from (start): hollow square (stroke only) to indicate origin
                {
                    let r = egui::Rect::from_center_size(start_screen, size);
                    painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, c), egui::StrokeKind::Middle);
                }
                // to (end): solid filled square to indicate target
                {
                    let r = egui::Rect::from_center_size(end_screen, size);
                    painter.rect_filled(r, 0.0, c);
                }
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

            let draw_arc_stroke = |painter: &Painter| {
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
            };

            if is_closed_fill && has_fill && stroke_behind {
                draw_arc_stroke(painter);
            }

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

            if !is_closed_fill || !has_fill || !stroke_behind {
                draw_arc_stroke(painter);
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
    clip_masks: &indexmap::IndexMap<uuid::Uuid, crate::document::ClipMaskEffect>,
) -> Option<Rect> {
    // Clip unit (image + mask): selection box is the **mask solid-face bounds** only,
    // not the full reference image.
    if selection.len() == 2 {
        if let Some(cm) = clip_masks.values().find(|cm| {
            (selection[0] == cm.source_id && selection[1] == cm.mask_id)
                || (selection[0] == cm.mask_id && selection[1] == cm.source_id)
        }) {
            if let Some(mask) = nodes.get(cm.mask_id) {
                let b = mask.bounds_with_store(nodes);
                let tl = viewport.doc_to_screen((b.x0, b.y0), origin);
                let br = viewport.doc_to_screen((b.x1, b.y1), origin);
                return Some(Rect::from_min_max(tl, br));
            }
        }
    }

    let mut union: Option<kurbo::Rect> = None;
    for id in selection {
        let Some(node) = nodes.get(*id) else { continue };
        // Single ghost image under an active clip: still show mask bounds if it's the source.
        if selection.len() == 1 {
            if let Some(cm) = clip_masks.values().find(|cm| cm.source_id == *id) {
                if let Some(mask) = nodes.get(cm.mask_id) {
                    let b = mask.bounds_with_store(nodes);
                    let tl = viewport.doc_to_screen((b.x0, b.y0), origin);
                    let br = viewport.doc_to_screen((b.x1, b.y1), origin);
                    return Some(Rect::from_min_max(tl, br));
                }
            }
        }
        let mut b = node.bounds_with_store(nodes);
        if let Some(e) = tiling_effects.values().find(|e| e.source_id == *id) {
            let whole = crate::document::compute_tiling_whole_bounds(node, e);
            b = if e.hide_source {
                whole
            } else {
                b.union(whole)
            };
        }
        if let Some(e) = circular_effects.values().find(|e| e.source_id == *id) {
            let whole = crate::document::compute_circular_whole_bounds(node, e);
            // Don't keep the hidden source's old bbox edge glued to the selection box.
            b = if e.hide_source {
                whole
            } else {
                b.union(whole)
            };
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
    page_w: f32,
    page_h: f32,
    selection: &[NodeId],
    hidden: &HashSet<NodeId>,
    loft_paths: &HashSet<NodeId>,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
) {
    if crate::blend::document_needs_blend_composite(nodes, order, hidden) {
        draw_nodes_with_blend(
            painter,
            nodes,
            order,
            viewport,
            origin,
            page_w,
            page_h,
            selection,
            hidden,
            loft_paths,
            fonts,
            image_textures,
        );
        return;
    }
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

        // Groups use child union bounds (plain bounds() is ZERO → false top-left handle).
        let b = node.bounds_with_store(nodes);
        if b.width() > 1e-9 || b.height() > 1e-9 {
            let tl = viewport.doc_to_screen((b.x0, b.y0), origin);
            let br = viewport.doc_to_screen((b.x1, b.y1), origin);
            let nr = egui::Rect::from_min_max(
                egui::pos2(tl.x as f32, tl.y as f32),
                egui::pos2(br.x as f32, br.y as f32),
            );
            if !painter.clip_rect().intersects(nr) {
                continue;
            }
        }

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

/// Fixed pixel budget for blend ROIs — independent of zoom (high zoom just upscales).
const BLEND_ROI_MAX_EDGE: u32 = 192;

/// Cached CPU blend ROI texture (rebuild when content/zoom changes, not every pan).
#[derive(Clone)]
struct BlendRoiCache {
    key: u64,
    tex: egui::TextureHandle,
}

fn blend_content_key(zoom: f32, blend_id: NodeId, under: &[(NodeId, &Node)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Coarse zoom buckets: small zoom tweaks don't thrash the cache.
    ((zoom * 8.0).round() as i32).hash(&mut h);
    blend_id.hash(&mut h);
    for (id, n) in under {
        id.hash(&mut h);
        let b = n.bounds();
        ((b.x0 * 50.0).round() as i64).hash(&mut h);
        ((b.y0 * 50.0).round() as i64).hash(&mut h);
        ((b.x1 * 50.0).round() as i64).hash(&mut h);
        ((b.y1 * 50.0).round() as i64).hash(&mut h);
        n.style.blend_mode.label().hash(&mut h);
        ((n.style.opacity * 255.0) as u8).hash(&mut h);
        match &n.style.fill {
            Fill::Solid(p) => {
                for c in p.rgba {
                    ((c * 255.0) as u8).hash(&mut h);
                }
            }
            Fill::None => 0u8.hash(&mut h),
            _ => 1u8.hash(&mut h),
        }
        ((n.style.stroke.width * 10.0) as i32).hash(&mut h);
        if let NodeKind::Image { bytes, width, height, .. } = &n.kind {
            bytes.len().hash(&mut h);
            ((*width * 10.0) as i64).hash(&mut h);
            ((*height * 10.0) as i64).hash(&mut h);
        }
    }
    h.finish()
}

fn draw_nodes_with_blend(
    painter: &Painter,
    nodes: &NodeStore,
    order: &[NodeId],
    viewport: &Viewport,
    origin: Pos2,
    page_w: f32,
    page_h: f32,
    selection: &[NodeId],
    hidden: &HashSet<NodeId>,
    loft_paths: &HashSet<NodeId>,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
) {
    let _ = (page_w, page_h);
    let vis = painter.clip_rect();
    if vis.width() < 1.0 || vis.height() < 1.0 {
        return;
    }

    // Paint list of ids only (avoid cloning image bytes every frame).
    let mut paint_ids: Vec<(NodeId, bool)> = Vec::with_capacity(order.len());
    for id in order {
        if hidden.contains(id) {
            continue;
        }
        let Some(raw_node) = nodes.get(*id) else {
            continue;
        };
        let sel = selection.contains(id);
        if let NodeKind::Group { children } = &raw_node.kind {
            for cid in children {
                if nodes.get(*cid).is_some() {
                    paint_ids.push((*cid, sel));
                }
            }
            continue;
        }
        paint_ids.push((*id, sel));
    }

    // Loft style override (rare) — only clone those paths.
    let loft_override = |id: NodeId, n: &Node| -> Option<Node> {
        if !loft_paths.contains(&id) {
            return None;
        }
        if !matches!(n.kind, NodeKind::Path { .. }) {
            return None;
        }
        let mut c = n.clone();
        if !selection.contains(&id) {
            c.style.stroke.width = 0.0;
        }
        c.style.fill = Fill::None;
        Some(c)
    };

    for i in 0..paint_ids.len() {
        let (id, sel) = paint_ids[i];
        let Some(raw) = nodes.get(id) else {
            continue;
        };
        let lofted = loft_override(id, raw);
        let node = lofted.as_ref().unwrap_or(raw);

        if node.style.blend_mode == crate::document::BlendMode::Normal {
            draw_node(painter, node, viewport, origin, sel, fonts, image_textures);
            continue;
        }

        let b = node.bounds();
        if b.width() < 0.5 || b.height() < 0.5 {
            continue;
        }
        let stroke_pad = (node.style.stroke.width.max(0.0) * 0.5) as f64;
        let ntl = viewport.doc_to_screen((b.x0 - stroke_pad, b.y0 - stroke_pad), origin);
        let nbr = viewport.doc_to_screen((b.x1 + stroke_pad, b.y1 + stroke_pad), origin);
        let node_screen = Rect::from_min_max(ntl, nbr).expand(1.0);
        let roi = node_screen.intersect(vis);
        if roi.width() < 1.0 || roi.height() < 1.0 {
            continue;
        }

        // Cache key from store refs (pan-independent doc bounds + coarse zoom).
        let mut under_refs: Vec<(NodeId, &Node)> = Vec::with_capacity(i + 1);
        for j in 0..=i {
            let (uid, _) = paint_ids[j];
            if let Some(un) = nodes.get(uid) {
                under_refs.push((uid, un));
            }
        }
        let key = blend_content_key(viewport.zoom, id, &under_refs);

        let cache_id = egui::Id::new("blend_roi_v3").with(id);
        let cached_tex = painter.ctx().data(|d| {
            d.get_temp::<BlendRoiCache>(cache_id)
                .filter(|e| e.key == key)
                .map(|e| e.tex.clone())
        });

        let tex = if let Some(t) = cached_tex {
            t
        } else {
            // Always build a tiny buffer (≤192px edge) — zoom only changes display scale.
            let full_w = roi.width().max(1.0);
            let full_h = roi.height().max(1.0);
            let down = (BLEND_ROI_MAX_EDGE as f32 / full_w.max(full_h)).min(1.0);
            let rw = (full_w * down).round().max(1.0) as u32;
            let rh = (full_h * down).round().max(1.0) as u32;
            let sx = rw as f32 / full_w;
            let sy = rh as f32 / full_h;

            let mut layer = vec![255u8; (rw * rh * 4) as usize];
            for j in 0..=i {
                let (uid, _) = paint_ids[j];
                let Some(un) = nodes.get(uid) else {
                    continue;
                };
                let lofted_u = loft_override(uid, un);
                let under_n = lofted_u.as_ref().unwrap_or(un);
                if matches!(under_n.kind, NodeKind::Group { .. }) {
                    continue;
                }
                stamp_node_into_blend_roi(
                    &mut layer,
                    rw,
                    rh,
                    roi,
                    sx,
                    sy,
                    under_n,
                    nodes,
                    viewport,
                    origin,
                );
            }

            let image =
                egui::ColorImage::from_rgba_premultiplied([rw as usize, rh as usize], &layer);
            let tex = painter.ctx().load_texture(
                format!("blend_roi_{id}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            painter.ctx().data_mut(|d| {
                d.insert_temp(
                    cache_id,
                    BlendRoiCache {
                        key,
                        tex: tex.clone(),
                    },
                );
            });
            tex
        };

        painter.image(
            tex.id(),
            roi,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Stamp a node into an ROI buffer at exact pixel size of the ROI∩node intersection
/// (prevents white cuts / vertical squeeze from max-edge caps).
fn stamp_node_into_blend_roi(
    layer: &mut [u8],
    rw: u32,
    rh: u32,
    roi: Rect,
    sx: f32,
    sy: f32,
    node: &Node,
    nodes: &NodeStore,
    viewport: &Viewport,
    origin: Pos2,
) {
    let b = node.bounds();
    if b.width() < 0.5 || b.height() < 0.5 {
        return;
    }
    let utl = viewport.doc_to_screen((b.x0, b.y0), origin);
    let ubr = viewport.doc_to_screen((b.x1, b.y1), origin);
    let under_screen = Rect::from_min_max(utl, ubr);
    let hit = under_screen.intersect(roi);
    if hit.width() < 0.5 || hit.height() < 0.5 {
        return;
    }

    // Destination pixel rect inside ROI buffer (uniform mapping).
    let ox = ((hit.left() - roi.left()) * sx).floor() as i32;
    let oy = ((hit.top() - roi.top()) * sy).floor() as i32;
    let nw = ((hit.width() * sx).ceil() as u32).max(1).min(rw);
    let nh = ((hit.height() * sy).ceil() as u32).max(1).min(rh);

    // UV of hit within the node's full screen rect (for cropping images / SVG).
    let u0 = ((hit.left() - under_screen.left()) / under_screen.width()).clamp(0.0, 1.0);
    let v0 = ((hit.top() - under_screen.top()) / under_screen.height()).clamp(0.0, 1.0);
    let u1 = ((hit.right() - under_screen.left()) / under_screen.width()).clamp(0.0, 1.0);
    let v1 = ((hit.bottom() - under_screen.top()) / under_screen.height()).clamp(0.0, 1.0);

    let Some(rgba) = rasterize_node_region(node, nodes, u0, v0, u1, v1, nw, nh) else {
        return;
    };
    crate::blend::composite_stamp(
        layer,
        rw,
        rh,
        &rgba,
        nw,
        nh,
        ox,
        oy,
        node.style.blend_mode,
        node.style.opacity,
    );
}

/// Rasterize a sub-rectangle of a node to exactly `out_w`×`out_h` (no aspect distortion).
fn rasterize_node_region(
    node: &Node,
    nodes: &NodeStore,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    out_w: u32,
    out_h: u32,
) -> Option<Vec<u8>> {
    if out_w == 0 || out_h == 0 {
        return None;
    }
    let u0 = u0.clamp(0.0, 1.0);
    let v0 = v0.clamp(0.0, 1.0);
    let u1 = u1.clamp(u0 + 1e-5, 1.0);
    let v1 = v1.clamp(v0 + 1e-5, 1.0);

    match &node.kind {
        NodeKind::Image { bytes, .. } if !bytes.is_empty() => {
            let img = decode_image_cached(bytes)?;
            let iw = img.width() as f32;
            let ih = img.height() as f32;
            let x0 = (u0 * iw).floor().max(0.0) as u32;
            let y0 = (v0 * ih).floor().max(0.0) as u32;
            let x1 = (u1 * iw).ceil().min(iw) as u32;
            let y1 = (v1 * ih).ceil().min(ih) as u32;
            if x1 <= x0 || y1 <= y0 {
                return None;
            }
            let crop = image::imageops::crop_imm(&img, x0, y0, x1 - x0, y1 - y0).to_image();
            let resized = if crop.width() == out_w && crop.height() == out_h {
                crop
            } else {
                image::imageops::resize(
                    &crop,
                    out_w,
                    out_h,
                    image::imageops::FilterType::Triangle,
                )
            };
            Some(resized.into_raw())
        }
        NodeKind::Rect { rx, .. }
            if *rx <= 0.5 && matches!(node.style.fill, Fill::Solid(_) | Fill::None) =>
        {
            // Solid fill for the hit region (stroke approximated when full node is covered).
            let mut rgba = vec![0u8; (out_w * out_h * 4) as usize];
            if let Fill::Solid(p) = &node.style.fill {
                let a = (p.rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8;
                let r = (p.rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                let g = (p.rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                let bch = (p.rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                for px in rgba.chunks_exact_mut(4) {
                    px[0] = r;
                    px[1] = g;
                    px[2] = bch;
                    px[3] = a;
                }
            }
            // Stroke only when the region includes the shape edge (near u/v border).
            let near_edge = u0 < 0.02 || v0 < 0.02 || u1 > 0.98 || v1 > 0.98;
            if near_edge
                && node.style.stroke.width > 0.01
                && node.style.stroke.style.is_visible()
            {
                if let Fill::Solid(sp) = &node.style.stroke.style {
                    let sa = (sp.rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let sr = (sp.rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let sg = (sp.rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let sb = (sp.rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                    let t = ((out_w.min(out_h) as f32) * 0.04).ceil().max(1.0) as u32;
                    let t = t.min(out_w / 2).min(out_h / 2).max(1);
                    for y in 0..out_h {
                        for x in 0..out_w {
                            // Map pixel back to node UV; paint stroke if near geometric edge.
                            let pu = u0 + (u1 - u0) * (x as f32 + 0.5) / out_w as f32;
                            let pv = v0 + (v1 - v0) * (y as f32 + 0.5) / out_h as f32;
                            let edge = pu < 0.02
                                || pv < 0.02
                                || pu > 0.98
                                || pv > 0.98
                                || x < t
                                || y < t
                                || x >= out_w - t
                                || y >= out_h - t;
                            if edge && (pu < 0.04 || pv < 0.04 || pu > 0.96 || pv > 0.96) {
                                let i = ((y * out_w + x) * 4) as usize;
                                rgba[i] = sr;
                                rgba[i + 1] = sg;
                                rgba[i + 2] = sb;
                                rgba[i + 3] = sa;
                            }
                        }
                    }
                }
            }
            Some(rgba)
        }
        _ => {
            // SVG fallback: rasterize full node then crop+resize to exact out size.
            let b = node.bounds();
            let svg = crate::io::node_svg_for_bounds(node, b, nodes);
            // Aim for a source large enough for the crop.
            let src_w = ((out_w as f32) / (u1 - u0).max(0.05)).ceil().min(1024.0) as u32;
            let src_h = ((out_h as f32) / (v1 - v0).max(0.05)).ceil().min(1024.0) as u32;
            let scale = (src_w as f32 / b.width().max(1.0) as f32)
                .min(src_h as f32 / b.height().max(1.0) as f32)
                .max(0.01);
            let (fw, fh, full) = crate::io::render_svg_to_rgba(&svg, scale)?;
            let x0 = (u0 * fw as f32).floor().max(0.0) as u32;
            let y0 = (v0 * fh as f32).floor().max(0.0) as u32;
            let x1 = (u1 * fw as f32).ceil().min(fw as f32) as u32;
            let y1 = (v1 * fh as f32).ceil().min(fh as f32) as u32;
            if x1 <= x0 || y1 <= y0 {
                return None;
            }
            let cropped = crop_rgba(&full, fw, fh, x0, y0, x1, y1);
            Some(resize_rgba(&cropped, x1 - x0, y1 - y0, out_w, out_h))
        }
    }
}

fn decode_image_cached(bytes: &[u8]) -> Option<image::RgbaImage> {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<u64, image::RgbaImage>>> = Mutex::new(None);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    // Sample ends to reduce collision risk without hashing whole buffer every time.
    if bytes.len() > 64 {
        bytes[..32].hash(&mut hasher);
        bytes[bytes.len() - 32..].hash(&mut hasher);
    } else {
        bytes.hash(&mut hasher);
    }
    let key = hasher.finish();

    let mut guard = CACHE.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(img) = map.get(&key) {
        return Some(img.clone());
    }
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    // Bound cache size.
    if map.len() > 8 {
        map.clear();
    }
    map.insert(key, img.clone());
    Some(img)
}

fn crop_rgba(src: &[u8], sw: u32, sh: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
    let cw = x1 - x0;
    let ch = y1 - y0;
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        let sy = y0 + y;
        if sy >= sh {
            break;
        }
        for x in 0..cw {
            let sx = x0 + x;
            if sx >= sw {
                break;
            }
            let si = ((sy * sw + sx) * 4) as usize;
            let di = ((y * cw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

fn resize_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    if let Some(img) = image::RgbaImage::from_raw(sw, sh, src.to_vec()) {
        let resized =
            image::imageops::resize(&img, dw, dh, image::imageops::FilterType::Triangle);
        return resized.into_raw();
    }
    // Nearest-neighbor fallback.
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        let sy = (y as u64 * sh as u64 / dh as u64) as u32;
        for x in 0..dw {
            let sx = (x as u64 * sw as u64 / dw as u64) as u32;
            let si = ((sy * sw + sx) * 4) as usize;
            let di = ((y * dw + x) * 4) as usize;
            if si + 3 < src.len() {
                out[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
    out
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
        let n = effect.copies.max(3);
        for i in 0..n {
            let pl = effect.path_placement(i);
            let inst = node_at_placement(src_face, &pl);
            draw_node(painter, &inst, viewport, origin, false, fonts, image_textures);
        }
        if selection.contains(&effect.source_id) {
            let p0 = viewport.doc_to_screen((effect.base_x, effect.base_y), origin);
            let p1 = viewport.doc_to_screen((effect.origin_x, effect.origin_y), origin);
            let (p2x, p2y) = effect.placement_xy(1.min(n.saturating_sub(1)));
            let p2 = viewport.doc_to_screen((p2x, p2y), origin);
            let col = Color32::from_rgb(255, 165, 0);
            // Radius line (base ↔ origin) + angle sector (origin ↔ next copy).
            painter.line_segment([p0, p1], Stroke::new(2.5, col));
            painter.line_segment([p1, p2], Stroke::new(2.0, col));
            // Base (object on ring) — larger white handle
            painter.circle_filled(p0, 7.0, Color32::WHITE);
            painter.circle_stroke(p0, 7.0, Stroke::new(2.0, col));
            // Origin (center) — filled orange
            painter.circle_filled(p1, 7.0, col);
            painter.circle_stroke(p1, 7.0, Stroke::new(1.5, Color32::WHITE));
            // Angle tip (next copy) — small white
            painter.circle_filled(p2, 5.5, Color32::WHITE);
            painter.circle_stroke(p2, 5.5, Stroke::new(1.5, col));
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

// ===== Path marker / arrow geometry drawing (for Pen paths) =====

fn marker_local_points(kind: MarkerKind, size: f32) -> Vec<(f32, f32)> {
    let h = size / 2.0;
    match kind {
        // Attach point at (0,0) in local space (on the line), tip forward +x
        MarkerKind::Triangle => vec![(h, 0.0), (0.0, -h * 0.65), (0.0, h * 0.65)],
        MarkerKind::Square => vec![(-h, -h), (h, -h), (h, h), (-h, h)],  // centered, (0,0) center ok
        MarkerKind::HollowSquare => vec![(-h, -h), (h, -h), (h, h), (-h, h)],
        MarkerKind::Ring => {
            let mut v = vec![];
            for i in 0..12 {
                let a = (i as f32) * std::f32::consts::TAU / 12.0;
                v.push((h * 0.88 * a.cos(), h * 0.88 * a.sin()));
            }
            v
        }
        MarkerKind::Line => vec![(0.0, -h), (0.0, h)],  // centered vertical for perp
        MarkerKind::Arrow => vec![
            (h, 0.0),
            (0.0, -0.48 * h),
            (-0.6 * h, -0.48 * h),
            (-0.6 * h, 0.48 * h),
            (0.0, 0.48 * h),
        ],
        MarkerKind::None => vec![],
    }
}

fn transform_to_screen(local: &[(f32, f32)], rot: f32, center: Pos2) -> Vec<Pos2> {
    let c = rot.cos();
    let s = rot.sin();
    local
        .iter()
        .map(|&(lx, ly)| {
            let rx = lx * c - ly * s;
            let ry = lx * s + ly * c;
            Pos2::new(center.x + rx, center.y + ry)
        })
        .collect()
}

fn draw_one_marker(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    attach_x: f64,
    attach_y: f64,
    tangent_angle: f64,
    m: &PathMarker,
) {
    if m.kind == MarkerKind::None {
        return;
    }
    // size in document units, scale by zoom for screen
    let size = (m.size as f32 * viewport.zoom).max(1.0);
    let tangent = tangent_angle;

    let base = if m.auto_rotate { tangent } else { 0.0 };
    let rot = (base + m.rotation.to_radians()) as f32;
    let c = rot.cos() as f64;
    let s = rot.sin() as f64;

    // Apply 2D offset in marker's local (rotated) space
    let lo_x = m.offset[0];
    let lo_y = m.offset[1];
    let ax = attach_x + lo_x * c - lo_y * s;
    let ay = attach_y + lo_x * s + lo_y * c;

    let sp = viewport.doc_to_screen((ax, ay), origin);

    let col = m.color.to_egui();

    match m.kind {
        MarkerKind::Triangle | MarkerKind::Square | MarkerKind::Arrow => {
            let loc = marker_local_points(m.kind, size);
            let tpts = transform_to_screen(&loc, rot, sp);
            painter.add(Shape::convex_polygon(tpts, col, Stroke::NONE));
        }
        MarkerKind::HollowSquare => {
            let loc = marker_local_points(m.kind, size);
            let tpts = transform_to_screen(&loc, rot, sp);
            painter.add(Shape::Path(PathShape {
                points: tpts,
                closed: true,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(size * 0.13, col),
            }));
        }
        MarkerKind::Ring => {
            let r = size * 0.42;
            painter.add(Shape::circle_stroke(sp, r, egui::Stroke::new(size * 0.11, col)));
        }
        MarkerKind::Line => {
            let loc = marker_local_points(m.kind, size);
            let tpts = transform_to_screen(&loc, rot, sp);
            painter.add(Shape::Path(PathShape {
                points: tpts,
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(size * 0.16, col),
            }));
        }
        MarkerKind::None => {}
    }
}

fn get_marker_placements(bez: &BezPath) -> (Option<(f64, f64, f64)>, Vec<(f64, f64, f64)>, Option<(f64, f64, f64)>) {
    let els = bez.elements();
    if els.is_empty() {
        return (None, vec![], None);
    }
    let mut knots: Vec<(f64, f64)> = vec![];
    let mut out_tans: Vec<f64> = vec![];  // forward (outgoing) tangent at this knot, from bezier control if curve
    let mut prev_pt: Option<(f64, f64)> = None;
    for el in els {
        match el {
            PathEl::MoveTo(p) => {
                let pt = (p.x, p.y);
                knots.push(pt);
                out_tans.push(0.0); // will be set by next segment
                prev_pt = Some(pt);
            }
            PathEl::LineTo(p) => {
                let pt = (p.x, p.y);
                if let Some((px, py)) = prev_pt {
                    let dx = p.x - px;
                    let dy = p.y - py;
                    let ang = dy.atan2(dx);
                    // set outgoing for the previous knot
                    if let Some(last) = out_tans.last_mut() {
                        *last = ang;
                    }
                }
                knots.push(pt);
                out_tans.push(0.0);
                prev_pt = Some(pt);
            }
            PathEl::QuadTo(p1, p2) => {
                let pt = (p2.x, p2.y);
                if let Some((px, py)) = prev_pt {
                    // tangent at start of quad: direction from on-curve to first control p1
                    let dx = p1.x - px;
                    let dy = p1.y - py;
                    let ang = dy.atan2(dx);
                    if let Some(last) = out_tans.last_mut() {
                        *last = ang;
                    }
                }
                knots.push(pt);
                out_tans.push(0.0);
                prev_pt = Some(pt);
            }
            PathEl::CurveTo(p1, p2, p3) => {
                let pt = (p3.x, p3.y);
                if let Some((px, py)) = prev_pt {
                    // tangent at start of cubic: to first control p1
                    let dx = p1.x - px;
                    let dy = p1.y - py;
                    let ang = dy.atan2(dx);
                    if let Some(last) = out_tans.last_mut() {
                        *last = ang;
                    }
                }
                knots.push(pt);
                out_tans.push(0.0);
                prev_pt = Some(pt);
            }
            PathEl::ClosePath => {}
        }
    }
    let n = knots.len();
    if n == 0 {
        return (None, vec![], None);
    }
    // Fill in unset (use incoming for last, chord fallback for mids)
    if n >= 2 {
        // for last knot use the direction from prev segment (incoming as forward at end)
        if out_tans[n-1] == 0.0 {
            let (px, py) = knots[n-2];
            let (x, y) = knots[n-1];
            out_tans[n-1] = (y - py).atan2(x - px);
        }
    }
    for i in 1..n-1 {
        if out_tans[i] == 0.0 {
            // fallback to chord
            let (px, py) = knots[i-1];
            let (nx, ny) = knots[i+1];
            out_tans[i] = (ny - py).atan2(nx - px);
        }
    }
    // for first if still unset (should have been set by first segment)
    if n >= 2 && out_tans[0] == 0.0 {
        let (x, y) = knots[0];
        let (nx, ny) = knots[1];
        out_tans[0] = (ny - y).atan2(nx - x);
    }
    let mut res = vec![];
    for i in 0..n {
        res.push((knots[i].0, knots[i].1, out_tans[i]));
    }
    let start = res.first().copied();
    let end = res.last().copied();
    let mids = if n > 2 { res[1..n-1].to_vec() } else { vec![] };
    (start, mids, end)
}

fn draw_path_markers(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    bez: &BezPath,
    closed: bool,
    stroke: &DocStroke,
) {
    let (start, mids, end) = get_marker_placements(bez);
    if let Some((x, y, ang)) = start {
        let adj_ang = if stroke.start_marker.auto_rotate {
            ang + std::f64::consts::PI  // opposite for start
        } else {
            ang
        };
        draw_one_marker(painter, viewport, origin, x, y, adj_ang, &stroke.start_marker);
    }
    for (x, y, ang) in mids {
        draw_one_marker(painter, viewport, origin, x, y, ang, &stroke.mid_marker);
    }
    if let Some((x, y, ang)) = end {
        let same_as_start = closed
            && start
                .map(|(sx, sy, _)| (sx - x).abs() < 1e-6 && (sy - y).abs() < 1e-6)
                .unwrap_or(false);
        if !same_as_start {
            draw_one_marker(painter, viewport, origin, x, y, ang, &stroke.end_marker);
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

pub fn draw_preview_bezier(
    painter: &Painter,
    viewport: &Viewport,
    origin: Pos2,
    bez: &kurbo::BezPath,
) {
    let stroke = Stroke::new(2.5, Color32::from_rgb(0, 120, 215));
    let mut last_pt = None;
    for elem in bez.elements() {
        match elem {
            kurbo::PathEl::MoveTo(p) => {
                last_pt = Some(p);
            }
            kurbo::PathEl::LineTo(p) => {
                if let Some(prev) = last_pt {
                    let s_prev = viewport.doc_to_screen((prev.x, prev.y), origin);
                    let s_curr = viewport.doc_to_screen((p.x, p.y), origin);
                    painter.line_segment([s_prev, s_curr], stroke);
                }
                last_pt = Some(p);
            }
            kurbo::PathEl::QuadTo(p1, p2) => {
                if let Some(prev) = last_pt {
                    let s_prev = viewport.doc_to_screen((prev.x, prev.y), origin);
                    let s_p1 = viewport.doc_to_screen((p1.x, p1.y), origin);
                    let s_p2 = viewport.doc_to_screen((p2.x, p2.y), origin);
                    let mut prev_t = s_prev;
                    for step in 1..=8 {
                        let t = step as f32 / 8.0;
                        let x = (1.0 - t).powi(2) * s_prev.x + 2.0 * (1.0 - t) * t * s_p1.x + t.powi(2) * s_p2.x;
                        let y = (1.0 - t).powi(2) * s_prev.y + 2.0 * (1.0 - t) * t * s_p1.y + t.powi(2) * s_p2.y;
                        let curr_t = Pos2::new(x, y);
                        painter.line_segment([prev_t, curr_t], stroke);
                        prev_t = curr_t;
                    }
                }
                last_pt = Some(p2);
            }
            kurbo::PathEl::CurveTo(p1, p2, p3) => {
                if let Some(prev) = last_pt {
                    let s_prev = viewport.doc_to_screen((prev.x, prev.y), origin);
                    let s_p1 = viewport.doc_to_screen((p1.x, p1.y), origin);
                    let s_p2 = viewport.doc_to_screen((p2.x, p2.y), origin);
                    let s_p3 = viewport.doc_to_screen((p3.x, p3.y), origin);
                    let mut prev_t = s_prev;
                    for step in 1..=12 {
                        let t = step as f32 / 12.0;
                        let mt = 1.0 - t;
                        let x = mt.powi(3) * s_prev.x + 3.0 * mt.powi(2) * t * s_p1.x + 3.0 * mt * t.powi(2) * s_p2.x + t.powi(3) * s_p3.x;
                        let y = mt.powi(3) * s_prev.y + 3.0 * mt.powi(2) * t * s_p1.y + 3.0 * mt * t.powi(2) * s_p2.y + t.powi(3) * s_p3.y;
                        let curr_t = Pos2::new(x, y);
                        painter.line_segment([prev_t, curr_t], stroke);
                        prev_t = curr_t;
                    }
                }
                last_pt = Some(p3);
            }
            kurbo::PathEl::ClosePath => {}
        }
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

    // Draw yellow corner curve controls (LPE style) for filleted corners
    // (visible after enabling Corner curve when the point is selected)
    // T1/T2 placed at equal D = R / tan(θ/2) from V.
    if let NodeKind::Path { path } = &node.kind {
        let anchors = path.anchor_positions();
        for (&k, _f) in &path.corner_fillets {
            if k >= anchors.len() { continue; }
            let p = anchors[k];
            let prev = if k > 0 { k - 1 } else if path.is_closed() && anchors.len() > 2 { anchors.len() - 1 } else { continue };
            let pa = anchors[prev];
            let lenp = ((p.0 - pa.0).powi(2) + (p.1 - pa.1).powi(2)).sqrt().max(1e-9);
            let uxp = (pa.0 - p.0) / lenp;
            let uyp = (pa.1 - p.1) / lenp;
            let D = path.fillet_tangent_d(k);
            let t1x = p.0 + uxp * D;
            let t1y = p.1 + uyp * D;
            // next leg
            let nxt = if k + 1 < anchors.len() { k + 1 } else if path.is_closed() && anchors.len() > 2 { 0 } else { continue };
            let pb = anchors[nxt];
            let lenn = ((p.0 - pb.0).powi(2) + (p.1 - pb.1).powi(2)).sqrt().max(1e-9);
            let uxn = (pb.0 - p.0) / lenn;
            let uyn = (pb.1 - p.1) / lenn;
            let t2x = p.0 + uxn * D;
            let t2y = p.1 + uyn * D;
            let y1 = viewport.doc_to_screen((t1x, t1y), origin);
            let y2 = viewport.doc_to_screen((t2x, t2y), origin);
            painter.circle_filled(y1, 5.0, Color32::from_rgb(255, 255, 80));
            painter.circle_stroke(y1, 5.0, Stroke::new(1.0, Color32::BLACK));
            painter.circle_filled(y2, 5.0, Color32::from_rgb(255, 255, 80));
            painter.circle_stroke(y2, 5.0, Stroke::new(1.0, Color32::BLACK));
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
                        Stroke::new(1.5, Color32::from_rgb(255, 255, 80)), // yellow for curvable mid controls etc.
                    );
                    painter.rect_filled(
                        Rect::from_center_size(cin, Vec2::splat(6.0)),
                        0.0,
                        Color32::from_rgb(255, 255, 80),
                    );
                }
                if let Some(co) = ctrl_out {
                    let cout = viewport.doc_to_screen(co, origin);
                    painter.line_segment(
                        [a, cout],
                        Stroke::new(1.5, Color32::from_rgb(255, 255, 80)), // yellow for curvable mid controls etc.
                    );
                    painter.rect_filled(
                        Rect::from_center_size(cout, Vec2::splat(6.0)),
                        0.0,
                        Color32::from_rgb(255, 255, 80),
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
            let is_path_point = matches!(&node.kind, NodeKind::Path { .. });
            let smooth = matches!(
                &node.kind,
                NodeKind::Path { path } if path.is_anchor_smooth(i)
            );
            let radius = if is_selected { 7.0 } else { 5.0 };
            if is_path_point {
                // Explicit circle icon for path points (sharp or smooth)
                let fill = if is_selected {
                    colors::ACCENT
                } else if smooth {
                    Color32::from_rgb(255, 180, 60)
                } else {
                    Color32::from_rgb(200, 220, 255)
                };
                painter.circle_filled(s, radius, fill);
                painter.circle_stroke(s, radius, Stroke::new(2.0, Color32::from_rgb(0, 100, 200)));
                // inner dot for icon feel
                painter.circle_filled(s, radius * 0.4, Color32::from_rgb(30, 60, 120));
            } else {
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

fn paint_to_overlay_color(p: crate::document::Paint) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (p.rgba[0].clamp(0.0, 1.0) * 255.0) as u8,
        (p.rgba[1].clamp(0.0, 1.0) * 255.0) as u8,
        (p.rgba[2].clamp(0.0, 1.0) * 255.0) as u8,
        255,
    )
}

/// Draw every gradient stop as a colored disc along the linear flow line (or radial rings).
fn draw_stop_markers_on_line(
    painter: &Painter,
    a: Pos2,
    b: Pos2,
    stops: &[crate::document::GradientStop],
) {
    if stops.is_empty() {
        return;
    }
    for stop in stops {
        let t = stop.pos.clamp(0.0, 1.0);
        let p = a + (b - a) * t;
        let fill = paint_to_overlay_color(stop.color);
        painter.circle_filled(p, 5.5, fill);
        painter.circle_stroke(p, 5.5, Stroke::new(1.5, Color32::WHITE));
        painter.circle_stroke(p, 5.5, Stroke::new(1.0, colors::ACCENT));
    }
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
    stops: &[crate::document::GradientStop],
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
            // Multi-stop preview stroke along the flow line.
            if stops.len() >= 2 {
                const SEGS: usize = 32;
                for i in 0..SEGS {
                    let t0 = i as f32 / SEGS as f32;
                    let t1 = (i + 1) as f32 / SEGS as f32;
                    let p0 = a + (b - a) * t0;
                    let p1 = a + (b - a) * t1;
                    let c = paint_to_overlay_color(crate::document::sample_stops(stops, t0));
                    painter.line_segment([p0, p1], Stroke::new(4.0, c));
                }
            } else {
                painter.line_segment([a, b], Stroke::new(3.0, colors::ACCENT));
            }
            // Endpoint / mid handles (flow geometry), then all stop markers on top.
            painter.circle_filled(a, 6.0, Color32::WHITE);
            painter.circle_filled(b, 6.0, Color32::WHITE);
            painter.circle_stroke(a, 6.0, Stroke::new(1.5, colors::ACCENT));
            painter.circle_stroke(b, 6.0, Stroke::new(1.5, colors::ACCENT));
            painter.circle_filled(mid, 5.0, colors::ACCENT);
            painter.circle_stroke(mid, 5.0, Stroke::new(1.5, Color32::WHITE));
            draw_stop_markers_on_line(painter, a, b, stops);
        }
        crate::document::FillKind::RadialGradient => {
            let focal = Pos2::new(
                r.left() + r.width() * radial_cx,
                r.top() + r.height() * radial_cy,
            );
            // Approximate stop rings: t maps to radius via same scale as sample_at (dist * 1.25).
            let max_r = (r.width().hypot(r.height()) * 0.5).max(8.0);
            for stop in stops {
                let rad = (stop.pos / 1.25).clamp(0.0, 1.0) * max_r;
                if rad > 2.0 {
                    let c = paint_to_overlay_color(stop.color);
                    painter.circle_stroke(focal, rad, Stroke::new(2.0, c));
                }
            }
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

/// Draw Clip Mask: raster `source_id` clipped to the **solid face** of `mask_id`.
pub fn draw_clip_mask_effects(
    painter: &Painter,
    nodes: &NodeStore,
    clip_masks: &indexmap::IndexMap<uuid::Uuid, crate::document::ClipMaskEffect>,
    viewport: &Viewport,
    origin: Pos2,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<NodeId, egui::TextureHandle>,
    selection: &[NodeId],
) {
    for cm in clip_masks.values() {
        let Some(mask_node) = nodes.get(cm.mask_id) else { continue };
        let Some(source_node) = nodes.get(cm.source_id) else { continue };

        let mask_bounds = mask_node.bounds();
        let tl = viewport.doc_to_screen((mask_bounds.x0, mask_bounds.y0), origin);
        let br = viewport.doc_to_screen((mask_bounds.x1, mask_bounds.y1), origin);
        let clip_rect = egui::Rect::from_min_max(tl, br);

        let mut drew = false;

        // 1) Preferred for images: tessellate mask solid face + UV-map the image texture
        //    (true solid-face clip, works even when SVG bake fails).
        if let NodeKind::Image {
            x,
            y,
            width,
            height,
            ..
        } = &source_node.kind
        {
            if let Some(src_tex) = image_textures.get(&cm.source_id) {
                let img_rect = kurbo::Rect::new(*x, *y, *x + *width, *y + *height);
                if let Some(mesh) =
                    clip_image_mesh(mask_node, img_rect, src_tex.id(), viewport, origin)
                {
                    painter.add(Shape::mesh(mesh));
                    drew = true;
                }
            }
        }

        // 2) Pre-baked solid-face clip texture (SVG clipPath).
        if !drew {
            if let Some(tex) = image_textures.get(&cm.id) {
                painter.image(
                    tex.id(),
                    clip_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                drew = true;
            }
        }

        // 3) Last resort for non-image: scissor (bbox) — better than nothing.
        if !drew {
            let clipped_painter = painter.with_clip_rect(clip_rect);
            draw_node(
                &clipped_painter,
                source_node,
                viewport,
                origin,
                false,
                fonts,
                image_textures,
            );
        }

        // Subtle dashed outline only when selected (avoid soft white rim looking like a lens).
        let is_selected = selection.contains(&cm.source_id) || selection.contains(&cm.mask_id);
        if is_selected {
            let bez = mask_node.bez_path();
            let (screen_pts, _) = polyline_from_bez(&bez, viewport, origin, true);
            draw_dashed_polyline(
                painter,
                &screen_pts,
                true,
                5.0,
                4.0,
                egui::Stroke::new(1.25, Color32::from_rgb(180, 100, 255)),
            );
        }
    }
}

/// Tessellate **mask ∩ image-rect** solid face and UV-map the image.
/// Intersecting first avoids transparent/white vertex blends that looked like a
/// "lens" highlight on the top rim of circular clips.
fn clip_image_mesh(
    mask_node: &Node,
    image_doc_rect: kurbo::Rect,
    texture_id: egui::TextureId,
    viewport: &Viewport,
    origin: Pos2,
) -> Option<Mesh> {
    use crate::document::node_to_multipolygon;
    use geo::BooleanOps;
    use geo::{Coord, LineString, MultiPolygon, Polygon};

    let mask_mp = node_to_multipolygon(mask_node, 0.5)?;
    let r = image_doc_rect;
    let img_ring = vec![
        Coord { x: r.x0, y: r.y0 },
        Coord { x: r.x1, y: r.y0 },
        Coord { x: r.x1, y: r.y1 },
        Coord { x: r.x0, y: r.y1 },
        Coord { x: r.x0, y: r.y0 },
    ];
    let img_mp = MultiPolygon::new(vec![Polygon::new(LineString::new(img_ring), vec![])]);
    let clipped = mask_mp.intersection(&img_mp);
    if clipped.0.is_empty() {
        return None;
    }

    // Build lyon path from intersection polygons (doc → screen).
    let mut builder = Path::builder();
    let map = |x: f64, y: f64| {
        let s = viewport.doc_to_screen((x, y), origin);
        Point::new(s.x, s.y)
    };
    for poly in &clipped.0 {
        let coords: Vec<_> = poly.exterior().coords().collect();
        if coords.len() < 3 {
            continue;
        }
        builder.begin(map(coords[0].x, coords[0].y));
        for c in coords.iter().skip(1) {
            builder.line_to(map(c.x, c.y));
        }
        builder.close();
        for hole in poly.interiors() {
            let hc: Vec<_> = hole.coords().collect();
            if hc.len() < 3 {
                continue;
            }
            builder.begin(map(hc[0].x, hc[0].y));
            for c in hc.iter().skip(1) {
                builder.line_to(map(c.x, c.y));
            }
            builder.close();
        }
    }
    let lyon_path = builder.build();

    let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    if tess
        .tessellate_path(
            &lyon_path,
            &lyon_fill_options(viewport),
            &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| vertex.position()),
        )
        .is_err()
        || buffers.vertices.is_empty()
    {
        return None;
    }

    let iw = image_doc_rect.width().max(1e-6);
    let ih = image_doc_rect.height().max(1e-6);
    let mut mesh = Mesh::with_texture(texture_id);
    for v in &buffers.vertices {
        let doc = viewport.screen_to_doc(Pos2::new(v.x, v.y), origin);
        let u = ((doc.0 - image_doc_rect.x0) / iw) as f32;
        let vv = ((doc.1 - image_doc_rect.y0) / ih) as f32;
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(v.x, v.y),
            // Geometry already clipped to image rect — UV stays in [0,1].
            uv: egui::pos2(u.clamp(0.0, 1.0), vv.clamp(0.0, 1.0)),
            color: Color32::WHITE,
        });
    }
    for tri in buffers.indices.chunks_exact(3) {
        mesh.indices
            .extend_from_slice(&[tri[0] as u32, tri[1] as u32, tri[2] as u32]);
    }
    Some(mesh)
}

/// Draw a dashed polyline.
fn draw_dashed_polyline(
    painter: &Painter,
    pts: &[Pos2],
    closed: bool,
    dash: f32,
    gap: f32,
    stroke: egui::Stroke,
) {
    if pts.len() < 2 {
        return;
    }
    let mut draw_state = true;
    let mut remaining = dash;
    let mut current_pt = pts[0];
    let end_idx = if closed { pts.len() } else { pts.len() - 1 };

    for idx in 0..end_idx {
        let next_pt = pts[(idx + 1) % pts.len()];
        let d = next_pt - current_pt;
        let mut len = d.length();
        if len < 1e-4 {
            continue;
        }
        let dir = d / len;

        while len > 0.0 {
            if len <= remaining {
                if draw_state {
                    painter.line_segment([current_pt, next_pt], stroke);
                }
                remaining -= len;
                if remaining <= 0.0 {
                    draw_state = !draw_state;
                    remaining = if draw_state { dash } else { gap };
                }
                break;
            } else {
                let step_pt = current_pt + dir * remaining;
                if draw_state {
                    painter.line_segment([current_pt, step_pt], stroke);
                }
                current_pt = step_pt;
                len -= remaining;
                draw_state = !draw_state;
                remaining = if draw_state { dash } else { gap };
            }
        }
        current_pt = next_pt;
    }
}