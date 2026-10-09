//! Node graph API (Phase 5).
//!
//! ## Input
//!
//! Kind names, node ids (UUID strings), ports, and param values from Lua.
//!
//! ## Output
//!
//! Preallocated ids usable in the same run; staged [`GraphCommand`]s applied
//! post-return with optimistic snapshots.
//!
//! ## Errors
//!
//! Unknown kinds/ids/ports, type mismatches, and cycles fail at call time
//! (mirroring the host `try_add_link`); file-backed kinds are denied.
//!
//! Lua shape:
//! ```lua
//! local layers = vblua.graph.layers()          -- [{id, name, nodes, links}]
//! local blur = vblua.graph.create("Blur")       -- -> id string (stable Uuid)
//! vblua.graph.connect(blur, "output", out, "input")
//! vblua.graph.disconnect(out, "input")
//! vblua.graph.rename(blur, "My Blur")
//! local n = vblua.graph.get(blur)               -- {id, name, kind, layer}
//! local hits = vblua.graph.find("Blur")         -- [ids]
//! ```
//!
//! Rules (mirroring the host `try_add_link`):
//! - IDs are Uuid strings; Lua never sees pointers.
//! - Creates pre-allocate the Uuid so scripts can wire the node in the same
//!   run (snapshot updated optimistically, commands applied after return).
//! - `connect` validates ports/direction/types/cycles against the snapshot at
//!   call time — failures are immediate Lua errors, not silent drops.
//! - Path-carrying Object nodes (`Image/Video/Audio/...`) are denied: file
//!   access goes through the asset abstraction (Phase 11), never Lua strings.

use crate::document::{Document, GraphNode, GraphNodeKind, LayerKind, PortDir, PortType};

/// Read-only node row (snapshot).
#[derive(Debug, Clone)]
pub struct GraphNodeSnapshot {
    pub id: String,
    pub name: String,
    pub kind_name: String,
    pub layer_id: String,
    pub x: f32,
    pub y: f32,
}

/// Read-only link row (snapshot).
#[derive(Debug, Clone)]
pub struct GraphLinkSnapshot {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

/// Read-only per-layer graph view.
#[derive(Debug, Clone, Default)]
pub struct GraphLayerSnapshot {
    pub layer_id: String,
    pub layer_name: String,
    pub nodes: Vec<GraphNodeSnapshot>,
    pub links: Vec<GraphLinkSnapshot>,
}

/// All NodeEditor graphs in the document.
#[derive(Debug, Clone, Default)]
pub struct GraphSnapshot {
    pub layers: Vec<GraphLayerSnapshot>,
    pub active_layer_index: usize,
    /// Document layer id at `active_layer_index` (for target resolution).
    pub active_layer_id: Option<String>,
}

impl GraphSnapshot {
    pub fn capture(doc: &Document) -> Self {
        let mut layers = Vec::new();
        for layer in &doc.layers {
            if layer.kind != LayerKind::NodeEditor {
                continue;
            }
            let Some(g) = layer.node_graph.as_ref() else {
                continue;
            };
            layers.push(GraphLayerSnapshot {
                layer_id: layer.id.to_string(),
                layer_name: layer.name.clone(),
                nodes: g
                    .nodes
                    .values()
                    .map(|n| GraphNodeSnapshot {
                        id: n.id.to_string(),
                        name: n.name.clone(),
                        kind_name: kind_label(&n.kind).to_string(),
                        layer_id: layer.id.to_string(),
                        x: n.x,
                        y: n.y,
                    })
                    .collect(),
                links: g
                    .links
                    .iter()
                    .map(|l| GraphLinkSnapshot {
                        from_node: l.from_node.to_string(),
                        from_port: l.from_port.clone(),
                        to_node: l.to_node.to_string(),
                        to_port: l.to_port.clone(),
                    })
                    .collect(),
            });
        }
        Self {
            layers,
            active_layer_index: doc.active_layer_index,
            active_layer_id: doc
                .layers
                .get(doc.active_layer_index)
                .map(|l| l.id.to_string()),
        }
    }

    pub fn resolve_target(&self) -> Result<usize, String> {
        if self.layers.is_empty() {
            return Err("no NodeEditor layer with a graph — create one first".to_string());
        }
        // Prefer the document's active layer when it hosts a graph.
        if let Some(active) = self.active_layer_id.as_deref()
            && let Some(pos) = self.layers.iter().position(|l| l.layer_id == active)
        {
            return Ok(pos);
        }
        Ok(0)
    }

    pub fn find_node(&self, id: &str) -> Option<(usize, usize)> {
        for (li, l) in self.layers.iter().enumerate() {
            for (ni, n) in l.nodes.iter().enumerate() {
                if n.id == id {
                    return Some((li, ni));
                }
            }
        }
        None
    }
}

/// One staged graph mutation (applied by Rust after the script returns).
#[derive(Debug, Clone)]
pub enum GraphCommand {    CreateNode {
        id: uuid::Uuid,
        layer_id: uuid::Uuid,
        kind: GraphNodeKind,
        x: f32,
        y: f32,
        name: Option<String>,
    },
    DeleteNode {
        node_id: uuid::Uuid,
    },
    RenameNode {
        node_id: uuid::Uuid,
        name: String,
    },
    Connect {
        from_node: uuid::Uuid,
        from_port: String,
        to_node: uuid::Uuid,
        to_port: String,
    },
    Disconnect {
        to_node: uuid::Uuid,
        to_port: String,
    },
    SetNodeParam {
        node_id: uuid::Uuid,
        key: String,
        value: ParamValue,
    },
}

impl GraphCommand {
    /// Apply one command; `true` when the document changed.
    pub fn apply_to(&self, doc: &mut Document) -> bool {
        match self {
            GraphCommand::CreateNode {
                id,
                layer_id,
                kind,
                x,
                y,
                name,
            } => {
                let Some(layer) = doc.layers.iter_mut().find(|l| l.id == *layer_id) else {
                    return false;
                };
                if layer.kind != LayerKind::NodeEditor {
                    return false;
                }
                layer.ensure_node_graph();
                let Some(g) = layer.node_graph.as_mut() else {
                    return false;
                };
                if g.nodes.contains_key(id) {
                    return false;
                }
                let mut node = GraphNode::new(kind.clone(), *x, *y);
                node.id = *id;
                if let Some(n) = name
                    && !n.is_empty()
                {
                    node.name = n.chars().take(128).collect();
                }
                if matches!(node.kind, GraphNodeKind::OutputObject) && g.output_node_id.is_none() {
                    g.output_node_id = Some(*id);
                }
                g.nodes.insert(*id, node);
                true
            }
            GraphCommand::DeleteNode { node_id } => {
                for layer in doc.layers.iter_mut() {
                    if layer.kind != LayerKind::NodeEditor {
                        continue;
                    }
                    if let Some(g) = layer.node_graph.as_mut()
                        && g.nodes.contains_key(node_id)
                    {
                        g.remove_node(*node_id);
                        return true;
                    }
                }
                false
            }
            GraphCommand::RenameNode { node_id, name } => {
                if name.is_empty() {
                    return false;
                }
                let name: String = name.chars().take(128).collect();
                for layer in doc.layers.iter_mut() {
                    if let Some(g) = layer.node_graph.as_mut()
                        && let Some(n) = g.nodes.get_mut(node_id)
                    {
                        n.name = name.clone();
                        return true;
                    }
                }
                false
            }
            GraphCommand::Connect {
                from_node,
                from_port,
                to_node,
                to_port,
            } => {
                for layer in doc.layers.iter_mut() {
                    if layer.kind != LayerKind::NodeEditor {
                        continue;
                    }
                    let hit = layer
                        .node_graph
                        .as_ref()
                        .is_some_and(|g| g.nodes.contains_key(from_node) && g.nodes.contains_key(to_node));
                    if !hit {
                        continue;
                    }
                    if let Some(g) = layer.node_graph.as_mut() {
                        return g.try_add_link(*from_node, from_port, *to_node, to_port).is_ok();
                    }
                }
                false
            }
            GraphCommand::Disconnect { to_node, to_port } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(g) = layer.node_graph.as_mut() {
                        let before = g.links.len();
                        g.links.retain(|l| !(l.to_node == *to_node && l.to_port == *to_port));
                        if g.links.len() != before {
                            return true;
                        }
                    }
                }
                false
            }
            GraphCommand::SetNodeParam { node_id, key, value } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(g) = layer.node_graph.as_mut()
                        && let Some(n) = g.nodes.get_mut(node_id)
                    {
                        return set_node_param(&mut n.kind, key, value.clone()).is_ok();
                    }
                }
                false
            }
        }
    }
}

/// User-facing kind label (matches `default_title` wording).
pub fn kind_label(kind: &GraphNodeKind) -> &'static str {
    kind.default_title()
}

/// Parse a script kind name into a [`GraphNodeKind`].
///
/// Filesystem-backed Object nodes and `ObjectFromApp`/`Param*` references are
/// denied here (they need asset/parameter abstractions from later phases).
pub fn parse_kind(name: &str) -> Result<GraphNodeKind, String> {
    let key = name.trim().to_lowercase().replace(['_', '-', ' '], "");
    let denied = [
        "objectimage",
        "image",
        "objectvideo",
        "video",
        "objectaudio",
        "audio",
        "objectfromapp",
        "fromapp",
        "objectseptic",
        "septic",
        "objectmouse",
        "mouse",
        "paramreal",
        "paramcolor",
        "paramposition",
    ];
    if denied.contains(&key.as_str()) {
        return Err(format!(
            "node kind '{name}' needs a file/asset reference — use the Asset API (Phase 11)"
        ));
    }
    Ok(match key.as_str() {
        "blur" | "linearblur" => GraphNodeKind::LinearBlur,
        "brightness" => GraphNodeKind::Brightness,
        "colorchanger" | "color" => GraphNodeKind::ColorChanger,
        "zoom" => GraphNodeKind::Zoom,
        "equalizer" | "eq" => GraphNodeKind::Equalizer,
        "speed" => GraphNodeKind::Speed,
        "reverse" | "rewind" => GraphNodeKind::Reverse,
        "timeoffset" | "time_offset" | "offset" => GraphNodeKind::TimeOffset,
        "freezeframe" | "freeze_frame" | "freeze" | "hold" => GraphNodeKind::FreezeFrame,
        "timeremap" | "time_remap" | "remap" => GraphNodeKind::TimeRemap { points: Vec::new() },
        "transform" | "xform" => GraphNodeKind::Transform,
        "crop" => GraphNodeKind::Crop,
        "fliphorizontal" | "flip_horizontal" | "flip_h" | "mirror_h" => GraphNodeKind::FlipHorizontal,
        "flipvertical" | "flip_vertical" | "flip_v" | "mirror_v" => GraphNodeKind::FlipVertical,
        // Zoom is resolution-independent (normalized center): the same node
        // serves image and video chains, hence both aliases.
        "zoomvideo" | "zoom_video" => GraphNodeKind::Zoom,
        "zoomimage" | "zoom_image" => GraphNodeKind::Zoom,
        "value" => GraphNodeKind::Value { value: 0.0 },
        "exprx" | "expr" | "expression" => GraphNodeKind::ExprX { expr: "x".into() },
        "exprxy" => GraphNodeKind::ExprXy { expr: "x+y".into() },
        "exprxyz" => GraphNodeKind::ExprXyz { expr: "x+y+z".into() },
        "frame" => GraphNodeKind::Frame,
        "time" => GraphNodeKind::Time,
        "visualizer" => GraphNodeKind::Visualizer { gain: 1.0 },
        "chromakey" | "chroma" => GraphNodeKind::ChromaKey,
        "applymask" | "mask" => GraphNodeKind::ApplyMask,
        "backgroundblur" => GraphNodeKind::BackgroundBlur,
        "regionsfrommanual" | "regions" => GraphNodeKind::RegionsFromManual,
        "privacyblur" | "privacy" => GraphNodeKind::PrivacyBlur,
        "detectface" | "facedetect" => GraphNodeKind::DetectFace,
        "trackmotion" | "track" => GraphNodeKind::TrackMotion,
        "unionimage2" | "union2" => GraphNodeKind::UnionImage2,
        "unionimage3" | "union3" => GraphNodeKind::UnionImage3,
        "unionimage5" | "union5" => GraphNodeKind::UnionImage5,
        "unionimage7" | "union7" => GraphNodeKind::UnionImage7,
        "geosize" | "size" => GraphNodeKind::GeoSize,
        "geoplancement" | "placement" => GraphNodeKind::GeoPlacement,
        "georotate" | "rotate" => GraphNodeKind::GeoRotate,
        "geotrapezoid" | "trapezoid" => GraphNodeKind::GeoTrapezoid,
        "geomirror" | "mirror" => GraphNodeKind::GeoMirror,
        "geoadd" | "add" => GraphNodeKind::GeoAdd,
        "videoplayer" => GraphNodeKind::VideoPlayer,
        "septicplayer" => GraphNodeKind::SepticPlayer,
        "mouseencoder" => GraphNodeKind::MouseEncoder {
            time_threshold: 0.20,
            gain: 6.0,
        },
        "outputobject" | "output" => GraphNodeKind::OutputObject,
        _ => {
            return Err(format!(
                "unknown node kind '{name}' (try Blur, Brightness, Zoom, Value, Frame, Time, GeoRotate, ...)"
            ));
        }
    })
}

/// Validate `from.output -> to.input` against live kinds + snapshot links.
/// Mirrors `NodeGraph::try_add_link` so failures surface at call time.
pub fn validate_connect(
    snap: &GraphSnapshot,
    layer_idx: usize,
    from_node: &str,
    from_port: &str,
    to_node: &str,
    to_port: &str,
    kind_of: &dyn Fn(&str) -> Option<GraphNodeKind>,
) -> Result<(), String> {
    if from_node == to_node {
        return Err("cannot connect a node to itself".into());
    }
    let layer = snap
        .layers
        .get(layer_idx)
        .ok_or("unknown graph layer".to_string())?;
    let from_kind = kind_of(from_node).ok_or("unknown source node".to_string())?;
    let to_kind = kind_of(to_node).ok_or("unknown destination node".to_string())?;
    let has_node = |id: &str| layer.nodes.iter().any(|n| n.id == id);
    if !has_node(from_node) || !has_node(to_node) {
        return Err("both nodes must live in the same graph layer".to_string());
    }
    let find_port = |kind: &GraphNodeKind, port: &str| {
        kind.ports().into_iter().find(|p| p.id == port)
    };
    let fp = find_port(&from_kind, from_port).ok_or("unknown output port".to_string())?;
    let tp = find_port(&to_kind, to_port).ok_or("unknown input port".to_string())?;
    if fp.dir != PortDir::Output || tp.dir != PortDir::Input {
        return Err("wire must go from output to input".to_string());
    }
    if !PortType::can_connect(fp.ty, tp.ty) {
        return Err(format!("type mismatch: {} → {}", fp.ty.label(), tp.ty.label()));
    }
    if can_reach(&layer.links, to_node, from_node) {
        return Err("would create a cycle".to_string());
    }
    Ok(())
}

/// Port/dir/type check between two live kinds (no snapshot needed).
/// Used by template compilation; cycle checks stay with the caller.
pub fn validate_connect_kinds(
    kinds: &[GraphNodeKind],
    from: usize,
    from_port: &str,
    to: usize,
    to_port: &str,
) -> Result<(), String> {
    let fk = kinds.get(from).ok_or("unknown source slot".to_string())?;
    let tk = kinds.get(to).ok_or("unknown destination slot".to_string())?;
    let find = |kind: &GraphNodeKind, port: &str| {
        kind.ports().into_iter().find(|p| p.id == port)
    };
    let fp = find(fk, from_port).ok_or("unknown output port".to_string())?;
    let tp = find(tk, to_port).ok_or("unknown input port".to_string())?;
    if fp.dir != PortDir::Output || tp.dir != PortDir::Input {
        return Err("wire must go from output to input".to_string());
    }
    if !PortType::can_connect(fp.ty, tp.ty) {
        return Err(format!("type mismatch: {} → {}", fp.ty.label(), tp.ty.label()));
    }
    Ok(())
}

/// BFS over snapshot links: can `start` reach `goal`?
fn can_reach(links: &[GraphLinkSnapshot], start: &str, goal: &str) -> bool {
    use std::collections::{HashSet, VecDeque};
    if start == goal {
        return true;
    }
    let mut seen = HashSet::new();
    let mut q = VecDeque::new();
    q.push_back(start.to_string());
    seen.insert(start.to_string());
    let mut guard = 0usize;
    while let Some(cur) = q.pop_front() {
        guard += 1;
        if guard > 10_000 {
            return true;
        }
        for l in links {
            if l.from_node == cur && seen.insert(l.to_node.clone()) {
                if l.to_node == goal {
                    return true;
                }
                q.push_back(l.to_node.clone());
            }
        }
    }
    false
}

// ── Phase 6: generic node parameters ──────────────────────────────────────
//
// Lua: `vblua.graph.set_param(id, "radius", 20)` / `get_param(id, "radius")`.
// Values cross as `VbValue` (Int/Num -> number, Str -> string); vectors,
// colors, node/asset refs are rejected with a clear message until a node
// field actually takes them. Unknown keys and wrong types fail at call time.

/// Script-settable parameter value (subset of `VbValue` the graph uses today).
#[derive(Debug, Clone)]
pub enum ParamValue {
    Num(f64),
    Str(String),
    /// Time-remap curve: raw `(input, output)` seconds; canonicalized on write.
    Curve(Vec<(f64, f64)>),
}

/// Parse a Lua `{{t0, s0}, {t1, s1}, ...}` curve from a `VbValue` list.
/// Accepts 2-number lists and `Vec2`s; integers coerce. Errors name the shape.
pub fn param_curve_from_vb(value: &super::value::VbValue) -> Result<Vec<(f64, f64)>, String> {
    const SHAPE: &str = "curve must be {{t0, s0}, {t1, s1}, ...} (2..512 points)";
    let items = match value {
        super::value::VbValue::List(items) => items,
        _ => return Err(SHAPE.to_string()),
    };
    if items.len() < 2 || items.len() > 512 {
        return Err(format!("{SHAPE} — got {} points", items.len()));
    }
    let num = |v: &super::value::VbValue| -> Option<f64> {
        match v {
            super::value::VbValue::Num(n) => Some(*n),
            super::value::VbValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let pt = match item {
            super::value::VbValue::Vec2([x, y]) => Some((*x, *y)),
            super::value::VbValue::List(pair) if pair.len() == 2 => {
                match (num(&pair[0]), num(&pair[1])) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((x, y)) = pt else {
            return Err(SHAPE.to_string());
        };
        if !x.is_finite() || !y.is_finite() {
            return Err("curve points must be finite".to_string());
        }
        out.push((x, y));
    }
    Ok(out)
}

/// `(key, type)` pairs scriptable on `kind`. Empty = no scriptable params.
pub fn param_keys(kind: &GraphNodeKind) -> Vec<(&'static str, &'static str)> {
    match kind {
        GraphNodeKind::Value { .. } => vec![("value", "number")],
        GraphNodeKind::TimeRemap { .. } => vec![("curve", "curvelist")],
        GraphNodeKind::ExprX { .. } | GraphNodeKind::ExprXy { .. } | GraphNodeKind::ExprXyz { .. } => {
            vec![("expr", "string")]
        }
        GraphNodeKind::MouseEncoder { .. } => {
            vec![("time_threshold", "number"), ("gain", "number")]
        }
        GraphNodeKind::Visualizer { .. } => vec![("gain", "number")],
        _ => vec![],
    }
}

/// Read one parameter from a live kind.
pub fn get_node_param(kind: &GraphNodeKind, key: &str) -> Result<ParamValue, String> {
    let k = key.trim().to_lowercase();
    match (kind, k.as_str()) {
        (GraphNodeKind::Value { value }, "value") => Ok(ParamValue::Num(*value)),
        (GraphNodeKind::ExprX { expr }, "expr")
        | (GraphNodeKind::ExprXy { expr }, "expr")
        | (GraphNodeKind::ExprXyz { expr }, "expr") => Ok(ParamValue::Str(expr.clone())),
        (GraphNodeKind::MouseEncoder { time_threshold, .. }, "time_threshold") => {
            Ok(ParamValue::Num(*time_threshold))
        }
        (GraphNodeKind::MouseEncoder { gain, .. }, "gain")
        | (GraphNodeKind::Visualizer { gain }, "gain") => Ok(ParamValue::Num(*gain)),
        (GraphNodeKind::TimeRemap { points }, "curve") => {
            Ok(ParamValue::Curve(points.clone()))
        }
        _ => Err(unknown_key_error(kind, key)),
    }
}

/// Write one parameter into a live kind (clamped/truncated, never panics).
pub fn set_node_param(
    kind: &mut GraphNodeKind,
    key: &str,
    new_value: ParamValue,
) -> Result<(), String> {
    let k = key.trim().to_lowercase();
    // Precomputed for the fallthrough errors (matching below moves `kind`).
    let err_no_params = format!(
        "'{}' has no scriptable parameters (effect amounts arrive via input wires, not fields)",
        kind_label(kind)
    );
    let err_unknown = {
        let list = param_keys(kind)
            .iter()
            .map(|(kk, t)| format!("{kk} ({t})"))
            .collect::<Vec<_>>()
            .join(", ");
        if list.is_empty() {
            None
        } else {
            Some(format!("unknown key '{key}' — scriptable here: {list}"))
        }
    };
    let unknown = || err_unknown.clone().unwrap_or_else(|| err_no_params.clone());
    match (kind, k.as_str()) {
        (GraphNodeKind::Value { value }, "value") => match new_value {
            ParamValue::Num(n) => {
                if !n.is_finite() {
                    return Err("value must be finite".to_string());
                }
                *value = n;
                Ok(())
            }
            ParamValue::Str(_) => Err("value takes a number".to_string()),
            ParamValue::Curve(_) => Err("value takes a number".to_string()),
        },
        (
            GraphNodeKind::ExprX { expr }
            | GraphNodeKind::ExprXy { expr }
            | GraphNodeKind::ExprXyz { expr },
            "expr",
        ) => match new_value {
            ParamValue::Str(s) => {
                if s.is_empty() {
                    return Err("expr must not be empty".to_string());
                }
                // The host evaluates this per-frame; cap length (expression DoS guard).
                *expr = s.chars().take(512).collect();
                Ok(())
            }
            ParamValue::Num(_) => Err("expr takes a string".to_string()),
            ParamValue::Curve(_) => Err("expr takes a string".to_string()),
        },
        (GraphNodeKind::MouseEncoder { time_threshold, .. }, "time_threshold") => match new_value {
            ParamValue::Num(n) => {
                if !n.is_finite() {
                    return Err("time_threshold must be finite".to_string());
                }
                *time_threshold = n.clamp(0.01, 5.0);
                Ok(())
            }
            ParamValue::Str(_) => Err("time_threshold takes a number".to_string()),
            ParamValue::Curve(_) => Err("time_threshold takes a number".to_string()),
        },
        (
            GraphNodeKind::MouseEncoder { gain, .. } | GraphNodeKind::Visualizer { gain },
            "gain",
        ) => match new_value {
            ParamValue::Num(n) => {
                if !n.is_finite() {
                    return Err("gain must be finite".to_string());
                }
                *gain = n.clamp(0.0, 100.0);
                Ok(())
            }
            ParamValue::Str(_) => Err("gain takes a number".to_string()),
            ParamValue::Curve(_) => Err("gain takes a number".to_string()),
        },
        (GraphNodeKind::TimeRemap { points }, "curve") => match new_value {
            ParamValue::Curve(raw) => {
                *points = crate::document::TimeWarp::canonicalize(&raw);
                Ok(())
            }
            ParamValue::Num(_) => Err("curve takes {{t0, s0}, ...} point lists".to_string()),
            ParamValue::Str(_) => Err("curve takes {{t0, s0}, ...} point lists".to_string()),
        },
        _ => Err(unknown()),
    }
}

fn unknown_key_error(kind: &GraphNodeKind, key: &str) -> String {
    let keys = param_keys(kind);
    if keys.is_empty() {
        format!(
            "'{}' has no scriptable parameters (effect amounts arrive via input wires, not fields)",
            kind_label(kind)
        )
    } else {
        let list = keys
            .iter()
            .map(|(k, t)| format!("{k} ({t})"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("unknown key '{key}' — scriptable here: {list}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Layer};

    fn doc_with_graph() -> Document {
        let mut doc = Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into())],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        };
        doc.layers[0].ensure_node_graph();
        doc
    }

    #[test]
    fn kind_allowlist_and_denials() {
        assert!(parse_kind("Blur").is_ok());
        assert!(parse_kind("linear_blur").is_ok());
        assert!(parse_kind("nope").is_err());
        // File-backed kinds must refuse with an asset pointer.
        for denied in ["Image", "Video", "ObjectFromApp", "ParamReal"] {
            let e = parse_kind(denied).unwrap_err();
            assert!(e.contains("Asset"), "{denied}: {e}");
        }
    }

    #[test]
    fn create_rename_connect_disconnect_delete_round_trip() {
        let mut doc = doc_with_graph();
        let layer_id = doc.layers[0].id;
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        // Blur output port name? discover from kind.
        let blur_ports = GraphNodeKind::LinearBlur.ports();
        let out = blur_ports
            .iter()
            .find(|p| p.dir == PortDir::Output)
            .unwrap()
            .id
            .clone();
        let inp = blur_ports
            .iter()
            .find(|p| p.dir == PortDir::Input)
            .unwrap()
            .id
            .clone();
        assert!(GraphCommand::CreateNode {
            id: a,
            layer_id,
            kind: GraphNodeKind::LinearBlur,
            x: 0.0,
            y: 0.0,
            name: None,
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::RenameNode {
            node_id: a,
            name: "My Blur".into()
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::CreateNode {
            id: b,
            layer_id,
            kind: GraphNodeKind::LinearBlur,
            x: 10.0,
            y: 0.0,
            name: None,
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::Connect {
            from_node: a,
            from_port: out.clone(),
            to_node: b,
            to_port: inp.clone(),
        }
        .apply_to(&mut doc));
        // Cycle must fail at apply time too.
        assert!(!GraphCommand::Connect {
            from_node: b,
            from_port: out.clone(),
            to_node: a,
            to_port: inp.clone(),
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::Disconnect {
            to_node: b,
            to_port: inp.clone(),
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::DeleteNode { node_id: a }.apply_to(&mut doc));
        assert!(GraphCommand::DeleteNode { node_id: b }.apply_to(&mut doc));
    }

    #[test]
    fn validate_connect_rejects_type_mismatch_and_self() {
        let kinds = std::collections::HashMap::<String, GraphNodeKind>::from([
            ("v".into(), GraphNodeKind::Value { value: 0.0 }),
            ("b".into(), GraphNodeKind::LinearBlur),
        ]);
        let snap = GraphSnapshot {
            layers: vec![GraphLayerSnapshot {
                layer_id: "l".into(),
                layer_name: "NE".into(),
                nodes: vec![
                    GraphNodeSnapshot {
                        id: "v".into(),
                        name: "Value".into(),
                        kind_name: "Value".into(),
                        layer_id: "l".into(),
                        x: 0.0,
                        y: 0.0,
                    },
                    GraphNodeSnapshot {
                        id: "b".into(),
                        name: "Blur".into(),
                        kind_name: "Blur".into(),
                        layer_id: "l".into(),
                        x: 10.0,
                        y: 0.0,
                    },
                ],
                links: vec![],
            }],
            active_layer_index: 0,
            active_layer_id: Some("l".into()),
        };
        let kind_of = |id: &str| kinds.get(id).cloned();
        // Value.out is Real, Blur.in is RawImage -> genuine type mismatch.
        assert!(validate_connect(&snap, 0, "v", "out", "b", "in", &kind_of).is_err());
        assert!(validate_connect(&snap, 0, "v", "nope", "b", "in", &kind_of).is_err());
        assert!(validate_connect(&snap, 0, "b", "out", "b", "in", &kind_of).is_err());
    }

    #[test]
    fn params_round_trip_and_clamp() {
        let mut v = GraphNodeKind::Value { value: 0.0 };
        set_node_param(&mut v, "value", ParamValue::Num(42.5)).unwrap();
        assert!(matches!(
            get_node_param(&v, "value").unwrap(),
            ParamValue::Num(n) if (n - 42.5).abs() < 1e-9
        ));
        // Wrong type + unknown key fail loudly.
        assert!(set_node_param(&mut v, "value", ParamValue::Str("x".into())).is_err());
        assert!(get_node_param(&v, "nope").is_err());
        // NaN is rejected (never poisons the graph).
        assert!(set_node_param(&mut v, "value", ParamValue::Num(f64::NAN)).is_err());

        let mut e = GraphNodeKind::ExprX { expr: "x".into() };
        set_node_param(&mut e, "expr", ParamValue::Str("x*2".into())).unwrap();
        assert!(set_node_param(&mut e, "expr", ParamValue::Str(String::new())).is_err());
        assert!(set_node_param(&mut e, "expr", ParamValue::Num(1.0)).is_err());
        // Long expressions truncate to 512 chars.
        set_node_param(&mut e, "expr", ParamValue::Str("y".repeat(600))).unwrap();
        match get_node_param(&e, "expr").unwrap() {
            ParamValue::Str(s) => assert_eq!(s.len(), 512),
            ParamValue::Num(_) | ParamValue::Curve(_) => panic!("expr must stay a string"),
        }

        let mut m = GraphNodeKind::MouseEncoder {
            time_threshold: 0.2,
            gain: 6.0,
        };
        set_node_param(&mut m, "gain", ParamValue::Num(1000.0)).unwrap();
        assert!(matches!(
            get_node_param(&m, "gain").unwrap(),
            ParamValue::Num(n) if (n - 100.0).abs() < 1e-9
        ));

        // Unit variants expose nothing (amounts arrive via wires).
        assert!(param_keys(&GraphNodeKind::LinearBlur).is_empty());
        assert!(get_node_param(&GraphNodeKind::LinearBlur, "radius").is_err());
    }

    #[test]
    fn set_param_command_applies_to_document() {
        let mut doc = doc_with_graph();
        let layer_id = doc.layers[0].id;
        let id = uuid::Uuid::new_v4();
        assert!(GraphCommand::CreateNode {
            id,
            layer_id,
            kind: GraphNodeKind::Value { value: 1.0 },
            x: 0.0,
            y: 0.0,
            name: None,
        }
        .apply_to(&mut doc));
        assert!(GraphCommand::SetNodeParam {
            node_id: id,
            key: "value".into(),
            value: ParamValue::Num(7.0),
        }
        .apply_to(&mut doc));
        let g = doc.layers[0].node_graph.as_ref().unwrap();
        assert!(matches!(
            g.nodes.get(&id).unwrap().kind,
            GraphNodeKind::Value { value } if (value - 7.0).abs() < 1e-9
        ));
    }
}
