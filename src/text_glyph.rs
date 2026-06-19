//! Vector text: glyph outlines tessellated with Lyon (true fill + stroke on paths).

use egui::{Color32, Mesh, Painter, Pos2, Shape};
use lyon::math::Point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::canvas::Viewport;
use crate::document::{Fill, LineCap, LineJoin, TextStyle};
use crate::fonts::FontRegistry;
use crate::render::sample_paint_fill;

struct GlyphOutline<'a> {
    builder: &'a mut lyon::path::path::Builder,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
    open: bool,
}

impl GlyphOutline<'_> {
    fn map(&self, x: f32, y: f32) -> Point {
        Point::new(
            self.origin_x + x * self.scale,
            self.origin_y - y * self.scale,
        )
    }
}

impl OutlineBuilder for GlyphOutline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.builder.end(false);
        }
        self.builder.begin(self.map(x, y));
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(self.map(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder
            .quadratic_bezier_to(self.map(x1, y1), self.map(x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_bezier_to(
            self.map(x1, y1),
            self.map(x2, y2),
            self.map(x, y),
        );
    }

    fn close(&mut self) {
        self.builder.close();
        self.open = false;
    }
}

fn to_lyon_join(join: LineJoin) -> lyon::path::LineJoin {
    match join {
        LineJoin::Miter => lyon::path::LineJoin::Miter,
        LineJoin::Round => lyon::path::LineJoin::Round,
        LineJoin::Bevel => lyon::path::LineJoin::Bevel,
    }
}

fn to_lyon_cap(cap: LineCap) -> lyon::path::LineCap {
    match cap {
        LineCap::Butt | LineCap::Square => lyon::path::LineCap::Butt,
        LineCap::Round => lyon::path::LineCap::Round,
    }
}

fn screen_norm(p: Pos2, bbox: egui::Rect) -> (f32, f32) {
    let w = bbox.width().max(1e-6);
    let h = bbox.height().max(1e-6);
    ((p.x - bbox.left()) / w, (p.y - bbox.top()) / h)
}

fn tessellate_fill_mesh(
    path: &Path,
    fill: &Fill,
    opacity: f32,
    bbox_screen: egui::Rect,
    tolerance: f32,
) -> Option<Mesh> {
    let mut tessellator = FillTessellator::new();
    let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
    let options = FillOptions::default().with_tolerance(tolerance);
    tessellator
        .tessellate_path(
            path,
            &options,
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| v.position()),
        )
        .ok()?;
    if buffers.indices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::default();
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
        mesh.colored_vertex(p0, sample_paint_fill(fill, opacity, nx0, ny0));
        let i1 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p1, sample_paint_fill(fill, opacity, nx1, ny1));
        let i2 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p2, sample_paint_fill(fill, opacity, nx2, ny2));
        mesh.add_triangle(i0, i1, i2);
    }
    Some(mesh)
}

fn tessellate_stroke_mesh(
    path: &Path,
    stroke: &Fill,
    opacity: f32,
    width: f32,
    join: LineJoin,
    cap: LineCap,
    tolerance: f32,
) -> Option<Mesh> {
    let color = sample_paint_fill(stroke, opacity, 0.5, 0.5);
    let mut tessellator = StrokeTessellator::new();
    let mut buffers: VertexBuffers<Point, u16> = VertexBuffers::new();
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_join(to_lyon_join(join))
        .with_line_cap(to_lyon_cap(cap))
        .with_tolerance(tolerance);
    tessellator
        .tessellate_path(
            path,
            &options,
            &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex| v.position()),
        )
        .ok()?;
    if buffers.indices.is_empty() {
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

fn build_text_path(
    face: &Face<'_>,
    viewport: &Viewport,
    origin: Pos2,
    x: f64,
    y: f64,
    style: &TextStyle,
) -> Option<(Path, egui::Rect)> {
    let upem = face.units_per_em() as f32;
    let scale_doc = style.font_size / upem;
    let scale_screen = scale_doc * viewport.zoom;
    let ascender_doc = face.ascender() as f32 * scale_doc;
    let line_height_doc = style.font_size * 1.25;

    let mut lyon_builder = Path::builder();
    let mut contour_open = false;
    let mut any_glyph = false;
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    let mut extend_bounds = |p: Point| {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    };

    for (line_idx, line) in style.content.lines().enumerate() {
        let baseline_doc = y + ascender_doc as f64 + line_idx as f64 * line_height_doc as f64;
        let mut pen_x = 0.0f64;

        for ch in line.chars() {
            let gid = face.glyph_index(ch).unwrap_or(GlyphId(0));
            let advance_doc = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale_doc;

            let (doc_x, doc_y) = (x + pen_x, baseline_doc);
            let screen_base = viewport.doc_to_screen((doc_x, doc_y), origin);
            let ox = screen_base.x;
            let oy = screen_base.y;

            let mut collector = GlyphOutline {
                builder: &mut lyon_builder,
                scale: scale_screen,
                origin_x: ox,
                origin_y: oy,
                open: false,
            };
            if face.outline_glyph(gid, &mut collector).is_some() {
                any_glyph = true;
                contour_open = collector.open;
            }

            if let Some(bb) = face.glyph_bounding_box(gid) {
                let x0 = ox + bb.x_min as f32 * scale_screen;
                let y0 = oy - bb.y_max as f32 * scale_screen;
                let x1 = ox + bb.x_max as f32 * scale_screen;
                let y1 = oy - bb.y_min as f32 * scale_screen;
                extend_bounds(Point::new(x0, y0));
                extend_bounds(Point::new(x1, y1));
            }

            pen_x += advance_doc as f64;
        }
    }

    if contour_open {
        lyon_builder.end(false);
    }

    let bbox = if min_x <= max_x && min_y <= max_y {
        egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
    } else {
        let tl = viewport.doc_to_screen((x, y), origin);
        let br = viewport.doc_to_screen(
            (x + style.font_size as f64, y + style.font_size as f64),
            origin,
        );
        egui::Rect::from_min_max(tl, br)
    };

    if !any_glyph {
        return None;
    }

    Some((lyon_builder.build(), bbox))
}

pub fn draw_text_glyphs(
    painter: &Painter,
    fonts: &FontRegistry,
    viewport: &Viewport,
    origin: Pos2,
    x: f64,
    y: f64,
    style: &TextStyle,
    fill: &Fill,
    stroke_style: &Fill,
    stroke_width_screen: Option<f32>,
    stroke_join: LineJoin,
    stroke_cap: LineCap,
    opacity: f32,
) -> bool {
    let Some(bytes) = fonts.query_face_bytes(&style.font_family, style.bold, style.italic) else {
        return false;
    };
    let Ok(face) = Face::parse(&bytes, 0) else {
        return false;
    };

    let Some((path, bbox)) = build_text_path(&face, viewport, origin, x, y, style) else {
        return false;
    };

    let tolerance = (0.15 / viewport.zoom.max(0.2)).clamp(0.02, 0.15);

    if let Some(sw) = stroke_width_screen.filter(|w| stroke_style.is_visible() && *w > 0.01) {
        if let Some(mesh) = tessellate_stroke_mesh(
            &path,
            stroke_style,
            opacity,
            sw,
            stroke_join,
            stroke_cap,
            tolerance,
        ) {
            painter.add(Shape::mesh(mesh));
        }
    }

    if fill.is_visible() {
        if let Some(mesh) = tessellate_fill_mesh(&path, fill, opacity, bbox, tolerance) {
            painter.add(Shape::mesh(mesh));
        }
    }

    true
}