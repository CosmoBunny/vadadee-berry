//! Desktop: E2EE WebSocket collaboration (tokio).

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::collab::protocol::{ChatLine, CollabMessage, RemotePeer};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CollabRole {
    #[default]
    Client,
    Server,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollabConfig {
    pub role: CollabRole,
    /// Client: WebSocket host (IP or hostname). Server: bind address (usually 127.0.0.1 or 0.0.0.0).
    pub host: String,
    pub port: u16,
    pub room_id: String,
    pub secret_key: String,
    pub username: String,
    pub live_canvas_sync: bool,
}

impl Default for CollabConfig {
    fn default() -> Self {
        Self {
            role: CollabRole::Client,
            host: "127.0.0.1".into(),
            port: 8080,
            room_id: "default".into(),
            secret_key: String::new(),
            username: "Artist".into(),
            live_canvas_sync: true,
        }
    }
}

impl CollabConfig {
    pub fn client_ws_url(&self) -> String {
        let host = self.host.trim();
        let room = self.room_id.trim();
        let room = if room.is_empty() { "default" } else { room };
        format!("ws://{host}:{}/ws/{room}", self.port)
    }

    pub fn server_bind_addr(&self) -> String {
        let host = self.host.trim();
        let host = if host.is_empty() { "127.0.0.1" } else { host };
        format!("{host}:{}", self.port)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollabStatus {
    Disconnected,
    Connecting,
    /// Relay running (Server role).
    Hosting(String),
    Connected,
    Error(String),
}

#[derive(Clone, Debug)]
enum CollabEvent {
    Status(CollabStatus),
    Message(CollabMessage),
    DecryptWarning,
}

enum NetCommand {
    StartServer { bind: String },
    StopServer,
    Connect(CollabConfig),
    Disconnect,
    SendWire(String),
}

#[derive(Clone, Debug)]
pub struct CollabUiStateApply {
    pub action_tab: Option<String>,
    pub active_layer_index: usize,
}

#[derive(Serialize, Deserialize)]
struct WirePacket {
    nonce_b64: String,
    ciphertext_b64: String,
}

pub struct CollabSession {
    pub ui_config: CollabConfig,
    pub user_id: String,
    pub local_color_rgb: [u8; 3],
    status: CollabStatus,
    pub chat_log: Vec<ChatLine>,
    pub peers: HashMap<String, RemotePeer>,
    pub pending_canvas_json: Option<String>,
    pub pending_ui_state: Option<CollabUiStateApply>,
    pub pending_chat_toasts: Vec<(String, String)>,
    event_rx: Receiver<CollabEvent>,
    cmd_tx: Sender<NetCommand>,
    decrypt_failures: u32,
    last_project_hash: u64,
    /// Client: false until first remote canvas is received (prevents empty doc wiping host).
    canvas_outbound_enabled: bool,
    canvas_push_requested: bool,
    hello_sent: bool,
    ping_accum: f32,
    pending_ping: Option<(u32, std::time::Instant)>,
    pub connection_latency_ms: Option<u32>,
    last_peer_activity: HashMap<String, std::time::Instant>,
}

impl CollabSession {
    pub fn new() -> Self {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        spawn_collab_thread(event_tx, cmd_rx);
        let user_id = uuid::Uuid::new_v4().to_string();
        let local_color_rgb = color_from_user_id(&user_id);
        Self {
            ui_config: CollabConfig::default(),
            user_id,
            local_color_rgb,
            status: CollabStatus::Disconnected,
            chat_log: Vec::new(),
            peers: HashMap::new(),
            pending_canvas_json: None,
            pending_ui_state: None,
            pending_chat_toasts: Vec::new(),
            event_rx,
            cmd_tx,
            decrypt_failures: 0,
            last_project_hash: 0,
            canvas_outbound_enabled: false,
            canvas_push_requested: false,
            hello_sent: false,
            ping_accum: 0.0,
            pending_ping: None,
            connection_latency_ms: None,
            last_peer_activity: HashMap::new(),
        }
    }

    pub fn tick_network(&mut self, dt: f32) {
        if !self.is_connected() {
            self.ping_accum = 0.0;
            return;
        }
        self.ping_accum += dt;
        if let Some((_, sent)) = self.pending_ping {
            if sent.elapsed().as_secs() > 4 {
                self.pending_ping = None;
            }
        }
        if self.ping_accum >= 2.0 {
            self.ping_accum = 0.0;
            self.send_ping();
        }
        let now = std::time::Instant::now();
        for peer in self.peers.values_mut() {
            peer.idle_ms = self
                .last_peer_activity
                .get(&peer.user_id)
                .map(|t| now.saturating_duration_since(*t).as_millis() as u32);
        }
        // Prune peers that left (no activity for long time, e.g. crashed without sending Leave)
        self.peers.retain(|uid, p| {
            if let Some(ms) = p.idle_ms {
                if ms > 12000 {
                    self.last_peer_activity.remove(uid);
                    return false;
                }
            }
            true
        });
    }

    fn send_ping(&mut self) {
        let seq = self
            .pending_ping
            .as_ref()
            .map(|(s, _)| s.wrapping_add(1))
            .unwrap_or(1);
        self.pending_ping = Some((seq, std::time::Instant::now()));
        self.send_message(CollabMessage::Ping {
            user_id: self.user_id.clone(),
            seq,
        });
    }

    pub fn connection_latency_ms(&self) -> Option<u32> {
        self.connection_latency_ms
    }

    pub fn send_ui_state(&mut self, action_tab: Option<&str>, active_layer_index: usize) {
        self.send_message(CollabMessage::UiState {
            user_id: self.user_id.clone(),
            action_tab: action_tab.map(str::to_string),
            active_layer_index,
        });
    }

    pub fn is_connected(&self) -> bool {
        matches!(
            self.status,
            CollabStatus::Connected | CollabStatus::Hosting(_)
        )
    }

    pub fn status(&self) -> &CollabStatus {
        &self.status
    }

    pub fn poll(&mut self) {
        loop {
            match self.event_rx.try_recv() {
                Ok(CollabEvent::Status(s)) => {
                    let was = self.status.clone();
                    self.status = s;
                    if matches!(self.status, CollabStatus::Connected)
                        && !matches!(was, CollabStatus::Connected)
                    {
                        self.reset_decrypt_warnings();
                        if !self.hello_sent {
                            self.broadcast_hello();
                            self.hello_sent = true;
                        }
                        if self.ui_config.role == CollabRole::Server {
                            self.canvas_outbound_enabled = true;
                            self.canvas_push_requested = true;
                        }
                    }
                    if matches!(self.status, CollabStatus::Disconnected) {
                        self.hello_sent = false;
                        self.peers.clear();
                        self.last_peer_activity.clear();
                        self.pending_ping = None;
                        self.connection_latency_ms = None;
                        self.last_project_hash = 0;
                        self.canvas_outbound_enabled = false;
                        self.canvas_push_requested = false;
                    }
                }
                Ok(CollabEvent::Message(msg)) => self.apply_remote_message(msg),
                Ok(CollabEvent::DecryptWarning) => {
                    self.decrypt_failures = self.decrypt_failures.saturating_add(1);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.status = CollabStatus::Disconnected;
                    break;
                }
            }
        }
    }

    pub fn decrypt_warning_count(&self) -> u32 {
        self.decrypt_failures
    }

    pub fn reset_decrypt_warnings(&mut self) {
        self.decrypt_failures = 0;
    }

    /// Start Server (host relay) or Client (connect to relay), depending on `ui_config.role`.
    pub fn start(&mut self) {
        if self.ui_config.secret_key.trim().is_empty() {
            self.status = CollabStatus::Error("Encryption secret is required".into());
            return;
        }
        self.status = CollabStatus::Connecting;
        self.canvas_outbound_enabled = self.ui_config.role == CollabRole::Server;
        self.canvas_push_requested = self.ui_config.role == CollabRole::Server;
        self.last_project_hash = 0;
        if self.ui_config.role == CollabRole::Server {
            let bind = self.ui_config.server_bind_addr();
            let _ = self.cmd_tx.send(NetCommand::StartServer { bind: bind.clone() });
            let _ = self.cmd_tx.send(NetCommand::Connect(self.ui_config.clone()));
        } else {
            let _ = self.cmd_tx.send(NetCommand::Connect(self.ui_config.clone()));
        }
    }

    pub fn connect(&mut self) {
        self.start();
    }

    pub fn disconnect(&mut self) {
        if self.is_connected() {
            if self.ui_config.role == CollabRole::Server {
                self.send_message(CollabMessage::RoomClosed {
                    reason: "Host ended the session".into(),
                });
            } else {
                self.send_message(CollabMessage::Leave {
                    user_id: self.user_id.clone(),
                    username: self.ui_config.username.clone(),
                });
            }
        }
        let _ = self.cmd_tx.send(NetCommand::Disconnect);
        self.status = CollabStatus::Disconnected;
    }

    pub fn take_pending_chat_toasts(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.pending_chat_toasts)
    }

    pub fn send_chat(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let username = self.ui_config.username.clone();
        self.chat_log.push(ChatLine {
            username: username.clone(),
            text: text.clone(),
        });
        self.send_message(CollabMessage::Chat {
            user_id: self.user_id.clone(),
            username,
            text,
        });
    }

    pub fn send_cursor(
        &mut self,
        doc_x: f64,
        doc_y: f64,
        tool: Option<String>,
        bubble: Option<String>,
    ) {
        self.send_message(CollabMessage::Cursor {
            user_id: self.user_id.clone(),
            username: self.ui_config.username.clone(),
            doc_x,
            doc_y,
            tool,
            bubble,
            color_rgb: self.local_color_rgb,
        });
    }

    pub fn canvas_outbound_enabled(&self) -> bool {
        self.canvas_outbound_enabled
    }

    pub fn enable_canvas_outbound(&mut self) {
        self.canvas_outbound_enabled = true;
    }

    pub fn take_canvas_push_requested(&mut self) -> bool {
        let v = self.canvas_push_requested;
        self.canvas_push_requested = false;
        v
    }

    pub fn set_last_sent_canvas_hash(&mut self, hash: u64) {
        self.last_project_hash = hash;
    }

    pub fn send_canvas_if_changed(&mut self, project_json: &str, force: bool) {
        if !self.ui_config.live_canvas_sync || !self.is_connected() || !self.canvas_outbound_enabled {
            return;
        }
        let hash = fx_hash_str(project_json);
        if !force && hash == self.last_project_hash {
            return;
        }
        self.last_project_hash = hash;
        self.send_message(CollabMessage::CanvasProject {
            user_id: self.user_id.clone(),
            project_json: project_json.to_string(),
        });
    }

    pub fn broadcast_hello(&mut self) {
        self.send_message(CollabMessage::Hello {
            user_id: self.user_id.clone(),
            username: self.ui_config.username.clone(),
            color_rgb: self.local_color_rgb,
        });
    }

    fn send_message(&mut self, msg: CollabMessage) {
        match encrypt_message(&self.ui_config.secret_key, &msg) {
            Ok(wire) => {
                let _ = self.cmd_tx.send(NetCommand::SendWire(wire));
            }
            Err(e) => self.status = CollabStatus::Error(e),
        }
    }

    pub fn chat_log(&self) -> &[ChatLine] {
        &self.chat_log
    }

    pub fn chat_log_plain(&self) -> String {
        self.chat_log
            .iter()
            .map(|l| format!("[{}]: {}", l.username, l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn peers_sorted(&self) -> Vec<RemotePeer> {
        let mut v: Vec<_> = self.peers.values().cloned().collect();
        v.sort_by(|a, b| a.username.cmp(&b.username));
        v
    }

    pub fn take_pending_canvas_json(&mut self) -> Option<String> {
        self.pending_canvas_json.take()
    }

    pub fn take_pending_ui_state(&mut self) -> Option<CollabUiStateApply> {
        self.pending_ui_state.take()
    }

    fn apply_remote_message(&mut self, msg: CollabMessage) {
        match msg {
            CollabMessage::Hello {
                user_id,
                username,
                color_rgb,
            } => {
                if user_id == self.user_id {
                    return;
                }
                self.peers.insert(
                    user_id.clone(),
                    RemotePeer {
                        user_id,
                        username,
                        color_rgb,
                        cursor_doc: None,
                        tool_label: None,
                        cursor_bubble: None,
                        idle_ms: None,
                    },
                );
                if self.ui_config.role == CollabRole::Server {
                    self.canvas_push_requested = true;
                }
            }
            CollabMessage::Chat {
                user_id,
                username,
                text,
            } => {
                if user_id == self.user_id {
                    return;
                }
                self.chat_log.push(ChatLine {
                    username: username.clone(),
                    text: text.clone(),
                });
                self.pending_chat_toasts.push((username, text));
            }
            CollabMessage::RoomClosed { reason } => {
                self.status = CollabStatus::Error(format!("Room closed: {reason}"));
                let _ = self.cmd_tx.send(NetCommand::Disconnect);
            }
            CollabMessage::Leave { user_id, .. } => {
                if user_id != self.user_id {
                    self.peers.remove(&user_id);
                    self.last_peer_activity.remove(&user_id);
                }
            }
            CollabMessage::Cursor {
                user_id,
                username,
                doc_x,
                doc_y,
                tool,
                bubble,
                color_rgb,
            } => {
                if user_id == self.user_id {
                    return;
                }
                self.last_peer_activity
                    .insert(user_id.clone(), std::time::Instant::now());
                let peer = self.peers.entry(user_id.clone()).or_insert(RemotePeer {
                    user_id: user_id.clone(),
                    username: username.clone(),
                    color_rgb,
                    cursor_doc: None,
                    tool_label: None,
                    cursor_bubble: None,
                    idle_ms: None,
                });
                peer.username = username;
                peer.color_rgb = color_rgb;
                peer.cursor_doc = Some((doc_x, doc_y));
                peer.tool_label = tool;
                peer.cursor_bubble = bubble;
            }
            CollabMessage::UiState {
                user_id,
                action_tab,
                active_layer_index,
            } => {
                if user_id == self.user_id {
                    return;
                }
                self.pending_ui_state = Some(CollabUiStateApply {
                    action_tab,
                    active_layer_index,
                });
            }
            CollabMessage::Ping { user_id, seq } => {
                if user_id == self.user_id {
                    return;
                }
                self.send_message(CollabMessage::Pong {
                    user_id: self.user_id.clone(),
                    seq,
                });
            }
            CollabMessage::Pong { user_id: _, seq } => {
                if let Some((pending_seq, sent)) = self.pending_ping {
                    if pending_seq == seq {
                        let ms = sent.elapsed().as_millis().min(999) as u32;
                        self.connection_latency_ms = Some(ms);
                        self.pending_ping = None;
                    }
                }
            }
            CollabMessage::CanvasProject {
                user_id,
                project_json,
            } => {
                if user_id == self.user_id || !self.ui_config.live_canvas_sync {
                    return;
                }
                self.canvas_outbound_enabled = true;
                self.pending_canvas_json = Some(project_json);
            }
        }
    }
}

/// Distinct, high-contrast colors on white page backgrounds.
fn color_from_user_id(id: &str) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [210, 45, 55],
        [35, 95, 210],
        [15, 140, 85],
        [130, 55, 200],
        [210, 95, 20],
        [20, 150, 165],
        [175, 40, 120],
        [90, 70, 30],
    ];
    let h = fx_hash_str(id);
    PALETTE[(h as usize) % PALETTE.len()]
}

fn fx_hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    s.hash(&mut h);
    h.finish()
}

fn spawn_collab_thread(event_tx: Sender<CollabEvent>, cmd_rx: Receiver<NetCommand>) {
    std::thread::Builder::new()
        .name("vadadee-collab-ws".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(1)
                .build()
            else {
                return;
            };
            rt.block_on(collab_network_loop(event_tx, cmd_rx));
        })
        .ok();
}

async fn collab_network_loop(event_tx: Sender<CollabEvent>, cmd_rx: Receiver<NetCommand>) {
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use tokio::sync::{mpsc as tokio_mpsc, Mutex};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let outbound_to_ws: Arc<Mutex<Option<tokio_mpsc::UnboundedSender<String>>>> =
        Arc::new(Mutex::new(None));
    let mut ws_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut relay_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut relay_stop: Option<tokio::sync::watch::Sender<bool>> = None;

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                NetCommand::StopServer => {
                    if let Some(tx) = relay_stop.take() {
                        let _ = tx.send(true);
                    }
                    if let Some(h) = relay_handle.take() {
                        h.abort();
                    }
                }
                NetCommand::StartServer { bind } => {
                    if let Some(tx) = relay_stop.take() {
                        let _ = tx.send(true);
                    }
                    if let Some(h) = relay_handle.take() {
                        h.abort();
                    }
                    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                    relay_stop = Some(stop_tx);
                    let bind2 = bind.clone();
                    let event_tx2 = event_tx.clone();
                    relay_handle = Some(tokio::spawn(async move {
                        let url = format!("ws://{bind2}/ws/{{room}}");
                        let _ = event_tx2.send(CollabEvent::Status(CollabStatus::Hosting(url)));
                        let _ = crate::collab::relay::run_relay_until_stopped(&bind2, stop_rx).await;
                    }));
                }
                NetCommand::Disconnect => {
                    if let Some(tx) = relay_stop.take() {
                        let _ = tx.send(true);
                    }
                    if let Some(h) = relay_handle.take() {
                        h.abort();
                    }
                    if let Some(h) = ws_handle.take() {
                        h.abort();
                    }
                    *outbound_to_ws.lock().await = None;
                    let _ = event_tx.send(CollabEvent::Status(CollabStatus::Disconnected));
                }
                NetCommand::Connect(cfg) => {
                    if let Some(h) = ws_handle.take() {
                        h.abort();
                    }
                    let url = cfg.client_ws_url();
                    let is_client = cfg.role == CollabRole::Client;
                    let event_tx2 = event_tx.clone();
                    let secret = cfg.secret_key.clone();
                    let (wire_tx, mut wire_rx) = tokio_mpsc::unbounded_channel::<String>();
                    *outbound_to_ws.lock().await = Some(wire_tx);
                    ws_handle = Some(tokio::spawn(async move {
                        let _ = event_tx2.send(CollabEvent::Status(CollabStatus::Connecting));
                        match connect_async(&url).await {
                            Ok((ws, _)) => {
                                let _ = event_tx2.send(CollabEvent::Status(CollabStatus::Connected));
                                let (mut write, mut read) = ws.split();
                                loop {
                                    tokio::select! {
                                        incoming = read.next() => {
                                            match incoming {
                                                Some(Ok(Message::Binary(bin))) => {
                                                    if let Ok(s) = std::str::from_utf8(&bin) {
                                                        handle_incoming(&secret, s, &event_tx2);
                                                    }
                                                }
                                                Some(Ok(Message::Text(t))) => {
                                                    handle_incoming(&secret, &t, &event_tx2);
                                                }
                                                Some(Ok(Message::Close(_))) | None => break,
                                                Some(Err(e)) => {
                                                    let _ = event_tx2.send(CollabEvent::Status(CollabStatus::Error(e.to_string())));
                                                    break;
                                                }
                                                _ => {}
                                            }
                                        }
                                        wire = wire_rx.recv() => {
                                            match wire {
                                                Some(w) => {
                                                    if write.send(Message::Binary(w.into_bytes())).await.is_err() {
                                                        break;
                                                    }
                                                }
                                                None => break,
                                            }
                                        }
                                    }
                                }
                                let status = if is_client {
                                    CollabStatus::Error(
                                        "Host disconnected — session ended".into(),
                                    )
                                } else {
                                    CollabStatus::Disconnected
                                };
                                let _ = event_tx2.send(CollabEvent::Status(status));
                            }
                            Err(e) => {
                                let _ = event_tx2.send(CollabEvent::Status(CollabStatus::Error(format!(
                                    "WebSocket handshake failed: {e}"
                                ))));
                            }
                        }
                    }));
                }
                NetCommand::SendWire(w) => {
                    if let Some(tx) = outbound_to_ws.lock().await.as_ref() {
                        let _ = tx.send(w);
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(8)).await;
    }
}

fn handle_incoming(secret: &str, wire: &str, event_tx: &Sender<CollabEvent>) {
    match decrypt_message(secret, wire) {
        Ok(msg) => {
            let _ = event_tx.send(CollabEvent::Message(msg));
        }
        Err(_) => {
            let _ = event_tx.send(CollabEvent::DecryptWarning);
        }
    }
}

fn derive_key(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

fn encrypt_message(secret: &str, msg: &CollabMessage) -> Result<String, String> {
    let key = derive_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plain = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let ciphertext = cipher
        .encrypt(nonce, plain.as_ref())
        .map_err(|e| e.to_string())?;
    let packet = WirePacket {
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    };
    serde_json::to_string(&packet).map_err(|e| e.to_string())
}

fn decrypt_message(secret: &str, wire: &str) -> Result<CollabMessage, String> {
    let packet: WirePacket = serde_json::from_str(wire).map_err(|e| e.to_string())?;
    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(packet.nonce_b64)
        .map_err(|e| e.to_string())?;
    if nonce_bytes.len() != 12 {
        return Err("invalid nonce length".into());
    }
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(packet.ciphertext_b64)
        .map_err(|e| e.to_string())?;
    let key = derive_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "decryption failed".to_string())?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}