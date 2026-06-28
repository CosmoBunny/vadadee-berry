use std::fs;
use std::path::Path;

use kurbo::BezPath;
use thiserror::Error;

use crate::document::{
    Document, Fill, LineCap, LineJoin, Node, NodeKind, NodeStore, Paint, PathData, ProjectFile,
    Stroke, regular_polygon_vertices,
};

/// Native project file extension (e.g. `drawing.vadadee-berry.json`).
pub const PROJECT_FILE_EXTENSION: &str = "vadadee-berry.json";

pub fn default_project_filename(title: &str) -> String {
    let stem = title
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else if c.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(|c: char| c == '-' || c == '_')
        .to_string();
    let stem = if stem.is_empty() { "untitled" } else { stem.as_str() };
    format!("{stem}.{PROJECT_FILE_EXTENSION}")
}

#[derive(Debug, Error)]
pub enum IoError {
    #[error("{0}")]
    Msg(String),
}

pub fn load_project(path: &Path) -> Result<ProjectFile, IoError> {
    let data = fs::read_to_string(path).map_err(|e| IoError::Msg(e.to_string()))?;
    serde_json::from_str(&data).map_err(|e| IoError::Msg(e.to_string()))
}

pub fn save_project(path: &Path, project: &ProjectFile) -> Result<(), IoError> {
    let data = serde_json::to_string_pretty(project).map_err(|e| IoError::Msg(e.to_string()))?;
    fs::write(path, data).map_err(|e| IoError::Msg(e.to_string()))
}

pub fn import_svg(path: &Path) -> Result<ProjectFile, IoError> {
    let data = fs::read(path).map_err(|e| IoError::Msg(e.to_string()))?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| IoError::Msg(e.to_string()))?;
    let size = tree.size();
    let mut document = Document {
        title: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .to_string(),
        width: size.width() as f64,
        height: size.height() as f64,
        active_layer_index: 0,
        layers: vec![],
        defs: Default::default(),
        path_effects: Default::default(),
        tiling_effects: Default::default(),
        circular_effects: Default::default(),
        page_color: [1.0, 1.0, 1.0, 1.0],
    };
    let mut nodes = NodeStore::default();
    let mut layer_nodes = Vec::new();

    for child in tree.root().children() {
        if let usvg::Node::Path(ref path) = *child {
            if let Some(node) = path_from_usvg(path) {
                let id = nodes.insert(node);
                layer_nodes.push(id);
            }
        }
    }

    if layer_nodes.is_empty() {
        let id = nodes.insert(Node::rect(
            0.0,
            0.0,
            document.width.min(400.0),
            document.height.min(300.0),
            Fill::Solid(Paint::from_hex(0xcccccc, 0.3)),
        ));
        layer_nodes.push(id);
    }

    document.layers.push(crate::document::Layer::new_image(
        uuid::Uuid::new_v4(),
        "Imported".into(),
        true,
        false,
        layer_nodes,
    ));

    Ok(ProjectFile::new(document, nodes))
}

fn path_from_usvg(path: &usvg::Path) -> Option<Node> {
    let tiny = path.data();
    let mut bez = BezPath::new();
    for seg in tiny.segments() {
        use usvg::tiny_skia_path::PathSegment;
        match seg {
            PathSegment::MoveTo(p) => bez.move_to((p.x as f64, p.y as f64)),
            PathSegment::LineTo(p) => bez.line_to((p.x as f64, p.y as f64)),
            PathSegment::QuadTo(p1, p2) => {
                bez.quad_to((p1.x as f64, p1.y as f64), (p2.x as f64, p2.y as f64));
            }
            PathSegment::CubicTo(p1, p2, p3) => bez.curve_to(
                (p1.x as f64, p1.y as f64),
                (p2.x as f64, p2.y as f64),
                (p3.x as f64, p3.y as f64),
            ),
            PathSegment::Close => bez.close_path(),
        }
    }
    let mut node = Node::path_from_bez(bez, "Path");
    if let Some(fill) = path.fill() {
        if let usvg::Paint::Color(c) = fill.paint() {
            node.style.fill = Fill::Solid(Paint {
                rgba: [
                    c.red as f32 / 255.0,
                    c.green as f32 / 255.0,
                    c.blue as f32 / 255.0,
                    fill.opacity().get(),
                ],
            });
        }
    }
    if let Some(stroke) = path.stroke() {
        if let usvg::Paint::Color(c) = stroke.paint() {
            node.style.stroke.style = Fill::Solid(Paint {
                rgba: [
                    c.red as f32 / 255.0,
                    c.green as f32 / 255.0,
                    c.blue as f32 / 255.0,
                    stroke.opacity().get(),
                ],
            });
            node.style.stroke.width = stroke.width().get();
        }
    }
    let kind = NodeKind::Path {
        path: PathData::from_bez(&node.bez_path()),
    };
    node.kind = kind;
    Some(node)
}

pub fn export_svg(path: &Path, project: &ProjectFile) -> Result<(), IoError> {
    fs::write(path, document_svg_string(project, 0, &std::collections::HashMap::new())).map_err(|e| IoError::Msg(e.to_string()))
}

/// Full document SVG (for raster export / video frames).
pub fn document_svg_string(
    project: &ProjectFile,
    current_frame: usize,
    video_frames: &std::collections::HashMap<uuid::Uuid, Vec<u8>>,
) -> String {
    use base64::Engine;
    let w = project.document.width;
    let h = project.document.height;
    let bg_color = project.document.page_color_svg();
    let mut svg = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<rect width="{w}" height="{h}" {bg_color}/>
"#
    );
    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        match layer.kind {
            crate::document::LayerKind::Image => {
                for id in &layer.nodes {
                    let Some(node) = project.nodes.get(*id) else { continue };
                    svg.push_str(&node_to_svg_fragment(node, &project.nodes));
                }
            }
            crate::document::LayerKind::Video => {
                if let Some(bytes) = video_frames.get(&layer.id) {
                    let mut opacity = 1.0;
                    let mut dx = layer.x as f64;
                    let mut dy = layer.y as f64;
                    let mut rot = layer.rotation as f64;
                    if let Some(track) = project.anim_timeline.nodes.get(&layer.id) {
                        if let Some(o) = track.opacity.interpolate(current_frame) {
                            opacity = o;
                        }
                        if let Some(x) = track.pos_x.interpolate(current_frame) {
                            dx = x;
                        }
                        if let Some(y) = track.pos_y.interpolate(current_frame) {
                            dy = y;
                        }
                        if let Some(r) = track.rotation.interpolate(current_frame) {
                            rot = r;
                        }
                    }
                    
                    let mut aspect = 1.0;
                    if let Ok(dyn_img) = image::load_from_memory(bytes) {
                        if dyn_img.height() > 0 {
                            aspect = dyn_img.width() as f32 / dyn_img.height() as f32;
                        }
                    }
                    
                    let mut w = layer.width;
                    let mut h = layer.height;
                    if layer.aspect_ratio_locked {
                        if w / h > aspect {
                            w = h * aspect;
                        } else {
                            h = w / aspect;
                        }
                    }
                    
                    let cx = dx + w as f64 / 2.0;
                    let cy = dy + h as f64 / 2.0;
                    
                    let transform_attr = if rot != 0.0 {
                        format!(" transform=\"rotate({}, {}, {})\"", rot, cx, cy)
                    } else {
                        String::new()
                    };
                    
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    svg.push_str(&format!(
                        r#"<image href="data:image/png;base64,{b64}" x="{dx}" y="{dy}" width="{w}" height="{h}" opacity="{opacity}"{transform_attr}/>"#,
                    ));
                }
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Rasterize a single node into a tight SVG view box (transparent background).
pub fn node_svg_for_bounds(node: &Node, bounds: kurbo::Rect, nodes: &crate::document::NodeStore) -> String {
    let w = bounds.width().max(1.0);
    let h = bounds.height().max(1.0);
    let x0 = bounds.x0;
    let y0 = bounds.y0;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<g transform="translate({},{})">{}</g>
</svg>
"#,
        -x0,
        -y0,
        node_to_svg_fragment(node, nodes)
    )
}

pub fn node_to_svg_fragment(node: &Node, nodes: &crate::document::NodeStore) -> String {
    let fill_grad_id = format!("fill-{}", node.id.as_simple());
    let stroke_grad_id = format!("stroke-{}", node.id.as_simple());
    let (fill, fill_defs) = fill_svg(&node.style.fill, &fill_grad_id);
    let (stroke, stroke_defs) = if node.style.stroke.width > 0.0 && node.style.stroke.style.is_visible() {
        stroke_svg(&node.style.stroke, &stroke_grad_id)
    } else {
        (r#"stroke="none""#.into(), String::new())
    };
    let defs = format!("{fill_defs}{stroke_defs}");
    let op = node.style.opacity;
    let blend = node.style.blend_mode.svg_value();
    let body = match &node.kind {
        NodeKind::Rect { x, y, w, h, rx } => format!(
            r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" {fill} {stroke} opacity="{op}"/>"#,
        ),
        NodeKind::Ellipse { cx, cy, rx, ry } => format!(
            r#"<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" {fill} {stroke} opacity="{op}"/>"#,
        ),
        NodeKind::Polygon {
            cx,
            cy,
            r,
            sides,
            rotation_rad,
        } => {
            let pts: Vec<String> = regular_polygon_vertices(*cx, *cy, *r, *sides, *rotation_rad)
                .into_iter()
                .map(|(x, y)| format!("{x},{y}"))
                .collect();
            format!(
                r#"<polygon points="{}" {fill} {stroke} opacity="{op}"/>"#,
                pts.join(" ")
            )
        }
        NodeKind::Path { path } => {
            let d = path_to_svg_d(path);
            format!(r#"<path d="{d}" {fill} {stroke} opacity="{op}"/>"#)
        }
        NodeKind::Text { x, y, style } => {
            let weight = if style.bold { "bold" } else { "normal" };
            let font_style = if style.italic { "italic" } else { "normal" };
            let family = style.font_family.replace('"', "'");
            let escaped = style
                .content
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!(
                r#"<text x="{x}" y="{y}" font-size="{}" font-family="{family}" font-weight="{weight}" font-style="{font_style}" {fill} {stroke} opacity="{op}">{escaped}</text>"#,
                style.font_size
            )
        }
        NodeKind::Group { children } => {
            let mut inner = String::new();
            for cid in children {
                if let Some(child) = nodes.get(*cid) {
                    inner.push_str(&node_to_svg_fragment(child, nodes));
                }
            }
            format!(r#"<g>{inner}</g>"#)
        }
        NodeKind::Image { x, y, width, height, bytes } => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            format!(
                r#"<image x="{x}" y="{y}" width="{width}" height="{height}" href="data:image/png;base64,{b64}" opacity="{op}"/>"#
            )
        }
        NodeKind::Arc { cx, cy, radius, start_angle_rad, sweep_angle_rad, join } => {
            let bez = crate::document::build_arc_bez(*cx, *cy, *radius, *start_angle_rad, *sweep_angle_rad, *join);
            let d = bez.to_svg();
            format!(r#"<path d="{d}" {fill} {stroke} opacity="{op}"/>"#)
        }
        NodeKind::BrushStroke { points } => {
            let mut svg = String::new();
            for (pos, width) in points {
                let r = width / 2.0;
                if r > 0.1 {
                    svg.push_str(&format!(r#"<circle cx="{}" cy="{}" r="{}" {fill} opacity="{op}"/>"#, pos[0], pos[1], r));
                }
            }
            svg
        }
    };
    format!(r#"<g style="mix-blend-mode:{blend}">{defs}{body}</g>"#)
}


fn stops_svg(stops: &[crate::document::GradientStop]) -> String {
    stops
        .iter()
        .map(|s| {
            format!(
                r#"<stop offset="{:.2}%" {} />"#,
                s.pos * 100.0,
                stop_attr(&s.color)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn fill_svg(fill: &Fill, id: &str) -> (String, String) {
    match fill {
        Fill::None => (r#"fill="none""#.into(), String::new()),
        Fill::Solid(p) => (paint_attr(p), String::new()),
        Fill::LinearGradient {
            line_x0,
            line_y0,
            line_x1,
            line_y1,
            stops,
            ..
        } => {
            let stops_xml = stops_svg(stops);
            let defs = format!(
                r#"<defs><linearGradient id="{id}" gradientUnits="objectBoundingBox" x1="{line_x0}" y1="{line_y0}" x2="{line_x1}" y2="{line_y1}">{stops_xml}</linearGradient></defs>"#
            );
            (format!(r#"fill="url(#{id})""#), defs)
        }
        Fill::RadialGradient {
            center_x,
            center_y,
            stops,
        } => {
            let stops_xml = stops_svg(stops);
            let defs = format!(
                r#"<defs><radialGradient id="{id}" cx="{center_x}" cy="{center_y}" r="0.5">{stops_xml}</radialGradient></defs>"#
            );
            (format!(r#"fill="url(#{id})""#), defs)
        }
    }
}

fn stroke_join_attr(j: LineJoin) -> &'static str {
    match j {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    }
}

fn stroke_cap_attr(c: LineCap) -> &'static str {
    match c {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    }
}

fn stroke_svg(stroke: &Stroke, id: &str) -> (String, String) {
    let width = stroke.width;
    let extra = format!(
        r#" stroke-linejoin="{}" stroke-linecap="{}""#,
        stroke_join_attr(stroke.line_join),
        stroke_cap_attr(stroke.line_cap),
    );
    match &stroke.style {
        Fill::None => (r#"stroke="none""#.into(), String::new()),
        Fill::Solid(p) => (
            format!(
                r#"stroke="rgb({},{},{})" stroke-width="{width}" stroke-opacity="{}"{extra}"#,
                (p.rgba[0] * 255.0) as u8,
                (p.rgba[1] * 255.0) as u8,
                (p.rgba[2] * 255.0) as u8,
                p.rgba[3],
            ),
            String::new(),
        ),
        Fill::LinearGradient {
            line_x0,
            line_y0,
            line_x1,
            line_y1,
            stops,
            ..
        } => {
            let stops_xml = stops_svg(stops);
            let defs = format!(
                r#"<defs><linearGradient id="{id}" gradientUnits="objectBoundingBox" x1="{line_x0}" y1="{line_y0}" x2="{line_x1}" y2="{line_y1}">{stops_xml}</linearGradient></defs>"#
            );
            (
                format!(r#"stroke="url(#{id})" stroke-width="{width}"{extra}"#),
                defs,
            )
        }
        Fill::RadialGradient {
            center_x,
            center_y,
            stops,
        } => {
            let stops_xml = stops_svg(stops);
            let defs = format!(
                r#"<defs><radialGradient id="{id}" cx="{center_x}" cy="{center_y}" r="0.5">{stops_xml}</radialGradient></defs>"#
            );
            (
                format!(r#"stroke="url(#{id})" stroke-width="{width}"{extra}"#),
                defs,
            )
        }
    }
}

fn stop_attr(p: &Paint) -> String {
    format!(
        r#"stop-color="rgb({},{},{})" stop-opacity="{}""#,
        (p.rgba[0] * 255.0) as u8,
        (p.rgba[1] * 255.0) as u8,
        (p.rgba[2] * 255.0) as u8,
        p.rgba[3],
    )
}

fn paint_attr(p: &Paint) -> String {
    format!(
        r#"fill="rgb({},{},{})" fill-opacity="{}""#,
        (p.rgba[0] * 255.0) as u8,
        (p.rgba[1] * 255.0) as u8,
        (p.rgba[2] * 255.0) as u8,
        p.rgba[3],
    )
}

fn path_to_svg_d(path: &PathData) -> String {
    let mut out = String::new();
    let mut pi = 0;
    for v in &path.verbs {
        match v {
            0 => {
                if pi < path.points.len() {
                    let p = path.points[pi];
                    out.push_str(&format!("M {} {} ", p[0], p[1]));
                    pi += 1;
                }
            }
            1 => {
                if pi < path.points.len() {
                    let p = path.points[pi];
                    out.push_str(&format!("L {} {} ", p[0], p[1]));
                    pi += 1;
                }
            }
            4 => out.push('Z'),
            _ => {}
        }
    }
    out
}

use crate::document::NodeId;
use kurbo::Rect;

pub fn export_selected_svg_string(project: &ProjectFile, selection: &[NodeId], bounds: Rect) -> String {
    let w = bounds.width();
    let h = bounds.height();
    let mut svg = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<g transform="translate({tx}, {ty})">
"#,
        tx = -bounds.x0,
        ty = -bounds.y0
    );
    for id in selection {
        let Some(node) = project.nodes.get(*id) else { continue };
        svg.push_str(&node_to_svg_fragment(node, &project.nodes));
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

pub fn render_svg_to_rgba(svg_data: &str, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg_data, &opt).ok()?;

    let pixmap_size = tree.size().to_int_size();
    let pixel_w = (pixmap_size.width() as f32 * scale).round() as u32;
    let pixel_h = (pixmap_size.height() as f32 * scale).round() as u32;
    
    if pixel_w == 0 || pixel_h == 0 {
        return None;
    }
    
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_w, pixel_h)?;
    
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    
    Some((pixel_w, pixel_h, pixmap.take()))
}