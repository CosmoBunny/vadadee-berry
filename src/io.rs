use std::fs;
use std::path::Path;

use kurbo::BezPath;
use thiserror::Error;

use crate::document::{
    ArcJoin, Document, Fill, LineCap, LineJoin, Node, NodeId, NodeKind, NodeStore, PageUnit, Paint,
    PathData, ProjectFile, Stroke, regular_polygon_vertices,
};

/// Decoded video layer pixels for one export frame.
#[derive(Debug, Clone)]
pub struct VideoLayerBuffer {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub type VideoFrameMap = rustc_hash::FxHashMap<uuid::Uuid, VideoLayerBuffer>;

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
    let stem = if stem.is_empty() {
        "untitled"
    } else {
        stem.as_str()
    };
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
    let opt = crate::fonts::usvg_options();
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
        clip_masks: Default::default(),
        boolean_effects: Default::default(),
        page_color: [1.0, 1.0, 1.0, 1.0],
        page_unit: PageUnit::Px,
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
    fs::write(
        path,
        document_svg_string(project, 0, &std::collections::HashMap::new()),
    )
    .map_err(|e| IoError::Msg(e.to_string()))
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

    let mut clip_defs = String::new();
    let mut clip_map = std::collections::HashMap::new();
    let mut mask_set = std::collections::HashSet::new();
    for cm in project.document.clip_masks.values() {
        clip_map.insert(cm.source_id, cm.clone());
        if cm.hide_mask {
            mask_set.insert(cm.mask_id);
        }
        if let Some(mask_node) = project.nodes.get(cm.mask_id) {
            let shape_svg = node_to_svg_fragment(mask_node, &project.nodes);
            clip_defs.push_str(&format!(
                r#"  <clipPath id="clip-{}">
    {}
  </clipPath>
"#,
                cm.id.as_simple(),
                shape_svg
            ));
        }
    }

    let mut defs_str = String::new();
    if !clip_defs.is_empty() {
        defs_str = format!("<defs>\n{}</defs>\n", clip_defs);
    }

    let mut svg = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
{}<rect width="{w}" height="{h}" {bg_color}/>
"#,
        defs_str
    );
    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        match layer.kind {
            crate::document::LayerKind::Image => {
                for id in &layer.nodes {
                    if mask_set.contains(id) {
                        continue;
                    }
                    let Some(node) = project.nodes.get(*id) else {
                        continue;
                    };
                    let node_svg = node_to_svg_fragment(node, &project.nodes);
                    if let Some(cm) = clip_map.get(id) {
                        svg.push_str(&format!(
                            r#"<g clip-path="url(#clip-{})">{}</g>"#,
                            cm.id.as_simple(),
                            node_svg
                        ));
                    } else {
                        svg.push_str(&node_svg);
                    }
                }
            }
            crate::document::LayerKind::AV => {
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
            crate::document::LayerKind::Shading => {}
            crate::document::LayerKind::Flowchart => {}
            crate::document::LayerKind::ScreenRecord => {}
            crate::document::LayerKind::NodeEditor => {
                // P5: note file-path Output as image reference when path is absolute PNG/JPG.
                if let Some(g) = layer.node_graph.as_ref() {
                    let eval = g.resolve_output_image();
                    if let crate::document::GraphImageSource::FilePath(path) = &eval.image {
                        let lower = path.to_ascii_lowercase();
                        if lower.ends_with(".png")
                            || lower.ends_with(".jpg")
                            || lower.ends_with(".jpeg")
                        {
                            let dx = layer.x as f64 + eval.geo_off_x;
                            let dy = layer.y as f64 + eval.geo_off_y;
                            let w = layer.width as f64 * eval.geo_scale_w;
                            let h = layer.height as f64 * eval.geo_scale_h;
                            // External href — exporters that resolve paths can embed later.
                            svg.push_str(&format!(
                                r#"<image href="{}" x="{dx}" y="{dy}" width="{w}" height="{h}" opacity="1"/>"#,
                                path.replace('&', "&amp;").replace('"', "&quot;")
                            ));
                            svg.push('\n');
                        }
                    }
                }
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Rasterize a single node into a tight SVG view box (transparent background).
pub fn node_svg_for_bounds(
    node: &Node,
    bounds: kurbo::Rect,
    nodes: &crate::document::NodeStore,
) -> String {
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
    let arc_open = matches!(
        &node.kind,
        NodeKind::Arc {
            join: ArcJoin::NoJoin,
            ..
        }
    );
    let (fill, fill_defs) = if arc_open {
        (r#"fill="none""#.into(), String::new())
    } else {
        fill_svg(&node.style.fill, &fill_grad_id)
    };
    let (stroke, stroke_defs) =
        if node.style.stroke.width > 0.0 && node.style.stroke.style.is_visible() {
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
        NodeKind::Plotter {
            x,
            y,
            w,
            h,
            plot_stroke_rgba,
            plot_stroke_width,
            ..
        } => {
            let mut s = format!(
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" {fill} {stroke} opacity="{op}"/>"#
            );
            if let Some((pts, _, _)) = node.plotter_polyline() {
                if pts.len() >= 2 {
                    let mut d = String::new();
                    for (i, (px, py)) in pts.iter().enumerate() {
                        if i == 0 {
                            d.push_str(&format!("M{px} {py}"));
                        } else {
                            d.push_str(&format!(" L{px} {py}"));
                        }
                    }
                    let pr = (plot_stroke_rgba[0] * 255.0) as u8;
                    let pg = (plot_stroke_rgba[1] * 255.0) as u8;
                    let pb = (plot_stroke_rgba[2] * 255.0) as u8;
                    let pa = plot_stroke_rgba[3];
                    s.push_str(&format!(
                        r#"<path d="{d}" fill="none" stroke="rgba({pr},{pg},{pb},{pa})" stroke-width="{plot_stroke_width}" opacity="{op}"/>"#
                    ));
                }
            }
            s
        }
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
            let family = crate::fonts::sanitize_svg_font_family(&style.font_family);
            let escaped = style
                .content
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            // Canvas treats (x,y) as top-left of the glyph box; SVG text y is baseline —
            // hang so unrotated export matches on-canvas placement.
            let text_el = format!(
                r#"<text x="{x}" y="{y}" font-size="{}" font-family="{family}" font-weight="{weight}" font-style="{font_style}" dominant-baseline="hanging" {fill} {stroke} opacity="{op}">{escaped}</text>"#,
                style.font_size
            );
            // Live paint rotates about glyph-box center via transform.rotation_rad.
            // Export used to drop that → upright text after video export.
            let rot = node.transform.rotation_rad;
            if rot.abs() > 1e-12 {
                let b = crate::document::text_bounds(*x, *y, style);
                let cx = (b.x0 + b.x1) * 0.5;
                let cy = (b.y0 + b.y1) * 0.5;
                format!(
                    r#"<g transform="rotate({} {cx} {cy})">{text_el}</g>"#,
                    rot.to_degrees()
                )
            } else {
                text_el
            }
        }
        NodeKind::Group { children } => {
            let mut inner = String::new();
            for cid in children {
                if let Some(child) = nodes.get(*cid) {
                    inner.push_str(&node_to_svg_fragment(child, nodes));
                }
            }
            let rot = node.transform.rotation_rad;
            if rot.abs() > 1e-12 {
                let b = node.bounds_with_store(nodes);
                let cx = (b.x0 + b.x1) * 0.5;
                let cy = (b.y0 + b.y1) * 0.5;
                format!(
                    r#"<g transform="rotate({} {cx} {cy})">{inner}</g>"#,
                    rot.to_degrees()
                )
            } else {
                format!(r#"<g>{inner}</g>"#)
            }
        }
        NodeKind::Image {
            x,
            y,
            width,
            height,
            bytes,
            ..
        } => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            let img_el = format!(
                r#"<image x="{x}" y="{y}" width="{width}" height="{height}" href="data:image/png;base64,{b64}" opacity="{op}"/>"#
            );
            let rot = node.transform.rotation_rad;
            if rot.abs() > 1e-12 {
                let cx = *x + *width * 0.5;
                let cy = *y + *height * 0.5;
                format!(
                    r#"<g transform="rotate({} {cx} {cy})">{img_el}</g>"#,
                    rot.to_degrees()
                )
            } else {
                img_el
            }
        }
        NodeKind::Arc {
            cx,
            cy,
            radius,
            start_angle_rad,
            sweep_angle_rad,
            join,
        } => {
            let bez = crate::document::build_arc_bez(
                *cx,
                *cy,
                *radius,
                *start_angle_rad,
                *sweep_angle_rad,
                *join,
            );
            let d = bez.to_svg();
            format!(r#"<path d="{d}" {fill} {stroke} opacity="{op}"/>"#)
        }
        NodeKind::BrushStroke { points } => {
            let mut svg = String::new();
            for (pos, width) in points {
                let r = width / 2.0;
                if r > 0.1 {
                    svg.push_str(&format!(
                        r#"<circle cx="{}" cy="{}" r="{}" {fill} opacity="{op}"/>"#,
                        pos[0], pos[1], r
                    ));
                }
            }
            svg
        }
        NodeKind::FlowchartNode {
            cx,
            cy,
            w,
            h,
            corner_rx,
            ..
        } => {
            format!(
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" {fill} {stroke} opacity="{op}"/>"#,
                cx - w / 2.0,
                cy - h / 2.0,
                w,
                h,
                corner_rx
            )
        }
        NodeKind::FlowchartPath { path } => {
            if path.points.is_empty() {
                String::new()
            } else {
                let pts: Vec<String> = path
                    .points
                    .iter()
                    .map(|(px, py)| format!("{px},{py}"))
                    .collect();
                format!(
                    r#"<polyline points="{}" {fill} {stroke} opacity="{op}"/>"#,
                    pts.join(" ")
                )
            }
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
    path.to_bez().to_svg()
}

use kurbo::Rect;

/// Selected nodes in bottom-to-top paint order (matches canvas).
pub fn selection_paint_order(project: &ProjectFile, selection: &[NodeId]) -> Vec<NodeId> {
    let set: std::collections::HashSet<NodeId> = selection.iter().copied().collect();
    project
        .document
        .ordered_node_ids()
        .into_iter()
        .filter(|id| set.contains(id))
        .collect()
}

pub fn export_selected_svg_string(
    project: &ProjectFile,
    selection: &[NodeId],
    bounds: Rect,
) -> String {
    let ordered = selection_paint_order(project, selection);
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
    for id in &ordered {
        let Some(node) = project.nodes.get(*id) else {
            continue;
        };
        svg.push_str(&node_to_svg_fragment(node, &project.nodes));
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

/// Rasterize current selection (merged) to RGBA.
///
/// Uses the live preview renderer ([`crate::export_render`]) at export scale —
/// no SVG intermediate, so text layout/baselines/rotation match the canvas.
pub fn rasterize_selection_rgba(
    project: &ProjectFile,
    selection: &[NodeId],
    bounds: Rect,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    if selection.is_empty() {
        return None;
    }
    crate::export_render::render_selection_rgba(project, selection, bounds, scale)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportImageFormat {
    Png,
    Jpeg,
    Bmp,
    /// Width, height, then premultiplied RGBA8 (little-endian header).
    RawRgba,
}

impl ExportImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "Bitmap (BMP)",
            Self::RawRgba => "Raw RGBA",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
            Self::RawRgba => "rgba",
        }
    }
}

pub fn write_image_file(
    path: &Path,
    format: ExportImageFormat,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), IoError> {
    match format {
        ExportImageFormat::RawRgba => {
            let mut out = Vec::with_capacity(8 + rgba.len());
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(rgba);
            fs::write(path, out).map_err(|e| IoError::Msg(e.to_string()))
        }
        ExportImageFormat::Png | ExportImageFormat::Jpeg | ExportImageFormat::Bmp => {
            let Some(img) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
                return Err(IoError::Msg("Invalid RGBA buffer".into()));
            };
            match format {
                ExportImageFormat::Png => {
                    img.save(path).map_err(|e| IoError::Msg(e.to_string()))?;
                }
                ExportImageFormat::Jpeg => {
                    let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
                    rgb.save_with_format(path, image::ImageFormat::Jpeg)
                        .map_err(|e| IoError::Msg(e.to_string()))?;
                }
                ExportImageFormat::Bmp => {
                    let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
                    rgb.save_with_format(path, image::ImageFormat::Bmp)
                        .map_err(|e| IoError::Msg(e.to_string()))?;
                }
                ExportImageFormat::RawRgba => unreachable!(),
            }
            Ok(())
        }
    }
}

/// Full-document raster export through the live preview renderer
/// ([`crate::export_render`]) — same painter, same text layout as the canvas.
/// (Vector SVG export stays on `document_svg_string`; only PNG/JPEG/BMP
/// changed: they must not round-trip through SVG.)
///
/// NodeEditor FilePath/BakedCache output composites on top via the shared
/// [`composite_ne_file_output`] software path (same policy input shape as
/// video fallback, but baked at full still resolution). Skipped when the
/// page itself is translucent: the pixmap round-trip is only straight-safe
/// over an opaque background.
///
/// Shading layers apply through the same [`apply_shading_passes_skia_public`]
/// CPU path as the video fallback (frozen time): still export must show the
/// same presets instead of a flat page. Unknown custom WGSL is a no-op in
/// both — only the GPU video path runs real WGSL.
pub fn export_document_raster(
    project: &ProjectFile,
    format: ExportImageFormat,
    scale: f32,
    path: &Path,
) -> Result<(), IoError> {
    let (w, h, rgba) = crate::export_render::render_document_rgba(project, scale)
        .ok_or_else(|| IoError::Msg("Rasterize failed".into()))?;
    let needs_ne = project.document.layers.iter().any(|l| {
        if !l.visible || !l.is_renderer || l.kind != crate::document::LayerKind::NodeEditor {
            return false;
        }
        l.node_graph.as_ref().is_some_and(|g| {
            matches!(
                g.resolve_output_image().image,
                crate::document::GraphImageSource::FilePath(_)
                    | crate::document::GraphImageSource::BakedCache { .. }
            )
        })
    });
    let needs_shading = project.document.layers.iter().any(|l| {
        l.visible
            && l.is_renderer
            && l.kind == crate::document::LayerKind::Shading
            && l.shading_passes.iter().any(|p| p.enabled)
    });
    if (needs_ne || needs_shading) && project.document.page_color[3] >= 0.999 {
        let target = crate::render_pipeline::RenderTarget {
            width: w,
            height: h,
            scale,
        };
        let mut pixmap = straight_rgba_to_pixmap(w, h, &rgba)
            .ok_or_else(|| IoError::Msg("Rasterize failed".into()))?;
        // Full still resolution (bake caps internally at 4096).
        let bake_side = w.max(h).max(64);
        for layer in &project.document.layers {
            if !layer.visible || !layer.is_renderer {
                continue;
            }
            match layer.kind {
                crate::document::LayerKind::Shading => {
                    apply_shading_passes_skia_public(&mut pixmap, &layer.shading_passes, 0.0);
                }
                crate::document::LayerKind::NodeEditor => {
                    composite_ne_file_output(&mut pixmap, project, layer, &target, bake_side);
                }
                _ => {}
            }
        }
        // Alpha is 1 everywhere (opaque page + over-composite) → take() is
        // straight-safe.
        write_image_file(path, format, w, h, &pixmap.take())
    } else {
        write_image_file(path, format, w, h, &rgba)
    }
}

pub fn export_selection_raster(
    project: &ProjectFile,
    selection: &[NodeId],
    bounds: Rect,
    format: ExportImageFormat,
    scale: f32,
    path: &Path,
) -> Result<(), IoError> {
    let (w, h, rgba) = rasterize_selection_rgba(project, selection, bounds, scale)
        .ok_or_else(|| IoError::Msg("Selection rasterize failed".into()))?;
    write_image_file(path, format, w, h, &rgba)
}

/// Document SVG cropped to a document-space rectangle (`viewBox`).
pub fn document_svg_for_view(
    project: &ProjectFile,
    view: kurbo::Rect,
    current_frame: usize,
    video_frames: &std::collections::HashMap<uuid::Uuid, Vec<u8>>,
) -> String {
    let mut svg = document_svg_string(project, current_frame, video_frames);
    let vw = view.width().max(1.0);
    let vh = view.height().max(1.0);
    if let Some(start) = svg.find("<svg ") {
        if let Some(rel) = svg[start..].find('>') {
            let end = start + rel + 1;
            let head = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{vw}" height="{vh}" viewBox="{} {} {vw} {vh}""#,
                view.x0, view.y0,
            );
            svg.replace_range(start..end, &(head + ">"));
        }
    }
    svg
}

pub fn default_document_view(project: &ProjectFile) -> kurbo::Rect {
    kurbo::Rect::new(0.0, 0.0, project.document.width, project.document.height)
}

pub fn resolve_capture_view(
    project: &ProjectFile,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> kurbo::Rect {
    let full = default_document_view(project);
    let x0 = x.unwrap_or(full.x0);
    let y0 = y.unwrap_or(full.y0);
    let ww = w.unwrap_or(full.width());
    let hh = h.unwrap_or(full.height());
    kurbo::Rect::new(x0, y0, x0 + ww.max(1.0), y0 + hh.max(1.0))
}

/// Rasterize a document region. `resolution_percent` is 1..100 (100 = 1:1 px per doc unit).
///
/// Uses the same layer stack as export (Node Editor bake + shading + images), not SVG-only,
/// so galaxy shaders and NE video appear in MCP snapshots / previews.
pub fn rasterize_document_view(
    project: &ProjectFile,
    view: kurbo::Rect,
    resolution_percent: f32,
    current_frame: usize,
    video_frames: &std::collections::HashMap<uuid::Uuid, Vec<u8>>,
) -> Option<(u32, u32, Vec<u8>)> {
    let scale = (resolution_percent / 100.0).clamp(0.01, 2.0);
    // Prefer wall-clock-ish time for animated shaders when frame scrub is 0.
    let time_secs = (current_frame as f32 / 24.0).max(0.0);
    // Convert loose HashMap to Fx map for composite (empty AV still ok — NE bakes itself).
    let mut vf: VideoFrameMap = rustc_hash::FxHashMap::default();
    for (k, v) in video_frames {
        // Only used if caller packs VideoLayerBuffer elsewhere; bare RGBA map is unused here.
        let _ = (k, v);
    }
    let (pw, ph, mut rgba) = {
        let target = crate::render_pipeline::RenderTarget::for_video(
            project.document.width,
            project.document.height,
            scale,
        )?;
        let ctx = crate::render_pipeline::RenderContext::new(current_frame, time_secs);
        composite_export_frame(project, &ctx, &vf, &target)?
    };

    // Crop to requested view when not the full page.
    let doc_w = project.document.width;
    let doc_h = project.document.height;
    let full = (view.x0.abs() < 0.5
        && view.y0.abs() < 0.5
        && (view.width() - doc_w).abs() < 1.0
        && (view.height() - doc_h).abs() < 1.0)
        || view.width() >= doc_w - 1.0 && view.height() >= doc_h - 1.0;
    if full {
        return Some((pw, ph, rgba));
    }
    let x0 = ((view.x0 / doc_w) * pw as f64).floor().max(0.0) as u32;
    let y0 = ((view.y0 / doc_h) * ph as f64).floor().max(0.0) as u32;
    let x1 = (((view.x0 + view.width()) / doc_w) * pw as f64)
        .ceil()
        .min(pw as f64) as u32;
    let y1 = (((view.y0 + view.height()) / doc_h) * ph as f64)
        .ceil()
        .min(ph as f64) as u32;
    let cw = x1.saturating_sub(x0).max(1);
    let ch = y1.saturating_sub(y0).max(1);
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    for row in 0..ch {
        let src_y = y0 + row;
        if src_y >= ph {
            break;
        }
        let src_off = ((src_y * pw + x0) * 4) as usize;
        let dst_off = (row * cw * 4) as usize;
        let n = (cw * 4) as usize;
        if src_off + n <= rgba.len() && dst_off + n <= out.len() {
            out[dst_off..dst_off + n].copy_from_slice(&rgba[src_off..src_off + n]);
        }
    }
    let _ = &mut rgba;
    Some((cw, ch, out))
}

pub fn render_svg_to_rgba(svg_data: &str, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let opt = crate::fonts::usvg_options();
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

pub(crate) fn layer_anim_transform(
    layer: &crate::document::Layer,
    project: &ProjectFile,
    current_frame: usize,
) -> (f64, f64, f64, f32) {
    let mut dx = layer.x as f64;
    let mut dy = layer.y as f64;
    let mut rot = layer.rotation as f64;
    let mut opacity = 1.0f32;
    if let Some(track) = project.anim_timeline.nodes.get(&layer.id) {
        if let Some(o) = track.opacity.interpolate(current_frame) {
            opacity = o as f32;
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
    (dx, dy, rot, opacity)
}

pub(crate) fn video_layer_dest_size(
    layer: &crate::document::Layer,
    frame_w: u32,
    frame_h: u32,
) -> (f32, f32) {
    let aspect = if frame_h > 0 {
        frame_w as f32 / frame_h as f32
    } else {
        1.0
    };
    let mut w = layer.width;
    let mut h = layer.height;
    if layer.aspect_ratio_locked {
        if w / h > aspect {
            w = h * aspect;
        } else {
            h = w / aspect;
        }
    }
    (w, h)
}

/// Raster export frame following document layer stack order (bottom → top).
/// CPU fallback compositor: dimensions and overlay transforms come from the
/// authoritative [`crate::render_pipeline`] contract. Static vector layers
/// (Image/Flowchart) are painted with the same `draw_nodes_ex` + effect
/// passes as preview via [`crate::export_render::render_static_order_rgba`]
/// — no SVG `<text>` reconstruction. Only true overlays (decoded AV pixels,
/// CPU shading heuristics, NodeEditor bakes) composite as pixmaps.
///
/// `target` is the caller-declared size promise (even dimensions for video);
/// `ctx` carries frame/time. Callers build both via the contract types —
/// this function never derives sizes or animation state itself.
///
/// Segments share one [`crate::export_render::PainterSession`] for the call
/// (fonts/atlas/decoded images compiled once, shapes re-recorded per flush).
pub fn composite_export_frame(
    project: &ProjectFile,
    ctx: &crate::render_pipeline::RenderContext,
    video_frames: &VideoFrameMap,
    target: &crate::render_pipeline::RenderTarget,
) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::tiny_skia::{Color, Pixmap};

    // Single authoritative size: even dimensions for video encoders.
    let (pixel_w, pixel_h) = (target.width, target.height);
    if pixel_w == 0 || pixel_h == 0 {
        return None;
    }

    let mut pixmap = Pixmap::new(pixel_w, pixel_h)?;
    let pc = project.document.page_color;
    let bg = Color::from_rgba(
        pc[0].clamp(0.0, 1.0),
        pc[1].clamp(0.0, 1.0),
        pc[2].clamp(0.0, 1.0),
        pc[3].clamp(0.0, 1.0),
    )
    .unwrap_or(Color::WHITE);
    pixmap.fill(bg);

    // Consecutive static layers accumulate here and flush as ONE painter
    // segment, preserving stack order against interleaved overlays.
    let mut pending_static: Vec<crate::document::NodeId> = Vec::new();
    let mut session = crate::export_render::PainterSession::new();

    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        match layer.kind {
            crate::document::LayerKind::AV => {
                flush_static_segment(
                    &mut pixmap,
                    project,
                    &mut pending_static,
                    target,
                    &mut session,
                );
                let Some(buf) = video_frames.get(&layer.id) else {
                    continue;
                };
                blit_av_layer(&mut pixmap, target, project, layer, buf, ctx)?;
            }
            crate::document::LayerKind::Image | crate::document::LayerKind::Flowchart => {
                // Static vector content: painter segment, NOT svg→resvg.
                // (Flowchart previously rendered nothing here — now matches preview.)
                pending_static.extend(layer.nodes.iter().copied());
            }
            crate::document::LayerKind::Shading => {
                flush_static_segment(
                    &mut pixmap,
                    project,
                    &mut pending_static,
                    target,
                    &mut session,
                );
                apply_shading_passes_skia(&mut pixmap, &layer.shading_passes, ctx.time_secs);
            }
            crate::document::LayerKind::ScreenRecord => {
                flush_static_segment(
                    &mut pixmap,
                    project,
                    &mut pending_static,
                    target,
                    &mut session,
                );
            }
            crate::document::LayerKind::NodeEditor => {
                flush_static_segment(
                    &mut pixmap,
                    project,
                    &mut pending_static,
                    target,
                    &mut session,
                );
                // P6c: software path for NE Output (GPU export uses export_worker caches).
                composite_ne_file_output(
                    &mut pixmap,
                    project,
                    layer,
                    target,
                    pixel_w.max(pixel_h).clamp(256, 2048).min(512),
                );
            }
        }
    }
    flush_static_segment(
        &mut pixmap,
        project,
        &mut pending_static,
        target,
        &mut session,
    );

    // NE AppObjects: base segments hide these sources (painted at the NE
    // slot instead), so composite them here — same painter as preview, same
    // position as the worker fast path. Without this, thumbnails of such
    // docs lose the content entirely.
    for layer in &project.document.layers {
        if !layer.visible
            || !layer.is_renderer
            || layer.kind != crate::document::LayerKind::NodeEditor
        {
            continue;
        }
        let Some(g) = &layer.node_graph else {
            continue;
        };
        let eval = g.resolve_output_image();
        let crate::document::GraphImageSource::AppObjects(ids) = &eval.image else {
            continue;
        };
        if ids.is_empty() {
            continue;
        }
        if let Some(rgba) = session.render_ne_appobjects(project, ids, target) {
            blit_transparent_full_frame(&mut pixmap, pixel_w, pixel_h, &rgba);
        }
    }

    Some((pixel_w, pixel_h, pixmap.take()))
}

/// Composite a NodeEditor layer's FilePath/BakedCache output onto a frame.

/// Composite a NodeEditor layer's FilePath/BakedCache output onto a frame.
///
/// Shared by the video fallback compositor and still export: one bake policy
/// input (`bake_max_side`), one geometry fit, one transform path. Returns
/// true when something was composited. `bake_max_side` differs by consumer
/// (video fallback caps low for speed; still export uses full target size,
/// bake caps internally at 4096) — same code path, different resolution
/// promise, no third policy.
pub(crate) fn composite_ne_file_output(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    project: &ProjectFile,
    layer: &crate::document::Layer,
    target: &crate::render_pipeline::RenderTarget,
    bake_max_side: u32,
) -> bool {
    let Some(g) = &layer.node_graph else {
        return false;
    };
    let eval = g.resolve_output_image();
    if !matches!(
        eval.image,
        crate::document::GraphImageSource::FilePath(_)
            | crate::document::GraphImageSource::BakedCache { .. }
    ) {
        return false;
    }
    let Some(rgba) = crate::document::bake_graph_eval_rgba(&eval, bake_max_side, 1.0, None, None)
    else {
        return false;
    };
    let (tw, th) = rgba.dimensions();
    let Some(src) = straight_rgba_to_pixmap(tw, th, &rgba) else {
        return false;
    };
    let (dx, dy, mut w, mut h, rot_rad) = layer.ne_output_paint_geom(&project.nodes, &eval);
    let doc_w = project.document.width;
    let doc_h = project.document.height;
    let def_w = layer.width as f64;
    let def_h = layer.height as f64;
    let near_default = (w - def_w).abs() < 2.0 && (h - def_h).abs() < 2.0;
    let near_a4 = (w - crate::document::A4_WIDTH_PX).abs() < 2.0
        && (h - crate::document::A4_HEIGHT_PX).abs() < 2.0;
    if near_default || near_a4 {
        let page_w = doc_w.max(1.0);
        let page_h = doc_h.max(1.0);
        let mut nw = tw as f64;
        let mut nh = th as f64;
        if nw > page_w || nh > page_h {
            let s = (page_w / nw).min(page_h / nh);
            nw *= s;
            nh *= s;
        }
        w = nw.max(1.0);
        h = nh.max(1.0);
    }
    let rot_deg = rot_rad.to_degrees() as f32;
    let transform =
        crate::render_pipeline::pixmap_transform(&target, dx, dy, w, h, rot_deg, tw, th);
    let paint = resvg::tiny_skia::PixmapPaint::default();
    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
    true
}

/// Flush one pending static segment: paint accumulated vector nodes with the
/// same painter + effect passes as preview, then blit in stack order.
/// Shared by both frame compositors so segment boundaries can't drift.
/// Paints on the caller's [`crate::export_render::PainterSession`] so font
/// installs, atlas and decoded images survive across segments and frames.
pub(crate) fn flush_static_segment(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    project: &ProjectFile,
    pending: &mut Vec<crate::document::NodeId>,
    target: &crate::render_pipeline::RenderTarget,
    session: &mut crate::export_render::PainterSession,
) {
    if pending.is_empty() {
        return;
    }
    if let Some(rgba) = session.render_static_order(project, pending, target) {
        blit_transparent_full_frame(pixmap, target.width, target.height, &rgba);
    }
    pending.clear();
}

/// Upload straight-sRGBA bytes into a premultiplied pixmap.
///
/// tiny-skia blends in premultiplied space; copying straight bytes in raw
/// brightens translucent fringes (antialiased text/rect edges, soft bake
/// alpha) versus what the painter produced. Opaque pixels are unaffected.
pub(crate) fn straight_rgba_to_pixmap(
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Option<resvg::tiny_skia::Pixmap> {
    if rgba.len() != (w * h * 4) as usize {
        return None;
    }
    let mut px = resvg::tiny_skia::Pixmap::new(w, h)?;
    for (d, s) in px.data_mut().chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        let a = s[3] as u32;
        d[0] = ((s[0] as u32 * a + 127) / 255) as u8;
        d[1] = ((s[1] as u32 * a + 127) / 255) as u8;
        d[2] = ((s[2] as u32 * a + 127) / 255) as u8;
        d[3] = s[3];
    }
    Some(px)
}

/// Composite one decoded AV frame at its animated layer geometry.
/// Shared by both frame compositors: single `pixmap_transform` path, single
/// opacity handling.
pub(crate) fn blit_av_layer(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    target: &crate::render_pipeline::RenderTarget,
    project: &ProjectFile,
    layer: &crate::document::Layer,
    buf: &VideoLayerBuffer,
    ctx: &crate::render_pipeline::RenderContext,
) -> Option<()> {
    let src = straight_rgba_to_pixmap(buf.width, buf.height, &buf.rgba)?;
    let (dx, dy, rot, opacity) = layer_anim_transform(layer, project, ctx.frame);
    let (dw, dh) = video_layer_dest_size(layer, buf.width, buf.height);
    let transform = crate::render_pipeline::pixmap_transform(
        target, dx, dy, dw as f64, dh as f64, rot as f32, buf.width, buf.height,
    );
    let mut paint = resvg::tiny_skia::PixmapPaint::default();
    paint.opacity = opacity;
    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
    Some(())
}

/// Blit a full-frame transparent painter buffer onto the frame pixmap.
/// Straight-sRGBA bytes are repacked through tiny-skia so alpha composites
/// instead of overwriting.
pub(crate) fn blit_transparent_full_frame(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    w: u32,
    h: u32,
    rgba: &[u8],
) {
    if rgba.len() != (w * h * 4) as usize {
        return;
    }
    let Some(src) = straight_rgba_to_pixmap(w, h, rgba) else {
        return;
    };
    let paint = resvg::tiny_skia::PixmapPaint::default();
    pixmap.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &paint,
        resvg::tiny_skia::Transform::identity(),
        None,
    );
}

/// CPU shading for named presets (galaxy / starfield / blackhole). Used by export fast path.
pub fn apply_shading_passes_skia_public(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    passes: &[crate::document::ShadingPass],
    time_secs: f32,
) {
    apply_shading_passes_skia(pixmap, passes, time_secs);
}

fn apply_shading_passes_skia(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    passes: &[crate::document::ShadingPass],
    time_secs: f32,
) {
    if let Some(pass) = passes.first().filter(|p| p.enabled) {
        let name = pass.name.to_ascii_lowercase();
        let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl);
        // Only named built-in presets use CPU heuristics — never hijack custom WGSL
        // that merely mentions "star" (e.g. galaxy shaders).
        let is_blackhole = name == "blackhole" || name.starts_with("blackhole ");
        let is_starfield = matches!(
            name.as_str(),
            "starfield" | "stars" | "space" | "star field"
        ) || name.starts_with("starfield");

        let is_galaxy = name.contains("galaxy")
            || wgsl.contains("Procedural galaxy")
            || wgsl.contains("// Procedural galaxy");

        if is_galaxy || is_starfield {
            let w = pixmap.width();
            let h = pixmap.height();
            let aspect = (w as f32 / (h as f32).max(1.0)).max(0.25);

            let t = if pass.uniforms.len() >= 1 {
                pass.uniforms[0] + time_secs
            } else {
                time_secs
            };

            let data = pixmap.data_mut();
            for y in 0..h {
                let v = (y as f32 + 0.5) / h as f32;
                let row_offset = y as usize * w as usize * 4;
                for x in 0..w {
                    let u_val = (x as f32 + 0.5) / w as f32;
                    let rgb = if is_galaxy {
                        crate::shading::procedural_blackhole::sample_galaxy((u_val, v), t, aspect)
                    } else {
                        crate::shading::procedural_blackhole::sample_starfield(
                            (u_val, v),
                            t,
                            aspect,
                        )
                    };
                    let idx = row_offset + x as usize * 4;
                    data[idx] = rgb[0];
                    data[idx + 1] = rgb[1];
                    data[idx + 2] = rgb[2];
                    data[idx + 3] = 255;
                }
            }
        } else if is_blackhole {
            let w = pixmap.width() as f32;
            let h = pixmap.height() as f32;

            let mut u = crate::shading::procedural_blackhole::BlackholeParams::default();
            if pass.uniforms.len() >= 3 {
                u.time = pass.uniforms[0] + time_secs;
                u.strength = pass.uniforms[1];
                u.disk_radius = pass.uniforms[2];
            } else {
                u.time = time_secs;
            }
            u.aspect = (w / h.max(1.0)).max(0.25);

            let cols = 160usize;
            let rows = ((cols as f32 * h / w).ceil() as usize).clamp(90, 200);
            let cw = w / cols as f32;
            let ch = h / rows as f32;

            for row in 0..rows {
                for col in 0..cols {
                    let x0 = col as f32 * cw;
                    let y0 = row as f32 * ch;
                    let x1 = x0 + cw;
                    let y1 = y0 + ch;

                    let u0 = col as f32 / cols as f32;
                    let v0 = row as f32 / rows as f32;
                    let u1 = (col + 1) as f32 / cols as f32;
                    let v1 = (row + 1) as f32 / rows as f32;

                    let rgb = crate::shading::procedural_blackhole::sample(
                        ((u0 + u1) * 0.5, (v0 + v1) * 0.5),
                        &u,
                    );

                    if let Some(r_rect) = resvg::tiny_skia::Rect::from_ltrb(x0, y0, x1, y1) {
                        let mut paint = resvg::tiny_skia::Paint::default();
                        paint.set_color(resvg::tiny_skia::Color::from_rgba8(
                            rgb[0], rgb[1], rgb[2], 255,
                        ));
                        pixmap.fill_rect(
                            r_rect,
                            &paint,
                            resvg::tiny_skia::Transform::identity(),
                            None,
                        );
                    }
                }
            }
        } else if name.contains("crt") || wgsl.contains("scan") {
            let w = pixmap.width() as usize;
            let h = pixmap.height() as usize;
            let data = pixmap.data_mut();

            for y in 0..h {
                if y % 3 == 0 {
                    let row_offset = y * w * 4;
                    for x in 0..w {
                        let idx = row_offset + x * 4;
                        data[idx] = (data[idx] as f32 * 0.89) as u8;
                        data[idx + 1] = (data[idx + 1] as f32 * 0.89) as u8;
                        data[idx + 2] = (data[idx + 2] as f32 * 0.89) as u8;
                    }
                }
            }
            apply_vignette_pixels(pixmap, 0.35);
        } else if name.contains("vignette") {
            apply_vignette_pixels(pixmap, 0.65);
        }
    }
}

fn apply_vignette_pixels(pixmap: &mut resvg::tiny_skia::Pixmap, strength: f32) {
    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let radius = w.max(h) as f32 * 0.55;
    let data = pixmap.data_mut();

    for y in 0..h {
        let dy = y as f32 - cy;
        let row_offset = y * w * 4;
        for x in 0..w {
            let dx = x as f32 - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            let t = dist / radius;
            let alpha = (t * strength * 0.55).clamp(0.0, 0.85);
            let factor = 1.0 - alpha;
            let idx = row_offset + x * 4;
            data[idx] = (data[idx] as f32 * factor) as u8;
            data[idx + 1] = (data[idx + 1] as f32 * factor) as u8;
            data[idx + 2] = (data[idx + 2] as f32 * factor) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_svg_d_includes_cubic_segments() {
        let mut path = PathData {
            verbs: vec![0, 1, 1, 1, 4],
            points: vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [20.0, 0.0],
                [20.0, 10.0],
                [10.0, 20.0],
            ],
            closed: true,
            smooth_anchors: Vec::new(),
            handle_out_offset: Default::default(),
            handle_in_offset: Default::default(),
            handle_modes: Default::default(),
            corner_fillets: Default::default(),
        };
        // Trigger a cubic via corner fillet (non-destructive arc approx)
        path.set_corner_fillet(2, 3.0);
        let d = path_to_svg_d(&path);
        assert!(d.contains('C') || d.contains('c'), "expected cubic in {d}");
        assert!(d.contains('Z') || d.contains('z'));
    }

    #[test]
    fn text_svg_export_includes_transform_rotation() {
        use crate::document::{Node, NodeStore, TextStyle};
        let mut node = Node::text(
            10.0,
            20.0,
            TextStyle {
                content: "hello".into(),
                font_size: 24.0,
                ..Default::default()
            },
        );
        node.set_rotation(std::f64::consts::FRAC_PI_4); // 45°
        let store = NodeStore::default();
        let svg = node_to_svg_fragment(&node, &store);
        assert!(
            svg.contains("rotate("),
            "export SVG must keep text rotation, got: {svg}"
        );
        assert!(
            svg.contains("dominant-baseline=\"hanging\""),
            "top-left text origin for canvas parity"
        );
    }

    /// End-to-end: hybrid export rasterizes Image-layer text via resvg SVG.
    /// Rotated text must paint a different pixel footprint than upright text.
    /// Uses the same font options as real video export (`fonts::usvg_options`).
    #[test]
    fn text_rotation_survives_svg_raster_export_path() {
        use crate::document::{Fill, Node, NodeStore, Paint, TextStyle};
        use resvg::tiny_skia::{Pixmap, Transform};

        fn raster_text(rot_rad: f64) -> Pixmap {
            let mut node = Node::text(
                80.0,
                80.0,
                TextStyle {
                    content: "EXPORT".into(),
                    font_size: 48.0,
                    ..Default::default()
                },
            );
            // Opaque black so ink is unambiguous against white page.
            node.style.fill = Fill::Solid(Paint::from_hex(0x000000, 1.0));
            node.set_rotation(rot_rad);
            let store = NodeStore::default();
            let frag = node_to_svg_fragment(&node, &store);
            let svg = format!(
                r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" fill="white"/>{frag}</svg>"#
            );
            // Vector SVG text path (unchanged feature: .svg file export).
            let opt = crate::fonts::usvg_options();
            let tree = usvg::Tree::from_str(&svg, &opt).expect("parse svg");
            let mut pm = Pixmap::new(240, 240).expect("pixmap");
            resvg::render(&tree, Transform::identity(), &mut pm.as_mut());
            pm
        }

        let upright = raster_text(0.0);
        let rotated = raster_text(std::f64::consts::FRAC_PI_4);
        let ink = |pm: &Pixmap| {
            pm.data()
                .chunks(4)
                .filter(|c| c[0] < 250 || c[1] < 250 || c[2] < 250)
                .count()
        };
        assert!(
            ink(&upright) > 50,
            "upright text not painted (font/fill broken)"
        );
        assert!(
            ink(&rotated) > 50,
            "rotated text not painted (font/fill broken)"
        );
        // Pixel footprints must differ — proves rotation is not dropped before resvg.
        assert_ne!(
            upright.data(),
            rotated.data(),
            "rotated text raster identical to upright — export would still drop rotation"
        );
    }

    /// Render-contract lock: the video frame compositor must reproduce the
    /// authoritative still export for static documents (rect + text, no
    /// AV/shading/NE layers). Any future scene reinterpretation in either
    /// path breaks this instead of shipping a preview≠video discrepancy.
    #[test]
    fn composite_frame_matches_document_export_for_static_doc() {
        use crate::document::{Document, Fill, Node, Paint, TextStyle};

        let mut project = Document::new_empty_project();
        let rect = Node::rect(
            60.0,
            60.0,
            200.0,
            120.0,
            Fill::Solid(Paint::from_hex(0xc0392b, 1.0)),
        );
        let rect_id = rect.id;
        project.nodes.insert(rect);
        let mut text = Node::text(
            80.0,
            220.0,
            TextStyle {
                content: "Frame".into(),
                font_size: 36.0,
                ..Default::default()
            },
        );
        text.style.fill = Fill::Solid(Paint::from_hex(0x000000, 1.0));
        let text_id = text.id;
        project.nodes.insert(text);
        project.document.append_to_active_layer(rect_id);
        project.document.append_to_active_layer(text_id);

        let (w, h, still) =
            crate::export_render::render_document_rgba(&project, 1.0).expect("still export");
        let vf = VideoFrameMap::default();
        let target = crate::render_pipeline::RenderTarget::for_video(
            project.document.width,
            project.document.height,
            1.0,
        )
        .expect("target");
        let ctx = crate::render_pipeline::RenderContext::new(0, 0.0);
        let (fw, fh, frame) =
            composite_export_frame(&project, &ctx, &vf, &target).expect("frame composite");
        // Video encoders need even dims: odd page sides shave one row/col.
        // The overlap must still be the same render (scale 1.0, origin ZERO).
        assert_eq!(fw, w - w % 2);
        assert_eq!(fh, h - h % 2);
        assert!(ink_pixels(&still) > 500, "fixture must paint ink");
        let row_bytes = (fw * 4) as usize;
        let mut worst = 0i32;
        for y in 0..fh as usize {
            let fo = y * (w * 4) as usize;
            let go = y * row_bytes;
            for (a, b) in still[fo..fo + row_bytes]
                .iter()
                .zip(frame[go..go + row_bytes].iter())
            {
                worst = worst.max((*a as i32 - *b as i32).abs());
            }
        }
        assert!(
            worst <= 2,
            "frame must match still export, worst channel diff {worst}"
        );
    }

    /// Frame fallback must composite NE AppObjects output (base segments
    /// hide these sources). Without the overlay, thumbnails of such docs
    /// lose the content that preview and video export show.
    #[test]
    fn frame_composite_keeps_ne_appobjects_content() {
        use crate::document::{Document, Fill, GraphNodeKind, Layer, Node, NodeGraph, Paint};

        let mut project = Document::new_empty_project();
        let red = Fill::Solid(Paint::from_hex(0xc0392b, 1.0));
        let rect = Node::rect(120.0, 120.0, 60.0, 60.0, red);
        let rect_id = rect.id;
        project.nodes.insert(rect);
        project.document.append_to_active_layer(rect_id);

        let mut layer = Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into());
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded output");
        let src = g.add_node(
            GraphNodeKind::ObjectFromApp {
                node_ids: vec![rect_id],
            },
            0.0,
            0.0,
        );
        g.try_add_link(src, "out", out_id, "image")
            .expect("link app object to output");
        g.eval_reals(0, 30.0);
        let ev = g.resolve_output_image();
        assert!(
            matches!(
                ev.image,
                crate::document::GraphImageSource::AppObjects(ref ids)
                    if ids.contains(&rect_id)
            ),
            "fixture must resolve AppObjects, got {:?}",
            ev.image
        );
        layer.node_graph = Some(g);
        project.document.layers.push(layer);

        let vf = VideoFrameMap::default();
        let target = crate::render_pipeline::RenderTarget::for_video(
            project.document.width,
            project.document.height,
            1.0,
        )
        .expect("target");
        let ctx = crate::render_pipeline::RenderContext::new(0, 0.0);
        let (w, h, rgba) =
            composite_export_frame(&project, &ctx, &vf, &target).expect("frame composite");
        assert_eq!((w, h), (target.width, target.height));
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        let bg = px(5, 5);
        let ink = px(130, 130);
        let diff = (ink[0] as i32 - bg[0] as i32).abs()
            + (ink[1] as i32 - bg[1] as i32).abs()
            + (ink[2] as i32 - bg[2] as i32).abs();
        assert!(
            diff > 60,
            "AppObjects content must survive, got {ink:?} vs {bg:?}"
        );
    }

    fn ink_pixels(rgba: &[u8]) -> usize {
        let bg = [rgba[0] as i32, rgba[1] as i32, rgba[2] as i32];
        rgba.chunks_exact(4)
            .filter(|p| {
                (p[0] as i32 - bg[0]).abs()
                    + (p[1] as i32 - bg[1]).abs()
                    + (p[2] as i32 - bg[2]).abs()
                    > 36
            })
            .count()
    }

    /// Still export must composite NodeEditor FilePath output (was: proxy /
    /// nothing). Uses the shared `composite_ne_file_output` software path at
    /// full still resolution — same policy shape as video fallback.
    #[test]
    fn still_export_composites_ne_file_output() {
        use crate::document::{Document, GraphNodeKind, Layer, NodeGraph};

        let tag = std::process::id();
        let dir = std::env::temp_dir();
        let src_path = dir.join(format!("vadadee_ne_src_{tag}.png"));
        let out_path = dir.join(format!("vadadee_ne_still_{tag}.png"));

        // 64x64 solid red source image.
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([220, 30, 30, 255]));
        img.save(&src_path).expect("write temp source");

        let mut project = Document::new_empty_project();
        let mut layer = Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into());
        layer.x = 100.0;
        layer.y = 100.0;
        layer.width = 64.0;
        layer.height = 64.0;
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded output");
        let img_node = g.add_node(
            GraphNodeKind::ObjectImage {
                path: src_path.to_string_lossy().into_owned(),
            },
            0.0,
            0.0,
        );
        g.try_add_link(img_node, "out", out_id, "image")
            .expect("link image to output");
        g.eval_reals(0, 30.0);
        let ev = g.resolve_output_image();
        assert!(
            matches!(ev.image, crate::document::GraphImageSource::FilePath(_)),
            "fixture must resolve FilePath, got {:?}",
            ev.image
        );
        layer.node_graph = Some(g);
        project.document.layers.push(layer);

        export_document_raster(&project, ExportImageFormat::Png, 1.0, &out_path)
            .expect("still export");
        let back = image::open(&out_path).expect("read back").to_rgba8();
        let (w, h) = (back.width(), back.height());
        assert_eq!((w, h), (794, 1123));
        let bg = back.get_pixel(5, 5);
        let ink = back.get_pixel(110, 110);
        let diff = (ink[0] as i32 - bg[0] as i32).abs()
            + (ink[1] as i32 - bg[1] as i32).abs()
            + (ink[2] as i32 - bg[2] as i32).abs();
        assert!(diff > 60, "NE output must appear, ink {ink:?} vs bg {bg:?}");

        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&out_path);
    }

    /// Still export must apply shading layers through the shared CPU path
    /// (was: flat page). Same presets as the video fallback at frozen time;
    /// unknown custom WGSL stays a no-op in both.
    #[test]
    fn still_export_applies_shading_presets() {
        use crate::document::{Document, Layer, ShadingPass};

        let tag = std::process::id();
        let dir = std::env::temp_dir();
        let plain_path = dir.join(format!("vadadee_shade_plain_{tag}.png"));
        let shade_path = dir.join(format!("vadadee_shade_star_{tag}.png"));

        let plain = Document::new_empty_project();
        export_document_raster(&plain, ExportImageFormat::Png, 1.0, &plain_path)
            .expect("plain export");

        let mut project = Document::new_empty_project();
        let mut layer = Layer::new_shading_layer(uuid::Uuid::new_v4(), "Shade".into());
        layer
            .shading_passes
            .push(ShadingPass::new_preset("Starfield", String::new()));
        project.document.layers.push(layer);
        export_document_raster(&project, ExportImageFormat::Png, 1.0, &shade_path)
            .expect("shaded export");

        let a = image::open(&plain_path).expect("read plain").to_rgba8();
        let b = image::open(&shade_path).expect("read shaded").to_rgba8();
        assert_eq!((a.width(), a.height()), (b.width(), b.height()));
        let n = (a.width() * a.height()) as usize;
        let mut total = 0u64;
        for i in 0..n {
            let o = i * 4;
            for c in 0..3 {
                total += (a.as_raw()[o + c] as i32 - b.as_raw()[o + c] as i32).abs() as u64;
            }
        }
        let mean = total as f64 / (n * 3) as f64;
        assert!(
            mean > 5.0,
            "shading preset must change still export, mean diff {mean}"
        );

        let _ = std::fs::remove_file(&plain_path);
        let _ = std::fs::remove_file(&shade_path);
    }
}
