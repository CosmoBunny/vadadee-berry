//! E2EE payloads exchanged in the collaboration room.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollabMessage {
    Hello {
        user_id: String,
        username: String,
        color_rgb: [u8; 3],
    },
    Chat {
        user_id: String,
        username: String,
        text: String,
    },
    Cursor {
        user_id: String,
        username: String,
        doc_x: f64,
        doc_y: f64,
        tool: Option<String>,
        bubble: Option<String>,
        color_rgb: [u8; 3],
    },
    CanvasProject {
        user_id: String,
        project_json: String,
    },
    /// Layer / Objects tab + active layer index (lightweight UI sync).
    UiState {
        user_id: String,
        action_tab: Option<String>,
        active_layer_index: usize,
    },
    Ping {
        user_id: String,
        seq: u32,
    },
    Pong {
        user_id: String,
        seq: u32,
    },
    RoomClosed {
        reason: String,
    },
    Leave {
        user_id: String,
        username: String,
    },
}

#[derive(Clone, Debug)]
pub struct ChatLine {
    pub username: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct RemotePeer {
    pub user_id: String,
    pub username: String,
    pub color_rgb: [u8; 3],
    pub cursor_doc: Option<(f64, f64)>,
    pub tool_label: Option<String>,
    pub cursor_bubble: Option<String>,
    /// Milliseconds since last cursor update from this peer (freshness).
    pub idle_ms: Option<u32>,
}