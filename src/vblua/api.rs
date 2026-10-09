//! `vblua.*` standard library (Phases 1 + 3 + 4).
//!
//! Platform-independent by construction: Lua sees `vblua.platform()` as an
//! informational string only. There is deliberately no
//! `vblua.android_* / vblua.ios_* / vblua.linux_*` surface —
//! platform behaviour stays behind Rust interfaces.

use std::sync::{Arc, Mutex};

use super::animation::{AnimCommand, AnimSnapshot};
use super::context::{DocumentCommand, DocumentSnapshot};
use super::graph::{GraphCommand, GraphSnapshot, kind_label, parse_kind, validate_connect};
use super::sandbox::{Capability, SandboxPolicy};
use super::{API_VERSION, VBLUA_MATURITY, VBLUA_VERSION};

/// Mutable host side shared with every Lua callback.
pub struct ApiHost {
    pub snapshot: DocumentSnapshot,
    pub has_document: bool,
    pub commands: Vec<DocumentCommand>,
    /// Node graph snapshot (all NodeEditor layers) + staged graph commands.
    pub graphs: GraphSnapshot,
    pub graph_commands: Vec<GraphCommand>,
    /// Live kind table for call-time `connect` validation (id -> kind).
    pub graph_kinds: std::collections::HashMap<String, crate::document::GraphNodeKind>,
    /// Timeline snapshot + staged animation commands (Phase 7).
    pub anim: AnimSnapshot,
    pub anim_commands: Vec<AnimCommand>,
    /// Shading snapshot + staged shader commands (Phase 9).
    pub shaders: super::shader::ShaderSnapshot,
    pub shader_commands: Vec<super::shader::ShaderCommand>,
    /// Full source text for passes created/set this run (snapshot keeps lengths).
    pub shader_sources: std::collections::HashMap<String, String>,
    /// Clip snapshot + staged video commands (Phase 10).
    pub video: super::video::VideoSnapshot,
    pub video_commands: Vec<super::video::VideoCommand>,
    /// Asset inventory snapshot, read-only (Phase 11).
    pub assets: super::assets::AssetSnapshot,
    /// Platform picker hook + project-store flag (Phase 12).
    pub picker: Option<super::file::PickerHook>,
    pub has_nodestore: bool,
    pub file_commands: Vec<super::file::FileCommand>,
    /// Scripted node templates (engine-owned recipes, Phase 14).
    pub templates: super::scripted::TemplateRegistry,
    /// Local addon manager + sources queued to run after this chunk (Phase 18).
    pub addons: super::addons::AddonManager,
    pub pending_addons: Vec<(String, String)>,
    /// Canvas inventory + staged node ops + transaction flags (Phase 16).
    pub editor: super::editor::EditorSnapshot,
    pub node_commands: Vec<super::editor::NodeCommand>,
    pub transactional: bool,
    pub rollback_requested: bool,
    /// Image/kernel handles (GPU runtime milestone 1) + asset bytes cache.
    pub images: std::collections::HashMap<String, super::image::VbImage>,
    pub image_next: u64,
    pub kernels: std::collections::HashMap<String, super::kernel::VbKernel>,
    pub kernel_next: u64,
    pub asset_bytes: std::collections::HashMap<String, Vec<u8>>,
    /// Read-only playback position (R2): frames are canonical, seconds derive
    /// as frame/fps. Set by the host before each run; scripts never write it.
    pub playback_frame: u64,
    pub playback_fps: u32,
    /// Timeline-eval marks (R3a): set when a run reads time/frame/fps or calls
    /// range(). Drained per script run into the session eval state.
    pub timeline_touched: bool,
    /// Declared domain of the current run: (start, end, is_frames).
    pub timeline_declared: Option<(f64, f64, bool)>,
    /// Event subscriptions (Phase 28).
    pub events: super::events::EventBus,
    /// Unified hook state for the debugger (Phase 26; set on reset).
    pub hook_state: Option<std::sync::Arc<super::sandbox::HookState>>,
    /// Extension panels + Lua callbacks + pending renderer events (Phase 13).
    pub ui: super::ui_widgets::UiRegistry,
    pub ui_callbacks: std::collections::HashMap<u64, mlua::Function>,
    pub ui_events: Vec<super::ui_widgets::UiEvent>,
    pub policy: SandboxPolicy,
    pub console: Vec<String>,
}

impl ApiHost {
    fn push_command(&mut self, cmd: DocumentCommand) -> Result<bool, String> {
        if !self.policy.allows(Capability::DocumentWrite) {
            return Err("document.write capability is not granted".to_string());
        }
        if self.commands.len() >= self.policy.max_commands {
            return Err(format!(
                "too many staged commands (limit {})",
                self.policy.max_commands
            ));
        }
        self.commands.push(cmd);
        Ok(true)
    }
}

fn clamp01_len(s: &str) -> String {
    const MAX: usize = 4096;
    if s.len() > MAX {
        s[..MAX].to_string()
    } else {
        s.to_string()
    }
}

/// Register the `vblua` + `vb` tables plus the math helpers on `lua`.
pub fn register(lua: &mlua::Lua, host: Arc<Mutex<ApiHost>>) -> mlua::Result<()> {
    let globals = lua.globals();
    let vblua = lua.create_table()?;

    vblua.set("_VERSION", VBLUA_VERSION)?;
    vblua.set(
        "version",
        lua.create_function(|_, _: ()| Ok(VBLUA_VERSION.to_string()))?,
    )?;
    vblua.set(
        "api_version",
        lua.create_function(|_, _: ()| Ok(API_VERSION.to_string()))?,
    )?;
    vblua.set(
        "maturity",
        lua.create_function(|_, _: ()| Ok(VBLUA_MATURITY.to_string()))?,
    )?;
    vblua.set(
        "platform",
        lua.create_function(|_, _: ()| {
            // Informational only — scripts must use capability APIs, not branches.
            Ok(std::env::consts::OS.to_string())
        })?,
    )?;
    // Capability probe (Phase 21): `vblua.has("file_picker")`.
    // Unknown names are absent (safe default — never claim what isn't there).
    {
        let h = host.clone();
        vblua.set(
            "has",
            lua.create_function(move |_, name: String| {
                let h = h.lock().unwrap();
                let key = name.trim().to_ascii_lowercase();
                if let Ok(caps) = super::sandbox::permissions_from_manifest(&[key.clone()])
                    && let Some(c) = caps.into_iter().next()
                {
                    return Ok(h.policy.allows(c));
                }
                match key.as_str() {
                    "file_picker" | "native_file_picker" => Ok(h.picker.is_some()),
                    "addon_storage" | "persistent_addons" => Ok(h.addons.dir().is_some()),
                    _ => Ok(false),
                }
            })?,
        )?;
    }
    // Granted capabilities (transparency for scripts + manifests).
    {
        let h = host.clone();
        vblua.set(
            "permissions",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let mut names: Vec<String> = [
                    super::sandbox::Capability::DocumentRead,
                    super::sandbox::Capability::DocumentWrite,
                    super::sandbox::Capability::AssetRead,
                    super::sandbox::Capability::Ui,
                    super::sandbox::Capability::Clipboard,
                    super::sandbox::Capability::Network,
                    super::sandbox::Capability::FilesystemRead,
                    super::sandbox::Capability::FilesystemWrite,
                    super::sandbox::Capability::Process,
                ]
                .into_iter()
                .filter(|c| h.policy.allows(*c))
                .map(|c| c.name().to_string())
                .collect();
                names.sort();
                let t = lua.create_table()?;
                for (i, n) in names.iter().enumerate() {
                    t.set(i + 1, n.clone())?;
                }
                Ok(t)
            })?,
        )?;
    };

    // --- logging: vblua.log / warn / error -> host console (never stdout) ---
    for (name, prefix) in [("log", ""), ("warn", "WARN: "), ("error", "ERROR: ")] {
        let h = host.clone();
        let p = prefix.to_string();
        vblua.set(
            name,
            lua.create_function(move |_, msg: mlua::Value| {
                let text = match msg {
                    mlua::Value::String(s) => s.to_str()?.to_string(),
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::Nil => "nil".into(),
                    other => format!("{other:?}"),
                };
                let mut h = h.lock().unwrap();
                h.console.push(format!("{p}{}", clamp01_len(&text)));
                if h.console.len() > 512 {
                    let drain = h.console.len() - 512;
                    h.console.drain(..drain);
                }
                Ok(())
            })?,
        )?;
    }

    // --- math (Rust-backed, no reinvented numerics) ---
    let math = lua.create_table()?;
    math.set(
        "clamp",
        lua.create_function(|_, (v, lo, hi): (f64, f64, f64)| Ok(v.clamp(lo.min(hi), lo.max(hi))))?,
    )?;
    math.set(
        "lerp",
        lua.create_function(|_, (a, b, t): (f64, f64, f64)| Ok(a + (b - a) * t))?,
    )?;
    math.set(
        "smoothstep",
        lua.create_function(|_, (e0, e1, x): (f64, f64, f64)| {
            let t = ((x - e0) / (e1 - e0).max(f64::EPSILON)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        })?,
    )?;
    for (name, n) in [("vec2", 2), ("vec3", 3), ("vec4", 4)] {
        math.set(
            name,
            lua.create_function(move |lua, args: mlua::MultiValue| {
                let vals: Vec<f64> = args
                    .into_iter()
                    .map(|v| match v {
                        mlua::Value::Integer(i) => Ok(i as f64),
                        mlua::Value::Number(x) => Ok(x),
                        _ => Err(mlua::Error::BadArgument {
                            to: Some(name.to_string()),
                            pos: 1,
                            name: Some("number".to_string()),
                            cause: std::sync::Arc::new(mlua::Error::external(
                                "vblua.math: expected numbers",
                            )),
                        }),
                    })
                    .collect::<mlua::Result<_>>()?;
                if vals.len() != n {
                    return Err(mlua::Error::BadArgument {
                        to: Some(name.to_string()),
                        pos: vals.len() + 1,
                        name: Some(format!("expected {n} numbers")),
                        cause: std::sync::Arc::new(mlua::Error::external(format!(
                            "vblua.math.{name}: expected {n} numbers, got {}",
                            vals.len()
                        ))),
                    });
                }
                let t = lua.create_table()?;
                for (i, v) in vals.iter().enumerate() {
                    t.set(i + 1, *v)?;
                }
                Ok(t)
            })?,
        )?;
    }
    vblua.set("math", math)?;

    // --- document API (Phase 4): read from snapshot, write via command queue ---
    let document = lua.create_table()?;
    {
        let h = host.clone();
        document.set(
            "current",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                if !h.has_document {
                    return Ok(mlua::Value::Nil);
                }
                Ok(mlua::Value::Table(h.snapshot.clone().into_lua_table(lua)?))
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "rename",
            lua.create_function(move |_, title: String| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::Rename { title })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "resize",
            lua.create_function(move |_, (w, hgt): (f64, f64)| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::Resize {
                        width: w,
                        height: hgt,
                    })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "set_layer_visible",
            lua.create_function(move |_, (id, visible): (String, bool)| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::SetLayerVisible {
                        layer_id: id,
                        visible,
                    })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    // layers() -> [{id, name, kind, visible}] (snapshot rows, same-run writes included).
    {
        let h = host.clone();
        document.set(
            "layers",
            lua.create_function(move |lua, ()| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                for (i, l) in h.snapshot.layers.iter().enumerate() {
                    let lt = lua.create_table()?;
                    lt.set("id", l.id.clone())?;
                    lt.set("name", l.name.clone())?;
                    lt.set("kind", l.kind.clone())?;
                    lt.set("visible", l.visible)?;
                    t.set(i + 1, lt)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // get_layer(name) -> layer row | nil (exact name match).
    {
        let h = host.clone();
        document.set(
            "get_layer",
            lua.create_function(move |lua, name: String| {
                let h = h.lock().unwrap();
                match h.snapshot.layers.iter().find(|l| l.name == name) {
                    Some(l) => {
                        let lt = lua.create_table()?;
                        lt.set("id", l.id.clone())?;
                        lt.set("name", l.name.clone())?;
                        lt.set("kind", l.kind.clone())?;
                        lt.set("visible", l.visible)?;
                        Ok(mlua::Value::Table(lt))
                    }
                    None => Ok(mlua::Value::Nil),
                }
            })?,
        )?;
    }
    // create_layer({name, type?}) -> id. Idempotent by name: same-kind
    // returns the existing id, different-kind errors (safe per-frame use).
    {
        let h = host.clone();
        document.set(
            "create_layer",
            lua.create_function(move |_, opts: mlua::Table| {
                write_guard(&h)?;
                let name: String = opts.get::<String>("name").map_err(|_| {
                    mlua::Error::external("create_layer needs {name = \"...\"}")
                })?;
                let name: String = name.chars().take(128).collect();
                if name.is_empty() {
                    return Err(mlua::Error::external("layer name must be non-empty"));
                }
                let ty: String = opts.get("type").unwrap_or_else(|_| "image".to_string());
                let (kind, canonical) =
                    super::context::parse_layer_kind(&ty).ok_or_else(|| {
                        mlua::Error::external(format!(
                            "unknown layer type '{ty}' (image/av/shading/flowchart/node_editor/screen_record)"
                        ))
                    })?;
                let mut h = h.lock().unwrap();
                // Idempotent: same name + compatible type returns the existing id.
                if let Some(l) = h.snapshot.layers.iter().find(|l| l.name == name) {
                    if l.kind == canonical {
                        return Ok(l.id.clone());
                    }
                    return Err(mlua::Error::external(format!(
                        "\"{name}\" already exists as {} (requested {canonical})",
                        l.kind
                    )));
                }
                let new_id = uuid::Uuid::new_v4();
                h.commands.push(super::context::DocumentCommand::CreateLayer {
                    layer_id: new_id,
                    name: name.clone(),
                    kind,
                });
                h.snapshot.layers.push(super::context::LayerInfo {
                    id: new_id.to_string(),
                    name,
                    visible: true,
                    kind: canonical.to_string(),
                    node_count: 0,
                });
                Ok(new_id.to_string())
            })?,
        )?;
    }
    // File I/O is intentionally unavailable to scripts: it must go through the
    // platform file abstraction (Phase 12). Loud stub, not silent nil.
    for name in ["open", "save", "close", "create"] {
        document.set(
            name,
            lua.create_function(move |_, _: ()| {
                Err::<bool, _>(mlua::Error::external(format!(
                    "document.{name}() is not scriptable yet — file access goes through the Vadadee Berry picker (Phase 12)"
                )))
            })?,
        )?;
    }
    vblua.set("document", document)?;

    register_graph_api(lua, host.clone(), &vblua)?;
    register_animation_api(lua, host.clone(), &vblua)?;
    register_kinematics_api(lua, &vblua)?;
    register_shader_api(lua, host.clone(), &vblua)?;
    register_video_api(lua, host.clone(), &vblua)?;
    register_assets_api(lua, host.clone(), &vblua)?;
    register_file_api(lua, host.clone(), &vblua)?;
    register_batch_api(lua, host.clone(), &vblua)?;
    register_editor_api(lua, host.clone(), &vblua)?;
    register_scripted_api(lua, host.clone(), &vblua)?;
    register_addons_api(lua, host.clone(), &vblua)?;
    register_ui_api(lua, host.clone(), &vblua)?;
    register_debug_api(lua, host.clone(), &vblua)?;
    register_image_api(lua, host.clone(), &vblua)?;
    register_events_api(lua, host.clone(), &vblua)?;
    register_timeline_api(lua, host.clone(), &vblua)?;

    globals.set("vblua", vblua.clone())?;
    // Short alias (mirrors the `traya`/`tlua` convention in the sibling project).
    globals.set("vb", vblua)?;
    Ok(())
}

/// `vblua.graph` — Phase 5 node graph API over the snapshot + command queue.
fn register_graph_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let graph = lua.create_table()?;

    // layers() -> [{id, name, nodes, links}]
    {
        let h = host.clone();
        graph.set(
            "layers",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                for (i, l) in h.graphs.layers.iter().enumerate() {
                    let lt = lua.create_table()?;
                    lt.set("id", l.layer_id.clone())?;
                    lt.set("name", l.layer_name.clone())?;
                    lt.set("nodes", l.nodes.len() as i64)?;
                    lt.set("links", l.links.len() as i64)?;
                    t.set(i + 1, lt)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // nodes([layer_id]) -> [{id, name, kind}]
    {
        let h = host.clone();
        graph.set(
            "nodes",
            lua.create_function(move |lua, layer: Option<String>| {
                let h = h.lock().unwrap();
                let layers: Vec<usize> = match layer {
                    Some(id) => h
                        .graphs
                        .layers
                        .iter()
                        .position(|l| l.layer_id == id)
                        .map(|p| vec![p])
                        .unwrap_or_default(),
                    None => {
                        // Target layer, else all.
                        match h.graphs.resolve_target() {
                            Ok(t) => vec![t],
                            Err(_) => (0..h.graphs.layers.len()).collect(),
                        }
                    }
                };
                let t = lua.create_table()?;
                let mut n = 0;
                for li in layers {
                    for node in &h.graphs.layers[li].nodes {
                        n += 1;
                        let nt = lua.create_table()?;
                        nt.set("id", node.id.clone())?;
                        nt.set("name", node.name.clone())?;
                        nt.set("kind", node.kind_name.clone())?;
                        nt.set("layer", node.layer_id.clone())?;
                        nt.set("x", node.x as f64)?;
                        nt.set("y", node.y as f64)?;
                        t.set(n, nt)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // create(kind[, opts]) -> id. opts: {layer, x, y, name}
    {
        let h = host.clone();
        graph.set(
            "create",
            lua.create_function(move |_, (kind_name, opts): (String, Option<mlua::Table>)| {
                let kind = parse_kind(&kind_name).map_err(mlua::Error::external)?;
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if !h.has_document {
                    return Err(mlua::Error::external("no document open"));
                }
                let (mut layer_idx, mut x, mut y, mut name) =
                    (None, 40.0f32, 40.0f32, None);
                if let Some(o) = opts {
                    if let Ok(l) = o.get::<String>("layer") {
                        layer_idx = h.graphs.layers.iter().position(|l2| l2.layer_id == l);
                        if layer_idx.is_none() {
                            return Err(mlua::Error::external("unknown graph layer"));
                        }
                    }
                    x = o.get::<f32>("x").unwrap_or(x);
                    y = o.get::<f32>("y").unwrap_or(y);
                    name = o.get::<String>("name").ok();
                }
                let li = match layer_idx {
                    Some(i) => i,
                    None => h.graphs.resolve_target().map_err(mlua::Error::external)?,
                };
                if h.graph_commands.len() + h.commands.len() >= h.policy.max_commands {
                    return Err(mlua::Error::external("too many staged commands"));
                }
                let id = uuid::Uuid::new_v4();
                let layer_id: uuid::Uuid = h.graphs.layers[li]
                    .layer_id
                    .parse()
                    .map_err(|_| mlua::Error::external("corrupt layer id"))?;
                h.graph_commands.push(GraphCommand::CreateNode {
                    id,
                    layer_id,
                    kind: kind.clone(),
                    x: x.clamp(-4000.0, 4000.0),
                    y: y.clamp(-4000.0, 4000.0),
                    name: name.clone(),
                });
                // Optimistic snapshot so the id is usable in the same run.
                let label = name.unwrap_or_else(|| kind_label(&kind).to_string());
                let canonical = kind_label(&kind).to_string();
                h.graph_kinds.insert(id.to_string(), kind);
                let layer_id_str = h.graphs.layers[li].layer_id.clone();
                h.graphs.layers[li].nodes.push(super::graph::GraphNodeSnapshot {
                    id: id.to_string(),
                    name: label,
                    kind_name: canonical,
                    layer_id: layer_id_str,
                    x,
                    y,
                });
                Ok(id.to_string())
            })?,
        )?;
    }
    // get(id) -> node table | nil
    {
        let h = host.clone();
        graph.set(
            "get",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                for layer in &h.graphs.layers {
                    for node in &layer.nodes {
                        if node.id == id {
                            let t = lua.create_table()?;
                            t.set("id", node.id.clone())?;
                            t.set("name", node.name.clone())?;
                            t.set("kind", node.kind_name.clone())?;
                            t.set("layer", node.layer_id.clone())?;
                            t.set("x", node.x as f64)?;
                            t.set("y", node.y as f64)?;
                            // Scriptable params + current values (Phase 6).
                            if let Some(kind) = h.graph_kinds.get(&id) {
                                let pt = lua.create_table()?;
                                for (k, _) in super::graph::param_keys(kind) {
                                    match super::graph::get_node_param(kind, k) {
                                        Ok(super::graph::ParamValue::Num(n)) => {
                                            pt.set(k, n)?;
                                        }
                                        Ok(super::graph::ParamValue::Str(s)) => {
                                            pt.set(k, s)?;
                                        }
                                        Ok(super::graph::ParamValue::Curve(pts)) => {
                                            pt.set(k, curve_to_lua(lua, &pts)?)?;
                                        }
                                        Err(_) => {}
                                    }
                                }
                                t.set("params", pt)?;
                            }
                            // Outgoing + incoming links for this node.
                            let links = lua.create_table()?;
                            let mut n = 0;
                            for l in &layer.links {
                                if l.from_node == id || l.to_node == id {
                                    n += 1;
                                    let lt = lua.create_table()?;
                                    lt.set("from", l.from_node.clone())?;
                                    lt.set("from_port", l.from_port.clone())?;
                                    lt.set("to", l.to_node.clone())?;
                                    lt.set("to_port", l.to_port.clone())?;
                                    links.set(n, lt)?;
                                }
                            }
                            t.set("links", links)?;
                            return Ok(mlua::Value::Table(t));
                        }
                    }
                }
                Ok(mlua::Value::Nil)
            })?,
        )?;
    }
    // ports(id) -> [{id, name, type, dir}] (stop guessing port ids).
    {
        let h = host.clone();
        graph.set(
            "ports",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let kind = h
                    .graph_kinds
                    .get(&id)
                    .ok_or_else(|| mlua::Error::external("unknown node id"))?;
                let t = lua.create_table()?;
                for (i, p) in kind.ports().iter().enumerate() {
                    let pt = lua.create_table()?;
                    pt.set("id", p.id.clone())?;
                    pt.set("name", p.name.clone())?;
                    pt.set("type", p.ty.label().to_string())?;
                    pt.set(
                        "dir",
                        match p.dir {
                            crate::document::PortDir::Input => "input",
                            crate::document::PortDir::Output => "output",
                        },
                    )?;
                    t.set(i + 1, pt)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // find(name) -> [ids] (substring match on node name)
    {
        let h = host.clone();
        graph.set(
            "find",
            lua.create_function(move |lua, needle: String| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                let mut n = 0;
                for layer in &h.graphs.layers {
                    for node in &layer.nodes {
                        if node.name.contains(&needle) {
                            n += 1;
                            t.set(n, node.id.clone())?;
                        }
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // rename(id, name) -> true
    {
        let h = host.clone();
        graph.set(
            "rename",
            lua.create_function(move |_, (id, name): (String, String)| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if name.is_empty() || name.len() > 128 {
                    return Err(mlua::Error::external("name must be 1..128 chars"));
                }
                let Some((li, ni)) = h.graphs.find_node(&id) else {
                    return Err(mlua::Error::external("unknown node id"));
                };
                let node_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                h.graph_commands.push(GraphCommand::RenameNode {
                    node_id,
                    name: name.clone(),
                });
                h.graphs.layers[li].nodes[ni].name = name;
                Ok(true)
            })?,
        )?;
    }
    // remove(id) -> true
    {
        let h = host.clone();
        graph.set(
            "remove",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                let Some((li, ni)) = h.graphs.find_node(&id) else {
                    return Err(mlua::Error::external("unknown node id"));
                };
                let node_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                h.graph_commands.push(GraphCommand::DeleteNode { node_id });
                h.graphs.layers[li].nodes.remove(ni);
                h.graphs.layers[li]
                    .links
                    .retain(|l| l.from_node != id && l.to_node != id);
                h.graph_kinds.remove(&id);
                Ok(true)
            })?,
        )?;
    }
    // duplicate(id[, offset]) -> new id
    {
        let h = host.clone();
        graph.set(
            "duplicate",
            lua.create_function(move |_, (id, offset): (String, Option<f64>)| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                let (li, ni) = h
                    .graphs
                    .find_node(&id)
                    .ok_or_else(|| mlua::Error::external("unknown node id"))?;
                let kind = h
                    .graph_kinds
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::external("cannot duplicate this node"))?;
                let new_id = uuid::Uuid::new_v4();
                let layer_id: uuid::Uuid = h.graphs.layers[li]
                    .layer_id
                    .parse()
                    .map_err(|_| mlua::Error::external("corrupt layer id"))?;
                let off = offset.unwrap_or(24.0).clamp(-1000.0, 1000.0) as f32;
                let name = format!("{} copy", h.graphs.layers[li].nodes[ni].name);
                let kind_name = h.graphs.layers[li].nodes[ni].kind_name.clone();
                let layer_id_str = h.graphs.layers[li].layer_id.clone();
                let (sx, sy) = {
                    let src = &h.graphs.layers[li].nodes[ni];
                    (src.x, src.y)
                };
                let (nx, ny) = (
                    (sx + off).clamp(-4000.0, 4000.0),
                    (sy + off).clamp(-4000.0, 4000.0),
                );
                h.graph_commands.push(GraphCommand::CreateNode {
                    id: new_id,
                    layer_id,
                    kind: kind.clone(),
                    x: nx,
                    y: ny,
                    name: Some(name.clone()),
                });
                h.graph_kinds.insert(new_id.to_string(), kind);
                h.graphs.layers[li].nodes.push(super::graph::GraphNodeSnapshot {
                    id: new_id.to_string(),
                    name,
                    kind_name,
                    layer_id: layer_id_str,
                    x: nx,
                    y: ny,
                });
                Ok(new_id.to_string())
            })?,
        )?;
    }
    // connect(from, from_port, to, to_port) -> true (validated now)
    {
        let h = host.clone();
        graph.set(
            "connect",
            lua.create_function(
                move |_, (from, from_port, to, to_port): (String, String, String, String)| {
                    let mut h = h.lock().unwrap();
                    if !h.policy.allows(Capability::DocumentWrite) {
                        return Err(mlua::Error::external(
                            "document.write capability is not granted",
                        ));
                    }
                    let li = h.graphs.resolve_target().map_err(mlua::Error::external)?;
                    // Both endpoints must be on the target layer for now.
                    let on_target = |id: &str| {
                        h.graphs.layers[li].nodes.iter().any(|n| n.id == id)
                    };
                    if !on_target(&from) || !on_target(&to) {
                        return Err(mlua::Error::external(
                            "both nodes must live in the target graph layer",
                        ));
                    }
                    validate_connect(&h.graphs, li, &from, &from_port, &to, &to_port, &|id| {
                        h.graph_kinds.get(id).cloned()
                    })
                    .map_err(mlua::Error::external)?;
                    let parse = |id: &str| {
                        id.parse::<uuid::Uuid>()
                            .map_err(|_| mlua::Error::external("bad node id"))
                    };
                    h.graph_commands.push(GraphCommand::Connect {
                        from_node: parse(&from)?,
                        from_port: from_port.clone(),
                        to_node: parse(&to)?,
                        to_port: to_port.clone(),
                    });
                    // Optimistic link (one link per input port, like the host).
                    h.graphs.layers[li]
                        .links
                        .retain(|l| !(l.to_node == to && l.to_port == to_port));
                    h.graphs.layers[li].links.push(super::graph::GraphLinkSnapshot {
                        from_node: from,
                        from_port,
                        to_node: to,
                        to_port,
                    });
                    Ok(true)
                },
            )?,
        )?;
    }
    // disconnect(to, to_port) -> true
    {
        let h = host.clone();
        graph.set(
            "disconnect",
            lua.create_function(move |_, (to, to_port): (String, String)| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                let to_id: uuid::Uuid =
                    to.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                h.graph_commands.push(GraphCommand::Disconnect {
                    to_node: to_id,
                    to_port: to_port.clone(),
                });
                for layer in &mut h.graphs.layers {
                    layer.links.retain(|l| !(l.to_node == to && l.to_port == to_port));
                }
                Ok(true)
            })?,
        )?;
    }
    // set_param(id, key, value) -> true. Values cross as VbValue.
    {
        let h = host.clone();
        graph.set(
            "set_param",
            lua.create_function(
                move |lua, (id, key, value): (String, String, mlua::Value)| {
                    let param = vb_value_to_param(value, lua)?;
                    let mut h = h.lock().unwrap();
                    if !h.policy.allows(Capability::DocumentWrite) {
                        return Err(mlua::Error::external(
                            "document.write capability is not granted",
                        ));
                    }
                    let kind = h
                        .graph_kinds
                        .get_mut(&id)
                        .ok_or_else(|| mlua::Error::external("unknown node id"))?;
                    super::graph::set_node_param(kind, &key, param.clone())
                        .map_err(mlua::Error::external)?;
                    let node_id: uuid::Uuid =
                        id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                    h.graph_commands.push(GraphCommand::SetNodeParam {
                        node_id,
                        key,
                        value: param,
                    });
                    Ok(true)
                },
            )?,
        )?;
    }
    // get_param(id, key) -> number | string (live value, incl. this run's sets)
    {
        let h = host.clone();
        graph.set(
            "get_param",
            lua.create_function(move |lua, (id, key): (String, String)| {
                let h = h.lock().unwrap();
                let kind = h
                    .graph_kinds
                    .get(&id)
                    .ok_or_else(|| mlua::Error::external("unknown node id"))?;
                match super::graph::get_node_param(kind, &key)
                    .map_err(mlua::Error::external)?
                {
                    super::graph::ParamValue::Num(n) => Ok(mlua::Value::Number(n)),
                    super::graph::ParamValue::Str(s) => Ok(mlua::Value::String(
                        lua.create_string(&s)?,
                    )),
                    super::graph::ParamValue::Curve(pts) => {
                        Ok(mlua::Value::Table(curve_to_lua(lua, &pts)?))
                    }
                }
            })?,
        )?;
    }
    // params(id) -> {key = type, ...} (discover scriptable keys)
    {
        let h = host.clone();
        graph.set(
            "params",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let kind = h
                    .graph_kinds
                    .get(&id)
                    .ok_or_else(|| mlua::Error::external("unknown node id"))?;
                let t = lua.create_table()?;
                for (k, ty) in super::graph::param_keys(kind) {
                    t.set(k, ty)?;
                }
                Ok(t)
            })?,
        )?;
    }
    vblua.set("graph", graph)?;
    Ok(())
}

/// Convert a Lua argument into a graph `ParamValue` via the `VbValue` layer.
fn vb_value_to_param(
    value: mlua::Value,
    lua: &mlua::Lua,
) -> mlua::Result<super::graph::ParamValue> {
    match super::value::VbValue::from_lua(value, lua) {
        Ok(super::value::VbValue::Int(i)) => Ok(super::graph::ParamValue::Num(i as f64)),
        Ok(super::value::VbValue::Num(n)) => Ok(super::graph::ParamValue::Num(n)),
        Ok(super::value::VbValue::Str(s)) => Ok(super::graph::ParamValue::Str(s)),
        Ok(v @ super::value::VbValue::List(_)) => super::graph::param_curve_from_vb(&v)
            .map(super::graph::ParamValue::Curve)
            .map_err(mlua::Error::external),
        Ok(super::value::VbValue::Bool(_)) => Err(mlua::Error::external(
            "no boolean parameters exist yet — pass a number, string, or curve list",
        )),
        Ok(super::value::VbValue::Nil) => Err(mlua::Error::external(
            "nil is not a parameter value — pass a number, string, or curve list",
        )),
        Ok(_) => Err(mlua::Error::external(
            "vectors/colors/refs are not parameter values yet — pass a number, string, or curve list",
        )),
        Err(e) => Err(mlua::Error::external(e.to_string())),
    }
}

/// Render canonical curve points as a Lua `{{t0, s0}, ...}` table (round-trips
/// through `set_param`).
fn curve_to_lua(
    lua: &mlua::Lua,
    pts: &[(f64, f64)],
) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    for (i, (x, y)) in pts.iter().enumerate() {
        let p = lua.create_table()?;
        p.set(1, *x)?;
        p.set(2, *y)?;
        t.set(i + 1, p)?;
    }
    Ok(t)
}

/// `vblua.timeline` — R2 read-only playback state (no evaluation yet).
/// Frames are canonical; seconds derive as frame/fps (project fps).
/// `range()` is a loud stub until R3a (declared ranges need the eval runtime).
fn register_timeline_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let timeline = lua.create_table()?;
    // time() -> seconds (frame / fps).
    {
        let h = host.clone();
        timeline.set(
            "time",
            lua.create_function(move |_, ()| {
                let mut h = h.lock().unwrap();
                h.timeline_touched = true;
                Ok(h.playback_frame as f64 / h.playback_fps.max(1) as f64)
            })?,
        )?;
    }
    // frame() -> integer playhead frame.
    {
        let h = host.clone();
        timeline.set(
            "frame",
            lua.create_function(move |_, ()| {
                let mut h = h.lock().unwrap();
                h.timeline_touched = true;
                Ok(h.playback_frame as i64)
            })?,
        )?;
    }
    // fps() -> project frames per second.
    {
        let h = host.clone();
        timeline.set(
            "fps",
            lua.create_function(move |_, ()| {
                let mut h = h.lock().unwrap();
                h.timeline_touched = true;
                Ok(h.playback_fps.max(1) as i64)
            })?,
        )?;
    }
    // range(start, finish, unit) -> true. Declares this run's temporal domain
    // ("seconds" | "frames"); recorded runtime-side for gating + panel display.
    {
        let h = host.clone();
        timeline.set(
            "range",
            lua.create_function(
                move |_, (start, finish, unit): (f64, f64, String)| {
                    if !start.is_finite() || !finish.is_finite() {
                        return Err(mlua::Error::external("range bounds must be finite"));
                    }
                    if start < 0.0 || finish < 0.0 {
                        return Err(mlua::Error::external("range bounds must be >= 0"));
                    }
                    if finish < start {
                        return Err(mlua::Error::external("range finish must be >= start"));
                    }
                    let is_frames = match unit.trim().to_lowercase().as_str() {
                        "seconds" | "second" | "sec" | "s" => false,
                        "frames" | "frame" | "f" => true,
                        _ => {
                            return Err(mlua::Error::external(
                                "range unit must be \"seconds\" or \"frames\"",
                            ))
                        }
                    };
                    let mut h = h.lock().unwrap();
                    h.timeline_touched = true;
                    h.timeline_declared = Some((start, finish, is_frames));
                    Ok(true)
                },
            )?,
        )?;
    }
    vblua.set("timeline", timeline)?;
    Ok(())
}

/// `vblua.animation` — Phase 7 keyframe API over the snapshot + command queue.
fn register_animation_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::animation::{parse_interp, valid_track, TRACK_LABELS};
    let animation = lua.create_table()?;

    // set_keyframe(id, track, frame, value[, interp]) -> true
    {
        let h = host.clone();
        animation.set(
            "set_keyframe",
            lua.create_function(
                move |_, (id, track, frame, value, interp): (
                    String,
                    String,
                    i64,
                    f64,
                    Option<String>,
                )| {
                    let interp = parse_interp(interp.as_deref()).map_err(mlua::Error::external)?;
                    if !valid_track(&track) {
                        return Err(mlua::Error::external(format!(
                            "unknown track '{track}' — use pos_x, pos_y, rotation, opacity, color_r/g/b/a, stroke_*, geom_N, param:*"
                        )));
                    }
                    if frame < 0 || frame > 1_000_000 {
                        return Err(mlua::Error::external("frame must be 0..1000000"));
                    }
                    if !value.is_finite() {
                        return Err(mlua::Error::external("value must be finite"));
                    }
                    let mut h = h.lock().unwrap();
                    if !h.policy.allows(Capability::DocumentWrite) {
                        return Err(mlua::Error::external(
                            "document.write capability is not granted",
                        ));
                    }
                    if !h.has_document {
                        return Err(mlua::Error::external("no document open"));
                    }
                    if !h.anim.targets.contains(&id) {
                        return Err(mlua::Error::external(
                            "unknown node/layer id — keyframes target canvas nodes or layers",
                        ));
                    }
                    if h.anim_commands.len() + h.graph_commands.len() + h.commands.len()
                        >= h.policy.max_commands
                    {
                        return Err(mlua::Error::external("too many staged commands"));
                    }
                    let target: uuid::Uuid =
                        id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                    h.anim_commands.push(AnimCommand::SetKeyframe {
                        target,
                        track: track.clone(),
                        frame: frame as usize,
                        value,
                        interp,
                    });
                    // Optimistic snapshot so sample()/keyframes() see this run's sets.
                    let entry = h.anim.nodes.entry(id.clone()).or_default();
                    let kfs = entry.tracks.entry(track).or_default();
                    if let Some(k) = kfs.iter_mut().find(|k| k.frame == frame as usize) {
                        k.value = value;
                        k.interp = match interp {
                            crate::document::InterpolationMode::Linear => "linear",
                            crate::document::InterpolationMode::Bezier => "bezier",
                        };
                    } else {
                        kfs.push(super::animation::KfSnapshot {
                            frame: frame as usize,
                            value,
                            interp: match interp {
                                crate::document::InterpolationMode::Linear => "linear",
                                crate::document::InterpolationMode::Bezier => "bezier",
                            },
                            hr: (5.0, 0.0),
                        });
                        kfs.sort_by_key(|k| k.frame);
                    }
                    Ok(true)
                },
            )?,
        )?;
    }
    // remove_keyframe(id, track, frame) -> true if one existed
    {
        let h = host.clone();
        animation.set(
            "remove_keyframe",
            lua.create_function(move |_, (id, track, frame): (String, String, i64)| {
                if !valid_track(&track) {
                    return Err(mlua::Error::external(format!("unknown track '{track}'")));
                }
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external(
                        "document.write capability is not granted",
                    ));
                }
                let target: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                h.anim_commands.push(AnimCommand::RemoveKeyframe {
                    target,
                    track: track.clone(),
                    frame: frame.max(0) as usize,
                });
                let existed = h
                    .anim
                    .nodes
                    .get_mut(&id)
                    .map(|n| {
                        n.tracks.get_mut(&track).map(|kfs| {
                            let before = kfs.len();
                            kfs.retain(|k| k.frame != frame.max(0) as usize);
                            before != kfs.len()
                        })
                    })
                    .flatten()
                    .unwrap_or(false);
                Ok(existed)
            })?,
        )?;
    }
    // keyframes(id, track) -> [{frame, value, interp}]
    {
        let h = host.clone();
        animation.set(
            "keyframes",
            lua.create_function(move |lua, (id, track): (String, String)| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                if let Some(n) = h.anim.nodes.get(&id)
                    && let Some(kfs) = n.tracks.get(&track)
                {
                    for (i, k) in kfs.iter().enumerate() {
                        let kt = lua.create_table()?;
                        kt.set("frame", k.frame as i64)?;
                        kt.set("value", k.value)?;
                        kt.set("interp", k.interp)?;
                        t.set(i + 1, kt)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // sample(id, track, frame) -> number | nil (keyframes only; stack spans excluded)
    {
        let h = host.clone();
        animation.set(
            "sample",
            lua.create_function(move |_, (id, track, frame): (String, String, i64)| {
                let h = h.lock().unwrap();
                let v = h
                    .anim
                    .nodes
                    .get(&id)
                    .and_then(|n| n.tracks.get(&track))
                    .and_then(|kfs| interpolate_snapshot(kfs, frame.max(0) as usize));
                Ok(v)
            })?,
        )?;
    }
    // tracks() -> [labels]
    {
        animation.set(
            "tracks",
            lua.create_function(move |lua, _: ()| {
                let t = lua.create_table()?;
                for (i, label) in TRACK_LABELS.iter().enumerate() {
                    t.set(i + 1, *label)?;
                }
                t.set(TRACK_LABELS.len() + 1, "geom_N")?;
                t.set(TRACK_LABELS.len() + 2, "param:*")?;
                Ok(t)
            })?,
        )?;
    }
    // max_frame() -> content span at fps=60 (transport fps is app state)
    {
        let h = host.clone();
        animation.set(
            "max_frame",
            lua.create_function(move |_, _: ()| Ok(h.lock().unwrap().anim.max_frame_60 as i64))?,
        )?;
    }
    vblua.set("animation", animation)?;
    Ok(())
}

/// Linear/bezier snapshot interpolation (mirrors `KeyframeTrack::interpolate`
/// for the linear case; bezier uses the same quadratic solve inputs).
pub(crate) fn interpolate_snapshot(kfs: &[super::animation::KfSnapshot], frame: usize) -> Option<f64> {
    if kfs.is_empty() {
        return None;
    }
    if frame <= kfs[0].frame {
        return Some(kfs[0].value);
    }
    let last = kfs.len() - 1;
    if frame >= kfs[last].frame {
        return Some(kfs[last].value);
    }
    for w in kfs.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if frame >= a.frame && frame <= b.frame {
            let range = (b.frame - a.frame) as f64;
            if range < 1e-9 {
                return Some(a.value);
            }
            if a.interp == "bezier" {
                // Host-identical quadratic solve (cf. document::animation).
                let x_target = (frame - a.frame) as f64;
                let x1 = a.hr.0.clamp(0.0, range);
                let u = solve_u(x_target, x1, range);
                let omt = 1.0 - u;
                let y0 = a.value;
                let y1 = a.value + a.hr.1;
                let y2 = b.value;
                return Some(omt * omt * y0 + 2.0 * omt * u * y1 + u * u * y2);
            }
            let t = (frame - a.frame) as f64 / range;
            return Some(a.value + t * (b.value - a.value));
        }
    }
    None
}

/// Bisection solve for the bezier time parameter (mirrors the host).
fn solve_u(x_target: f64, x1: f64, range: f64) -> f64 {
    if range < 1e-9 {
        return 0.0;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let u = (low + high) * 0.5;
        let omt = 1.0 - u;
        let x = 2.0 * omt * u * x1 + u * u * range;
        if x < x_target {
            low = u;
        } else {
            high = u;
        }
    }
    (low + high) * 0.5
}

/// `vblua.kinematics` — Phase 8 pure-math FK/IK + motion helpers.
/// No document access, no commands, no capability: like `vblua.math`.
fn register_kinematics_api(
    lua: &mlua::Lua,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::kinematics as K;
    let kin = lua.create_table()?;

    // fk(lengths, angles[, base]) -> [{x,y}...] (base included as [1]).
    kin.set(
        "fk",
        lua.create_function(
            move |lua, (lengths, angles, base): (
                Vec<f64>,
                Vec<f64>,
                Option<mlua::Table>,
            )| {
                K::check_lengths(&lengths).map_err(mlua::Error::external)?;
                if angles.len() > lengths.len() {
                    return Err(mlua::Error::external("more angles than segments"));
                }
                let b = table_xy(base.as_ref(), [0.0, 0.0])?;
                let pts = K::fk(&lengths, &angles, b);
                let t = lua.create_table()?;
                for (i, p) in pts.iter().enumerate() {
                    let pt = lua.create_table()?;
                    pt.set(1, p[0])?;
                    pt.set(2, p[1])?;
                    pt.set("x", p[0])?;
                    pt.set("y", p[1])?;
                    t.set(i + 1, pt)?;
                }
                Ok(t)
            },
        )?,
    )?;
    // ik(lengths, target[, opts]) -> [angles] (CCD; opts: {base, initial, iters}).
    kin.set(
        "ik",
        lua.create_function(
            move |lua, (lengths, target, opts): (Vec<f64>, mlua::Table, Option<mlua::Table>)| {
                K::check_lengths(&lengths).map_err(mlua::Error::external)?;
                let tgt = table_xy(Some(&target), [0.0, 0.0])?;
                let (base, initial, iters) = match opts {
                    Some(o) => {
                        let b = match o.get::<mlua::Value>("base") {
                            Ok(mlua::Value::Table(t)) => table_xy(Some(&t), [0.0, 0.0])?,
                            _ => [0.0, 0.0],
                        };
                        let init = match o.get::<mlua::Value>("initial") {
                            Ok(mlua::Value::Table(t)) => table_nums(&t)?,
                            _ => vec![0.0; lengths.len()],
                        };
                        let it = o.get::<i64>("iters").unwrap_or(32).clamp(1, 256) as usize;
                        (b, init, it)
                    }
                    None => ([0.0, 0.0], vec![0.0; lengths.len()], 32),
                };
                let angles = K::ik_ccd(&lengths, base, tgt, &initial, iters);
                let t = lua.create_table()?;
                for (i, a) in angles.iter().enumerate() {
                    t.set(i + 1, *a)?;
                }
                Ok(t)
            },
        )?,
    )?;
    // ik2(l1, l2, base, target) -> {a1, a2} (analytic; clamps unreachable).
    kin.set(
        "ik2",
        lua.create_function(
            move |lua, (l1, l2, base, target): (f64, f64, mlua::Table, mlua::Table)| {
                if !(l1.is_finite() && l2.is_finite() && l1 > 0.0 && l2 > 0.0) {
                    return Err(mlua::Error::external("lengths must be finite positives"));
                }
                let b = table_xy(Some(&base), [0.0, 0.0])?;
                let tgt = table_xy(Some(&target), [0.0, 0.0])?;
                let (a1, a2) = K::ik_two_bone(l1, l2, b, tgt);
                let t = lua.create_table()?;
                t.set(1, a1)?;
                t.set(2, a2)?;
                t.set("a1", a1)?;
                t.set("a2", a2)?;
                Ok(t)
            },
        )?,
    )?;
    // damp(current, target, lambda, dt) -> number
    kin.set(
        "damp",
        lua.create_function(move |_, (c, t, l, dt): (f64, f64, f64, f64)| Ok(K::damp(c, t, l, dt)))?,
    )?;
    // spring(pos, vel, target, stiffness, damping, dt) -> {pos, vel}
    kin.set(
        "spring",
        lua.create_function(
            move |lua, (p, v, t, k, c, dt): (f64, f64, f64, f64, f64, f64)| {
                let (np, nv) = K::spring_step(p, v, t, k, c, dt);
                let out = lua.create_table()?;
                out.set(1, np)?;
                out.set(2, nv)?;
                out.set("pos", np)?;
                out.set("vel", nv)?;
                Ok(out)
            },
        )?,
    )?;
    // look_at(from, to) -> angle (radians)
    kin.set(
        "look_at",
        lua.create_function(move |_, (from, to): (mlua::Table, mlua::Table)| {
            Ok(K::look_at(table_xy(Some(&from), [0.0, 0.0])?, table_xy(Some(&to), [0.0, 0.0])?))
        })?,
    )?;
    // orbit(center, radius, angle) -> {x, y}
    kin.set(
        "orbit",
        lua.create_function(
            move |lua, (center, radius, angle): (mlua::Table, f64, f64)| {
                let p = K::orbit(table_xy(Some(&center), [0.0, 0.0])?, radius, angle);
                let t = lua.create_table()?;
                t.set(1, p[0])?;
                t.set(2, p[1])?;
                t.set("x", p[0])?;
                t.set("y", p[1])?;
                Ok(t)
            },
        )?,
    )?;
    // chain(lengths) -> {solve(target) -> angles, fk(angles) -> joints}
    kin.set(
        "chain",
        lua.create_function(move |lua, lengths: Vec<f64>| {
            K::check_lengths(&lengths).map_err(mlua::Error::external)?;
            let t = lua.create_table()?;
            {
                let lens = lengths.clone();
                t.set(
                    "solve",
                    lua.create_function(move |lua, target: mlua::Table| {
                        let tgt = table_xy(Some(&target), [0.0, 0.0])?;
                        let angles = K::ik_ccd(&lens, [0.0, 0.0], tgt, &vec![0.0; lens.len()], 64);
                        let out = lua.create_table()?;
                        for (i, a) in angles.iter().enumerate() {
                            out.set(i + 1, *a)?;
                        }
                        Ok(out)
                    })?,
                )?;
            }
            {
                let lens = lengths.clone();
                t.set(
                    "fk",
                    lua.create_function(move |lua, angles: Vec<f64>| {
                        let pts = K::fk(&lens, &angles, [0.0, 0.0]);
                        let out = lua.create_table()?;
                        for (i, p) in pts.iter().enumerate() {
                            let pt = lua.create_table()?;
                            pt.set("x", p[0])?;
                            pt.set("y", p[1])?;
                            out.set(i + 1, pt)?;
                        }
                        Ok(out)
                    })?,
                )?;
            }
            t.set("segments", lengths.len() as i64)?;
            Ok(t)
        })?,
    )?;
    vblua.set("kinematics", kin)?;
    Ok(())
}

/// Read `{x, y}` / `[x, y]` from a Lua table (defaults when absent).
fn table_xy(t: Option<&mlua::Table>, default: [f64; 2]) -> mlua::Result<[f64; 2]> {
    let Some(t) = t else { return Ok(default) };
    let num = |k: &str, i: i64| -> f64 {
        t.get::<f64>(k)
            .ok()
            .or_else(|| t.get::<f64>(i).ok())
            .or_else(|| t.get::<i64>(k).ok().map(|v| v as f64))
            .or_else(|| t.get::<i64>(i).ok().map(|v| v as f64))
            .unwrap_or(if k == "x" || i == 1 { default[0] } else { default[1] })
    };
    let (x, y) = (num("x", 1), num("y", 2));
    if !x.is_finite() || !y.is_finite() {
        return Err(mlua::Error::external("coordinates must be finite"));
    }
    Ok([x, y])
}

/// Read a sequence of numbers from a Lua table.
fn table_nums(t: &mlua::Table) -> mlua::Result<Vec<f64>> {
    let mut out = Vec::new();
    for v in t.sequence_values::<mlua::Value>() {
        match v? {
            mlua::Value::Integer(i) => out.push(i as f64),
            mlua::Value::Number(n) => out.push(n),
            _ => return Err(mlua::Error::external("expected a list of numbers")),
        }
    }
    Ok(out)
}

/// `vblua.shader` — Phase 9 safe WGSL pass management.
fn register_shader_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::shader::{ShaderCommand, validate_source, MAX_UNIFORMS};
    let shader = lua.create_table()?;

    // validate(src) -> true (pure static check, no document).
    shader.set(
        "validate",
        lua.create_function(move |_, src: String| {
            validate_source(&src).map_err(mlua::Error::external)?;
            Ok(true)
        })?,
    )?;
    // passes([layer_id]) -> [{id, name, enabled, uniforms, source_len, error}]
    {
        let h = host.clone();
        shader.set(
            "passes",
            lua.create_function(move |lua, layer: Option<String>| {
                let h = h.lock().unwrap();
                let idx: Vec<usize> = match layer {
                    Some(id) => h
                        .shaders
                        .layers
                        .iter()
                        .position(|l| l.layer_id == id)
                        .map(|p| vec![p])
                        .unwrap_or_default(),
                    None => (0..h.shaders.layers.len()).collect(),
                };
                let t = lua.create_table()?;
                let mut n = 0;
                for li in idx {
                    for p in &h.shaders.layers[li].passes {
                        n += 1;
                        let pt = lua.create_table()?;
                        pt.set("id", p.id.clone())?;
                        pt.set("layer", p.layer_id.clone())?;
                        pt.set("name", p.name.clone())?;
                        pt.set("enabled", p.enabled)?;
                        pt.set("source_len", p.source_len as i64)?;
                        pt.set("has_error", p.has_error)?;
                        let ut = lua.create_table()?;
                        for (i, u) in p.uniforms.iter().enumerate() {
                            ut.set(i + 1, *u as f64)?;
                        }
                        pt.set("uniforms", ut)?;
                        t.set(n, pt)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // create([layer_id[, name]]) -> id (disabled template pass).
    {
        let h = host.clone();
        shader.set(
            "create",
            lua.create_function(move |_, (layer, name): (Option<String>, Option<String>)| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if !h.has_document {
                    return Err(mlua::Error::external("no document open"));
                }
                let li = h.shaders.resolve_layer(layer.as_deref()).map_err(mlua::Error::external)?;
                if h.shader_commands.len() + h.commands.len() >= h.policy.max_commands {
                    return Err(mlua::Error::external("too many staged commands"));
                }
                let name = name.unwrap_or_else(|| "Custom".to_string());
                if name.is_empty() || name.len() > 128 {
                    return Err(mlua::Error::external("name must be 1..128 chars"));
                }
                let id = uuid::Uuid::new_v4();
                let layer_id: uuid::Uuid = h.shaders.layers[li]
                    .layer_id
                    .parse()
                    .map_err(|_| mlua::Error::external("corrupt layer id"))?;
                h.shader_commands.push(ShaderCommand::CreatePass {
                    id,
                    layer_id,
                    name: name.clone(),
                });
                let layer_id_str = h.shaders.layers[li].layer_id.clone();
                h.shaders.layers[li].passes.push(super::shader::ShaderPassSnapshot {
                    id: id.to_string(),
                    layer_id: layer_id_str,
                    name,
                    enabled: false,
                    uniforms: vec![0.0, 0.5, 0.0],
                    source_len: crate::document::CUSTOM_WGSL_TEMPLATE.len(),
                    has_error: false,
                });
                Ok(id.to_string())
            })?,
        )?;
    }
    // get_source(id) -> string for passes created/set in this run.
    // Full 64 KiB sources are NOT snapshotted (memory); passes() gives metadata.
    {
        let h = host.clone();
        shader.set(
            "get_source",
            lua.create_function(move |_, id: String| {
                let h = h.lock().unwrap();
                h.shader_sources.get(&id).cloned().ok_or_else(|| {
                    mlua::Error::external(
                        "source text is only available for passes created or set in this run (use passes() for metadata)",
                    )
                })
            })?,
        )?;
    }
    // set_source(id, src) -> true (statically validated now).
    {
        let h = host.clone();
        shader.set(
            "set_source",
            lua.create_function(move |_, (id, src): (String, String)| {
                validate_source(&src).map_err(mlua::Error::external)?;
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if h.shaders.find_pass(&id).is_none() {
                    return Err(mlua::Error::external("unknown shader pass id"));
                }
                let pass_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad pass id"))?;
                h.shader_commands.push(ShaderCommand::SetSource {
                    pass_id,
                    source: src.clone(),
                });
                if let Some((li, pi)) = h.shaders.find_pass(&id) {
                    h.shaders.layers[li].passes[pi].source_len = src.len();
                    h.shaders.layers[li].passes[pi].has_error = false;
                }
                h.shader_sources.insert(id, src);
                Ok(true)
            })?,
        )?;
    }
    // set_uniforms(id, {...}) -> true (capped finite floats).
    {
        let h = host.clone();
        shader.set(
            "set_uniforms",
            lua.create_function(move |_, (id, vals): (String, Vec<f64>)| {
                if vals.len() > MAX_UNIFORMS {
                    return Err(mlua::Error::external(format!(
                        "at most {MAX_UNIFORMS} uniforms"
                    )));
                }
                if vals.iter().any(|v| !v.is_finite()) {
                    return Err(mlua::Error::external("uniforms must be finite"));
                }
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if h.shaders.find_pass(&id).is_none() {
                    return Err(mlua::Error::external("unknown shader pass id"));
                }
                let pass_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad pass id"))?;
                let floats: Vec<f32> = vals.iter().map(|v| *v as f32).collect();
                h.shader_commands.push(ShaderCommand::SetUniforms {
                    pass_id,
                    values: floats.clone(),
                });
                if let Some((li, pi)) = h.shaders.find_pass(&id) {
                    h.shaders.layers[li].passes[pi].uniforms = floats;
                }
                Ok(true)
            })?,
        )?;
    }
    // set_enabled(id, bool) -> true.
    {
        let h = host.clone();
        shader.set(
            "set_enabled",
            lua.create_function(move |_, (id, enabled): (String, bool)| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                if h.shaders.find_pass(&id).is_none() {
                    return Err(mlua::Error::external("unknown shader pass id"));
                }
                let pass_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad pass id"))?;
                h.shader_commands.push(ShaderCommand::SetEnabled { pass_id, enabled });
                if let Some((li, pi)) = h.shaders.find_pass(&id) {
                    h.shaders.layers[li].passes[pi].enabled = enabled;
                }
                Ok(true)
            })?,
        )?;
    }
    // rename(id, name) / remove(id) -> true.
    {
        let h = host.clone();
        shader.set(
            "rename",
            lua.create_function(move |_, (id, name): (String, String)| {
                if name.is_empty() || name.len() > 128 {
                    return Err(mlua::Error::external("name must be 1..128 chars"));
                }
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                let Some((li, pi)) = h.shaders.find_pass(&id) else {
                    return Err(mlua::Error::external("unknown shader pass id"));
                };
                let pass_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad pass id"))?;
                h.shader_commands.push(ShaderCommand::RenamePass {
                    pass_id,
                    name: name.clone(),
                });
                h.shaders.layers[li].passes[pi].name = name;
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        shader.set(
            "remove",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::DocumentWrite) {
                    return Err(mlua::Error::external("document.write capability is not granted"));
                }
                let Some((li, _)) = h.shaders.find_pass(&id) else {
                    return Err(mlua::Error::external("unknown shader pass id"));
                };
                let pass_id: uuid::Uuid =
                    id.parse().map_err(|_| mlua::Error::external("bad pass id"))?;
                h.shader_commands.push(ShaderCommand::RemovePass { pass_id });
                h.shaders.layers[li].passes.retain(|p| p.id != id);
                h.shader_sources.remove(&id);
                Ok(true)
            })?,
        )?;
    }
    vblua.set("shader", shader)?;
    Ok(())
}

/// `vblua.video` — Phase 10 AV clip orchestration (existing media only).
fn register_video_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::video::{VideoCommand, MAX_TIME_SEC, MIN_CLIP_SEC, preview_split};
    let video = lua.create_table()?;

    // clips([layer_id]) -> [{id, name, kind, start, length, offset, row, duration}]
    {
        let h = host.clone();
        video.set(
            "clips",
            lua.create_function(move |lua, layer: Option<String>| {
                let h = h.lock().unwrap();
                let idx: Vec<usize> = match layer {
                    Some(id) => h
                        .video
                        .layers
                        .iter()
                        .position(|l| l.layer_id == id)
                        .map(|p| vec![p])
                        .unwrap_or_default(),
                    None => (0..h.video.layers.len()).collect(),
                };
                let t = lua.create_table()?;
                let mut n = 0;
                for li in idx {
                    for c in &h.video.layers[li].clips {
                        n += 1;
                        let ct = lua.create_table()?;
                        ct.set("id", c.id.clone())?;
                        ct.set("layer", c.layer_id.clone())?;
                        ct.set("name", c.name.clone())?;
                        ct.set("kind", c.kind.clone())?;
                        ct.set("start", c.timeline_start as f64)?;
                        ct.set("length", c.play_length as f64)?;
                        ct.set("offset", c.start_offset as f64)?;
                        ct.set("row", c.track_row as i64)?;
                        ct.set("muted", c.muted)?;
                        ct.set("locked", c.locked)?;
                        match c.duration {
                            Some(d) => ct.set("duration", d as f64)?,
                            None => ct.set("duration", mlua::Nil)?,
                        }
                        t.set(n, ct)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // get(id) -> clip table | nil.
    {
        let h = host.clone();
        video.set(
            "get",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                for layer in &h.video.layers {
                    for c in &layer.clips {
                        if c.id == id {
                            let ct = lua.create_table()?;
                            ct.set("id", c.id.clone())?;
                            ct.set("layer", c.layer_id.clone())?;
                            ct.set("name", c.name.clone())?;
                            ct.set("kind", c.kind.clone())?;
                            ct.set("start", c.timeline_start as f64)?;
                            ct.set("length", c.play_length as f64)?;
                            ct.set("offset", c.start_offset as f64)?;
                            ct.set("row", c.track_row as i64)?;
                            ct.set("muted", c.muted)?;
                            ct.set("locked", c.locked)?;
                            match c.duration {
                                Some(d) => ct.set("duration", d as f64)?,
                                None => ct.set("duration", mlua::Nil)?,
                            }
                            return Ok(mlua::Value::Table(ct));
                        }
                    }
                }
                Ok(mlua::Value::Nil)
            })?,
        )?;
    }
    // set_muted(id, bool) / set_locked(id, bool) -> true.
    for (op, kind) in [("set_muted", true), ("set_locked", false)] {
        let h = host.clone();
        video.set(
            op,
            lua.create_function(move |_, (id, flag): (String, bool)| {
                write_guard(&h)?;
                let mut h = h.lock().unwrap();
                let (li, ci) = h.video.find_clip(&id).ok_or_else(|| mlua::Error::external("unknown clip id"))?;
                let cmd = if kind {
                    super::video::VideoCommand::SetMuted { clip_id: parse_uuid(&id)?, muted: flag }
                } else {
                    super::video::VideoCommand::SetLocked { clip_id: parse_uuid(&id)?, locked: flag }
                };
                h.video_commands.push(cmd);
                if kind {
                    h.video.layers[li].clips[ci].muted = flag;
                } else {
                    h.video.layers[li].clips[ci].locked = flag;
                }
                Ok(true)
            })?,
        )?;
    }
    // duplicate(id) -> new id (shares media, appended after original).
    {
        let h = host.clone();
        video.set(
            "duplicate",
            lua.create_function(move |_, id: String| {
                write_guard(&h)?;
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                let new_id = uuid::Uuid::new_v4();
                h.video_commands.push(super::video::VideoCommand::Duplicate {
                    clip_id: parse_uuid(&id)?,
                    new_id,
                });
                // Optimistic row: copy after the original (mirrors apply).
                let mut row = h.video.layers[li].clips[ci].clone();
                row.id = new_id.to_string();
                row.name = format!("{} copy", row.name.chars().take(100).collect::<String>());
                row.timeline_start += row.play_length;
                h.video.layers[li].clips.insert(ci + 1, row);
                Ok(new_id.to_string())
            })?,
        )?;
    }
    // ripple_delete(id) -> true (remove + close the gap on its row).
    {
        let h = host.clone();
        video.set(
            "ripple_delete",
            lua.create_function(move |_, id: String| {
                write_guard(&h)?;
                let _ = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                h.video_commands.push(super::video::VideoCommand::RippleDelete {
                    clip_id: parse_uuid(&id)?,
                });
                // Optimistic snapshot: drop + shift followers (mirrors apply,
                // including the source-duration cap on the shifted span).
                for layer in &mut h.video.layers {
                    if let Some(pos) = layer.clips.iter().position(|c| c.id == id) {
                        let gone = layer.clips[pos].clone();
                        let cap = gone.duration.unwrap_or(gone.play_length).max(0.0);
                        let remaining = (cap - gone.start_offset.max(0.0)).max(0.0);
                        let span = if gone.play_length >= 3599.0 {
                            remaining
                        } else {
                            gone.play_length.min(remaining).max(0.0)
                        };
                        layer.clips.remove(pos);
                        for c in layer.clips.iter_mut() {
                            if c.track_row == gone.track_row
                                && c.timeline_start >= gone.timeline_start
                            {
                                c.timeline_start = (c.timeline_start - span).max(0.0);
                            }
                        }
                        break;
                    }
                }
                Ok(true)
            })?,
        )?;
    }
    // markers() -> [{id, name, time}] (document timeline bookmarks, sorted).
    {
        let h = host.clone();
        video.set(
            "markers",
            lua.create_function(move |lua, ()| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                for (i, m) in h.video.markers.iter().enumerate() {
                    let mt = lua.create_table()?;
                    mt.set("id", m.id.clone())?;
                    mt.set("name", m.name.clone())?;
                    mt.set("time", m.time as f64)?;
                    t.set(i + 1, mt)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // add_marker(name, time) -> id.
    {
        let h = host.clone();
        video.set(
            "add_marker",
            lua.create_function(move |_, (name, time): (String, f64)| {
                write_guard(&h)?;
                if !time.is_finite() {
                    return Err(mlua::Error::external("time must be finite"));
                }
                let mut h = h.lock().unwrap();
                let new_id = uuid::Uuid::new_v4();
                let t = (time.clamp(0.0, MAX_TIME_SEC as f64)) as f32;
                h.video_commands.push(super::video::VideoCommand::AddMarker {
                    marker_id: new_id,
                    name: name.clone(),
                    time: t,
                });
                // Optimistic snapshot (mirrors apply: default name, sort by time).
                let nm: String = name.chars().take(128).collect();
                h.video.markers.push(super::video::MarkerSnapshot {
                    id: new_id.to_string(),
                    name: if nm.is_empty() { "Marker".into() } else { nm },
                    time: t,
                });
                h.video.sort_markers();
                Ok(new_id.to_string())
            })?,
        )?;
    }
    // rename_marker(id, name) / move_marker(id, time) / remove_marker(id) -> true.
    {
        let h = host.clone();
        video.set(
            "rename_marker",
            lua.create_function(move |_, (id, name): (String, String)| {
                write_guard(&h)?;
                if name.is_empty() {
                    return Err(mlua::Error::external("name must be non-empty"));
                }
                let mut h = h.lock().unwrap();
                let mi = h
                    .video
                    .find_marker(&id)
                    .ok_or_else(|| mlua::Error::external("unknown marker id"))?;
                h.video_commands.push(super::video::VideoCommand::RenameMarker {
                    marker_id: parse_uuid(&id)?,
                    name: name.clone(),
                });
                h.video.markers[mi].name = name.chars().take(128).collect();
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        video.set(
            "move_marker",
            lua.create_function(move |_, (id, time): (String, f64)| {
                write_guard(&h)?;
                if !time.is_finite() {
                    return Err(mlua::Error::external("time must be finite"));
                }
                let mut h = h.lock().unwrap();
                let mi = h
                    .video
                    .find_marker(&id)
                    .ok_or_else(|| mlua::Error::external("unknown marker id"))?;
                let t = (time.clamp(0.0, MAX_TIME_SEC as f64)) as f32;
                h.video_commands.push(super::video::VideoCommand::MoveMarker {
                    marker_id: parse_uuid(&id)?,
                    time: t,
                });
                h.video.markers[mi].time = t;
                h.video.sort_markers();
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        video.set(
            "remove_marker",
            lua.create_function(move |_, id: String| {
                write_guard(&h)?;
                let mut h = h.lock().unwrap();
                let mi = h
                    .video
                    .find_marker(&id)
                    .ok_or_else(|| mlua::Error::external("unknown marker id"))?;
                h.video_commands.push(super::video::VideoCommand::RemoveMarker {
                    marker_id: parse_uuid(&id)?,
                });
                h.video.markers.remove(mi);
                Ok(true)
            })?,
        )?;
    }
    // add_clip(path, ...) -> denied (needs Asset API + picker).
    video.set(
        "add_clip",
        lua.create_function(move |_, _: mlua::MultiValue| {
            Err::<bool, _>(mlua::Error::external(
                "video.add_clip(path) is not scriptable — new media arrives via the Asset API (Phase 11) + platform picker (Phase 12), never Lua path strings",
            ))
        })?,
    )?;
    // move(id, timeline_start) -> true.
    {
        let h = host.clone();
        video.set(
            "move",
            lua.create_function(move |_, (id, start): (String, f64)| {
                write_guard(&h)?;
                if !start.is_finite() {
                    return Err(mlua::Error::external("start must be finite"));
                }
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                let clip_id = parse_uuid(&id)?;
                h.video_commands.push(VideoCommand::Move {
                    clip_id,
                    timeline_start: start.clamp(0.0, MAX_TIME_SEC as f64) as f32,
                });
                h.video.layers[li].clips[ci].timeline_start =
                    start.clamp(0.0, MAX_TIME_SEC as f64) as f32;
                Ok(true)
            })?,
        )?;
    }
    // trim(id, {offset, length}) -> true.
    {
        let h = host.clone();
        video.set(
            "trim",
            lua.create_function(move |_, (id, opts): (String, mlua::Table)| {
                write_guard(&h)?;
                let offset: f64 = opts.get("offset").unwrap_or(0.0);
                let length: f64 = opts.get("length").unwrap_or(1.0);
                if !offset.is_finite() || !length.is_finite() {
                    return Err(mlua::Error::external("offset/length must be finite"));
                }
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                let clip_id = parse_uuid(&id)?;
                let (off, len) = (offset.max(0.0) as f32, length.clamp(MIN_CLIP_SEC as f64, MAX_TIME_SEC as f64) as f32);
                h.video_commands.push(VideoCommand::Trim {
                    clip_id,
                    start_offset: off,
                    play_length: len,
                });
                h.video.layers[li].clips[ci].start_offset = off;
                h.video.layers[li].clips[ci].play_length = len;
                Ok(true)
            })?,
        )?;
    }
    // set_row(id, row) / rename(id, name) / remove(id) -> true.
    {
        let h = host.clone();
        video.set(
            "set_row",
            lua.create_function(move |_, (id, row): (String, i64)| {
                write_guard(&h)?;
                if row < 0 || row > 64 {
                    return Err(mlua::Error::external("row must be 0..64"));
                }
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                h.video_commands.push(VideoCommand::SetRow {
                    clip_id: parse_uuid(&id)?,
                    row: row as u32,
                });
                h.video.layers[li].clips[ci].track_row = row as u32;
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        video.set(
            "rename",
            lua.create_function(move |_, (id, name): (String, String)| {
                write_guard(&h)?;
                if name.is_empty() || name.len() > 128 {
                    return Err(mlua::Error::external("name must be 1..128 chars"));
                }
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                h.video_commands.push(VideoCommand::Rename {
                    clip_id: parse_uuid(&id)?,
                    name: name.clone(),
                });
                h.video.layers[li].clips[ci].name = name;
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        video.set(
            "remove",
            lua.create_function(move |_, id: String| {
                write_guard(&h)?;
                let (li, _) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                h.video_commands.push(VideoCommand::Remove {
                    clip_id: parse_uuid(&id)?,
                });
                h.video.layers[li].clips.retain(|c| c.id != id);
                Ok(true)
            })?,
        )?;
    }
    // split(id, at_sec) -> new clip id (validated now).
    {
        let h = host.clone();
        video.set(
            "split",
            lua.create_function(move |_, (id, at_sec): (String, f64)| {
                write_guard(&h)?;
                if !at_sec.is_finite() {
                    return Err(mlua::Error::external("split point must be finite"));
                }
                let (li, ci) = clip_for_write(&h, &id)?;
                let mut h = h.lock().unwrap();
                let clip = h.video.layers[li].clips[ci].clone();
                let (left_len, right_start, right_off, right_len) =
                    preview_split(&clip, at_sec as f32).map_err(mlua::Error::external)?;
                let new_id = uuid::Uuid::new_v4();
                h.video_commands.push(VideoCommand::Split {
                    clip_id: parse_uuid(&id)?,
                    new_id,
                    at_sec: at_sec.clamp(0.0, MAX_TIME_SEC as f64) as f32,
                });
                // Optimistic snapshot: left shrinks, right appears after it.
                h.video.layers[li].clips[ci].play_length = left_len;
                let mut right = clip;
                right.id = new_id.to_string();
                right.name = format!("{} (split)", right.name.chars().take(100).collect::<String>());
                right.timeline_start = right_start;
                right.start_offset = right_off;
                right.play_length = right_len;
                h.video.layers[li].clips.insert(ci + 1, right);
                Ok(new_id.to_string())
            })?,
        )?;
    }
    vblua.set("video", video)?;
    Ok(())
}

/// Shared write-path guard: capability + budget (keeps handlers small).
fn write_guard(host: &Arc<Mutex<ApiHost>>) -> mlua::Result<()> {
    let h = host.lock().unwrap();
    if !h.policy.allows(Capability::DocumentWrite) {
        return Err(mlua::Error::external("document.write capability is not granted"));
    }
    if !h.has_document {
        return Err(mlua::Error::external("no document open"));
    }
    let staged = h.commands.len() + h.graph_commands.len() + h.anim_commands.len()
        + h.shader_commands.len()
        + h.video_commands.len();
    if staged >= h.policy.max_commands {
        return Err(mlua::Error::external("too many staged commands"));
    }
    Ok(())
}

/// Parse a Uuid-or-error for command queues.
fn parse_uuid(id: &str) -> mlua::Result<uuid::Uuid> {
    id.parse().map_err(|_| mlua::Error::external("bad id"))
}

/// Find a clip and refuse locked ones (call-time guard; apply re-checks).
fn clip_for_write(
    host: &Arc<Mutex<ApiHost>>,
    id: &str,
) -> mlua::Result<(usize, usize)> {
    let h = host.lock().unwrap();
    let pos = h
        .video
        .find_clip(id)
        .ok_or_else(|| mlua::Error::external("unknown clip id"))?;
    if h.video.layers[pos.0].clips[pos.1].locked {
        return Err(mlua::Error::external("clip is locked"));
    }
    Ok(pos)
}

/// `vblua.assets` — Phase 11 read-only inventory + opaque handles.
/// No commands, no paths, no bytes. `import` stays denied until Phase 12.
fn register_assets_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let assets = lua.create_table()?;

    // list([kind]) -> [{ref, kind, name, width?, height?, bytes?, duration?}]
    {
        let h = host.clone();
        assets.set(
            "list",
            lua.create_function(move |lua, kind: Option<String>| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                let mut n = 0;
                for item in &h.assets.items {
                    if let Some(k) = &kind
                        && item.kind != *k
                    {
                        continue;
                    }
                    n += 1;
                    t.set(n, asset_table(lua, item)?)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // info(ref) -> detail table | nil.
    {
        let h = host.clone();
        assets.set(
            "info",
            lua.create_function(move |lua, ref_id: String| {
                let h = h.lock().unwrap();
                match h.assets.items.iter().find(|i| i.ref_id == ref_id) {
                    Some(item) => Ok(mlua::Value::Table(asset_table(lua, item)?)),
                    None => Ok(mlua::Value::Nil),
                }
            })?,
        )?;
    }
    // kinds() -> ["audio", "image", "video"] present in this project.
    {
        let h = host.clone();
        assets.set(
            "kinds",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                for (i, k) in h.assets.kinds_present().iter().enumerate() {
                    t.set(i + 1, k.clone())?;
                }
                Ok(t)
            })?,
        )?;
    }
    // import(...) -> denied (needs the platform picker, Phase 12).
    assets.set(
        "import",
        lua.create_function(move |_, _: mlua::MultiValue| {
            Err::<bool, _>(mlua::Error::external(
                "assets.import() is not scriptable — new bytes enter only through the platform file picker (Phase 12); scripts use the refs from assets.list()",
            ))
        })?,
    )?;
    vblua.set("assets", assets)?;
    Ok(())
}

fn asset_table(
    lua: &mlua::Lua,
    item: &super::assets::AssetItem,
) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    t.set("ref", item.ref_id.clone())?;
    t.set("kind", item.kind.clone())?;
    t.set("name", item.name.clone())?;
    match item.width {
        Some(w) => t.set("width", w)?,
        None => t.set("width", mlua::Nil)?,
    }
    match item.height {
        Some(h) => t.set("height", h)?,
        None => t.set("height", mlua::Nil)?,
    }
    match item.bytes {
        Some(b) => t.set("bytes", b as i64)?,
        None => t.set("bytes", mlua::Nil)?,
    }
    match item.duration {
        Some(d) => t.set("duration", d as f64)?,
        None => t.set("duration", mlua::Nil)?,
    }
    Ok(t)
}

/// `vblua.file` — Phase 12 picker abstraction. Bytes in, never paths out.
fn register_file_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::file::{decode_pick, PickFilter, FileCommand};
    let file = lua.create_table()?;

    // status() -> {picker = bool} so scripts degrade gracefully on mobile.
    {
        let h = host.clone();
        file.set(
            "status",
            lua.create_function(move |lua, _: ()| {
                let t = lua.create_table()?;
                t.set("picker", h.lock().unwrap().picker.is_some())?;
                Ok(t)
            })?,
        )?;
    }
    // pick({filters, title}) -> image node id (bytes via the host hook).
    {
        let h = host.clone();
        file.set(
            "pick",
            lua.create_function(move |_, opts: Option<mlua::Table>| {
                let (filters, title) = match opts {
                    Some(o) => {
                        let f: Vec<String> = match o.get::<mlua::Value>("filters") {
                            Ok(mlua::Value::Table(t)) => {
                                let mut v = Vec::new();
                                for s in t.sequence_values::<String>() {
                                    v.push(s.map_err(|_| {
                                        mlua::Error::external("filters must be strings")
                                    })?);
                                }
                                v
                            }
                            Ok(mlua::Value::String(s)) => {
                                vec![s.to_str()?.to_string()]
                            }
                            _ => Vec::new(),
                        };
                        let title = o.get::<String>("title").ok();
                        (f, title)
                    }
                    None => (vec!["png".to_string(), "jpg".to_string()], None),
                };
                let filter = PickFilter::from_lua(filters, title).map_err(mlua::Error::external)?;
                let mut h = h.lock().unwrap();
                if !h.policy.allows(Capability::FilesystemRead) {
                    return Err(mlua::Error::external(
                        "filesystem.read capability is not granted",
                    ));
                }
                if !h.has_nodestore {
                    return Err(mlua::Error::external(
                        "file.pick needs the project entry point (console/app)",
                    ));
                }
                let hook = h.picker.clone().ok_or_else(|| {
                    mlua::Error::external(
                        "no platform picker wired (desktop wires rfd; mobile needs SAF/document picker)",
                    )
                })?;
                if !h.has_document {
                    return Err(mlua::Error::external("no document open"));
                }
                // Blocking platform dialog (rfd on desktop; host bridges async
                // pickers). Cancellation is a clean error, not a crash.
                let picked = hook(&filter).map_err(mlua::Error::external)?;
                let (w, hh) = decode_pick(&picked.bytes).map_err(mlua::Error::external)?;
                if h.assets.items.len() > 4096 {
                    return Err(mlua::Error::external("too many project assets"));
                }
                // Resolve target: active image layer, else first image layer.
                let layer_id: uuid::Uuid = {
                    let active = h.snapshot.active_layer_id.clone();
                    let ids: Vec<(String, bool)> = h
                        .snapshot
                        .layers
                        .iter()
                        .map(|l| (l.id.clone(), l.kind == "image"))
                        .collect();
                    let pick = active
                        .filter(|a| ids.iter().any(|(id, img)| id == a && *img))
                        .or_else(|| {
                            ids.iter().find(|(_, img)| *img).map(|(id, _)| id.clone())
                        })
                        .ok_or_else(|| {
                            mlua::Error::external("no image layer to import into")
                        })?;
                    pick.parse()
                        .map_err(|_| mlua::Error::external("corrupt layer id"))?
                };
                let id = uuid::Uuid::new_v4();
                let name: String = picked
                    .name
                    .chars()
                    .take(128)
                    .collect::<String>()
                    .trim()
                    .to_string();
                let name = if name.is_empty() { "Picked image".to_string() } else { name };
                h.file_commands.push(FileCommand::ImportImage {
                    id,
                    layer_id,
                    name: name.clone(),
                    width: w as f64,
                    height: hh as f64,
                    bytes: picked.bytes.clone(),
                });
                // Optimistic asset row so list()/info() see it this run.
                h.assets.items.push(super::assets::AssetItem {
                    ref_id: format!("node:{id}"),
                    kind: "image".to_string(),
                    name,
                    width: Some(w as f64),
                    height: Some(hh as f64),
                    bytes: Some(picked.bytes.len()),
                    duration: None,
                });
                Ok(id.to_string())
            })?,
        )?;
    }
    vblua.set("file", file)?;
    Ok(())
}

/// `vblua.ui` — Phase 13 extension panels (host renders, Lua declares).
fn register_ui_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::ui_widgets::UiWidgetKind;
    let ui = lua.create_table()?;

    // panel(id, title) -> true.
    {
        let h = host.clone();
        ui.set(
            "panel",
            lua.create_function(move |_, (id, title): (String, String)| {
                let mut h = h.lock().unwrap();
                ui_guard(&h)?;
                h.ui.panel(&id, &title).map_err(mlua::Error::external)?;
                Ok(true)
            })?,
        )?;
    }
    // drop_panel(id) / clear(id) -> true.
    {
        let h = host.clone();
        ui.set(
            "drop_panel",
            lua.create_function(move |lua, id: String| {
                let mut h = h.lock().unwrap();
                ui_guard(&h)?;
                let widget_ids: Vec<u64> = h
                    .ui
                    .panels
                    .get(&id)
                    .map(|p| p.widgets.iter().map(|w| w.id).collect())
                    .ok_or_else(|| mlua::Error::external("unknown panel id"))?;
                h.ui.drop_panel(&id);
                for wid in widget_ids {
                    h.ui_callbacks.remove(&wid);
                }
                let _ = lua;
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        ui.set(
            "clear",
            lua.create_function(move |lua, id: String| {
                let mut h = h.lock().unwrap();
                ui_guard(&h)?;
                let widget_ids: Vec<u64> = h
                    .ui
                    .panels
                    .get(&id)
                    .map(|p| p.widgets.iter().map(|w| w.id).collect())
                    .ok_or_else(|| mlua::Error::external("unknown panel id"))?;
                h.ui.clear_panel(&id).map_err(mlua::Error::external)?;
                for wid in widget_ids {
                    h.ui_callbacks.remove(&wid);
                }
                let _ = lua;
                Ok(true)
            })?,
        )?;
    }
    // panels() -> [{id, title, widgets}].
    {
        let h = host.clone();
        ui.set(
            "panels",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let t = lua.create_table()?;
                let mut n = 0;
                let mut ids: Vec<_> = h.ui.panels.keys().cloned().collect();
                ids.sort();
                for id in ids {
                    if let Some(p) = h.ui.panels.get(&id) {
                        n += 1;
                        let pt = lua.create_table()?;
                        pt.set("id", p.id.clone())?;
                        pt.set("title", p.title.clone())?;
                        pt.set("widgets", p.widgets.len() as i64)?;
                        t.set(n, pt)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // text(panel, content) -> widget id.
    {
        let h = host.clone();
        ui.set(
            "text",
            lua.create_function(move |_, (panel, content): (String, String)| {
                let mut h = h.lock().unwrap();
                ui_guard(&h)?;
                if content.len() > 1024 {
                    return Err(mlua::Error::external("text must be <= 1024 chars"));
                }
                h.ui
                    .push_widget(&panel, UiWidgetKind::Text { content })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    // button(panel, label, onclick) -> widget id.
    {
        let h = host.clone();
        ui.set(
            "button",
            lua.create_function(
                move |_, (panel, label, onclick): (String, String, mlua::Function)| {
                    if label.is_empty() || label.len() > 128 {
                        return Err(mlua::Error::external("label must be 1..128 chars"));
                    }
                    let mut h = h.lock().unwrap();
                    ui_guard(&h)?;
                    let id = h
                        .ui
                        .push_widget(&panel, UiWidgetKind::Button { label })
                        .map_err(mlua::Error::external)?;
                    h.ui_callbacks.insert(id, onclick);
                    mark_callback(&mut h.ui, &panel, id);
                    Ok(id as i64)
                },
            )?,
        )?;
    }
    // slider(panel, label, min, max[, initial, onchange]) -> widget id.
    {
        let h = host.clone();
        ui.set(
            "slider",
            lua.create_function(
                move |_, (panel, label, min, max, initial, onchange): (
                    String,
                    String,
                    f64,
                    f64,
                    Option<f64>,
                    Option<mlua::Function>,
                )| {
                    if label.is_empty() || label.len() > 128 {
                        return Err(mlua::Error::external("label must be 1..128 chars"));
                    }
                    if !min.is_finite() || !max.is_finite() || min >= max {
                        return Err(mlua::Error::external("need finite min < max"));
                    }
                    let key = onchange;
                    let mut h = h.lock().unwrap();
                    ui_guard(&h)?;
                    let id = h
                        .ui
                        .push_widget(&panel, UiWidgetKind::Slider { label, min, max })
                        .map_err(mlua::Error::external)?;
                    if let Some(p) = h.ui.panels.get_mut(&panel)
                        && let Some(w) = p.widgets.iter_mut().find(|w| w.id == id)
                    {
                        w.num_value = initial.unwrap_or((min + max) / 2.0).clamp(min, max);
                    }
                    if let Some(k) = key {
                        h.ui_callbacks.insert(id, k);
                        mark_callback(&mut h.ui, &panel, id);
                    }
                    Ok(id as i64)
                },
            )?,
        )?;
    }
    // checkbox(panel, label[, value, onchange]) -> widget id.
    {
        let h = host.clone();
        ui.set(
            "checkbox",
            lua.create_function(
                move |_, (panel, label, value, onchange): (
                    String,
                    String,
                    Option<bool>,
                    Option<mlua::Function>,
                )| {
                    if label.is_empty() || label.len() > 128 {
                        return Err(mlua::Error::external("label must be 1..128 chars"));
                    }
                    let key = onchange;
                    let mut h = h.lock().unwrap();
                    ui_guard(&h)?;
                    let id = h
                        .ui
                        .push_widget(&panel, UiWidgetKind::Checkbox { label })
                        .map_err(mlua::Error::external)?;
                    if let Some(p) = h.ui.panels.get_mut(&panel)
                        && let Some(w) = p.widgets.iter_mut().find(|w| w.id == id)
                    {
                        w.bool_value = value.unwrap_or(false);
                    }
                    if let Some(k) = key {
                        h.ui_callbacks.insert(id, k);
                        mark_callback(&mut h.ui, &panel, id);
                    }
                    Ok(id as i64)
                },
            )?,
        )?;
    }
    vblua.set("ui", ui)?;
    Ok(())
}

/// Flag a widget as callback-backed (split borrow from registry insert).
fn mark_callback(
    ui: &mut super::ui_widgets::UiRegistry,
    panel: &str,
    widget_id: u64,
) {
    if let Some(p) = ui.panels.get_mut(panel)
        && let Some(w) = p.widgets.iter_mut().find(|w| w.id == widget_id)
    {
        w.has_callback = true;
    }
}

/// `ui` capability guard (panels are a privileged surface).
fn ui_guard(host: &ApiHost) -> mlua::Result<()> {
    if !host.policy.allows(Capability::Ui) {
        return Err(mlua::Error::external("ui capability is not granted"));
    }
    Ok(())
}

/// `vblua.node` — Phase 14 scripted node types (template recipes).
fn register_scripted_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::scripted::compile_template;
    let node = lua.create_table()?;

    // define(name, {nodes, links}) -> true (validated now, engine owns it).
    {
        let h = host.clone();
        node.set(
            "define",
            lua.create_function(move |_, (name, spec): (String, mlua::Table)| {
                let raw = decode_template(&spec)?;
                let tpl =
                    compile_template(&name, raw).map_err(mlua::Error::external)?;
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                if h.templates.templates.len() >= super::scripted::MAX_TEMPLATES
                    && !h.templates.templates.contains_key(&name)
                {
                    return Err(mlua::Error::external("too many templates"));
                }
                h.templates.templates.insert(name, tpl);
                Ok(true)
            })?,
        )?;
    }
    // templates() -> [names].
    {
        let h = host.clone();
        node.set(
            "templates",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let mut names: Vec<_> = h.templates.templates.keys().cloned().collect();
                names.sort();
                let t = lua.create_table()?;
                for (i, n) in names.iter().enumerate() {
                    t.set(i + 1, n.clone())?;
                }
                Ok(t)
            })?,
        )?;
    }
    // drop(name) -> true.
    {
        let h = host.clone();
        node.set(
            "drop",
            lua.create_function(move |_, name: String| {
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                h.templates
                    .templates
                    .remove(&name)
                    .map(|_| true)
                    .ok_or_else(|| mlua::Error::external("unknown template"))
            })?,
        )?;
    }
    // spawn(name[, {layer, x, y}]) -> [ids] (one batch, ordinary host nodes).
    {
        let h = host.clone();
        node.set(
            "spawn",
            lua.create_function(
                move |lua, (name, opts): (String, Option<mlua::Table>)| {
                    let mut h = h.lock().unwrap();
                    write_cap(&h)?;
                    if !h.has_document {
                        return Err(mlua::Error::external("no document open"));
                    }
                    let tpl = h
                        .templates
                        .templates
                        .get(&name)
                        .cloned()
                        .ok_or_else(|| mlua::Error::external("unknown template"))?;
                    let (mut layer_idx, mut ox, mut oy) = (None, 0.0f32, 0.0f32);
                    if let Some(o) = opts {
                        if let Ok(l) = o.get::<String>("layer") {
                            layer_idx = h.graphs.layers.iter().position(|l2| l2.layer_id == l);
                            if layer_idx.is_none() {
                                return Err(mlua::Error::external("unknown graph layer"));
                            }
                        }
                        ox = o.get::<f32>("x").unwrap_or(0.0);
                        oy = o.get::<f32>("y").unwrap_or(0.0);
                    }
                    let li = match layer_idx {
                        Some(i) => i,
                        None => h.graphs.resolve_target().map_err(mlua::Error::external)?,
                    };
                    let staged = h.commands.len()
                        + h.graph_commands.len()
                        + h.anim_commands.len()
                        + h.shader_commands.len()
                        + h.video_commands.len()
                        + h.file_commands.len();
                    if staged + tpl.nodes.len() * 2 + tpl.links.len() > h.policy.max_commands {
                        return Err(mlua::Error::external("too many staged commands"));
                    }
                    let layer_id: uuid::Uuid = h.graphs.layers[li]
                        .layer_id
                        .parse()
                        .map_err(|_| mlua::Error::external("corrupt layer id"))?;
                    // Preallocate so internal wires resolve in the same run.
                    let ids: Vec<uuid::Uuid> =
                        (0..tpl.nodes.len()).map(|_| uuid::Uuid::new_v4()).collect();
                    for (i, tn) in tpl.nodes.iter().enumerate() {
                        let (nx, ny) = (
                            (ox + tn.dx).clamp(-4000.0, 4000.0),
                            (oy + tn.dy).clamp(-4000.0, 4000.0),
                        );
                        h.graph_commands.push(super::graph::GraphCommand::CreateNode {
                            id: ids[i],
                            layer_id,
                            kind: tn.kind.clone(),
                            x: nx,
                            y: ny,
                            name: tn.name.clone(),
                        });
                        for (k, v) in &tn.params {
                            h.graph_commands.push(super::graph::GraphCommand::SetNodeParam {
                                node_id: ids[i],
                                key: k.clone(),
                                value: v.clone(),
                            });
                        }
                        h.graph_kinds.insert(ids[i].to_string(), tn.kind.clone());
                        let snap_layer = h.graphs.layers[li].layer_id.clone();
                        let snap_name = tn
                            .name
                            .clone()
                            .unwrap_or_else(|| super::graph::kind_label(&tn.kind).to_string());
                        h.graphs.layers[li].nodes.push(super::graph::GraphNodeSnapshot {
                            id: ids[i].to_string(),
                            name: snap_name,
                            kind_name: tn.kind_name.clone(),
                            layer_id: snap_layer,
                            x: (ox + tn.dx).clamp(-4000.0, 4000.0),
                            y: (oy + tn.dy).clamp(-4000.0, 4000.0),
                        });
                    }
                    for tl in &tpl.links {
                        let (f, t) = (ids[tl.from], ids[tl.to]);
                        h.graph_commands.push(super::graph::GraphCommand::Connect {
                            from_node: f,
                            from_port: tl.from_port.clone(),
                            to_node: t,
                            to_port: tl.to_port.clone(),
                        });
                        h.graphs.layers[li].links.push(super::graph::GraphLinkSnapshot {
                            from_node: f.to_string(),
                            from_port: tl.from_port.clone(),
                            to_node: t.to_string(),
                            to_port: tl.to_port.clone(),
                        });
                    }
                    let out = lua.create_table()?;
                    for (i, id) in ids.iter().enumerate() {
                        out.set(i + 1, id.to_string())?;
                    }
                    Ok(out)
                },
            )?,
        )?;
    }
    vblua.set("node", node)?;
    Ok(())
}

/// Decode a Lua template spec into raw slots (1-based link indices).
fn decode_template(spec: &mlua::Table) -> mlua::Result<super::scripted::RawTemplate> {
    use super::scripted::{RawLink, RawNode, RawParam, RawTemplate};
    let nodes_t: mlua::Table = spec.get("nodes").map_err(|_| {
        mlua::Error::external("template needs nodes = {{ kind = ..., x = ..., y = ... }, ...}")
    })?;
    let mut nodes = Vec::new();
    for n in nodes_t.sequence_values::<mlua::Table>() {
        let n = n?;
        let kind: String = n.get("kind").map_err(|_| {
            mlua::Error::external("each template node needs kind = \"...\"")
        })?;
        let dx = n.get::<f64>("x").unwrap_or(0.0);
        let dy = n.get::<f64>("y").unwrap_or(0.0);
        let name = n.get::<String>("name").ok();
        let mut params = Vec::new();
        if let Ok(pt) = n.get::<mlua::Table>("params") {
            for pair in pt.pairs::<String, mlua::Value>() {
                let (k, v) = pair?;
                if k.is_empty() || k.len() > 64 {
                    return Err(mlua::Error::external("param keys must be 1..64 chars"));
                }
                let key = k.clone();
                params.push((
                    key,
                    match v {
                        mlua::Value::Integer(i) => RawParam::Num(i as f64),
                        mlua::Value::Number(x) => RawParam::Num(x),
                        mlua::Value::String(s) => RawParam::Str(
                            s.to_str()?.to_string(),
                        ),
                        _ => {
                            return Err(mlua::Error::external(format!(
                                "param '{k}' must be a number or string"
                            )));
                        }
                    },
                ));
            }
        }
        nodes.push(RawNode {
            kind,
            dx: dx as f32,
            dy: dy as f32,
            name,
            params,
        });
    }
    let mut links = Vec::new();
    if let Ok(lt) = spec.get::<mlua::Table>("links") {
        for l in lt.sequence_values::<mlua::Table>() {
            let l = l?;
            let from: i64 = l.get("from").map_err(|_| {
                mlua::Error::external("each link needs from/from_port/to/to_port (1-based slots)")
            })?;
            let to: i64 = l.get("to").map_err(|_| {
                mlua::Error::external("each link needs from/from_port/to/to_port (1-based slots)")
            })?;
            if from < 1 || to < 1 {
                return Err(mlua::Error::external("link slots are 1-based"));
            }
            links.push(RawLink {
                from: (from - 1) as usize,
                from_port: l.get::<String>("from_port").map_err(|_| {
                    mlua::Error::external("each link needs from_port")
                })?,
                to: (to - 1) as usize,
                to_port: l.get::<String>("to_port").map_err(|_| {
                    mlua::Error::external("each link needs to_port")
                })?,
            });
        }
    }
    Ok(RawTemplate { nodes, links })
}

/// Write-capability check shared by template mutators.
fn write_cap(host: &ApiHost) -> mlua::Result<()> {
    if !host.policy.allows(Capability::DocumentWrite) {
        return Err(mlua::Error::external("document.write capability is not granted"));
    }
    Ok(())
}

/// `vblua.batch` — Phase 15 procedural bulk operations (one call, one batch).
fn register_batch_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::batch::{MAX_BATCH_KEYS, MAX_BATCH_NODES};
    let batch = lua.create_table()?;

    // grid({x,y}, cols, dx, dy, count) -> [{x, y}...] (pure).
    batch.set(
        "grid",
        lua.create_function(
            move |lua, (origin, cols, dx, dy, count): (mlua::Table, i64, f64, f64, i64)| {
                if cols < 1 || cols > 128 || count < 0 || count > 4096 {
                    return Err(mlua::Error::external("cols 1..128, count 0..4096"));
                }
                if ![dx, dy].iter().all(|v| v.is_finite()) {
                    return Err(mlua::Error::external("dx/dy must be finite"));
                }
                let o = table_xy(Some(&origin), [0.0, 0.0])?;
                let pts = super::batch::grid(o, cols as usize, dx, dy, count as usize);
                let t = lua.create_table()?;
                for (i, p) in pts.iter().enumerate() {
                    let pt = lua.create_table()?;
                    pt.set("x", p[0])?;
                    pt.set("y", p[1])?;
                    t.set(i + 1, pt)?;
                }
                Ok(t)
            },
        )?,
    )?;
    // circle({x,y}, radius, count[, phase]) -> [{x, y}...] (pure).
    batch.set(
        "circle",
        lua.create_function(
            move |lua, (center, radius, count, phase): (mlua::Table, f64, i64, Option<f64>)| {
                if count < 0 || count > 4096 || !radius.is_finite() {
                    return Err(mlua::Error::external("count 0..4096, finite radius"));
                }
                let c = table_xy(Some(&center), [0.0, 0.0])?;
                let pts = super::batch::circle(c, radius, count as usize, phase.unwrap_or(0.0));
                let t = lua.create_table()?;
                for (i, p) in pts.iter().enumerate() {
                    let pt = lua.create_table()?;
                    pt.set("x", p[0])?;
                    pt.set("y", p[1])?;
                    t.set(i + 1, pt)?;
                }
                Ok(t)
            },
        )?,
    )?;
    // create_nodes(specs[, {layer, x, y}]) -> [ids].
    {
        let h = host.clone();
        batch.set(
            "create_nodes",
            lua.create_function(
                move |lua, (specs, opts): (Vec<mlua::Table>, Option<mlua::Table>)| {
                    if specs.is_empty() || specs.len() > MAX_BATCH_NODES {
                        return Err(mlua::Error::external(format!(
                            "1..={MAX_BATCH_NODES} specs per call"
                        )));
                    }
                    let mut decoded = Vec::with_capacity(specs.len());
                    for (i, s) in specs.iter().enumerate() {
                        decoded.push(
                            decode_node_spec(lua, s)
                                .map_err(|e| mlua::Error::external(format!("spec {}: {e}", i + 1)))?,
                        );
                    }
                    let mut h = h.lock().unwrap();
                    write_cap(&h)?;
                    if !h.has_document {
                        return Err(mlua::Error::external("no document open"));
                    }
                    let (mut layer_idx, mut ox, mut oy) = (None, 0.0f32, 0.0f32);
                    if let Some(o) = opts {
                        if let Ok(l) = o.get::<String>("layer") {
                            layer_idx = h.graphs.layers.iter().position(|l2| l2.layer_id == l);
                            if layer_idx.is_none() {
                                return Err(mlua::Error::external("unknown graph layer"));
                            }
                        }
                        ox = o.get::<f32>("x").unwrap_or(0.0);
                        oy = o.get::<f32>("y").unwrap_or(0.0);
                    }
                    let li = match layer_idx {
                        Some(i) => i,
                        None => h.graphs.resolve_target().map_err(mlua::Error::external)?,
                    };
                    let staged = h.commands.len()
                        + h.graph_commands.len()
                        + h.anim_commands.len()
                        + h.shader_commands.len()
                        + h.video_commands.len()
                        + h.file_commands.len();
                    if staged + decoded.len() * 2 > h.policy.max_commands {
                        return Err(mlua::Error::external("too many staged commands"));
                    }
                    let layer_id: uuid::Uuid = h.graphs.layers[li]
                        .layer_id
                        .parse()
                        .map_err(|_| mlua::Error::external("corrupt layer id"))?;
                    let created = super::batch::queue_creates(
                        &mut h.graph_commands,
                        layer_id,
                        decoded,
                        [ox, oy],
                    );
                    // Optimistic snapshot rows (kinds map + node list).
                    let snap_layer = h.graphs.layers[li].layer_id.clone();
                    let out = lua.create_table()?;
                    for (i, (id, kind, label, nx, ny)) in created.into_iter().enumerate() {
                        let canonical = super::graph::kind_label(&kind).to_string();
                        h.graph_kinds.insert(id.to_string(), kind);
                        h.graphs.layers[li].nodes.push(super::graph::GraphNodeSnapshot {
                            id: id.to_string(),
                            name: label,
                            kind_name: canonical,
                            layer_id: snap_layer.clone(),
                            x: nx,
                            y: ny,
                        });
                        out.set(i + 1, id.to_string())?;
                    }
                    Ok(out)
                },
            )?,
        )?;
    }
    // keyframes(id, track, {{frame, value[, interp]}...}) -> staged count.
    {
        let h = host.clone();
        batch.set(
            "keyframes",
            lua.create_function(
                move |_, (id, track, points): (String, String, Vec<mlua::Table>)| {
                    use super::animation::{parse_interp, valid_track};
                    if !valid_track(&track) {
                        return Err(mlua::Error::external(format!("unknown track '{track}'")));
                    }
                    if points.is_empty() || points.len() > MAX_BATCH_KEYS {
                        return Err(mlua::Error::external(format!(
                            "1..={MAX_BATCH_KEYS} points per call"
                        )));
                    }
                    let mut decoded = Vec::with_capacity(points.len());
                    for (i, p) in points.iter().enumerate() {
                        let frame: i64 = p.get(1).map_err(|_| {
                            mlua::Error::external(format!("point {}: needs {{frame, value}}", i + 1))
                        })?;
                        let value: f64 = p.get(2).map_err(|_| {
                            mlua::Error::external(format!("point {}: needs {{frame, value}}", i + 1))
                        })?;
                        let interp = p
                            .get::<Option<String>>(3)
                            .map_err(|_| mlua::Error::external("interp must be a string"))?;
                        let interp =
                            parse_interp(interp.as_deref()).map_err(mlua::Error::external)?;
                        if frame < 0 || frame > 1_000_000 || !value.is_finite() {
                            return Err(mlua::Error::external(format!(
                                "point {}: frame 0..1000000, finite value",
                                i + 1
                            )));
                        }
                        decoded.push((frame as usize, value, interp));
                    }
                    let mut h = h.lock().unwrap();
                    write_cap(&h)?;
                    if !h.has_document {
                        return Err(mlua::Error::external("no document open"));
                    }
                    if !h.anim.targets.contains(&id) {
                        return Err(mlua::Error::external("unknown node/layer id"));
                    }
                    let staged = h.commands.len()
                        + h.graph_commands.len()
                        + h.anim_commands.len()
                        + h.shader_commands.len()
                        + h.video_commands.len()
                        + h.file_commands.len();
                    if staged + decoded.len() > h.policy.max_commands {
                        return Err(mlua::Error::external("too many staged commands"));
                    }
                    let target: uuid::Uuid =
                        id.parse().map_err(|_| mlua::Error::external("bad node id"))?;
                    super::batch::queue_keyframes(
                        &mut h.anim_commands,
                        target,
                        track.clone(),
                        decoded.clone(),
                    );
                    // Optimistic snapshot merge.
                    let entry = h.anim.nodes.entry(id).or_default();
                    let kfs = entry.tracks.entry(track).or_default();
                    for (frame, value, interp) in decoded {
                        let label = match interp {
                            crate::document::InterpolationMode::Linear => "linear",
                            crate::document::InterpolationMode::Bezier => "bezier",
                        };
                        if let Some(k) = kfs.iter_mut().find(|k| k.frame == frame) {
                            k.value = value;
                            k.interp = label;
                        } else {
                            kfs.push(super::animation::KfSnapshot {
                                frame,
                                value,
                                interp: label,
                                hr: (5.0, 0.0),
                            });
                        }
                    }
                    kfs.sort_by_key(|k| k.frame);
                    Ok(points.len() as i64)
                },
            )?,
        )?;
    }
    vblua.set("batch", batch)?;
    Ok(())
}

/// Decode one bulk node spec (kind allowlist + params via VbValue).
fn decode_node_spec(lua: &mlua::Lua, s: &mlua::Table) -> Result<super::batch::NodeSpec, String> {
    use super::batch::NodeSpec;
    let kind_name: String = s
        .get("kind")
        .map_err(|_| "each spec needs kind = \"...\"".to_string())?;
    let kind = super::graph::parse_kind(&kind_name)?;
    let x = s.get::<f64>("x").unwrap_or(0.0);
    let y = s.get::<f64>("y").unwrap_or(0.0);
    if !x.is_finite() || !y.is_finite() {
        return Err("x/y must be finite".to_string());
    }
    let name = s.get::<String>("name").ok().filter(|n| !n.is_empty());
    if let Some(n) = &name
        && n.len() > 128
    {
        return Err("name must be <= 128 chars".to_string());
    }
    let mut params = Vec::new();
    if let Ok(pt) = s.get::<mlua::Table>("params") {
        for pair in pt.pairs::<String, mlua::Value>() {
            if params.len() >= 64 {
                return Err("at most 64 params per node".to_string());
            }
            let (k, v) = pair.map_err(|e| e.to_string())?;
            let pv = match v {
                mlua::Value::Integer(i) => super::graph::ParamValue::Num(i as f64),
                mlua::Value::Number(n) => super::graph::ParamValue::Num(n),
                mlua::Value::String(st) => super::graph::ParamValue::Str(
                    st.to_str().map_err(|e| e.to_string())?.to_string(),
                ),
                mlua::Value::Table(t) => {
                    let vv = super::value::VbValue::from_lua(mlua::Value::Table(t), lua)
                        .map_err(|e| e.to_string())?;
                    super::graph::param_curve_from_vb(&vv)
                        .map(super::graph::ParamValue::Curve)
                        .map_err(|e| format!("param '{k}': {e}"))?
                }
                _ => return Err(format!("param '{k}' must be a number, string, or curve list")),
            };
            params.push((k, pv));
        }
    }
    super::batch::check_node_spec(NodeSpec {
        kind,
        kind_name,
        x: x as f32,
        y: y as f32,
        name,
        params,
    })
}

/// Render one canvas node row as a Lua table (shared by nodes/get/layer_nodes).
fn node_row_to_lua(
    lua: &mlua::Lua,
    id: &str,
    row: &super::editor::NodeRow,
) -> mlua::Result<mlua::Table> {
    let nt = lua.create_table()?;
    nt.set("id", id.to_string())?;
    nt.set("name", row.name.clone())?;
    nt.set("layer", row.layer_id.clone())?;
    nt.set("kind", row.kind.clone())?;
    nt.set("x", row.x)?;
    nt.set("y", row.y)?;
    nt.set("rotation", row.rotation_deg)?;
    nt.set("opacity", row.opacity)?;
    Ok(nt)
}

/// `vblua.editor` — Phase 16 automation: transactions + canvas node ops.
fn register_editor_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let editor = lua.create_table()?;

    // transaction(fn) -> fn's results (all-or-nothing on error).
    {
        let h = host.clone();
        editor.set(
            "transaction",
            lua.create_function(move |_, func: mlua::Function| {
                {
                    let mut h = h.lock().unwrap();
                    if h.transactional {
                        return Err(mlua::Error::external(
                            "nested transactions are not allowed",
                        ));
                    }
                    h.transactional = true;
                }
                let result = func.call::<mlua::MultiValue>(());
                {
                    let mut h = h.lock().unwrap();
                    h.transactional = false;
                    if result.is_err() {
                        // Discard the whole batch (applied by the runtime tail).
                        h.rollback_requested = true;
                    }
                }
                result
            })?,
        )?;
    }
    // nodes() -> [{id, name, layer, kind, x, y, rotation, opacity}] (canvas inventory).
    {
        let h = host.clone();
        editor.set(
            "nodes",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let mut ids: Vec<_> = h.editor.nodes.keys().cloned().collect();
                ids.sort();
                let t = lua.create_table()?;
                for (i, id) in ids.iter().enumerate() {
                    if let Some(row) = h.editor.nodes.get(id) {
                        t.set(i + 1, node_row_to_lua(lua, id, row)?)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // get(id) -> node row | nil.
    {
        let h = host.clone();
        editor.set(
            "get",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                match h.editor.nodes.get(&id) {
                    Some(row) => Ok(mlua::Value::Table(node_row_to_lua(lua, &id, row)?)),
                    None => Ok(mlua::Value::Nil),
                }
            })?,
        )?;
    }
    // layer_nodes(layer) -> rows on one layer (layer id or name; unknown errors).
    {
        let h = host.clone();
        editor.set(
            "layer_nodes",
            lua.create_function(move |lua, layer: String| {
                let h = h.lock().unwrap();
                let lid = h
                    .snapshot
                    .layers
                    .iter()
                    .find(|l| l.id == layer || l.name == layer)
                    .map(|l| l.id.clone())
                    .ok_or_else(|| mlua::Error::external("unknown layer"))?;
                let mut ids: Vec<_> = h
                    .editor
                    .nodes
                    .iter()
                    .filter(|(_, r)| r.layer_id == lid)
                    .map(|(id, _)| id.clone())
                    .collect();
                ids.sort();
                let t = lua.create_table()?;
                for (i, id) in ids.iter().enumerate() {
                    if let Some(row) = h.editor.nodes.get(id) {
                        t.set(i + 1, node_row_to_lua(lua, id, row)?)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // rename_node(id, name) -> true.
    {
        let h = host.clone();
        editor.set(
            "rename_node",
            lua.create_function(move |_, (id, name): (String, String)| {
                if name.is_empty() || name.len() > 128 {
                    return Err(mlua::Error::external("name must be 1..128 chars"));
                }
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                let row = h
                    .editor
                    .nodes
                    .get_mut(&id)
                    .ok_or_else(|| mlua::Error::external("unknown canvas node id"))?;
                row.name = name.clone();
                h.node_commands.push(super::editor::NodeCommand::RenameNode {
                    node_id: parse_uuid(&id)?,
                    name,
                });
                Ok(true)
            })?,
        )?;
    }
    // duplicate_node(id) -> new id (preallocated).
    {
        let h = host.clone();
        editor.set(
            "duplicate_node",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                if !h.has_nodestore {
                    return Err(mlua::Error::external("needs the project entry point"));
                }
                if !h.editor.nodes.contains_key(&id) {
                    return Err(mlua::Error::external("unknown canvas node id"));
                }
                let new_id = uuid::Uuid::new_v4();
                h.node_commands.push(super::editor::NodeCommand::DuplicateNode {
                    src: parse_uuid(&id)?,
                    new_id,
                });
                Ok(new_id.to_string())
            })?,
        )?;
    }
    // delete_node(id) -> true.
    {
        let h = host.clone();
        editor.set(
            "delete_node",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                if !h.has_nodestore {
                    return Err(mlua::Error::external("needs the project entry point"));
                }
                if h.editor.nodes.remove(&id).is_none() {
                    return Err(mlua::Error::external("unknown canvas node id"));
                }
                h.node_commands.push(super::editor::NodeCommand::DeleteNode {
                    node_id: parse_uuid(&id)?,
                });
                Ok(true)
            })?,
        )?;
    }
    // move_node(id, x, y) -> true (absolute translation, canvas px).
    {
        let h = host.clone();
        editor.set(
            "move_node",
            lua.create_function(move |_, (id, x, y): (String, f64, f64)| {
                if !x.is_finite() || !y.is_finite() {
                    return Err(mlua::Error::external("x/y must be finite"));
                }
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                let row = h
                    .editor
                    .nodes
                    .get_mut(&id)
                    .ok_or_else(|| mlua::Error::external("unknown canvas node id"))?;
                let (cx, cy) = (
                    x.clamp(-16384.0, 16384.0),
                    y.clamp(-16384.0, 16384.0),
                );
                row.x = cx;
                row.y = cy;
                h.node_commands.push(super::editor::NodeCommand::MoveNode {
                    node_id: parse_uuid(&id)?,
                    x: cx,
                    y: cy,
                });
                Ok(true)
            })?,
        )?;
    }
    // set_opacity(id, o) -> true (clamped 0..1).
    {
        let h = host.clone();
        editor.set(
            "set_opacity",
            lua.create_function(move |_, (id, o): (String, f64)| {
                if !o.is_finite() {
                    return Err(mlua::Error::external("opacity must be finite"));
                }
                let mut h = h.lock().unwrap();
                write_cap(&h)?;
                let row = h
                    .editor
                    .nodes
                    .get_mut(&id)
                    .ok_or_else(|| mlua::Error::external("unknown canvas node id"))?;
                let co = o.clamp(0.0, 1.0);
                row.opacity = co;
                h.node_commands.push(super::editor::NodeCommand::SetOpacity {
                    node_id: parse_uuid(&id)?,
                    opacity: co as f32,
                });
                Ok(true)
            })?,
        )?;
    }
    vblua.set("editor", editor)?;
    Ok(())
}

/// `vblua.addons` — Phase 18 local lifecycle (scan/enable/disable).
fn register_addons_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let addons = lua.create_table()?;

    // dir() -> data-dir path | nil.
    {
        let h = host.clone();
        addons.set(
            "dir",
            lua.create_function(move |_, _: ()| {
                Ok(h.lock().unwrap().addons.dir().map(|p| p.display().to_string()))
            })?,
        )?;
    }
    // refresh() -> addon count (scan errors go to the console).
    {
        let h = host.clone();
        addons.set(
            "refresh",
            lua.create_function(move |_, _: ()| {
                let mut h = h.lock().unwrap();
                let errors = h.addons.refresh();
                for e in &errors {
                    h.console.push(format!("addons: {e}"));
                }
                Ok(h.addons.addons.len() as i64)
            })?,
        )?;
    }
    // list() -> [{id, name, version, enabled}].
    {
        let h = host.clone();
        addons.set(
            "list",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let mut ids: Vec<_> = h.addons.addons.keys().cloned().collect();
                ids.sort();
                let t = lua.create_table()?;
                for (i, id) in ids.iter().enumerate() {
                    if let Some(e) = h.addons.addons.get(id) {
                        let et = lua.create_table()?;
                        et.set("id", e.manifest.id.clone())?;
                        et.set("name", e.manifest.name.clone())?;
                        et.set("version", e.manifest.version.clone())?;
                        et.set("enabled", e.enabled)?;
                        t.set(i + 1, et)?;
                    }
                }
                Ok(t)
            })?,
        )?;
    }
    // info(id) -> detail | nil.
    {
        let h = host.clone();
        addons.set(
            "info",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                match h.addons.addons.get(&id) {
                    Some(e) => {
                        let t = lua.create_table()?;
                        t.set("id", e.manifest.id.clone())?;
                        t.set("name", e.manifest.name.clone())?;
                        t.set("version", e.manifest.version.clone())?;
                        t.set("description", e.manifest.description.clone())?;
                        t.set("enabled", e.enabled)?;
                        t.set("dir", e.dir.display().to_string())?;
                        let mut perms: Vec<_> = e
                            .manifest
                            .permissions
                            .iter()
                            .map(|c| c.name().to_string())
                            .collect();
                        perms.sort();
                        let pt = lua.create_table()?;
                        for (i, p) in perms.iter().enumerate() {
                            pt.set(i + 1, p.clone())?;
                        }
                        t.set("permissions", pt)?;
                        let dt = lua.create_table()?;
                        for (i, d) in e.manifest.deps.iter().enumerate() {
                            dt.set(i + 1, d.clone())?;
                        }
                        t.set("deps", dt)?;
                        Ok(mlua::Value::Table(t))
                    }
                    None => Ok(mlua::Value::Nil),
                }
            })?,
        )?;
    }
    // enable(id) -> true (deps checked, manifest permissions granted, source queued).
    {
        let h = host.clone();
        addons.set(
            "enable",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                let granted = h
                    .addons
                    .set_enabled(&id, true)
                    .map_err(mlua::Error::external)?;
                let source = h.addons.addon_source(&id).map_err(|e| {
                    let _ = h.addons.set_enabled(&id, false);
                    mlua::Error::external(e)
                })?;
                // Explicit enable = consent: union manifest permissions.
                for cap in h
                    .addons
                    .addons
                    .get(&id)
                    .map(|e| e.manifest.permissions.clone())
                    .unwrap_or_default()
                {
                    h.policy.grant(cap);
                }
                if !granted.is_empty() {
                    h.console.push(format!(
                        "addons: '{id}' granted {}",
                        granted.join(", ")
                    ));
                }
                h.pending_addons.push((id, source));
                Ok(true)
            })?,
        )?;
    }
    // disable(id) / uninstall(id) -> true.
    {
        let h = host.clone();
        addons.set(
            "disable",
            lua.create_function(move |_, id: String| {
                h.lock()
                    .unwrap()
                    .addons
                    .set_enabled(&id, false)
                    .map_err(mlua::Error::external)?;
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        addons.set(
            "reload",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                let entry = h
                    .addons
                    .addons
                    .get(&id)
                    .ok_or_else(|| mlua::Error::external("unknown addon"))?;
                if !entry.enabled {
                    return Err(mlua::Error::external("addon is not enabled"));
                }
                let source = h.addons.addon_source(&id).map_err(mlua::Error::external)?;
                if h.pending_addons.len() >= 16 {
                    return Err(mlua::Error::external("too many pending addon runs"));
                }
                h.pending_addons.push((id, source));
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        addons.set(
            "uninstall",
            lua.create_function(move |_, id: String| {
                h.lock()
                    .unwrap()
                    .addons
                    .uninstall(&id)
                    .map_err(mlua::Error::external)?;
                Ok(true)
            })?,
        )?;
    }
    vblua.set("addons", addons)?;
    Ok(())
}

/// `vblua.events` — Phase 28 subscriptions (runtime fires; one level only).
fn register_events_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let events = lua.create_table()?;

    // on(event, fn) -> true.
    {
        let h = host.clone();
        events.set(
            "on",
            lua.create_function(
                move |_, (event, func): (String, mlua::Function)| {
                    h.lock()
                        .unwrap()
                        .events
                        .subscribe(&event, func)
                        .map_err(mlua::Error::external)?;
                    Ok(true)
                },
            )?,
        )?;
    }
    // off(event) -> true if subscriptions existed.
    {
        let h = host.clone();
        events.set(
            "off",
            lua.create_function(move |_, event: String| {
                Ok(h.lock().unwrap().events.unsubscribe(&event))
            })?,
        )?;
    }
    // list() -> [known event names].
    events.set(
        "list",
        lua.create_function(move |lua, _: ()| {
            let t = lua.create_table()?;
            for (i, name) in super::events::KNOWN_EVENTS.iter().enumerate() {
                t.set(i + 1, *name)?;
            }
            Ok(t)
        })?,
    )?;
    vblua.set("events", events)?;

    // addons.reload(id) lives with the addon surface but is implemented here
    // to keep every glue block in one file.
    Ok(())
}

/// `vblua.debug` — Phase 26 tracing debugger (no locals by design:
/// `debug.*` stays stripped; the tracer records chunk+line only).
fn register_debug_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    let debug = lua.create_table()?;

    // trace(fn) -> runs fn with line tracing; error propagates. See trace_events().
    {
        let h = host.clone();
        debug.set(
            "trace",
            lua.create_function(move |_, func: mlua::Function| {
                let hook = h
                    .lock()
                    .unwrap()
                    .hook_state
                    .clone()
                    .ok_or_else(|| mlua::Error::external("debugger not armed"))?;
                hook.tracing.store(true, std::sync::atomic::Ordering::SeqCst);
                hook.trace.lock().unwrap().clear();
                let r: mlua::Result<mlua::MultiValue> = func.call(());
                hook.tracing.store(false, std::sync::atomic::Ordering::SeqCst);
                r
            })?,
        )?;
    }
    // trace_events() -> [{chunk, line}...] from the last trace() run.
    {
        let h = host.clone();
        debug.set(
            "trace_events",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                let hook = h.hook_state.clone().ok_or_else(|| {
                    mlua::Error::external("debugger not armed")
                })?;
                let trace = hook.trace.lock().unwrap();
                let t = lua.create_table()?;
                for (i, (chunk, line)) in trace.iter().enumerate() {
                    let et = lua.create_table()?;
                    et.set("chunk", chunk.clone())?;
                    et.set("line", *line)?;
                    t.set(i + 1, et)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // breakpoint(chunk, line) / clear_breakpoints() / breakpoints().
    {
        let h = host.clone();
        debug.set(
            "breakpoint",
            lua.create_function(move |_, (chunk, line): (String, i64)| {
                if chunk.is_empty() || chunk.len() > 128 || line < 1 || line > 100_000 {
                    return Err(mlua::Error::external("breakpoint needs chunk + line>=1"));
                }
                let h = h.lock().unwrap();
                let hook = h.hook_state.clone().ok_or_else(|| {
                    mlua::Error::external("debugger not armed")
                })?;
                hook.breakpoints.lock().unwrap().insert((chunk, line as i32));
                Ok(true)
            })?,
        )?;
    }
    {
        let h = host.clone();
        debug.set(
            "clear_breakpoints",
            lua.create_function(move |_, _: ()| {
                let h = h.lock().unwrap();
                if let Some(hook) = h.hook_state.clone() {
                    hook.breakpoints.lock().unwrap().clear();
                }
                Ok(true)
            })?,
        )?;
    }
    vblua.set("debug", debug)?;
    Ok(())
}

/// `vblua.image` + `vblua.kernel` — GPU kernel runtime, milestone 1.
/// Handles are opaque ids; images/kernels live in host maps (capped).
/// average_brightness uses GPU (luminance map + reduction, small readback)
/// when a device exists, else the bit-identical CPU oracle.
fn register_image_api(
    lua: &mlua::Lua,
    host: Arc<Mutex<ApiHost>>,
    vblua: &mlua::Table,
) -> mlua::Result<()> {
    use super::image::VbImage;
    let image = lua.create_table()?;

    // backend() -> "gpu" | "cpu" (transparency for Auto choice).
    image.set(
        "backend",
        lua.create_function(move |_, _: ()| {
            Ok(if super::gpu::gpu_available() { "gpu" } else { "cpu" })
        })?,
    )?;
    // solid(w, h[, {r,g,b,a}]) -> handle.
    {
        let h = host.clone();
        image.set(
            "solid",
            lua.create_function(
                move |_, (w, hh, color): (i64, i64, Option<mlua::Table>)| {
                    if w < 1 || hh < 1 || w > 4096 || hh > 4096 {
                        return Err(mlua::Error::external("size 1..4096"));
                    }
                    let c = match color {
                        Some(t) => [
                            t.get::<f64>(1).unwrap_or(0.0) as f32,
                            t.get::<f64>(2).unwrap_or(0.0) as f32,
                            t.get::<f64>(3).unwrap_or(0.0) as f32,
                            t.get::<f64>(4).unwrap_or(1.0) as f32,
                        ],
                        None => [0.0, 0.0, 0.0, 1.0],
                    };
                    let mut h = h.lock().unwrap();
                    if h.images.len() >= 64 {
                        return Err(mlua::Error::external("too many live images (max 64)"));
                    }
                    let img = VbImage::solid(w as u32, hh as u32, c)
                        .map_err(mlua::Error::external)?;
                    let id = format!("img{}", h.image_next);
                    h.image_next += 1;
                    h.images.insert(id.clone(), img);
                    Ok(id)
                },
            )?,
        )?;
    }
    // from_asset(ref) -> handle (decodes embedded project bytes).
    {
        let h = host.clone();
        image.set(
            "from_asset",
            lua.create_function(move |_, ref_id: String| {
                let mut h = h.lock().unwrap();
                let bytes = h
                    .asset_bytes
                    .get(&ref_id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::external(
                        "unknown asset ref (or bytes over budget — see assets.list)",
                    ))?;
                let dynimg = image::load_from_memory(&bytes)
                    .map_err(|_| mlua::Error::external("asset is not a decodable image"))?;
                let rgba = dynimg.to_rgba8();
                let (w, hh) = (rgba.width(), rgba.height());
                if h.images.len() >= 64 {
                    return Err(mlua::Error::external("too many live images (max 64)"));
                }
                let img = VbImage::new(w, hh, rgba.into_raw()).map_err(mlua::Error::external)?;
                let id = format!("img{}", h.image_next);
                h.image_next += 1;
                h.images.insert(id.clone(), img);
                Ok(id)
            })?,
        )?;
    }
    // size(id) -> {w, h}; drop(id) -> true.
    {
        let h = host.clone();
        image.set(
            "size",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                let t = lua.create_table()?;
                t.set("w", img.width as i64)?;
                t.set("h", img.height as i64)?;
                Ok(t)
            })?,
        )?;
    }
    {
        let h = host.clone();
        image.set(
            "drop",
            lua.create_function(move |_, id: String| {
                Ok(h.lock().unwrap().images.remove(&id).is_some())
            })?,
        )?;
    }
    // luminance(id) -> handle (grayscale image, feeds other ops).
    {
        let h = host.clone();
        image.set(
            "luminance",
            lua.create_function(move |_, id: String| {
                let mut h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?.clone();
                if h.images.len() >= 64 {
                    return Err(mlua::Error::external("too many live images (max 64)"));
                }
                let out = img.grayscale_cpu();
                let nid = format!("img{}", h.image_next);
                h.image_next += 1;
                h.images.insert(nid.clone(), out);
                Ok(nid)
            })?,
        )?;
    }
    // average_brightness(id) -> 0..1 (GPU reduction when available).
    {
        let h = host.clone();
        image.set(
            "average_brightness",
            lua.create_function(move |_, id: String| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                if super::gpu::gpu_available() {
                    if let Ok(v) = super::gpu::gpu_average_luminance(img.width, img.height, &img.rgba) {
                        return Ok(v as f64);
                    }
                    // GPU failed mid-run (device lost): fall through to CPU.
                }
                Ok(img.average_brightness_cpu() as f64)
            })?,
        )?;
    }
    // average_color(id) -> {r,g,b,a}.
    {
        let h = host.clone();
        image.set(
            "average_color",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                let c = img.average_color_cpu();
                let t = lua.create_table()?;
                t.set(1, c[0] as f64)?;
                t.set(2, c[1] as f64)?;
                t.set(3, c[2] as f64)?;
                t.set(4, c[3] as f64)?;
                Ok(t)
            })?,
        )?;
    }
    // histogram(id) -> 256 counts.
    {
        let h = host.clone();
        image.set(
            "histogram",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                let hist = img.histogram_cpu();
                let t = lua.create_table()?;
                for (i, c) in hist.iter().enumerate() {
                    t.set(i + 1, *c as i64)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // sample(id, u, v) -> {r,g,b,a} (normalized coords).
    {
        let h = host.clone();
        image.set(
            "sample",
            lua.create_function(move |lua, (id, u, v): (String, f64, f64)| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                let s = img.sample_cpu(u as f32, v as f32);
                let t = lua.create_table()?;
                for (i, c) in s.iter().enumerate() {
                    t.set(i + 1, *c as f64)?;
                }
                Ok(t)
            })?,
        )?;
    }
    // region_average(id, x, y, w, h) -> 0..1 (normalized rect).
    {
        let h = host.clone();
        image.set(
            "region_average",
            lua.create_function(move |_, (id, x, y, w, hh): (String, f64, f64, f64, f64)| {
                let h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?;
                Ok(img.region_average_cpu(x as f32, y as f32, w as f32, hh as f32) as f64)
            })?,
        )?;
    }
    // grayscale(id) / brightness(id, v) / contrast(id, v) -> handle.
    for op in ["grayscale", "brightness", "contrast"] {
        let h = host.clone();
        image.set(
            op,
            lua.create_function(move |_, (id, value): (String, Option<f64>)| {
                let mut h = h.lock().unwrap();
                let img = h.images.get(&id).ok_or_else(|| mlua::Error::external("unknown image"))?.clone();
                if h.images.len() >= 64 {
                    return Err(mlua::Error::external("too many live images (max 64)"));
                }
                let v = value.unwrap_or(1.0) as f32;
                if !v.is_finite() {
                    return Err(mlua::Error::external("value must be finite"));
                }
                let out = match op {
                    "grayscale" => img.grayscale_cpu(),
                    "brightness" => img.brightness_cpu(v),
                    _ => img.contrast_cpu(v),
                };
                let nid = format!("img{}", h.image_next);
                h.image_next += 1;
                h.images.insert(nid.clone(), out);
                Ok(nid)
            })?,
        )?;
    }
    // apply(frame, kernel[, {param = v}]) -> handle (GPU execution, milestone 2).
    {
        let h = host.clone();
        image.set(
            "apply",
            lua.create_function(
                move |_, (frame, kernel_id, overrides): (String, String, Option<mlua::Table>)| {
                    let mut ov = Vec::new();
                    if let Some(t) = overrides {
                        for pair in t.pairs::<String, mlua::Value>() {
                            let (k, v) = pair.map_err(|e| mlua::Error::external(e.to_string()))?;
                            let f = match v {
                                mlua::Value::Integer(i) => i as f64,
                                mlua::Value::Number(n) => n,
                                _ => {
                                    return Err(mlua::Error::external(format!(
                                        "parameter '{k}' must be a number"
                                    )));
                                }
                            };
                            if ov.len() >= 64 {
                                return Err(mlua::Error::external("at most 64 overrides"));
                            }
                            ov.push((k, f as f32));
                        }
                    }
                    let mut h = h.lock().unwrap();
                    let src = h.images.get(&frame).ok_or_else(|| mlua::Error::external("unknown image"))?.clone();
                    let kernel = h.kernels.get(&kernel_id).ok_or_else(|| mlua::Error::external("unknown kernel"))?.clone();
                    if h.images.len() >= 64 {
                        return Err(mlua::Error::external("too many live images (max 64)"));
                    }
                    let out = super::gpu::gpu_apply_kernel(
                        src.width,
                        src.height,
                        &src.rgba,
                        &kernel,
                        &ov,
                    )
                    .map_err(mlua::Error::external)?;
                    let img = super::image::VbImage::new(src.width, src.height, out)
                        .map_err(mlua::Error::external)?;
                    let nid = format!("img{}", h.image_next);
                    h.image_next += 1;
                    h.images.insert(nid.clone(), img);
                    Ok(nid)
                },
            )?,
        )?;
    }
    vblua.set("image", image)?;

    // ---- kernel registry (define + validate; execution via image.apply) ----
    let kernel = lua.create_table()?;
    {
        let h = host.clone();
        kernel.set(
            "create",
            lua.create_function(move |_, spec: mlua::Table| {
                let name: String = spec.get("name").map_err(|_| mlua::Error::external("kernel needs name"))?;
                let language: String = spec.get("language").unwrap_or_else(|_| "wgsl".to_string());
                let source: String = spec.get("source").map_err(|_| mlua::Error::external("kernel needs source"))?;
                let mut params = Vec::new();
                if let Ok(pt) = spec.get::<mlua::Table>("parameters") {
                    for pair in pt.pairs::<String, mlua::Value>() {
                        let (k, v) = pair.map_err(|e| mlua::Error::external(e.to_string()))?;
                        let f = match v {
                            mlua::Value::Integer(i) => i as f64,
                            mlua::Value::Number(n) => n,
                            _ => return Err(mlua::Error::external(format!("parameter '{k}' must be a number"))),
                        };
                        params.push((k, f as f32));
                    }
                    if params.len() > 32 {
                        return Err(mlua::Error::external("at most 32 kernel parameters"));
                    }
                }
                let k = super::kernel::VbKernel::create(name, &language, source, params)
                    .map_err(mlua::Error::external)?;
                let mut h = h.lock().unwrap();
                if h.kernels.len() >= 32 {
                    return Err(mlua::Error::external("too many live kernels (max 32)"));
                }
                let id = format!("kern{}", h.kernel_next);
                h.kernel_next += 1;
                h.kernels.insert(id.clone(), k);
                Ok(id)
            })?,
        )?;
    }
    {
        let h = host.clone();
        kernel.set(
            "info",
            lua.create_function(move |lua, id: String| {
                let h = h.lock().unwrap();
                let k = h.kernels.get(&id).ok_or_else(|| mlua::Error::external("unknown kernel"))?;
                let t = lua.create_table()?;
                t.set("name", k.name.clone())?;
                t.set("language", "wgsl")?;
                t.set("format", k.format)?;
                t.set("cache_key", format!("{:016x}", k.cache_key))?;
                let pt = lua.create_table()?;
                for (i, (name, v)) in k.params.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("name", name.clone())?;
                    row.set("default", *v as f64)?;
                    pt.set(i + 1, row)?;
                }
                t.set("parameters", pt)?;
                Ok(t)
            })?,
        )?;
    }
    {
        let h = host.clone();
        kernel.set(
            "drop",
            lua.create_function(move |_, id: String| {
                Ok(h.lock().unwrap().kernels.remove(&id).is_some())
            })?,
        )?;
    }
    vblua.set("kernel", kernel)?;
    Ok(())
}
