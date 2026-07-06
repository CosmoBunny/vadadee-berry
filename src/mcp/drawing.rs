//! Shared MCP drawing helpers (colors, tool schemas).

use serde_json::{json, Value};

use crate::document::{Fill, Paint, Stroke};

#[derive(Clone, Debug, Default)]
pub struct McpShapeStyle {
    pub name: Option<String>,
    pub fill_rgb: Option<u32>,
    pub fill_alpha: f32,
    pub stroke_rgb: Option<u32>,
    pub stroke_alpha: f32,
    pub stroke_width: f32,
}

pub fn style_props_schema() -> Value {
    json!({
        "fill_color": {
            "description": "Fill color as #RRGGBB, RRGGBB, or 0xRRGGBB integer",
            "type": "string"
        },
        "fill_alpha": { "type": "number", "description": "Fill opacity 0..1" },
        "stroke_color": {
            "description": "Stroke color as #RRGGBB, RRGGBB, or 0xRRGGBB integer",
            "type": "string"
        },
        "stroke_alpha": { "type": "number", "description": "Stroke opacity 0..1" },
        "stroke_width": { "type": "number", "description": "Stroke width in px" },
        "name": { "type": "string" }
    })
}

pub fn style_from_args(args: &Value) -> McpShapeStyle {
    McpShapeStyle {
        name: args
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        fill_rgb: args
            .get("fill_color")
            .and_then(parse_color_value)
            .or_else(|| args.get("fill").and_then(parse_color_value)),
        fill_alpha: args
            .get("fill_alpha")
            .and_then(|v| v.as_f64())
            .map(|a| a.clamp(0.0, 1.0) as f32)
            .unwrap_or(1.0),
        stroke_rgb: args
            .get("stroke_color")
            .and_then(parse_color_value)
            .or_else(|| args.get("stroke").and_then(parse_color_value)),
        stroke_alpha: args
            .get("stroke_alpha")
            .and_then(|v| v.as_f64())
            .map(|a| a.clamp(0.0, 1.0) as f32)
            .unwrap_or(1.0),
        stroke_width: args
            .get("stroke_width")
            .and_then(|v| v.as_f64())
            .map(|w| w.max(0.0) as f32)
            .unwrap_or(2.0),
    }
}

pub fn default_fill() -> Fill {
    Fill::Solid(Paint::from_hex(0x5b8def, 1.0))
}

pub fn fill_from_style(style: &McpShapeStyle) -> Fill {
    match style.fill_rgb {
        Some(rgb) => Fill::Solid(Paint::from_hex(rgb, style.fill_alpha)),
        None => default_fill(),
    }
}

pub fn stroke_from_style(style: &McpShapeStyle) -> Stroke {
    let mut stroke = Stroke::default();
    if let Some(rgb) = style.stroke_rgb {
        stroke.style = Fill::Solid(Paint::from_hex(rgb, style.stroke_alpha));
    }
    stroke.width = style.stroke_width.max(0.0);
    if stroke.width <= 0.0 {
        stroke.style = Fill::None;
    }
    stroke
}

pub fn apply_style_patch(style: &mut crate::document::NodeStyle, patch: &Value) -> Result<(), String> {
    if let Some(rgb) = patch
        .get("fill_color")
        .or_else(|| patch.get("fill"))
        .and_then(parse_color_value)
    {
        let a = patch
            .get("fill_alpha")
            .and_then(|v| v.as_f64())
            .map(|x| x.clamp(0.0, 1.0) as f32)
            .unwrap_or(1.0);
        style.fill = Fill::Solid(Paint::from_hex(rgb, a));
    } else if let Some(a) = patch.get("fill_alpha").and_then(|v| v.as_f64()) {
        if let Fill::Solid(ref mut p) = style.fill {
            p.rgba[3] = a.clamp(0.0, 1.0) as f32;
        }
    }
    if let Some(rgb) = patch
        .get("stroke_color")
        .or_else(|| patch.get("stroke"))
        .and_then(parse_color_value)
    {
        let a = patch
            .get("stroke_alpha")
            .and_then(|v| v.as_f64())
            .map(|x| x.clamp(0.0, 1.0) as f32)
            .unwrap_or(1.0);
        style.stroke.style = Fill::Solid(Paint::from_hex(rgb, a));
    }
    if let Some(w) = patch.get("stroke_width").and_then(|v| v.as_f64()) {
        style.stroke.width = w.max(0.0) as f32;
    }
    if style.stroke.width <= 0.0 {
        style.stroke.style = Fill::None;
    }
    if let Some(o) = patch.get("opacity").and_then(|v| v.as_f64()) {
        style.opacity = o.clamp(0.0, 1.0) as f32;
    }

    // Support for path markers (geometry on path arrows)
    apply_marker_patch(&mut style.stroke.start_marker, patch.get("start_marker"));
    apply_marker_patch(&mut style.stroke.mid_marker, patch.get("mid_marker"));
    apply_marker_patch(&mut style.stroke.end_marker, patch.get("end_marker"));

    Ok(())
}

fn apply_marker_patch(marker: &mut crate::document::PathMarker, p: Option<&Value>) {
    let Some(p) = p else { return; };
    if let Some(k) = p.get("kind").and_then(|v| v.as_str()) {
        marker.kind = match k.to_lowercase().as_str() {
            "triangle" => crate::document::MarkerKind::Triangle,
            "square" => crate::document::MarkerKind::Square,
            "hollowsquare" | "hollow_square" => crate::document::MarkerKind::HollowSquare,
            "ring" | "circle" => crate::document::MarkerKind::Ring,
            "line" => crate::document::MarkerKind::Line,
            "arrow" => crate::document::MarkerKind::Arrow,
            _ => crate::document::MarkerKind::None,
        };
    }
    if let Some(rgb) = p.get("color").or_else(|| p.get("fill_color")).and_then(parse_color_value) {
        let a = p.get("alpha").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
        marker.color = crate::document::Paint::from_hex(rgb, a);
    }
    if let Some(sz) = p.get("size").and_then(|v| v.as_f64()) {
        marker.size = sz as f32;
    }
    if let Some(arr) = p.get("offset").and_then(|v| v.as_array()) {
        if arr.len() >= 2 {
            marker.offset[0] = arr[0].as_f64().unwrap_or(0.0);
            marker.offset[1] = arr[1].as_f64().unwrap_or(0.0);
        }
    } else if let Some(o) = p.get("offset").and_then(|v| v.as_f64()) {
        marker.offset = [o, 0.0];
    }
    if let Some(r) = p.get("rotation").or_else(|| p.get("rotation_deg")).and_then(|v| v.as_f64()) {
        marker.rotation = r;
    }
    if let Some(a) = p.get("auto_rotate").and_then(|v| v.as_bool()) {
        marker.auto_rotate = a;
    }
}

pub fn parse_color_value(v: &Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return Some((n & 0xFFFFFF) as u32);
    }
    if let Some(n) = v.as_i64() {
        return Some((n as u32) & 0xFFFFFF);
    }
    let s = v.as_str()?.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// Decode raster bytes for [`create_image`] from MCP args.
pub fn load_image_bytes_from_args(args: &Value) -> Result<Vec<u8>, String> {
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
    {
        let p = std::path::Path::new(path.trim());
        if !p.is_file() {
            return Err(format!("image file not found: {}", p.display()));
        }
        return std::fs::read(p).map_err(|e| format!("read {}: {e}", p.display()));
    }
    if let Some(b64) = args
        .get("image_base64")
        .or_else(|| args.get("png_base64"))
        .or_else(|| args.get("base64"))
        .and_then(|v| v.as_str())
    {
        use base64::Engine as _;
        let trimmed = b64.trim();
        let payload = trimmed
            .strip_prefix("data:image/png;base64,")
            .or_else(|| trimmed.strip_prefix("data:image/jpeg;base64,"))
            .or_else(|| trimmed.strip_prefix("data:image/webp;base64,"))
            .unwrap_or(trimmed);
        return base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|e| format!("base64 decode failed: {e}"));
    }
    if let Some(arr) = args.get("rgba").and_then(|v| v.as_array()) {
        let pw = args
            .get("pixel_width")
            .and_then(|v| v.as_u64())
            .ok_or("pixel_width required with rgba")? as u32;
        let ph = args
            .get("pixel_height")
            .and_then(|v| v.as_u64())
            .ok_or("pixel_height required with rgba")? as u32;
        let expected = (pw as usize) * (ph as usize) * 4;
        let mut raw = Vec::with_capacity(expected);
        for v in arr {
            let b = v
                .as_u64()
                .ok_or("rgba values must be integers 0..255")? as u8;
            raw.push(b);
        }
        if raw.len() != expected {
            return Err(format!(
                "rgba length {} != pixel_width*pixel_height*4 ({expected})",
                raw.len()
            ));
        }
        let img = image::RgbaImage::from_raw(pw, ph, raw).ok_or("invalid rgba dimensions")?;
        let mut png = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png),
            image::ImageFormat::Png,
        )
        .map_err(|e| format!("rgba→png failed: {e}"))?;
        return Ok(png);
    }
    Err(
        "provide path (or file_path), image_base64/png_base64, or rgba+pixel_width+pixel_height"
            .into(),
    )
}

/// Natural pixel size of encoded image bytes (PNG/JPEG/WebP/…).
pub fn image_pixel_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode image: {e}"))?;
    Ok((img.width(), img.height()))
}

pub fn parse_arc_join(v: &Value) -> crate::document::ArcJoin {
    match v.as_str().unwrap_or("").to_ascii_lowercase().as_str() {
        "chord" | "segment" => crate::document::ArcJoin::Chord,
        "pie" | "to_origin" | "origin" => crate::document::ArcJoin::ToOrigin,
        _ => crate::document::ArcJoin::NoJoin,
    }
}

pub fn drawing_tools() -> Vec<Value> {
    let style = style_props_schema();
    let mut tools = Vec::new();
    let mut rect_props = json!({
        "x": { "type": "number" },
        "y": { "type": "number" },
        "w": { "type": "number" },
        "h": { "type": "number" },
        "rx": { "type": "number", "description": "Corner radius" }
    });
    merge_props(&mut rect_props, &style);
    tools.push(tool(
        "create_rectangle",
        "Create a rectangle on the active layer",
        rect_props.clone(),
        &["x", "y", "w", "h"],
    ));

    // Bulk version - essential for performance when creating pixel-art grids (thousands of rects)
    tools.push(tool(
        "create_rectangles",
        "Bulk-create many rectangles in one call (far faster + one history entry). 'rects' is an array of {x,y,w,h, fill_color?, stroke_width?, ...}",
        json!({
            "rects": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "w": { "type": "number" },
                        "h": { "type": "number" },
                        "fill_color": { "type": "string" },
                        "fill_alpha": { "type": "number" },
                        "stroke_color": { "type": "string" },
                        "stroke_alpha": { "type": "number" },
                        "stroke_width": { "type": "number" }
                    },
                    "required": ["x", "y", "w", "h"]
                }
            }
        }),
        &["rects"],
    ));

    tools.push(tool(
        "create_image",
        "Place a raster image on the active layer (PNG/JPEG/WebP). Use path for generated files, or image_base64 / raw rgba.",
        json!({
            "path": { "type": "string", "description": "Absolute or relative path to image file (e.g. from image_gen)" },
            "file_path": { "type": "string", "description": "Alias for path" },
            "image_base64": { "type": "string", "description": "Image file bytes base64 (PNG/JPEG/WebP); optional data: URL prefix" },
            "png_base64": { "type": "string", "description": "Alias for image_base64" },
            "rgba": {
                "type": "array",
                "description": "Raw RGBA8 (length = pixel_width * pixel_height * 4)",
                "items": { "type": "integer" }
            },
            "pixel_width": { "type": "integer", "description": "Required with rgba" },
            "pixel_height": { "type": "integer", "description": "Required with rgba" },
            "x": { "type": "number", "description": "Top-left X in document px (default 0)" },
            "y": { "type": "number", "description": "Top-left Y (default 0)" },
            "width": { "type": "number", "description": "Display width; default = image pixel width" },
            "height": { "type": "number", "description": "Display height; default = image pixel height" },
            "w": { "type": "number", "description": "Alias for width" },
            "h": { "type": "number", "description": "Alias for height" },
            "scale": { "type": "number", "description": "Multiply natural size for display (default 1)" },
            "name": { "type": "string" }
        }),
        &[],
    ));

    let mut circle_props = json!({
        "cx": { "type": "number" },
        "cy": { "type": "number" },
        "r": { "type": "number" }
    });
    merge_props(&mut circle_props, &style);
    tools.push(tool(
        "create_circle",
        "Create a circle (ellipse with equal radii)",
        circle_props,
        &["cx", "cy", "r"],
    ));

    let mut ellipse_props = json!({
        "cx": { "type": "number" },
        "cy": { "type": "number" },
        "rx": { "type": "number" },
        "ry": { "type": "number" }
    });
    merge_props(&mut ellipse_props, &style);
    tools.push(tool(
        "create_ellipse",
        "Create an ellipse",
        ellipse_props,
        &["cx", "cy", "rx", "ry"],
    ));

    let mut line_props = json!({
        "x0": { "type": "number" },
        "y0": { "type": "number" },
        "x1": { "type": "number" },
        "y1": { "type": "number" }
    });
    merge_props(&mut line_props, &style);
    tools.push(tool(
        "create_line",
        "Create a straight line (stroke)",
        line_props,
        &["x0", "y0", "x1", "y1"],
    ));

    let mut poly_props = json!({
        "cx": { "type": "number" },
        "cy": { "type": "number" },
        "r": { "type": "number" },
        "sides": { "type": "integer", "description": "Number of sides (>=3)" },
        "rotation_deg": { "type": "number" }
    });
    merge_props(&mut poly_props, &style);
    tools.push(tool(
        "create_polygon",
        "Create a regular polygon",
        poly_props,
        &["cx", "cy", "r", "sides"],
    ));

    let mut arc_props = json!({
        "cx": { "type": "number" },
        "cy": { "type": "number" },
        "radius": { "type": "number" },
        "start_angle_deg": { "type": "number" },
        "sweep_angle_deg": { "type": "number" },
        "join": {
            "type": "string",
            "description": "no_join | chord | pie"
        }
    });
    merge_props(&mut arc_props, &style);
    tools.push(tool(
        "create_arc",
        "Create an arc (optional fill when join is chord or pie)",
        arc_props,
        &["cx", "cy", "radius", "start_angle_deg", "sweep_angle_deg"],
    ));

    let mut text_props = json!({
        "x": { "type": "number" },
        "y": { "type": "number" },
        "text": { "type": "string" },
        "font_size": { "type": "number" }
    });
    merge_props(&mut text_props, &style);
    tools.push(tool(
        "create_text",
        "Create a text object",
        text_props,
        &["x", "y", "text"],
    ));

    tools.push(tool(
        "set_object_style",
        "Set fill, stroke, and opacity on any object (also accepts \"ids\": string[] for bulk)",
        json!({
            "id": { "type": "string" },
            "ids": { "type": "array", "items": { "type": "string" } },
            "fill_color": style["fill_color"].clone(),
            "fill_alpha": style["fill_alpha"].clone(),
            "stroke_color": style["stroke_color"].clone(),
            "stroke_alpha": style["stroke_alpha"].clone(),
            "stroke_width": style["stroke_width"].clone(),
            "opacity": { "type": "number" }
        }),
        &[],  // id or ids
    ));

    tools.push(tool(
        "set_objects_style",
        "Set fill, stroke, and opacity on many objects at once (ids array). Recommended for recoloring large numbers of pixel rects.",
        json!({
            "ids": { "type": "array", "items": { "type": "string" } },
            "fill_color": style["fill_color"].clone(),
            "fill_alpha": style["fill_alpha"].clone(),
            "stroke_color": style["stroke_color"].clone(),
            "stroke_alpha": style["stroke_alpha"].clone(),
            "stroke_width": style["stroke_width"].clone(),
            "opacity": { "type": "number" }
        }),
        &["ids"],
    ));

    tools.push(tool(
        "set_object_transform",
        "Set translation, scale, and rotation on any object",
        json!({
            "id": { "type": "string" },
            "translate_x": { "type": "number" },
            "translate_y": { "type": "number" },
            "scale_x": { "type": "number" },
            "scale_y": { "type": "number" },
            "rotation_deg": { "type": "number" }
        }),
        &["id"],
    ));

    tools.push(tool(
        "set_object_geometry",
        "Patch geometry by kind: rect(x,y,w,h,rx), ellipse(cx,cy,rx,ry), polygon(cx,cy,r,sides,rotation_deg), line(x0,y0,x1,y1), arc(cx,cy,radius,...), text(x,y,text,font_size)",
        json!({
            "id": { "type": "string" },
            "geometry": { "type": "object", "description": "Fields for the object's kind" }
        }),
        &["id", "geometry"],
    ));


    tools.push(tool(
        "create_path",
        "Create a closed/open path from SVG path d (M, L, C, Z commands)",
        {
            let mut props = json!({
                "svg_d": { "type": "string", "description": "SVG path data, e.g. M 0 0 C ... Z" },
                "closed": { "type": "boolean" }
            });
            merge_props(&mut props, &style);
            props
        },
        &["svg_d"],
    ));

    // Animation tools
    tools.push(tool(
        "set_keyframe",
        "Set or update a keyframe (interpolated value) for an object's animation property at a specific frame. Creates the animation entry if needed.",
        json!({
            "id": { "type": "string", "description": "Object UUID" },
            "property": { "type": "string", "description": "pos_x | pos_y | rotation | opacity | color_r | color_g | color_b | color_a | geom_0 | geom_1 | ..." },
            "frame": { "type": "integer", "description": "Frame number" },
            "value": { "type": "number" },
            "interpolation": { "type": "string", "description": "linear | bezier (default linear)" }
        }),
        &["id", "property", "frame", "value"],
    ));

    tools.push(tool(
        "remove_keyframe",
        "Remove a keyframe at a specific frame for a property of an object.",
        json!({
            "id": { "type": "string" },
            "property": { "type": "string" },
            "frame": { "type": "integer" }
        }),
        &["id", "property", "frame"],
    ));

    tools.push(tool(
        "get_keyframes",
        "Get all keyframes for an object's animation (or a specific property). Returns tracks with frames and values.",
        json!({
            "id": { "type": "string" },
            "property": { "type": "string", "description": "Optional: if omitted returns all tracks" }
        }),
        &["id"],
    ));

    tools.push(tool(
        "set_keyframe_interpolation",
        "Change interpolation mode and/or bezier handles for an existing keyframe.",
        json!({
            "id": { "type": "string" },
            "property": { "type": "string" },
            "frame": { "type": "integer" },
            "interpolation": { "type": "string" },
            "handle_left": { "type": "array", "items": {"type":"number"}, "description": "[dx, dy] relative" },
            "handle_right": { "type": "array", "items": {"type":"number"} },
            "handle_mode": { "type": "string", "description": "both | left | right | none" }
        }),
        &["id", "property", "frame"],
    ));

    tools.push(tool(
        "set_current_anim_frame",
        "Set the current animation frame (scrub the timeline).",
        json!({
            "frame": { "type": "integer" }
        }),
        &["frame"],
    ));

    tools.push(tool(
        "get_current_anim_frame",
        "Get the current animation playback frame.",
        json!({}),
        &[],
    ));

    tools.push(tool(
        "set_keyframes",
        "Batch set multiple keyframes at once (highly recommended for pixel art / large numbers of objects to avoid lag). 'keyframes' is array of {id, property, frame, value, interpolation?}",
        json!({
            "keyframes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "property": { "type": "string" },
                        "frame": { "type": "integer" },
                        "value": { "type": "number" },
                        "interpolation": { "type": "string" }
                    },
                    "required": ["id", "property", "frame", "value"]
                }
            }
        }),
        &["keyframes"]
    ));

    tools.push(tool(
        "clear_animation_track",
        "Remove all keyframes for a specific property on an object.",
        json!({
            "id": { "type": "string" },
            "property": { "type": "string" }
        }),
        &["id", "property"],
    ));

    tools.push(tool(
        "add_layer",
        "Add a new image layer and make it active",
        json!({
            "name": { "type": "string" }
        }),
        &[],
    ));

    tools
}

fn merge_props(target: &mut Value, extra: &Value) {
    let Some(t) = target.as_object_mut() else {
        return;
    };
    let Some(e) = extra.as_object() else {
        return;
    };
    for (k, v) in e {
        t.insert(k.clone(), v.clone());
    }
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required
        }
    })
}
