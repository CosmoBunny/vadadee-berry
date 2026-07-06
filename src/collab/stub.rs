//! Android stub (no WebSocket thread).

use serde::{Deserialize, Serialize};

use crate::collab::protocol::{ChatLine, RemotePeer};

#[derive(Clone, Debug)]
pub struct CollabUiStateApply {
    pub action_tab: Option<String>,
    pub active_layer_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CollabRole {
    #[default]
    Client,
    Server,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollabConfig {
    pub role: CollabRole,
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
        format!("ws://{}:{}/ws/{}", self.host, self.port, self.room_id)
    }

    pub fn server_bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollabStatus {
    Disconnected,
    Connecting,
    Hosting(String),
    Connected,
    Error(String),
}

pub struct CollabSession {
    pub ui_config: CollabConfig,
    pub local_color_rgb: [u8; 3],
    status: CollabStatus,
    chat_log: Vec<ChatLine>,
}

impl CollabSession {
    pub fn new() -> Self {
        Self {
            ui_config: CollabConfig::default(),
            local_color_rgb: [35, 95, 210],
            status: CollabStatus::Disconnected,
            chat_log: Vec::new(),
        }
    }

    pub fn is_connected(&self) -> bool {
        false
    }

    pub fn status(&self) -> &CollabStatus {
        &self.status
    }

    pub fn poll(&mut self) {}

    pub fn tick_network(&mut self, _dt: f32) {}

    pub fn decrypt_warning_count(&self) -> u32 {
        0
    }

    pub fn connection_latency_ms(&self) -> Option<u32> {
        None
    }

    pub fn start(&mut self) {
        self.status = CollabStatus::Error("Collaboration is desktop-only".into());
    }

    pub fn connect(&mut self) {
        self.start();
    }

    pub fn disconnect(&mut self) {
        self.status = CollabStatus::Disconnected;
    }

    pub fn send_chat(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.chat_log.push(ChatLine {
            username: self.ui_config.username.clone(),
            text,
        });
    }

    pub fn send_cursor(
        &mut self,
        _doc_x: f64,
        _doc_y: f64,
        _tool: Option<String>,
        _bubble: Option<String>,
    ) {
    }

    pub fn send_ui_state(&mut self, _action_tab: Option<&str>, _active_layer_index: usize) {}

    pub fn canvas_outbound_enabled(&self) -> bool {
        false
    }

    pub fn enable_canvas_outbound(&mut self) {}

    pub fn take_canvas_push_requested(&mut self) -> bool {
        false
    }

    pub fn set_last_sent_canvas_hash(&mut self, _hash: u64) {}

    pub fn send_canvas_if_changed(&mut self, _project_json: &str, _force: bool) {}

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
        Vec::new()
    }

    pub fn take_pending_canvas_json(&mut self) -> Option<String> {
        None
    }

    pub fn take_pending_ui_state(&mut self) -> Option<CollabUiStateApply> {
        None
    }

    pub fn take_pending_chat_toasts(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }
}