use std::fs;
use std::path::Path;

use kurbo::BezPath;
use thiserror::Error;

use crate::document::{
    ArcJoin, Document, Fill, LineCap, LineJoin, Node, NodeId, NodeKind, NodeStore, PageUnit,
    Paint, PathData, ProjectFile, Stroke, regular_polygon_vertices,
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
                    let Some(node) = project.nodes.get(*id) else { continue };
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
        NodeKind::Plotter { x, y, w, h, plot_stroke_rgba, plot_stroke_width, .. } => {
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
        NodeKind::FlowchartNode { cx, cy, w, h, corner_rx, .. } => {
            format!(r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" {fill} {stroke} opacity="{op}"/>"#, cx - w/2.0, cy - h/2.0, w, h, corner_rx)
        }
        NodeKind::FlowchartPath { path } => {
            if path.points.is_empty() {
                String::new()
            } else {
                let pts: Vec<String> = path.points.iter().map(|(px, py)| format!("{px},{py}")).collect();
                format!(r#"<polyline points="{}" {fill} {stroke} opacity="{op}"/>"#, pts.join(" "))
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

pub fn export_selected_svg_string(project: &ProjectFile, selection: &[NodeId], bounds: Rect) -> String {
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
        let Some(node) = project.nodes.get(*id) else { continue };
        svg.push_str(&node_to_svg_fragment(node, &project.nodes));
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

/// Rasterize current selection (merged) to RGBA.
pub fn rasterize_selection_rgba(
    project: &ProjectFile,
    selection: &[NodeId],
    bounds: Rect,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    if selection.is_empty() {
        return None;
    }
    let svg = export_selected_svg_string(project, selection, bounds);
    render_svg_to_rgba_even(&svg, scale)
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

pub fn export_document_raster(
    project: &ProjectFile,
    format: ExportImageFormat,
    scale: f32,
    path: &Path,
) -> Result<(), IoError> {
    let svg = document_svg_string(project, 0, &std::collections::HashMap::new());
    let (w, h, rgba) = render_svg_to_rgba_even(&svg, scale)
        .ok_or_else(|| IoError::Msg("Rasterize failed".into()))?;
    write_image_file(path, format, w, h, &rgba)
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
                view.x0,
                view.y0,
            );
            svg.replace_range(start..end, &(head + ">"));
        }
    }
    svg
}

pub fn default_document_view(project: &ProjectFile) -> kurbo::Rect {
    kurbo::Rect::new(
        0.0,
        0.0,
        project.document.width,
        project.document.height,
    )
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
pub fn rasterize_document_view(
    project: &ProjectFile,
    view: kurbo::Rect,
    resolution_percent: f32,
    current_frame: usize,
    video_frames: &std::collections::HashMap<uuid::Uuid, Vec<u8>>,
) -> Option<(u32, u32, Vec<u8>)> {
    let svg = document_svg_for_view(project, view, current_frame, video_frames);
    let scale = (resolution_percent / 100.0).clamp(0.01, 2.0);
    render_svg_to_rgba_even(&svg, scale)
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

pub fn render_svg_to_rgba_even(svg_data: &str, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let opt = crate::fonts::usvg_options();
    let tree = usvg::Tree::from_str(svg_data, &opt).ok()?;

    let pixmap_size = tree.size().to_int_size();
    let mut pixel_w = (pixmap_size.width() as f32 * scale).round() as u32;
    let mut pixel_h = (pixmap_size.height() as f32 * scale).round() as u32;
    
    if pixel_w % 2 != 0 {
        pixel_w = pixel_w.saturating_sub(1);
    }
    if pixel_h % 2 != 0 {
        pixel_h = pixel_h.saturating_sub(1);
    }
    
    if pixel_w == 0 || pixel_h == 0 {
        return None;
    }
    
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_w, pixel_h)?;
    
    let scale_x = pixel_w as f32 / pixmap_size.width() as f32;
    let scale_y = pixel_h as f32 / pixmap_size.height() as f32;
    
    let transform = resvg::tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    
    Some((pixel_w, pixel_h, pixmap.take()))
}

fn layer_anim_transform(
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

fn video_layer_dest_size(layer: &crate::document::Layer, frame_w: u32, frame_h: u32) -> (f32, f32) {
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

fn clip_defs_and_maps(
    project: &ProjectFile,
) -> (
    String,
    std::collections::HashMap<uuid::Uuid, crate::document::ClipMaskEffect>,
    std::collections::HashSet<uuid::Uuid>,
) {
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
    let defs_str = if clip_defs.is_empty() {
        String::new()
    } else {
        format!("<defs>\n{}</defs>\n", clip_defs)
    };
    (defs_str, clip_map, mask_set)
}

fn append_image_layer_nodes_to_svg(
    svg: &mut String,
    layer: &crate::document::Layer,
    project: &ProjectFile,
    clip_map: &std::collections::HashMap<uuid::Uuid, crate::document::ClipMaskEffect>,
    mask_set: &std::collections::HashSet<uuid::Uuid>,
    hidden: &std::collections::HashSet<crate::document::NodeId>,
) {
    for id in &layer.nodes {
        if mask_set.contains(id) || hidden.contains(id) {
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

pub fn document_svg_single_image_layer(
    project: &ProjectFile,
    layer: &crate::document::Layer,
    hidden: &std::collections::HashSet<crate::document::NodeId>,
) -> String {
    let (defs_str, clip_map, mask_set) = clip_defs_and_maps(project);
    let w = project.document.width;
    let h = project.document.height;
    let mut svg = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
{defs_str}"#,
    );
    append_image_layer_nodes_to_svg(&mut svg, layer, project, &clip_map, &mask_set, hidden);
    svg.push_str("</svg>\n");
    svg
}

/// Rasterize one image layer to RGBA at `scale` document-pixels per doc unit.
pub fn rasterize_image_layer(
    project: &ProjectFile,
    layer: &crate::document::Layer,
    hidden: &std::collections::HashSet<crate::document::NodeId>,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let svg = document_svg_single_image_layer(project, layer, hidden);
    render_svg_to_rgba_even(&svg, scale)
}

/// Raster export frame following document layer stack order (bottom → top).
pub fn composite_export_frame(
    project: &ProjectFile,
    current_frame: usize,
    video_frames: &VideoFrameMap,
    scale: f32,
    time_secs: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

    let doc_w = project.document.width;
    let doc_h = project.document.height;
    let mut pixel_w = (doc_w as f32 * scale).round() as u32;
    let mut pixel_h = (doc_h as f32 * scale).round() as u32;
    if pixel_w % 2 != 0 {
        pixel_w = pixel_w.saturating_sub(1);
    }
    if pixel_h % 2 != 0 {
        pixel_h = pixel_h.saturating_sub(1);
    }
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

    let scale_x = pixel_w as f32 / doc_w as f32;
    let scale_y = pixel_h as f32 / doc_h as f32;
    let svg_scale = Transform::from_scale(scale_x, scale_y);
    let opt = crate::fonts::usvg_options();

    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        match layer.kind {
            crate::document::LayerKind::AV => {
                let Some(buf) = video_frames.get(&layer.id) else {
                    continue;
                };
                let mut src = Pixmap::new(buf.width, buf.height)?;
                src.data_mut().copy_from_slice(&buf.rgba);
                let (dx, dy, rot, opacity) =
                    layer_anim_transform(layer, project, current_frame);
                let (dw, dh) = video_layer_dest_size(layer, buf.width, buf.height);
                let x = (dx as f32) * scale;
                let y = (dy as f32) * scale;
                let w = dw * scale;
                let h = dh * scale;
                let sx = w / buf.width as f32;
                let sy = h / buf.height as f32;
                let transform = if rot != 0.0 {
                    Transform::from_translate(x, y).pre_concat(
                        Transform::from_translate(w / 2.0, h / 2.0)
                            .pre_rotate(rot as f32)
                            .pre_translate(-w / 2.0, -h / 2.0)
                            .pre_scale(sx, sy),
                    )
                } else {
                    Transform::from_translate(x, y).pre_scale(sx, sy)
                };
                let mut paint = PixmapPaint::default();
                paint.opacity = opacity;
                pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
            }
            crate::document::LayerKind::Image => {
                let svg = document_svg_single_image_layer(project, layer, &std::collections::HashSet::new());
                if let Ok(tree) = usvg::Tree::from_str(&svg, &opt) {
                    resvg::render(&tree, svg_scale, &mut pixmap.as_mut());
                }
            }
            crate::document::LayerKind::Shading => {
                apply_shading_passes_skia(&mut pixmap, &layer.shading_passes, time_secs);
            }
            crate::document::LayerKind::Flowchart => {}
            crate::document::LayerKind::NodeEditor => {
                // Graph output compositing lands in a later phase.
            }
        }
    }

    Some((pixel_w, pixel_h, pixmap.take()))
}

fn apply_shading_passes_skia(
    pixmap: &mut resvg::tiny_skia::Pixmap,
    passes: &[crate::document::ShadingPass],
    time_secs: f32,
) {
    if let Some(pass) = passes.first().filter(|p| p.enabled) {
        let name = pass.name.to_ascii_lowercase();
        let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl);
        let is_blackhole = name.contains("blackhole") || wgsl.contains("blackhole");
        let is_starfield = name.contains("star") || name.contains("space") || wgsl.contains("starfield") || wgsl.contains("star");
        
        if is_starfield {
            let w = pixmap.width();
            let h = pixmap.height();
            let aspect = (w as f32 / h as f32).max(0.25);

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
                    let rgb = crate::shading::procedural_blackhole::sample_starfield(
                        (u_val, v),
                        t,
                        aspect,
                    );
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
                        paint.set_color(resvg::tiny_skia::Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255));
                        pixmap.fill_rect(r_rect, &paint, resvg::tiny_skia::Transform::identity(), None);
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
}