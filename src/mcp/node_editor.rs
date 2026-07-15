//! MCP tools for Node Editor graph layers (add/edit/connect nodes).

use serde_json::{json, Value};
use uuid::Uuid;

use crate::document::{GraphNodeKind, GraphParam, PortDir};

/// Tool definitions for `tools/list`.
pub fn node_editor_tools() -> Vec<Value> {
    vec![
        tool(
            "add_node_editor_layer",
            "Create a Node Editor layer (graph with Output Object), make it active, and open the editor.",
            json!({
                "name": { "type": "string", "description": "Layer name (default Node Editor)" }
            }),
            &[],
        ),
        tool(
            "list_graph_nodes",
            "List graph nodes on a Node Editor layer (id, kind, title, x, y, ports, fields). Defaults to active NE layer or first NE layer.",
            json!({
                "layer_id": { "type": "string", "description": "Layer UUID" },
                "layer_index": { "type": "integer", "description": "Layer index" }
            }),
            &[],
        ),
        tool(
            "list_graph_links",
            "List connector wires on a Node Editor layer (id, from_node, from_port, to_node, to_port).",
            json!({
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &[],
        ),
        tool(
            "add_graph_node",
            "Add a node to a Node Editor graph. kind: value|expr|frame|time|brightness|color_changer|linear_blur|speed|equalizer|visualizer|geo_size|geo_placement|geo_rotate|geo_trapezoid|geo_mirror|geo_add|object_image|object_video|object_audio|object_from_app|param_real|param_color|param_position|output_object. Optional: x,y,value,expr,path,name,app_object_ids,param_name,param_value,gain.",
            json!({
                "kind": { "type": "string" },
                "x": { "type": "number", "description": "Graph X (default auto layout)" },
                "y": { "type": "number", "description": "Graph Y" },
                "value": { "type": "number", "description": "For Value / ParamReal default" },
                "expr": { "type": "string", "description": "For Expr node" },
                "path": { "type": "string", "description": "File path for ObjectImage/Video/Audio" },
                "name": { "type": "string", "description": "Node display name" },
                "app_object_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Document object UUIDs for ObjectFromApp"
                },
                "param_name": { "type": "string", "description": "Parameter tab name (param_* kinds)" },
                "param_value": { "type": "number", "description": "Initial param real value" },
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &["kind"],
        ),
        tool(
            "edit_graph_node",
            "Edit a graph node: position (x,y), name, value, expr, path, app_object_ids. node_id required.",
            json!({
                "node_id": { "type": "string" },
                "x": { "type": "number" },
                "y": { "type": "number" },
                "name": { "type": "string" },
                "value": { "type": "number" },
                "expr": { "type": "string" },
                "path": { "type": "string" },
                "app_object_ids": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &["node_id"],
        ),
        tool(
            "remove_graph_node",
            "Remove a graph node and its connected wires. Cannot remove the last Output Object if it is the only sink (prefer leave Output).",
            json!({
                "node_id": { "type": "string" },
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &["node_id"],
        ),
        tool(
            "connect_graph_nodes",
            "Wire an output port to an input port (typed; one link per input). Ports e.g. out, image, amount, x, y.",
            json!({
                "from_node": { "type": "string" },
                "from_port": { "type": "string", "description": "default out" },
                "to_node": { "type": "string" },
                "to_port": { "type": "string", "description": "default image" },
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &["from_node", "to_node"],
        ),
        tool(
            "disconnect_graph_link",
            "Remove a wire by link_id, or by to_node+to_port (clears that input).",
            json!({
                "link_id": { "type": "string" },
                "to_node": { "type": "string" },
                "to_port": { "type": "string" },
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &[],
        ),
        tool(
            "get_graph_output",
            "Resolve Output Object chain: image + sound sources, blur, EQ, geometry summary (P5).",
            json!({
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &[],
        ),
        tool(
            "open_node_editor",
            "Open the Node Editor UI for a layer (by id/index or active NE layer).",
            json!({
                "layer_id": { "type": "string" },
                "layer_index": { "type": "integer" }
            }),
            &[],
        ),
        tool(
            "list_graph_node_kinds",
            "List valid kind strings for add_graph_node and their default ports.",
            json!({}),
            &[],
        ),
    ]
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

pub fn is_node_editor_tool(name: &str) -> bool {
    matches!(
        name,
        "add_node_editor_layer"
            | "list_graph_nodes"
            | "list_graph_links"
            | "add_graph_node"
            | "edit_graph_node"
            | "remove_graph_node"
            | "connect_graph_nodes"
            | "disconnect_graph_link"
            | "get_graph_output"
            | "open_node_editor"
            | "list_graph_node_kinds"
    )
}

/// Build a GraphNodeKind from MCP args. For param_* kinds, caller must attach GraphParam separately.
pub fn kind_from_args(kind: &str, args: &Value) -> Result<GraphNodeKind, String> {
    let k = kind.trim().to_ascii_lowercase().replace('-', "_");
    match k.as_str() {
        "value" => Ok(GraphNodeKind::Value {
            value: args.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }),
        "expr" | "expression" | "expr_x" | "exprx" => Ok(GraphNodeKind::ExprX {
            expr: args
                .get("expr")
                .and_then(|v| v.as_str())
                .unwrap_or("x")
                .to_string(),
        }),
        "expr_xy" | "exprxy" => Ok(GraphNodeKind::ExprXy {
            expr: args
                .get("expr")
                .and_then(|v| v.as_str())
                .unwrap_or("x+y")
                .to_string(),
        }),
        "expr_xyz" | "exprxyz" => Ok(GraphNodeKind::ExprXyz {
            expr: args
                .get("expr")
                .and_then(|v| v.as_str())
                .unwrap_or("x+y+z")
                .to_string(),
        }),
        "frame" => Ok(GraphNodeKind::Frame),
        "time" => Ok(GraphNodeKind::Time),
        "brightness" => Ok(GraphNodeKind::Brightness),
        "color_changer" | "color" => Ok(GraphNodeKind::ColorChanger),
        "linear_blur" | "blur" => Ok(GraphNodeKind::LinearBlur),
        "speed" => Ok(GraphNodeKind::Speed),
        "equalizer" | "eq" => Ok(GraphNodeKind::Equalizer),
        "visualizer" | "viz" => Ok(GraphNodeKind::Visualizer {
            gain: args.get("gain").and_then(|v| v.as_f64()).unwrap_or(1.0),
        }),
        "geo_size" | "size" => Ok(GraphNodeKind::GeoSize),
        "geo_placement" | "placement" => Ok(GraphNodeKind::GeoPlacement),
        "geo_rotate" | "rotate" => Ok(GraphNodeKind::GeoRotate),
        "geo_trapezoid" | "trapezoid" => Ok(GraphNodeKind::GeoTrapezoid),
        "geo_mirror" | "mirror" => Ok(GraphNodeKind::GeoMirror),
        "geo_add" | "add" => Ok(GraphNodeKind::GeoAdd),
        "object_image" | "image" => Ok(GraphNodeKind::ObjectImage {
            path: args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "object_video" | "video" => Ok(GraphNodeKind::ObjectVideo {
            path: args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "object_audio" | "audio" => Ok(GraphNodeKind::ObjectAudio {
            path: args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "object_from_app" | "app_object" | "from_app" => {
            let ids = parse_uuid_list(args.get("app_object_ids"));
            Ok(GraphNodeKind::ObjectFromApp { node_ids: ids })
        }
        "param_real" | "param" => {
            // Placeholder param_id; replaced when GraphParam is created.
            Ok(GraphNodeKind::ParamReal {
                param_id: Uuid::nil(),
            })
        }
        "param_color" => Ok(GraphNodeKind::ParamColor {
            param_id: Uuid::nil(),
        }),
        "param_position" | "param_pos" => Ok(GraphNodeKind::ParamPosition {
            param_id: Uuid::nil(),
        }),
        "output_object" | "output" => Ok(GraphNodeKind::OutputObject),
        "video_player" | "player" => Ok(GraphNodeKind::VideoPlayer),
        _ => Err(format!(
            "Unknown graph node kind \"{kind}\". Use list_graph_node_kinds for valid names."
        )),
    }
}

pub fn parse_uuid_list(v: Option<&Value>) -> Vec<Uuid> {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|x| x.as_str())
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect()
}

pub fn kind_label(kind: &GraphNodeKind) -> String {
    match kind {
        GraphNodeKind::Value { .. } => "value".into(),
        GraphNodeKind::ExprX { .. } => "expr_x".into(),
        GraphNodeKind::ExprXy { .. } => "expr_xy".into(),
        GraphNodeKind::ExprXyz { .. } => "expr_xyz".into(),
        GraphNodeKind::Frame => "frame".into(),
        GraphNodeKind::Time => "time".into(),
        GraphNodeKind::Brightness => "brightness".into(),
        GraphNodeKind::ColorChanger => "color_changer".into(),
        GraphNodeKind::LinearBlur => "linear_blur".into(),
        GraphNodeKind::Speed => "speed".into(),
        GraphNodeKind::Equalizer => "equalizer".into(),
        GraphNodeKind::Visualizer { .. } => "visualizer".into(),
        GraphNodeKind::GeoSize => "geo_size".into(),
        GraphNodeKind::GeoPlacement => "geo_placement".into(),
        GraphNodeKind::GeoRotate => "geo_rotate".into(),
        GraphNodeKind::GeoTrapezoid => "geo_trapezoid".into(),
        GraphNodeKind::GeoMirror => "geo_mirror".into(),
        GraphNodeKind::GeoAdd => "geo_add".into(),
        GraphNodeKind::ObjectImage { .. } => "object_image".into(),
        GraphNodeKind::ObjectVideo { .. } => "object_video".into(),
        GraphNodeKind::ObjectAudio { .. } => "object_audio".into(),
        GraphNodeKind::ObjectFromApp { .. } => "object_from_app".into(),
        GraphNodeKind::VideoPlayer => "video_player".into(),
        GraphNodeKind::ParamReal { .. } => "param_real".into(),
        GraphNodeKind::ParamColor { .. } => "param_color".into(),
        GraphNodeKind::ParamPosition { .. } => "param_position".into(),
        GraphNodeKind::OutputObject => "output_object".into(),
    }
}

pub fn node_fields_json(kind: &GraphNodeKind) -> Value {
    match kind {
        GraphNodeKind::Value { value } => json!({ "value": value }),
        GraphNodeKind::ExprX { expr }
        | GraphNodeKind::ExprXy { expr }
        | GraphNodeKind::ExprXyz { expr } => json!({ "expr": expr }),
        GraphNodeKind::ObjectImage { path }
        | GraphNodeKind::ObjectVideo { path }
        | GraphNodeKind::ObjectAudio { path } => json!({ "path": path }),
        GraphNodeKind::ObjectFromApp { node_ids } => json!({
            "app_object_ids": node_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        }),
        GraphNodeKind::ParamReal { param_id }
        | GraphNodeKind::ParamColor { param_id }
        | GraphNodeKind::ParamPosition { param_id } => {
            json!({ "param_id": param_id.to_string() })
        }
        _ => json!({}),
    }
}

pub fn ports_json(kind: &GraphNodeKind) -> Value {
    let ports = kind.ports();
    json!(ports
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "type": p.ty.label(),
                "dir": match p.dir {
                    PortDir::Input => "in",
                    PortDir::Output => "out",
                }
            })
        })
        .collect::<Vec<_>>())
}

/// Create GraphParam for param_* kinds; returns updated kind with real param_id.
pub fn attach_param(
    kind: GraphNodeKind,
    args: &Value,
) -> Result<(GraphNodeKind, Option<GraphParam>), String> {
    let name = args
        .get("param_name")
        .or_else(|| args.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("param")
        .to_string();
    let val = args
        .get("param_value")
        .or_else(|| args.get("value"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    match kind {
        GraphNodeKind::ParamReal { .. } => {
            let p = GraphParam::new_real(name, val);
            let id = p.id;
            Ok((GraphNodeKind::ParamReal { param_id: id }, Some(p)))
        }
        GraphNodeKind::ParamColor { .. } => {
            let p = GraphParam::new_color(name, val, val, val);
            let id = p.id;
            Ok((GraphNodeKind::ParamColor { param_id: id }, Some(p)))
        }
        GraphNodeKind::ParamPosition { .. } => {
            let p = GraphParam::new_position(name, val, 0.0);
            let id = p.id;
            Ok((GraphNodeKind::ParamPosition { param_id: id }, Some(p)))
        }
        other => Ok((other, None)),
    }
}

pub fn list_kinds_json() -> String {
    let kinds = [
        ("value", "Algebra Real constant"),
        ("expr_x", "Algebra Expr X (x)"),
        ("expr_xy", "Algebra Expr XY (x,y)"),
        ("expr_xyz", "Algebra Expr XYZ (x,y,z)"),
        ("frame", "Current animation frame"),
        ("time", "Time in seconds"),
        ("brightness", "Effect"),
        ("color_changer", "Effect"),
        ("linear_blur", "Effect"),
        ("speed", "Effect"),
        ("equalizer", "Effect"),
        ("geo_size", "Geometry"),
        ("geo_placement", "Geometry"),
        ("geo_rotate", "Geometry"),
        ("geo_trapezoid", "Geometry"),
        ("geo_mirror", "Geometry"),
        ("geo_add", "Geometry merge"),
        ("object_image", "File image source"),
        ("object_video", "File video source"),
        ("object_audio", "File audio source"),
        ("object_from_app", "Document object reference"),
        ("video_player", "Video + Time + Start/Duration → Image; Audio→Sound"),
        ("visualizer", "Audio + Freq → Level 0..1"),
        ("param_real", "Animatable real parameter"),
        ("param_color", "Animatable color parameter"),
        ("param_position", "Animatable position parameter"),
        ("output_object", "Graph sink (usually one already exists)"),
    ];
    serde_json::to_string_pretty(&json!({
        "kinds": kinds.iter().map(|(k, d)| json!({"kind": k, "description": d})).collect::<Vec<_>>()
    }))
    .unwrap_or_else(|_| "{}".into())
}
