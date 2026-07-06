//! MCP-style JSON-RPC over TCP so external AI clients can query/control the editor.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use serde::Deserialize;
use serde_json::{json, Value};

pub mod drawing;
pub mod path_parse;

/// Port for the in-app MCP bridge (override with `VADADEE_MCP_PORT`).
pub const DEFAULT_MCP_PORT: u16 = 17345;

#[derive(Clone, Debug)]
pub struct McpAppSnapshot {
    pub title: String,
    pub project_path: Option<String>,
    pub status_message: String,
    pub collab_text: String,
    pub anim_frame: usize,
    pub anim_playing: bool,
    pub ui_fps: f32,
}

#[derive(Debug)]
pub enum McpHostRequest {
    Snapshot,
    SaveProject { path: Option<String> },
    SetTitle(String),
    GetCollabText,
    SetCollabText(String),
    ProjectJson,
    ListObjects,
    ListAllObjects,
    GetObject { id: String },
    /// create_*, set_object_*, add_layer (see `drawing` module tool list).
    DrawingTool {
        name: String,
        args: Value,
    },
    UpdateObject {
        id: String,
        patch: serde_json::Value,
    },
    CaptureCanvasRaster {
        resolution_percent: f32,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        save_path: Option<String>,
    },
    DeleteObject { id: String },
    UiHealth,
}

#[derive(Debug)]
pub enum McpHostResponse {
    Snapshot(McpAppSnapshot),
    Ok { message: String },
    Err { message: String },
    Text(String),
    RasterPreview {
        meta_json: String,
        png_base64: String,
    },
}

struct McpShared {
    request_tx: Sender<(McpHostRequest, Sender<McpHostResponse>)>,
}

pub struct McpBridge {
    _server_thread: JoinHandle<()>,
    request_rx: Receiver<(McpHostRequest, Sender<McpHostResponse>)>,
}

impl McpBridge {
    pub fn try_start() -> Option<Self> {
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let shared = Arc::new(McpShared { request_tx });
        let port = std::env::var("VADADEE_MCP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MCP_PORT);
        let addr = format!("127.0.0.1:{port}");
        if std::net::TcpListener::bind(&addr).is_err() {
            log::warn!(
                "MCP bridge: port {port} busy — skipping TCP server (close other instance or set VADADEE_MCP_PORT)"
            );
            return None;
        }
        let shared2 = shared.clone();
        let server_thread = std::thread::Builder::new()
            .name("vadadee-mcp-tcp".into())
            .spawn(move || mcp_tcp_server(port, shared2))
            .ok()?;
        Some(Self {
            _server_thread: server_thread,
            request_rx,
        })
    }

    pub fn start() -> Self {
        Self::try_start().unwrap_or_else(|| {
            let (request_tx, request_rx) = std::sync::mpsc::channel();
            Self {
                _server_thread: std::thread::Builder::new()
                    .name("vadadee-mcp-stub".into())
                    .spawn(|| {})
                    .expect("stub thread"),
                request_rx,
            }
        })
    }

    pub fn drain_pending(
        &mut self,
    ) -> Vec<(
        McpHostRequest,
        std::sync::mpsc::Sender<McpHostResponse>,
    )> {
        let mut pending = Vec::new();
        loop {
            match self.request_rx.try_recv() {
                Ok(pair) => pending.push(pair),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        pending
    }
}

fn mcp_tcp_server(port: u16, shared: Arc<McpShared>) {
    let addr = format!("127.0.0.1:{port}");
    let Ok(listener) = TcpListener::bind(&addr) else {
        log::error!("MCP bridge: could not bind {addr}");
        return;
    };
    log::info!("MCP bridge listening on {addr} (line-delimited JSON-RPC)");
    for stream in listener.incoming().flatten() {
        let shared2 = shared.clone();
        std::thread::Builder::new()
            .name("vadadee-mcp-client".into())
            .spawn(move || handle_mcp_client(stream, shared2))
            .ok();
    }
}

fn handle_mcp_client(stream: TcpStream, shared: Arc<McpShared>) {
    let peer = stream.peer_addr().ok();
    let Ok(stream_read) = stream.try_clone() else {
        return;
    };
    let reader = BufReader::new(stream_read);
    let mut writer = stream;
    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => handle_jsonrpc(&shared, req),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": format!("parse error: {e}") },
                "id": null
            })),
        };
        let Some(response) = response else {
            continue;
        };
        if writeln!(writer, "{}", response.to_string()).is_err() {
            break;
        }
        let _ = writer.flush();
    }
    log::debug!("MCP client disconnected {:?}", peer);
}

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

fn empty_tool_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

fn mcp_tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

pub fn mcp_initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "vadadee-berry", "version": env!("CARGO_PKG_VERSION") }
    })
}

pub fn mcp_tools_list_result() -> Value {
    let mut tools: Vec<Value> = vec![
        mcp_tool("get_project_snapshot", "Title, path, status, timeline, collab text", empty_tool_schema()),
        mcp_tool("get_project_json", "Full project file as JSON (for AI editing)", empty_tool_schema()),
        mcp_tool("save_project", "Save current project (optional path param)", json!({
            "type": "object",
            "properties": { "path": { "type": "string" } }
        })),
        mcp_tool("set_document_title", "Set document title string", json!({
            "type": "object",
            "properties": { "title": { "type": "string" } },
            "required": ["title"]
        })),
        mcp_tool("get_collab_text", "Live collaboration chat log ([user]: message lines)", empty_tool_schema()),
        mcp_tool("set_collab_text", "Send a chat message to the collaboration room", json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        })),
        mcp_tool("list_objects", "List objects on the active layer (id, name, kind, bounds)", empty_tool_schema()),
        mcp_tool("get_object", "Get one object as JSON by UUID", json!({
            "type": "object",
            "properties": { "id": { "type": "string" } },
            "required": ["id"]
        })),
        mcp_tool("update_object", "Legacy patch: name, style colors, transform, and geometry fields", json!({
            "type": "object",
            "properties": { "id": { "type": "string" }, "patch": { "type": "object" } },
            "required": ["id", "patch"]
        })),
        mcp_tool("delete_object", "Delete object by UUID", json!({
            "type": "object",
            "properties": { "id": { "type": "string" } },
            "required": ["id"]
        })),
        mcp_tool(
            "capture_canvas_raster",
            "Raster preview of canvas region for AI vision (PNG base64). Does not flatten or lock objects.",
            json!({
                "type": "object",
                "properties": {
                    "resolution_percent": {
                        "type": "number",
                        "description": "Output scale 1-100 (100 = full doc px density in crop)"
                    },
                    "x": { "type": "number", "description": "Crop origin X (default 0)" },
                    "y": { "type": "number", "description": "Crop origin Y (default 0)" },
                    "w": { "type": "number", "description": "Crop width (default full page)" },
                    "h": { "type": "number", "description": "Crop height (default full page)" },
                    "save_path": { "type": "string", "description": "Optional path to write PNG file" }
                }
            }),
        ),
        mcp_tool(
            "list_all_objects",
            "All editable objects on every visible image layer with style/transform summary",
            empty_tool_schema(),
        ),
        mcp_tool("get_ui_health", "UI health metrics: current FPS (smoothed), object count, current frame. Use to diagnose lag with many objects (e.g. pixel art).", empty_tool_schema()),
        mcp_tool(
            "add_shading_layer",
            "Add a shading layer with WGSL source compiled at runtime on the GPU (no preset — pass wgsl string)",
            json!({
                "type": "object",
                "properties": {
                    "wgsl": {
                        "type": "string",
                        "description": "WGSL fragment module (@fragment fn main). Vertex shader auto-prepended if missing. Use @binding(0) uniform only for procedural; input_tex at 0 + sampler 1 + uniform 2 for compose."
                    },
                    "name": { "type": "string", "description": "Layer name (default Shading)" },
                    "pass_name": { "type": "string", "description": "Pass label in UI (default Shader)" },
                    "uniforms": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Uniform floats; runtime adds time to [0] and page aspect to [3]"
                    }
                },
                "required": ["wgsl"]
            }),
        ),
    ];
    tools.extend(drawing::drawing_tools());
    json!({ "tools": tools })
}

pub fn try_answer_stdio_line(line: &str) -> Option<Option<String>> {
    let req: JsonRpcRequest = serde_json::from_str(line).ok()?;
    if req.id.is_none() && is_mcp_notification(&req.method) {
        return Some(None);
    }
    let id = req.id.clone();
    let result = match req.method.as_str() {
        "ping" => json!({}),
        "initialize" => mcp_initialize_result(),
        "tools/list" => mcp_tools_list_result(),
        "tools/call" => return None,
        _ => {
            return Some(Some(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32601, "message": "method not found" },
                "id": id
            }).to_string()));
        }
    };
    Some(Some(json!({ "jsonrpc": "2.0", "result": result, "id": id }).to_string()))
}

fn handle_jsonrpc(shared: &McpShared, req: JsonRpcRequest) -> Option<Value> {
    if req.id.is_none() && is_mcp_notification(&req.method) {
        return None;
    }
    let id = req.id.clone();
    let result = match req.method.as_str() {
        "ping" => json!({}),
        "initialize" => mcp_initialize_result(),
        "tools/list" => mcp_tools_list_result(),
        "tools/call" => {
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            match name {
                "get_project_snapshot" => host_call(shared, McpHostRequest::Snapshot),
                "get_project_json" => host_call(shared, McpHostRequest::ProjectJson),
                "save_project" => {
                    let path = args.get("path").and_then(|v| v.as_str()).map(str::to_string);
                    host_call(shared, McpHostRequest::SaveProject { path })
                }
                "set_document_title" => {
                    let title = args
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    host_call(shared, McpHostRequest::SetTitle(title))
                }
                "get_collab_text" => host_call(shared, McpHostRequest::GetCollabText),
                "set_collab_text" => {
                    let text = args
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    host_call(shared, McpHostRequest::SetCollabText(text))
                }
                "list_objects" => host_call(shared, McpHostRequest::ListObjects),
                "list_all_objects" => host_call(shared, McpHostRequest::ListAllObjects),
                "capture_canvas_raster" => {
                    let resolution_percent = args
                        .get("resolution_percent")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(25.0) as f32;
                    let x = args.get("x").and_then(|v| v.as_f64());
                    let y = args.get("y").and_then(|v| v.as_f64());
                    let w = args.get("w").and_then(|v| v.as_f64());
                    let h = args.get("h").and_then(|v| v.as_f64());
                    let save_path = args
                        .get("save_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    host_call(
                        shared,
                        McpHostRequest::CaptureCanvasRaster {
                            resolution_percent,
                            x,
                            y,
                            w,
                            h,
                            save_path,
                        },
                    )
                }
                "get_object" => {
                    let id = args
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    host_call(shared, McpHostRequest::GetObject { id })
                }
                name if is_drawing_tool(name) => {
                    host_call(
                        shared,
                        McpHostRequest::DrawingTool {
                            name: name.to_string(),
                            args,
                        },
                    )
                }
                "update_object" => {
                    let id = args
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let patch = args.get("patch").cloned().unwrap_or(json!({}));
                    host_call(shared, McpHostRequest::UpdateObject { id, patch })
                }
                "delete_object" => {
                    let id = args
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    host_call(shared, McpHostRequest::DeleteObject { id })
                }
                "get_ui_health" => host_call(shared, McpHostRequest::UiHealth),
                _ => json!({ "isError": true, "content": [{ "type": "text", "text": "unknown tool" }] }),
            }
        }
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32601, "message": "method not found" },
                "id": id
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "result": result, "id": id }))
}

fn is_drawing_tool(name: &str) -> bool {
    matches!(
        name,
        "create_rectangle"
            | "create_rectangles"
            | "create_image"
            | "create_circle"
            | "create_ellipse"
            | "create_line"
            | "create_polygon"
            | "create_arc"
            | "create_text"
            | "set_object_style"
            | "set_objects_style"
            | "set_object_transform"
            | "set_object_geometry"
            | "add_layer"
            | "add_shading_layer"
            | "create_path"
            | "set_keyframe"
            | "remove_keyframe"
            | "get_keyframes"
            | "set_keyframe_interpolation"
            | "set_current_anim_frame"
            | "get_current_anim_frame"
            | "set_keyframes"
            | "clear_animation_track"
    )
}

fn is_mcp_notification(method: &str) -> bool {
    method.starts_with("notifications/") || method == "initialized" || method == "cancelled"
}

fn host_call(shared: &McpShared, req: McpHostRequest) -> Value {
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    if shared.request_tx.send((req, reply_tx)).is_err() {
        return json!({
            "isError": true,
            "content": [{ "type": "text", "text": "editor not running" }]
        });
    }
    match reply_rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(McpHostResponse::Snapshot(s)) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&json!({
                "title": s.title,
                "project_path": s.project_path,
                "status_message": s.status_message,
                "collab_text": s.collab_text,
                "anim_frame": s.anim_frame,
                "anim_playing": s.anim_playing,
                "ui_fps": s.ui_fps
            })).unwrap_or_default() }]
        }),
        Ok(McpHostResponse::Ok { message }) => json!({
            "content": [{ "type": "text", "text": message }]
        }),
        Ok(McpHostResponse::Err { message }) => json!({
            "isError": true,
            "content": [{ "type": "text", "text": message }]
        }),
        Ok(McpHostResponse::Text(t)) => json!({
            "content": [{ "type": "text", "text": t }]
        }),
        Ok(McpHostResponse::RasterPreview { meta_json, png_base64 }) => json!({
            "content": [
                { "type": "text", "text": meta_json },
                { "type": "image", "data": png_base64, "mimeType": "image/png" }
            ]
        }),
        Err(_) => json!({
            "isError": true,
            "content": [{ "type": "text", "text": "editor timeout — is the UI thread busy?" }]
        }),
    }
}