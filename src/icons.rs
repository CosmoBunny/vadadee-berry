//! Nerd Font icon glyphs (DaddyTimeMono Nerd Font).
pub const FONT_NAME: &str = "DaddyTimeMonoNerd";

pub const SELECT: &str = "󰩬";
pub const NODE: &str = "󰕟";
pub const RECT: &str = "󰗆";
pub const CIRCLE: &str = "󰕖";
pub const LINE: &str = "󰕞";
pub const POLY: &str = "󰕡";
/// Generic object / group (nf-fa-object-group).
pub const OBJECT: &str = "";
/// Group selection into a parent.
pub const GROUP: &str = "";
/// Dissolve / ungroup.
pub const UNGROUP: &str = "";
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
pub const IMAGE: &str = "󰥶";
pub const FLOWCHART: &str = "";
pub const EDIT: &str = "󰏫";
pub const ROBOT: &str = "󰚩";
pub const FIRE: &str = "";

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
/// Graph / function plotter (nf-md-chart-bell-curve-cumulative).
pub const PLOTTER: &str = "󰺒";
/// Right arrow (nf-fa-long-arrow-right) — use instead of Unicode → (missing in UI fonts → box).
pub const ARROW_RIGHT: &str = "";
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
/// Swap / reverse operands (nf-fa-exchange / ).
pub const SWAP: &str = "";
pub const RASTER: &str = "󰹑";
/// Raster paint brush (nf-md-brush).
pub const RASTER_BRUSH: &str = "󰃥";
/// Raster eraser (nf-md-eraser).
pub const ERASER: &str = "󰃢";
/// Bucket / flood fill (nf-md-format-color-fill).
pub const BUCKET: &str = "󰃡";
/// Smudge / blur finger (nf-md-water).
pub const SMUDGE: &str = "󰖌";
/// Raster region select (nf-md-selection-ellipse).
pub const RASTER_SELECT: &str = "󰒉";
/// Shader / shading layer (full-page pass; stack-order flexible via raise/lower).
pub const SHADING: &str = "󰽏";
/// Live collaboration chat (Nerd Font).
pub const CHAT: &str = "󰭹";
pub const COLLAB: &str = "󰒗";
/// Node Editor layer (nf-md-graph-outline style).
pub const NODE_EDITOR: &str = "󱁉";
/// Open node editor dialog.
pub const NODE_EDITOR_OPEN: &str = "";
/// Hide node editor dialog.
pub const NODE_EDITOR_HIDE: &str = "";
/// Parameters tab.
pub const PARAMETER: &str = "󰀻";
/// Screen Record / Stream layer (nf-cod-screen-full).
pub const SCREEN: &str = "";
/// Mouse object / encoder (nf-md-mouse).
pub const MOUSE: &str = "󰍽";
/// Septic session / player.
pub const SEPTIC: &str = "󰑋";

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