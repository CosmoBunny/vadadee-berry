//! Nerd Font icon glyphs (DaddyTimeMono Nerd Font).
pub const FONT_NAME: &str = "DaddyTimeMonoNerd";

pub const SELECT: &str = "󰩬";
pub const NODE: &str = "󰕟";
pub const RECT: &str = "󰗆";
pub const CIRCLE: &str = "󰕖";
pub const LINE: &str = "󰕞";
pub const POLY: &str = "󰕡";
pub const OBJECT: &str = "";
pub const LAYER: &str = "󰌨";
pub const COLOR: &str = "";
pub const PEN: &str = "";
pub const BRUSH: &str = "";
pub const EYE_DROPPER: &str = "";
pub const ELLIPSE: &str = "󰢓";
pub const BEZIER: &str = "";
pub const PATH_MAGIC: &str = "󱡄";
pub const TEXT: &str = "󱄽";
pub const BORDER_RADIUS: &str = "󰝊";
pub const ORIGIN: &str = "";

pub const POLY_TRI: &str = "󰔷";
pub const POLY_QUAD: &str = "";
pub const POLY_PENTA: &str = "󰜀";
pub const POLY_HEX: &str = "󰋙";
pub const POLY_MANY: &str = "󰙞";

pub const JOIN_SMOOTH: &str = "";
pub const JOIN_SHARP: &str = "󰃐";
pub const CAP_ROUND: &str = "";
pub const CAP_BUTT: &str = "󰹞";
pub const CENTER: &str = "󰌘";
pub const ARC: &str = "";
pub const ACTION_HIDE: &str = "";
pub const ACTION_SHOW: &str = "󰞓";
pub const CLOSE: &str = "";
pub const DELETE: &str = "󰆴";
pub const VIDEO: &str = "";
pub const AUDIO: &str = "";
pub const SPLIT: &str = "";
pub const MUSIC: &str = "󰽰";
pub const ADD: &str = "󰷫";
pub const REMOVE: &str = "󰇾";
pub const GRAB: &str = "";
pub const SHADING: &str = "󰌾";
/// Live collaboration chat (Nerd Font).
pub const CHAT: &str = "󰭹";
pub const COLLAB: &str = "󰒗";

pub fn nerd_font_id(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_NAME.into()))
}

pub fn polygon_icon(sides: u32) -> &'static str {
    match sides {
        3 => POLY_TRI,
        4 => POLY_QUAD,
        5 => POLY_PENTA,
        6 => POLY_HEX,
        _ => POLY_MANY,
    }
}