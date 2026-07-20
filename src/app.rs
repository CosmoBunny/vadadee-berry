use eframe::egui;
use egui::{Context, Event, Key, Pos2, Sense, Ui};
use kurbo::Shape;
use crate::animation::UiAnimation;
use crate::canvas::Viewport;
use crate::fonts::FontRegistry;
use crate::document::{
    default_gradient_stops, default_loft_gap_for_node, effect_placements, find_effect_for_pair,
    loft_sweep_node,
    build_path_effect_form_node, has_effect_for_objects, hidden_effect_sources, node_at_placement,
    node_uses_extended_pick_bounds, path_effect_by_form_node, path_effect_form_node_ids,
    path_effect_move_bundle, sync_path_effect_form_geometry, BezierHandleMode, Document,
    FaceRenderable, Fill, FillKind,
    GradientStop, Node, NodeId, NodeKind, ObjectOnPathEffect, OnPathMode, Paint, PathData, PathMagic, PathPlacement, TilingEffect, CircularCloneEffect, CircularRotateMode,
    BooleanEffect, BooleanOpKind, ClipMaskEffect, is_booleanable_shape, is_raster_image,
    compute_boolean_bez,
    PathEditTarget, ProjectFile, Stroke, TextStyle, text_display_name,
};
use crate::history::{snapshot_document, snapshot_project, History, ProjectEdit};
use crate::io;
use crate::render;
use crate::theme;
use crate::tools::{self, DragNewShape, MarqueeSelect, SelectDrag, ToolKind, ToolState};

#[derive(Clone, Debug, PartialEq)]
pub enum AudioExtractStatus {
    /// Left-to-right fill uses `progress` (0..1).
    Extracting { progress: f32 },
    Ready(std::path::PathBuf),
    Failed,
}

impl AudioExtractStatus {
    fn is_extracting(&self) -> bool {
        matches!(self, Self::Extracting { .. })
    }
}

/// How two selected nodes relate for Path Magic boolean / clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanPairMode {
    VectorBoolean { a: NodeId, b: NodeId },
    ImageClip { source: NodeId, mask: NodeId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GradientFlowTarget {
    Fill,
    Stroke,
}

/// GPU-resident graph FX texture (egui native TextureId, no CPU pixels).
#[derive(Debug, Clone)]
struct GraphGpuFxEntry {
    id: egui::TextureId,
    size: [usize; 2],
    /// Last baked blur (skip re-GPU when unchanged).
    blur_px: f32,
    /// Color-only cache key (brightness/contrast/sat/hue).
    color_key: String,
    /// Keep wgpu texture alive (egui bind group holds the view; we retain the Texture).
    _tex: std::sync::Arc<egui_wgpu::wgpu::Texture>,
}

#[derive(Debug, Clone, Copy)]
struct GradientFlowDrag {
    target: GradientFlowTarget,
    handle: crate::gradient_ui::GradientLineHandle,
    line_at_press: (f32, f32, f32, f32),
    doc_at_press: (f64, f64),
}
use crate::ui;

#[derive(Debug, Clone)]
struct ImagePastePlacement {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug)]
enum PasteTask {
    SystemImage {
        step: u8,
        rgba: Option<image::RgbaImage>,
        png: Option<Vec<u8>>,
        placement: Option<ImagePastePlacement>,
    },
    Objects {
        nodes: Vec<Node>,
        offset: (f64, f64),
        index: usize,
        new_sel: Vec<NodeId>,
    },
}

#[derive(Debug)]
struct PasteProgress {
    label: String,
    task: PasteTask,
}

use serde::{Deserialize, Serialize};
pub use crate::document::{InterpolationMode, Keyframe, KeyframeTrack, NodeAnimation, AnimationTimeline};

#[derive(Debug, Clone)]
pub struct AnimAppliedState {
    pub pos: (f64, f64),
    pub rotation: f64,
    pub opacity: f32,
    pub color: [f32; 4],
    pub stroke_width: f32,
    pub stroke_color: [f32; 4],
    pub geom_floats: Vec<f64>,
    pub fill: Fill,
}

/// Graph-editor interaction for stack animation function regions.
#[derive(Debug, Clone)]
pub enum AnimGraphStackDrag {
    Move {
        id: uuid::Uuid,
        grab_frame: f64,
        orig_start: usize,
    },
    ResizeEnd {
        id: uuid::Uuid,
        orig_duration: usize,
    },
}

#[derive(Debug, Clone)]
pub struct SnapGuide {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub is_tangent: bool,
}

pub struct VadadeeBerryApp {
    pub live_snap_guides: Vec<SnapGuide>,
    pub snap_magnet: bool,
    /// Pixel-art editing: nearest-neighbor feel, visible cell grid on canvas.
    pub pixel_art_mode: bool,
    /// Document units per pixel cell (e.g. 1.0 = one doc unit per pixel at export scale).
    pub pixel_cell_size: f32,
    pub anim_current_frame: usize,
    pub anim_is_playing: bool,
    /// Wall-clock last playback tick so timeline keeps advancing when the window is unfocused.
    pub anim_playback_wall: Option<std::time::Instant>,
    /// Absolute wall-clock play origin: (instant when play started, frame at that instant).
    /// Playhead = start_frame + elapsed * fps — skips frames under load (no lag catch-up).
    pub anim_play_origin: Option<(std::time::Instant, usize)>,
    pub anim_keyframing_mode: bool,
    pub anim_show_timeline_window: bool,
    pub show_video_editor_window: Option<uuid::Uuid>,
    pub show_shader_editor_window: Option<uuid::Uuid>,
    pub piano_roll_clip: Option<uuid::Uuid>,
    pub piano_roll_t: f32,
    pub piano_tool: crate::av_ui::PianoTool,
    pub piano_zoom: f32,
    pub piano_scroll_offset: f32,
    pub piano_pitch_scroll: f32,
    /// Sticky AV timeline clip/trim drag (survives clip moving under the cursor).
    pub av_timeline_drag: Option<crate::av_ui::AvTimelineDrag>,
    /// Node Editor dialog UI (open layer, tools, selection).
    pub node_editor_ui: crate::node_editor_ui::NodeEditorUiState,
    pub ui_shading_pass_sel: usize,
    pub anim_time_accumulator: f32,
    pub anim_last_seen_frame: usize,
    pub anim_last_applied_states: std::collections::HashMap<NodeId, AnimAppliedState>,
    pub anim_timeline_scroll: f32,
    pub anim_timeline_follow: bool,
    pub anim_edit_mode: bool,
    pub anim_dragged_keyframe: Option<(NodeId, String, usize)>,
    pub anim_selected_keyframe: Option<(NodeId, String, usize)>,
    pub anim_graph_editor_track: Option<(NodeId, String)>,
    pub anim_graph_editor_target_track: Option<(NodeId, String)>,
    pub anim_graph_editor_t: f32,
    pub anim_graph_editor_dragged_kf: Option<(String, usize)>,
    pub anim_graph_editor_dragged_handle: Option<(String, usize, bool)>, // (track_lbl, frame, is_left)
    /// When dragging a keyframe, record (track_lbl, frame, drag_start_pos) to detect real movement
    pub anim_graph_kf_drag_start: Option<(String, usize, egui::Pos2)>,
    /// Segment selected between two keyframe indices for bezier-add workflow: (track_lbl, left_frame, right_frame)
    pub anim_graph_selected_segment: Option<(String, usize, usize)>,
    /// Marquee region select on graph (start_frame, end_frame) while dragging / selected.
    pub anim_graph_region_select: Option<(usize, usize)>,
    /// Selected stack animation function id (for header edits / move / resize).
    pub anim_graph_selected_stack: Option<uuid::Uuid>,
    /// Drag state for stack region: Move { id, grab_start_frame, orig_start } or ResizeEnd { id, orig_duration }.
    pub anim_graph_stack_drag: Option<AnimGraphStackDrag>,
    /// Double-click formula dialog: (node_id, stack_id, channel_index).
    pub anim_stack_formula_dialog: Option<(NodeId, uuid::Uuid, usize)>,
    /// In-progress formula text for [`Self::anim_stack_formula_dialog`] (not written until Apply).
    pub anim_stack_formula_draft: String,
    /// Plotter formula dialog: node id (double-click expression in Geometry).
    pub plotter_formula_dialog: Option<NodeId>,
    /// Draft expression for [`Self::plotter_formula_dialog`].
    pub plotter_formula_draft: String,
    /// Inline Geometry-tab expression draft: (node_id, text). Avoids snap-back while typing.
    pub plotter_inline_expr: Option<(NodeId, String)>,
    /// Node snapshot when expression edit began (history committed on focus-lost / dialog Apply).
    pub plotter_expr_edit_before: Option<(NodeId, Node)>,
    /// Objects tab rename dialog: (node_or_layer id, draft name, is_layer).
    pub object_rename_dialog: Option<(uuid::Uuid, String, bool)>,
    /// Horizontal pan for the animation graph editor (frame index at left edge).
    pub anim_graph_scroll: f32,
    /// Visible frame span in the animation graph plot.
    pub anim_graph_visible_frames: f32,
    /// Visible frame span on the main animation / AV timelines.
    pub anim_timeline_visible_frames: f32,
    /// Smoothed graph Y-range (auto-fit to visible curves).
    pub anim_graph_view_val_min: f64,
    pub anim_graph_view_val_max: f64,
    pub anim_fps: u32,
    /// UI performance: smoothed frames per second.
    pub ui_fps: f32,
    /// Rasterize dense image layers to textures for pan/zoom FPS (View → Layer raster cache).
    pub enable_layer_raster_cache: bool,
    /// Compile and run shading layer WGSL on the GPU (dynamic `pass.wgsl`).
    pub gpu_shading: bool,
    /// Cloned from eframe at startup for runtime WGSL shading passes.
    pub wgpu_render: Option<egui_wgpu::RenderState>,
    /// Legacy single-frame cache (kept for backward compat with rendering code that reads it).
    pub video_frame_cache: Option<VideoFrameCache>,
    /// Per-layer async decode state. Replaces the single video_frame_cache for multi-video.
    pub video_layers: std::collections::HashMap<uuid::Uuid, VideoLayerState>,
    pub clip_mask_signatures: std::collections::HashMap<uuid::Uuid, String>,
    /// Cached full-layer rasters for dense vector content (Inkscape-style).
    layer_raster_cache: std::collections::HashMap<uuid::Uuid, crate::layer_cache::LayerRasterCacheEntry>,
    layer_cache_pending: std::collections::HashSet<uuid::Uuid>,
    layer_cache_result_tx: std::sync::mpsc::Sender<crate::layer_cache::LayerCacheResult>,
    layer_cache_result_rx: std::sync::mpsc::Receiver<crate::layer_cache::LayerCacheResult>,
    /// Keeps the default output device stream alive while audio plays.
    pub audio_device: Option<rodio::MixerDeviceSink>,
    pub audio_players: std::collections::HashMap<uuid::Uuid, rodio::Player>,
    /// File offset (seconds) at which each player's sample buffer starts.
    audio_player_buffer_offset: std::collections::HashMap<uuid::Uuid, f32>,
    /// Last timeline `file_pos` seen while a player is active (scrub/jump detection).
    audio_player_last_file_pos: std::collections::HashMap<uuid::Uuid, f32>,
    /// Playback rate used when the player was created (VideoPlayer Expr x*2 → 2.0).
    audio_player_playback_rate: std::collections::HashMap<uuid::Uuid, f32>,
    /// Media path the player was opened with — recreate when septic/video file changes.
    audio_player_media_path: std::collections::HashMap<uuid::Uuid, String>,
    /// Do not retry rodio open/decode for these layers until playback stops.
    audio_layers_skip: std::collections::HashSet<uuid::Uuid>,
    /// MP4/MOV/… → one-shot symphonia PCM wav for rodio.
    pub audio_extract_status: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, AudioExtractStatus>>>,
    /// Decoded PCM for extracted WAVs (avoids re-reading disk on seek).
    pub audio_pcm_cache: crate::audio_extract::AudioPcmCache,
    /// Background audio decode → main thread attaches rodio players.
    audio_prepare_rx:
        std::collections::HashMap<uuid::Uuid, std::sync::mpsc::Receiver<Option<crate::audio_extract::AudioPrepareResult>>>,
    /// Active OS screen captures (Screen Record layers) — desktop only.
    #[cfg(not(target_os = "android"))]
    pub screen_captures: std::collections::HashMap<uuid::Uuid, crate::screen_capture::ScreenCaptureSession>,

    pub project: ProjectFile,
    pub viewport: Viewport,
    pub tools: ToolState,
    pub selection: Vec<NodeId>,
    /// Multi-hit picker: when several objects share the same click, show an overlay list.
    /// `(screen_pos, candidate_ids)`.
    pub hit_pick_menu: Option<(Pos2, Vec<NodeId>)>,
    /// After choosing an object, ignore clicks on others until Esc / empty-space deselect.
    pub selection_sticky: bool,
    pub history: History,
    pub ui_fill_stops: Vec<GradientStop>,
    pub ui_fill_stop_sel: usize,
    pub ui_fill_edit_gradient_line: bool,
    pub ui_fill_kind: FillKind,
    pub ui_gradient_angle: f32,
    pub ui_fill_line_x0: f32,
    pub ui_fill_line_y0: f32,
    pub ui_fill_line_x1: f32,
    pub ui_fill_line_y1: f32,
    pub ui_radial_cx: f32,
    pub ui_radial_cy: f32,
    pub polygon_sides: u32,
    pub ui_stroke_stops: Vec<GradientStop>,
    pub ui_stroke_stop_sel: usize,
    pub ui_stroke_edit_gradient_line: bool,
    pub ui_stroke_line_join: crate::document::LineJoin,
    pub ui_stroke_line_cap: crate::document::LineCap,
    pub ui_stroke_paint_order: crate::document::StrokePaintOrder,
    pub ui_stroke_kind: FillKind,
    // Path marker (arrow / point icons) UI state for start/mid/end on pen paths
    pub ui_marker_start: crate::document::PathMarker,
    pub ui_marker_mid: crate::document::PathMarker,
    pub ui_marker_end: crate::document::PathMarker,
    pub ui_marker_use_common_size: bool,
    pub ui_marker_common_size: f32,
    pub ui_stroke_angle: f32,
    pub ui_stroke_line_x0: f32,
    pub ui_stroke_line_y0: f32,
    pub ui_stroke_line_x1: f32,
    pub ui_stroke_line_y1: f32,
    pub ui_stroke_radial_cx: f32,
    pub ui_stroke_radial_cy: f32,
    pub ui_stroke_width: f32,
    pub ui_text_content: String,
    pub ui_text_font_size: f32,
    /// Text box width in document px; `0` = auto (no wrap).
    pub ui_text_width: f32,
    pub ui_text_font_family: String,
    pub fonts: FontRegistry,
    pub ui_text_bold: bool,
    pub ui_text_italic: bool,
    pub fill_enabled: bool,
    pub stroke_enabled: bool,
    pub status_message: String,
    clipboard: Vec<Node>,
    /// After tab promote-to-front, animate scroll strip back to the first tab.
    pub action_tab_scroll_home: bool,
    /// Inline text editor over the canvas (no Geometry tab required).
    pub on_page_text_edit: Option<NodeId>,
    pub(crate) on_page_text_focus_pending: bool,
    on_page_text_before: Option<Node>,
    on_page_text_newly_created: bool,
    pub cursor_doc: Option<(f64, f64)>,
    pub canvas_focused: bool,
    /// Previous `ctx.input(|i| i.focused)` — for Unfocused / Switched status.
    pub window_was_focused: bool,
    pub action_bar_open: bool,
    pub action_bar_width: f32,
    pub action_tab: ui::ActionTab,
    pub action_tab_order: Vec<ui::ActionTab>,
    /// Object-on-path effect editor (Path Magic tab).
    pub ui_on_path_mode: OnPathMode,
    pub ui_on_path_gap: f64,
    pub ui_on_path_count: usize,
    pub ui_on_path_cyclic: bool,
    pub ui_on_path_rotate: bool,
    pub ui_on_path_loft_scale: f32,
    pub ui_on_path_loft_opacity: f32,
    /// Measured height of the Object on Path panel (drives expand animation).
    pub ui_on_path_container_h: f32,
    pub timeline_container_h: f32,
    pub timeline_container_w: f32,
    pub video_editor_container_h: f32,
    pub video_editor_container_w: f32,
    // Tiling params (2D)
    pub ui_tiling_rows: usize,
    pub ui_tiling_cols: usize,
    pub ui_tiling_offset_x: f64,
    pub ui_tiling_offset_y: f64,
    pub ui_tiling_row_rot: f64,
    pub ui_tiling_col_rot: f64,
    pub ui_tiling_row_scale: f64,
    pub ui_tiling_col_scale: f64,
    pub ui_tiling_gap_x: f64,
    pub ui_tiling_gap_y: f64,
    // CircularClone params
    pub ui_circular_copies: usize,
    /// Preferred boolean op in Path Magic (Union / Intersection / Difference).
    pub ui_boolean_op: BooleanOpKind,
    pub ui_circular_angle_offset: f64,
    pub ui_circular_origin_x: f64,
    pub ui_circular_origin_y: f64,
    /// CircularClone instance orientation mode (static | rotate about origin).
    pub ui_circular_rotate_mode: CircularRotateMode,
    pub ui_anim: UiAnimation,
    pub gradient_editor_focus: crate::gradient_ui::GradientEditorFocus,
    /// Cached textures for Image nodes (keyed by NodeId). Reloaded from .bytes on demand.
    image_textures: std::collections::HashMap<NodeId, egui::TextureHandle>,
    /// Cached decoded RGBA images for Eyedropper sampling to avoid massive decode frame drops.
    image_pixel_cache: std::collections::HashMap<NodeId, egui::ColorImage>,
    /// Animated flood-fill (water-drop expand) — commits on completion.
    flood_fill_anim: Option<FloodFillAnim>,
    /// Node Editor file-path image textures (ObjectImage / ObjectVideo stills).
    graph_path_textures: std::collections::HashMap<String, egui::TextureHandle>,
    /// GPU-baked FX textures (no CPU readback). Key = fx_cache_key; live path keys use `path|live`.
    graph_gpu_fx: std::collections::HashMap<String, GraphGpuFxEntry>,
    /// Decoded base RGBA for graph paths (avoid re-opening files every FX cache miss).
    graph_base_rgba: std::collections::HashMap<String, image::RgbaImage>,
    /// Downscaled base (path|side) — avoid re-downscaling full images every frame.
    graph_preview_rgba: std::collections::HashMap<String, image::RgbaImage>,
    /// Color-only (no blur) preview cache: path|color|side → rgba.
    graph_color_rgba: std::collections::HashMap<String, image::RgbaImage>,
    gradient_flow_drag: Option<GradientFlowDrag>,
    canvas_screen_rect: Option<egui::Rect>,
    canvas_origin: Pos2,
    pending_open_svg: bool,
    pending_open_project: bool,
    /// Snapshot of the project before the last Open Project (Ctrl+O).
    cached_project: Option<ProjectFile>,
    cached_project_label: Option<String>,
    pending_save_project: bool,
    pending_export_svg: bool,
    pending_export_image: bool,
    pub export_image_format: io::ExportImageFormat,
    /// When true, raster export uses selection bounds; otherwise full document.
    pub export_image_selection_only: bool,
    pub eyedropper_holding: bool,
    pub eyedropper_releasing: bool,
    pub eyedropper_t: f32,
    pub eyedropper_target_pos: Option<(f64, f64)>,
    /// Tracks Ctrl+V for paste fallback when egui-winit swallows the hotkey (image-only clipboard).
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    paste_hotkey_was_down: bool,
    /// Multi-frame paste shown on the 2nd status-bar label ("Pasting…").
    paste_progress: Option<PasteProgress>,
    pub toolbar_expanded: bool,
    pub toolbar_drag_active: bool,
    /// Updated each frame by `floating_toolbar` (dock anchors here).
    pub toolbar_outer_rect: Option<egui::Rect>,
    pub text_editor_rect: Option<egui::Rect>,
    text_pan_restore: Option<egui::Vec2>,
    text_pan_anim: Option<TextPanAnim>,
    pub last_android_text: String,
    pub path_overlay_rect: Option<egui::Rect>,
    /// Video render-to-file settings and live progress.
    pub video_export: VideoExportState,
    /// Last saved/opened project path (Ctrl+S saves here when set).
    pub project_save_path: Option<std::path::PathBuf>,
    pub left_dock: crate::left_dock::LeftDockState,
    pub collab: crate::collab::CollabSession,
    collab_last_cursor_sent: Option<(f64, f64, Option<String>, Option<String>)>,

    collab_canvas_sync_accum: f32,
    collab_last_ui_sync: (ui::ActionTab, usize),
    collab_last_wire_hash: u64,
    collab_asset_cache: std::collections::HashMap<String, Vec<u8>>,
    pub cursor_bubble_edit: bool,
    pub cursor_bubble_focus_pending: bool,
    pub cursor_bubble_text: String,
    #[cfg(not(target_os = "android"))]
    mcp_bridge: Option<crate::mcp::McpBridge>,
    #[cfg(not(target_os = "android"))]
    pub mcp_preview: McpPreviewState,
    #[cfg(not(target_os = "android"))]
    mcp_preview_update_tx: std::sync::mpsc::Sender<McpPreviewUpdate>,
    #[cfg(not(target_os = "android"))]
    mcp_preview_update_rx: std::sync::mpsc::Receiver<McpPreviewUpdate>,
    #[cfg(not(target_os = "android"))]
    pending_mcp_bulk_rects: Vec<Vec<Node>>,
    #[cfg(not(target_os = "android"))]
    mcp_bulk_staging: Vec<Node>,
    spatial_index: crate::spatial_index::SpatialIndex,
    cached_draw_order: Vec<NodeId>,
    cached_draw_order_revision: u64,
    audio_output_warned: bool,
}

/// In-progress flood-fill animation (expanding water-drop reveal).
struct FloodFillAnim {
    id: NodeId,
    before_bytes: Vec<u8>,
    /// Final filled RGBA (full buffer).
    after_rgba: Vec<u8>,
    width: u32,
    height: u32,
    seed_x: i32,
    seed_y: i32,
    /// Max distance of any filled pixel from seed (for normalize).
    max_dist: f32,
    /// Filled pixels as (x, y, dist_from_seed).
    filled: Vec<(i32, i32, f32)>,
    start: std::time::Instant,
    duration_secs: f32,
    /// Working base (before fill) RGBA for compositing frames.
    base_rgba: Vec<u8>,
}

#[cfg(not(target_os = "android"))]
#[derive(Default)]
pub struct McpPreviewState {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bounds: [f64; 4],
    pub resolution_percent: f32,
    pub updated_at: f64,
    pub texture: Option<egui::TextureHandle>,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug)]
struct McpPreviewUpdate {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    bounds: [f64; 4],
    resolution_percent: f32,
}

#[derive(Debug, Clone, Copy)]
struct TextPanAnim {
    from: egui::Vec2,
    to: egui::Vec2,
    elapsed: f32,
    duration: f32,
}

#[derive(Clone)]
pub struct VideoFrameCache {
    pub layer_id: uuid::Uuid,
    pub frame: usize,
    pub texture: egui::TextureHandle,
}

pub enum VideoCommand {
    GetFrame {
        timeline_frame: usize,
        source_frame: usize,
        fps: f32,
        path: String,
        sequential: bool,
    },
    StopStream,
    Stop,
}

/// Per-layer video decode state for async background decoding.
pub struct VideoLayerState {
    /// Currently displayed texture for this layer.
    pub texture: Option<egui::TextureHandle>,
    /// Frame index of the displayed texture.
    pub cached_frame: Option<usize>,
    /// Sender for commands to the decode thread.
    pub tx_cmd: std::sync::mpsc::Sender<VideoCommand>,
    /// Receiver for completed frame decodes.
    pub rx_frame: std::sync::mpsc::Receiver<(usize, usize, u32, u32, Vec<u8>)>,
    /// Source frame index of the displayed texture.
    pub cached_source_frame: Option<usize>,
    /// The frame we currently requested from the background thread.
    pub requested_frame: Option<usize>,
    /// Whether libav sequential decode is active for this layer.
    pub stream_active: bool,
    /// Last request timestamp in seconds (from egui time) to throttle scrubbing requests.
    pub last_req_time: Option<f64>,
    /// For object-linked stills: document revision last baked into this texture.
    pub object_link_rev: Option<u64>,
}


/// Which backend to use for video encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoBackend {
    #[default]
    Ffmpeg,
    Gstreamer,
}

impl VideoBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ffmpeg => "FFmpeg",
            Self::Gstreamer => "GStreamer",
        }
    }
}

/// CPU usage profile while encoding video (libav encoder thread count / preset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportPowerLevel {
    #[default]
    PowerSaving,
    FullPower,
}

impl ExportPowerLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerSaving => "Power saving",
            Self::FullPower => "Full power",
        }
    }
}

/// P7f: Node Editor / FX bake quality for export (max side + blur quantization).
///
/// Long-side caps must stay in the HD range — older 128/256/512 values made
/// 1080p exports look like soft mush after upscale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportFxQuality {
    /// Fast draft: 720p-class bake.
    Draft,
    /// Default: full HD long side.
    #[default]
    Normal,
    /// Best: 1440p-class bake (still capped by page size in the worker).
    High,
}

impl ExportFxQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft (fast)",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }

    /// Longest side of NE FilePath bake (pixels).
    pub fn max_side(self) -> u32 {
        match self {
            Self::Draft => 720,
            Self::Normal => 1080,
            Self::High => 1440,
        }
    }

    /// Blur radius quantization step for export FX cache keys.
    pub fn blur_step(self) -> f32 {
        match self {
            Self::Draft => 2.0,
            Self::Normal => 1.0,
            Self::High => 0.5,
        }
    }
}

/// Container format for video export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoFormat {
    #[default]
    Mp4,
    Mkv,
    Webm,
    Mov,
}

impl VideoFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4  => "MP4 (H.264)",
            Self::Mkv  => "MKV (H.264)",
            Self::Webm => "WebM (VP9)",
            Self::Mov  => "MOV (ProRes)",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4  => "mp4",
            Self::Mkv  => "mkv",
            Self::Webm => "webm",
            Self::Mov  => "mov",
        }
    }
}

/// All render-to-video settings plus live progress state.
pub struct VideoExportState {
    pub backend: VideoBackend,
    pub fps: u32,
    pub resolution_pct: u32,  // 25, 50, 75, 100, 150, 200
    pub bitrate_kbps: u32,
    pub format: VideoFormat,
    /// 0.0 – 1.0 while rendering, None when idle.
    pub progress: Option<f32>,
    /// True while the progress dialog is shown, false when hidden.
    pub progress_visible: bool,
    /// True when a render is actually running.
    pub rendering: bool,
    /// Latest status message from the encoder.
    pub status_msg: String,
    pub frame_done: usize,
    pub total_frames: usize,
    /// 0 = auto from timeline/content; otherwise fixed export length in seconds.
    pub export_duration_secs: f32,
    /// How many times to repeat the animation loop in the export (1 = once).
    pub export_cycles: u32,
    pub restore_anim_frame: usize,
    pub frames_dir: Option<std::path::PathBuf>,
    pub output_path: Option<std::path::PathBuf>,
    pub power_level: ExportPowerLevel,
    /// P7f: NE Output bake quality (Draft / Normal / High).
    pub fx_quality: ExportFxQuality,
    pub export_start_time: Option<std::time::Instant>,
    export_rx: Option<std::sync::mpsc::Receiver<crate::export_worker::ExportWorkerEvent>>,
    export_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,

    // System stats and Jokes fields:
    pub sys_stats: crate::sys_stats::SysStats,
    pub last_stats_update: std::time::Instant,
    pub last_joke_update: std::time::Instant,
    pub joke_rules: Vec<crate::sys_stats::JokeRule>,
    pub current_joke: String,
    /// Cycles through jokes sequentially (CPU, RAM, DEFAULT, etc.) instead of random.
    pub joke_cycle: usize,
    pub sec_per_frame: f32,
    pub last_frame_time: Option<std::time::Instant>,
    /// P7a: bar target from worker (`frame_done / total`).
    pub progress_target: f32,
    /// P7a: displayed bar value (lerps toward `progress_target` each UI frame).
    pub progress_smooth: f32,
    /// P7a: latest worker frame count (may jump ahead of displayed progress).
    pub worker_frame_done: usize,
    pub renderer_reclaim: std::sync::Arc<std::sync::Mutex<Vec<egui_wgpu::Renderer>>>,
}

impl std::fmt::Debug for VideoExportState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoExportState")
            .field("backend", &self.backend)
            .field("fps", &self.fps)
            .field("resolution_pct", &self.resolution_pct)
            .field("bitrate_kbps", &self.bitrate_kbps)
            .field("format", &self.format)
            .field("progress", &self.progress)
            .field("progress_visible", &self.progress_visible)
            .field("rendering", &self.rendering)
            .field("status_msg", &self.status_msg)
            .field("frame_done", &self.frame_done)
            .field("total_frames", &self.total_frames)
            .field("export_duration_secs", &self.export_duration_secs)
            .field("restore_anim_frame", &self.restore_anim_frame)
            .field("frames_dir", &self.frames_dir)
            .field("output_path", &self.output_path)
            .field("power_level", &self.power_level)
            .field("fx_quality", &self.fx_quality)
            .field("export_start_time", &self.export_start_time)
            .finish()
    }
}

impl Default for VideoExportState {
    fn default() -> Self {
        let mut rules = Vec::new();
        if let Ok(content) = std::fs::read_to_string("jokes_export.txt") {
            rules = crate::sys_stats::parse_jokes(&content);
        }
        if rules.is_empty() {
            rules = crate::sys_stats::parse_jokes(
                // ── Platform-independent jokes (no prefix) ──────────────────
                "[CPU 80..]\nYour CPU is working harder than a developer on a deadline.\n\
                 [CPU 80..]\nThe CPU is so hot, you could fry an egg on it.\n\
                 [CPU 80..]\nCPU became BBQ. Just cook food there and save the gas bill.\n\
                 [CPU ..2]\nCPU usage is basically 0%... did the export even start?\n\
                 [SEC_PER_FRAME 1..]\nAt this speed, a flipbook would be faster.\n\
                 [SEC_PER_FRAME 0.1..=1]\n1-10 fps? Your PC is giving every frame a hug.\n\
                 [RAM 16..]\nRAM eating competition — and your laptop/desktop is winning gold.\n\
                 [RAM ..4]\nWhere is the RAM? Are you exporting on a potato?\n\
                 [CPU_TEMP 80..]\nTemperature warning: things are getting spicy in there.\n\
                 [CPU_TEMP 80..]\nYour CPU temp is higher than my motivation on Monday.\n\
                 \
                 # ── Desktop-only jokes ──────────────────────────────────────
                 [DESKTOP CPU 80..]\nYour PC sounds like a jet engine. Ready for takeoff?\n\
                 [DESKTOP CPU ..2]\nDid you accidentally place your laptop/desktop in Antarctica?\n\
                 [DESKTOP SEC_PER_FRAME 1..]\nEven my grandma\'s old PC could export this faster.\n\
                 [DESKTOP RAM ..4]\nBro, you\'re exporting video with less RAM than a smart fridge.\n\
                 [DESKTOP RAM 32..]\nThat\'s a lot of RAM. Your PC could run the whole country.\n\
                 \
                 # ── Mobile-only jokes ───────────────────────────────────────
                 [MOBILE CPU 80..]\nYour phone is hotter than the sun right now. Poor little guy.\n\
                 [MOBILE CPU 80..]\nPhone CPU on max load — hope you\'re not using the camera too.\n\
                 [MOBILE CPU ..2]\nCPU at 0% on mobile? The app might be asleep at the wheel.\n\
                 [MOBILE SEC_PER_FRAME 1..]\nExporting video on a phone? Brave soul. Truly brave.\n\
                 [MOBILE SEC_PER_FRAME 2..]\nMaybe send the project to a PC... just a friendly suggestion.\n\
                 [MOBILE RAM ..4]\nYour phone is basically begging you to close some apps.\n\
                 [MOBILE RAM 8..]\nWow, 8 GB RAM on a phone. Overkill, but we love it.\n\
                 [MOBILE CPU_TEMP 45..]\nPhone getting warm... your pocket is a sauna now.\n\
                 \
                 # ── Fallback (DEFAULT applies everywhere) ───────────────────
                 [DEFAULT]\nStill rendering... go touch some grass.\n\
                 [DEFAULT]\nExporting... perfect time to hydrate.\n\
                 [DEFAULT]\nPatience is a virtue. You\'re basically a saint right now.\n\
                 [DEFAULT]\nStill going... you\'ve earned a snack break."
            );
        }

        Self {
            backend: VideoBackend::Ffmpeg,
            fps: 30,
            resolution_pct: 100,
            bitrate_kbps: 8000,
            format: VideoFormat::Mp4,
            progress: None,
            progress_visible: false,
            rendering: false,
            status_msg: String::new(),
            frame_done: 0,
            total_frames: 0,
            export_duration_secs: 0.0,
            export_cycles: 1,
            restore_anim_frame: 0,
            frames_dir: None,
            output_path: None,
            power_level: ExportPowerLevel::default(),
            fx_quality: ExportFxQuality::default(),
            export_start_time: None,
            export_rx: None,
            export_cancel: None,

            sys_stats: crate::sys_stats::SysStats::new(),
            last_stats_update: std::time::Instant::now(),
            last_joke_update: std::time::Instant::now(),
            joke_rules: rules,
            current_joke: "Still exporting... Go grab a coffee, or maybe grow a tree.".to_string(),
            joke_cycle: 0,
            sec_per_frame: 0.0,
            last_frame_time: None,
            progress_target: 0.0,
            progress_smooth: 0.0,
            worker_frame_done: 0,
            renderer_reclaim: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

fn collab_wire_hash(json: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    json.hash(&mut h);
    h.finish()
}

impl VadadeeBerryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let fonts = FontRegistry::new();
        let default_font = fonts.default_family();
        
        let mut initial_project = Document::new_default_project();
        let mut initial_status = "Idle".to_string();
        let mut initial_save_path: Option<std::path::PathBuf> = None;

        #[cfg(not(target_os = "android"))]
        {
            let args: Vec<String> = std::env::args().collect();
            if args.len() > 1 {
                let path = std::path::Path::new(&args[1]);
                if path.exists() && path.is_file() {
                    match io::load_project(path) {
                        Ok(p) => {
                            initial_project = p;
                            initial_save_path = Some(path.to_path_buf());
                            initial_status = format!("Loaded project: {}", path.display());
                        }
                        Err(e) => {
                            initial_status = format!("Failed to load project: {e}");
                        }
                    }
                }
            }
        }

        #[cfg(not(target_os = "android"))]
        let (mcp_preview_update_tx, mcp_preview_update_rx) = std::sync::mpsc::channel();
        #[cfg(target_os = "android")]
        let (mcp_preview_update_tx, mcp_preview_update_rx) = (std::sync::mpsc::channel().0, std::sync::mpsc::channel().1); // dummy

        let (layer_cache_result_tx, layer_cache_result_rx) = std::sync::mpsc::channel();
        let wgpu_render = cc.wgpu_render_state.clone();

        // Drop stale regenerable extracts left from previous runs (crash / no clean exit).
        #[cfg(not(target_os = "android"))]
        purge_vadadee_disk_caches(CachePurgeOpts::on_startup());

        let app = Self {
            live_snap_guides: Vec::new(),
            snap_magnet: true,
            pixel_art_mode: false,
            pixel_cell_size: 1.0,
            anim_current_frame: 0,
            anim_is_playing: false,
            anim_playback_wall: None,
            anim_play_origin: None,
            anim_keyframing_mode: false,
            anim_show_timeline_window: false,
            show_video_editor_window: None,
            show_shader_editor_window: None,
            piano_roll_clip: None,
            piano_roll_t: 0.0,
            piano_tool: crate::av_ui::PianoTool::default(),
            piano_zoom: 1.0,
            piano_scroll_offset: 0.0,
            piano_pitch_scroll: 36.0,
            av_timeline_drag: None,
            node_editor_ui: crate::node_editor_ui::NodeEditorUiState::default(),
            ui_shading_pass_sel: 0,
            anim_time_accumulator: 0.0,
            anim_last_seen_frame: 0,
            anim_last_applied_states: std::collections::HashMap::new(),
            anim_timeline_scroll: 0.0,
            anim_timeline_follow: true,
            anim_edit_mode: false,
            anim_dragged_keyframe: None,
            anim_selected_keyframe: None,
            anim_graph_editor_track: None,
            anim_graph_editor_target_track: None,
            anim_graph_editor_t: 0.0,
            anim_graph_editor_dragged_kf: None,
            anim_graph_editor_dragged_handle: None,
            anim_graph_kf_drag_start: None,
            anim_graph_selected_segment: None,
            anim_graph_region_select: None,
            anim_graph_selected_stack: None,
            anim_graph_stack_drag: None,
            anim_stack_formula_dialog: None,
            anim_stack_formula_draft: String::new(),
            plotter_formula_dialog: None,
            plotter_formula_draft: String::new(),
            plotter_inline_expr: None,
            plotter_expr_edit_before: None,
            object_rename_dialog: None,
            anim_graph_scroll: 0.0,
            anim_graph_visible_frames: 100.0,
            anim_timeline_visible_frames: 100.0,
            anim_graph_view_val_min: 0.0,
            anim_graph_view_val_max: 1.0,
            anim_fps: 60,
            ui_fps: 60.0,
            enable_layer_raster_cache: false,
            gpu_shading: true,
            wgpu_render,
            video_frame_cache: None,
            video_layers: std::collections::HashMap::new(),
            clip_mask_signatures: std::collections::HashMap::new(),
            layer_raster_cache: std::collections::HashMap::new(),
            layer_cache_pending: std::collections::HashSet::new(),
            layer_cache_result_tx,
            layer_cache_result_rx,
            audio_device: rodio::DeviceSinkBuilder::open_default_sink().ok(),
            audio_players: std::collections::HashMap::new(),
            audio_player_buffer_offset: std::collections::HashMap::new(),
            audio_player_last_file_pos: std::collections::HashMap::new(),
            audio_player_playback_rate: std::collections::HashMap::new(),
            audio_player_media_path: std::collections::HashMap::new(),
            audio_layers_skip: std::collections::HashSet::new(),
            audio_extract_status: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            audio_pcm_cache: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            audio_prepare_rx: std::collections::HashMap::new(),
            #[cfg(not(target_os = "android"))]
            screen_captures: std::collections::HashMap::new(),

            project: initial_project,
            viewport: Viewport::default(),
            tools: ToolState {
                active: ToolKind::Select,
                ..Default::default()
            },
            selection: vec![],
            hit_pick_menu: None,
            selection_sticky: false,
            history: History::default(),
            ui_fill_stops: default_gradient_stops(),
            ui_fill_stop_sel: 0,
            ui_fill_edit_gradient_line: false,
            ui_fill_kind: FillKind::Solid,
            ui_gradient_angle: 90.0,
            ui_fill_line_x0: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.0
            },
            ui_fill_line_y0: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.1
            },
            ui_fill_line_x1: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.2
            },
            ui_fill_line_y1: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.3
            },
            ui_radial_cx: 0.5,
            ui_radial_cy: 0.5,
            polygon_sides: 6,
            ui_stroke_stops: vec![
                GradientStop::new(0.0, Paint::from_hex(0x1a1f2e, 1.0)),
                GradientStop::new(1.0, Paint::from_hex(0x1a1f2e, 1.0)),
            ],
            ui_stroke_stop_sel: 0,
            ui_stroke_edit_gradient_line: false,
            ui_stroke_line_join: crate::document::LineJoin::Miter,
            ui_stroke_line_cap: crate::document::LineCap::Butt,
            ui_stroke_paint_order: crate::document::StrokePaintOrder::BehindFill,
            ui_stroke_kind: FillKind::Solid,
            ui_stroke_angle: 0.0,
            ui_marker_start: crate::document::PathMarker::default(),
            ui_marker_mid: crate::document::PathMarker::default(),
            ui_marker_end: crate::document::PathMarker::default(),
            ui_marker_use_common_size: false,
            ui_marker_common_size: 10.0,
            ui_stroke_line_x0: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.0
            },
            ui_stroke_line_y0: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.1
            },
            ui_stroke_line_x1: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.2
            },
            ui_stroke_line_y1: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.3
            },
            ui_stroke_radial_cx: 0.5,
            ui_stroke_radial_cy: 0.5,
            ui_stroke_width: 2.0,
            ui_text_content: "Text".into(),
            ui_text_font_size: 24.0,
            ui_text_width: 0.0,
            ui_text_font_family: default_font,
            fonts,
            ui_text_bold: false,
            ui_text_italic: false,
            fill_enabled: true,
            stroke_enabled: true,
            status_message: initial_status,
            clipboard: Vec::new(),
            action_tab_scroll_home: false,
            on_page_text_edit: None,
            on_page_text_focus_pending: false,
            on_page_text_before: None,
            on_page_text_newly_created: false,
            image_textures: std::collections::HashMap::new(),
            image_pixel_cache: std::collections::HashMap::new(),
            flood_fill_anim: None,
            graph_path_textures: std::collections::HashMap::new(),
            graph_gpu_fx: std::collections::HashMap::new(),
            graph_base_rgba: std::collections::HashMap::new(),
            graph_preview_rgba: std::collections::HashMap::new(),
            graph_color_rgba: std::collections::HashMap::new(),
            cursor_doc: None,
            action_bar_open: true,
            action_bar_width: 300.0,
            action_tab: ui::ActionTab::default(),
            action_tab_order: ui::ActionTab::all_tabs(),
            ui_on_path_mode: OnPathMode::GapDuplicate,
            ui_on_path_gap: 48.0,
            ui_on_path_count: 5,
            ui_on_path_cyclic: true,
            ui_on_path_rotate: true,
            ui_on_path_loft_scale: 1.0,
            ui_on_path_loft_opacity: 0.75,
            ui_on_path_container_h: 280.0,
            timeline_container_h: 56.0,
            timeline_container_w: 0.0,
            video_editor_container_h: 130.0,
            video_editor_container_w: 0.0,
            ui_tiling_rows: 3,
            ui_tiling_cols: 3,
            ui_tiling_offset_x: 0.0,
            ui_tiling_offset_y: 0.0,
            ui_tiling_row_rot: 0.0,
            ui_tiling_col_rot: 0.0,
            ui_tiling_row_scale: 0.0,
            ui_tiling_col_scale: 0.0,
            ui_tiling_gap_x: 48.0,
            ui_tiling_gap_y: 48.0,
            ui_circular_copies: 6,
            ui_boolean_op: BooleanOpKind::Union,
            ui_circular_angle_offset: 0.0,
            ui_circular_origin_x: 0.0,
            ui_circular_origin_y: 0.0,
            ui_circular_rotate_mode: CircularRotateMode::ReferenceOrigin,
            ui_anim: {
                let mut anim = UiAnimation::new();
                anim.seed_status_board("Idle", 80.0, 56.0);
                anim
            },
            gradient_editor_focus: crate::gradient_ui::GradientEditorFocus::None,
            gradient_flow_drag: None,
            canvas_screen_rect: None,
            canvas_origin: Pos2::ZERO,
            pending_open_svg: false,
            pending_open_project: false,
            cached_project: None,
            cached_project_label: None,
            pending_save_project: false,
            pending_export_svg: false,
            pending_export_image: false,
            export_image_format: io::ExportImageFormat::Png,
            export_image_selection_only: false,
            eyedropper_holding: false,
            eyedropper_releasing: false,
            eyedropper_t: 0.0,
            eyedropper_target_pos: None,
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
            paste_hotkey_was_down: false,
            paste_progress: None,
            toolbar_expanded: false,
            toolbar_drag_active: false,
            toolbar_outer_rect: None,
            text_editor_rect: None,
            text_pan_restore: None,
            text_pan_anim: None,
            last_android_text: String::new(),
            path_overlay_rect: None,
            video_export: VideoExportState::default(),
            project_save_path: initial_save_path,
            left_dock: crate::left_dock::LeftDockState::default(),
            collab: crate::collab::CollabSession::new(),
            collab_last_cursor_sent: None,

            collab_canvas_sync_accum: 0.0,
            collab_last_ui_sync: (ui::ActionTab::default(), 0),
            collab_last_wire_hash: 0,
            collab_asset_cache: std::collections::HashMap::new(),
            cursor_bubble_edit: false,
            cursor_bubble_focus_pending: false,
            cursor_bubble_text: String::new(),
            #[cfg(not(target_os = "android"))]
            mcp_bridge: crate::mcp::McpBridge::try_start(),
            #[cfg(not(target_os = "android"))]
            mcp_preview: McpPreviewState::default(),
            #[cfg(not(target_os = "android"))]
            mcp_preview_update_tx,
            #[cfg(not(target_os = "android"))]
            mcp_preview_update_rx,
            #[cfg(not(target_os = "android"))]
            pending_mcp_bulk_rects: Vec::new(),
            #[cfg(not(target_os = "android"))]
            mcp_bulk_staging: Vec::new(),
            spatial_index: crate::spatial_index::SpatialIndex::default(),
            cached_draw_order: Vec::new(),
            cached_draw_order_revision: u64::MAX,
            audio_output_warned: false,
            canvas_focused: false,
            window_was_focused: true,
        };
        if let Some(rs) = &app.wgpu_render {
            crate::shading::init_callback_resources(rs, crate::VIEWPORT_MSAA_SAMPLES);
        }
        app
    }

    pub fn window_title(&self) -> String {
        let name = self
            .project_save_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.project.document.title.clone());
        format!("{name} — Vadadee Berry")
    }

    fn sync_window_title(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
    }

    fn ensure_audio_output(&mut self) -> bool {
        if self.audio_device.is_some() {
            return true;
        }
        // Device was dropped (underrun / hotplug / OS sleep) — rebuild so audio
        // recovers without restarting the whole app.
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(sink) => {
                self.audio_device = Some(sink);
                self.audio_output_warned = false;
                self.audio_players.clear();
                self.audio_player_buffer_offset.clear();
                self.audio_player_last_file_pos.clear();
                self.audio_player_playback_rate.clear();
                self.audio_player_media_path.clear();
                self.audio_layers_skip.clear();
                log::info!("audio output device opened");
                true
            }
            Err(e) => {
                if !self.audio_output_warned {
                    self.audio_output_warned = true;
                    self.status_message =
                        format!("No audio output device ({e}). Timeline playback is silent.");
                    log::warn!("rodio open_default_sink failed: {e}");
                }
                false
            }
        }
    }

    /// Drop the audio device so the next frame reopens it (recovery path).
    fn reset_audio_output(&mut self, reason: &str) {
        log::warn!("resetting audio output: {reason}");
        self.audio_players.clear();
        self.audio_player_buffer_offset.clear();
        self.audio_player_last_file_pos.clear();
        self.audio_player_playback_rate.clear();
        self.audio_player_media_path.clear();
        self.audio_device = None;
        self.audio_output_warned = false;
    }

    fn save_project_to_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        crate::io::save_project(path, &self.project).map_err(|e| e.to_string())?;
        self.project_save_path = Some(path.to_path_buf());
        Ok(())
    }

    /// Document point under the pointer when it is over the canvas (no click required).
    fn doc_at_pointer_hover(&self, ctx: &egui::Context) -> Option<(f64, f64)> {
        let rect = self.canvas_screen_rect?;
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        if !rect.contains(pos) {
            return None;
        }
        let mut doc = self.viewport.screen_to_doc(pos, self.canvas_origin);
        doc = self.viewport.snap(doc);
        Some(doc)
    }

    fn ensure_cursor_doc_for_collab_bubble(&mut self, ctx: &egui::Context) {
        if self.cursor_doc.is_some() {
            return;
        }
        if let Some(doc) = self.doc_at_pointer_hover(ctx) {
            self.cursor_doc = Some(doc);
            return;
        }
        if let Some(rect) = self.canvas_screen_rect {
            let center = rect.center();
            let mut doc = self.viewport.screen_to_doc(center, self.canvas_origin);
            doc = self.viewport.snap(doc);
            self.cursor_doc = Some(doc);
        }
    }

    fn update_cursor_doc_from_pointer(&mut self, ctx: &egui::Context, response: &egui::Response) {
        let pointer = if response.hovered() || response.dragged() {
            response
                .hover_pos()
                .or_else(|| response.interact_pointer_pos())
                .or_else(|| ctx.input(|i| i.pointer.hover_pos()))
        } else {
            None
        };
        let pointer = pointer.and_then(|pos| {
            self.canvas_screen_rect
                .filter(|r| r.contains(pos))
                .map(|_| pos)
        });
        match pointer {
            Some(pos) => {
                let mut doc = self.viewport.screen_to_doc(pos, self.canvas_origin);
                doc = self.viewport.snap(doc);
                self.cursor_doc = Some(doc);
            }
            None if !self.cursor_bubble_edit => {
                self.cursor_doc = None;
            }
            None => {}
        }
    }

    pub fn canvas_has_active_focus(&self) -> bool {
        self.cursor_doc.is_some() || self.canvas_focused
    }

    /// Pan/zoom so a collaborator's document point is centered on the canvas.
    pub fn focus_viewport_on_peer(&mut self, doc_x: f64, doc_y: f64) {
        let Some(rect) = self.canvas_screen_rect else {
            return;
        };
        let center = rect.center();
        let origin = self.canvas_origin;
        self.viewport.pan.x = center.x - origin.x - doc_x as f32 * self.viewport.zoom;
        self.viewport.pan.y = center.y - origin.y - doc_y as f32 * self.viewport.zoom;
    }

    fn apply_collab_remote_ui(&mut self, state: crate::collab::CollabUiStateApply) {
        let layers_len = self.project.document.layers.len();
        if layers_len > 0 && state.active_layer_index < layers_len {
            self.project.document.active_layer_index = state.active_layer_index;
        }
        if let Some(ref slug) = state.action_tab {
            if let Some(tab) = ui::ActionTab::from_collab_slug(slug) {
                if matches!(tab, ui::ActionTab::Layer | ui::ActionTab::Objects)
                    && self.action_tab != tab
                {
                    ui::promote_action_tab_at(self, tab, 0);
                }
            }
        }
    }

    /// Network poll + canvas merge (runs in `logic`, before UI paints).
    fn tick_live_collaboration_poll(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
        self.collab.poll();
        self.collab.tick_network(dt);

        if let Some(state) = self.collab.take_pending_ui_state() {
            self.apply_collab_remote_ui(state);
        }
        for (user, text) in self.collab.take_pending_chat_toasts() {
            if self.left_dock.game_chat_notifications {
                self.left_dock.push_chat_toast(user, text);
            }
        }
        if let Some(json) = self.collab.take_pending_canvas_json() {
            if let Ok(loaded) = serde_json::from_str::<ProjectFile>(&json) {
                #[cfg(not(target_os = "android"))]
                {
                    crate::collab::merge_remote(
                        &mut self.project,
                        loaded,
                        &mut self.collab_asset_cache,
                    );
                    let stripped = crate::collab::strip_for_wire(
                        &self.project,
                        &mut self.collab_asset_cache,
                    );
                    if let Ok(merged_json) = serde_json::to_string(&stripped) {
                        let hash = collab_wire_hash(&merged_json);
                        self.collab_last_wire_hash = hash;
                        self.collab.set_last_sent_canvas_hash(hash);
                    }
                    self.pending_mcp_bulk_rects.clear();
                    self.mcp_bulk_staging.clear();
                }
                #[cfg(target_os = "android")]
                {
                    self.project = loaded;
                }
                self.collab.enable_canvas_outbound();
                self.status_message = "Canvas synced from peer".into();
            }
        }
        if !self.collab.is_connected() {
            self.collab_last_cursor_sent = None;
            self.collab_last_wire_hash = 0;
            self.collab_canvas_sync_accum = 0.0;
            return;
        }

        let layer_idx = self.project.document.active_layer_index;
        let ui_key = (self.action_tab, layer_idx);
        if ui_key != self.collab_last_ui_sync {
            let tab_slug = match self.action_tab {
                ui::ActionTab::Layer | ui::ActionTab::Objects => {
                    Some(self.action_tab.collab_slug())
                }
                _ => None,
            };
            self.collab.send_ui_state(tab_slug, layer_idx);
            self.collab_last_ui_sync = ui_key;
        }
    }

    /// Cursor + canvas sync after `canvas_ui` has updated `cursor_doc` (same frame).
    pub fn tick_live_collaboration_after_canvas(&mut self, ctx: &egui::Context) {
        if !self.collab.is_connected() {
            return;
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
        // Refresh from global hover so peers see movement without a canvas click.
        let hover_doc = self.doc_at_pointer_hover(ctx);
        if let Some(doc) = hover_doc {
            self.cursor_doc = Some(doc);
        }

        if let Some((cx, cy)) = self.cursor_doc {
            let tool = Some(self.tools.active.label().to_string());
            let bubble = if !self.cursor_bubble_text.is_empty() || self.cursor_bubble_edit {
                Some(self.cursor_bubble_text.clone())
            } else {
                None
            };
            let doc_eps = (0.5 / self.viewport.zoom as f64).max(0.01);
            let changed = self
                .collab_last_cursor_sent
                .as_ref()
                .map(|(px, py, prev_b, prev_tool)| {
                    (px - cx).hypot(py - cy) > doc_eps
                        || prev_b != &bubble
                        || prev_tool != &tool
                })
                .unwrap_or(true);
            if changed {
                self.collab_last_cursor_sent = Some((
                    cx,
                    cy,
                    bubble.clone(),
                    tool.clone(),
                ));
                self.collab.send_cursor(cx, cy, tool, bubble);
                ctx.request_repaint();
            }
        }
        if self.cursor_bubble_edit {
            ctx.request_repaint();
        }

        self.collab_canvas_sync_accum += dt;
        let dragging_objects = !self.tools.select.drag_snapshot.is_empty()
            || self.tools.drag_shape.is_some();
        let canvas_interval: f32 = if dragging_objects { 0.08 } else { 0.35 };
        let force_push = self.collab.take_canvas_push_requested();
        let due = force_push || self.collab_canvas_sync_accum >= canvas_interval;
        if due {
            if !force_push {
                self.collab_canvas_sync_accum = 0.0;
            }
            #[cfg(not(target_os = "android"))]
            {
                const COLLAB_CANVAS_MAX_NODES: usize = 500;
                if self.collab.canvas_outbound_enabled()
                    && (force_push || self.project.nodes.map.len() <= COLLAB_CANVAS_MAX_NODES)
                {
                    let stripped = crate::collab::strip_for_wire(
                        &self.project,
                        &mut self.collab_asset_cache,
                    );
                    if let Ok(json) = serde_json::to_string(&stripped) {
                        let wire_hash = collab_wire_hash(&json);
                        if force_push || wire_hash != self.collab_last_wire_hash {
                            self.collab_last_wire_hash = wire_hash;
                            self.collab.send_canvas_if_changed(&json, force_push);
                        }
                    }
                }
            }
            #[cfg(target_os = "android")]
            if let Ok(json) = serde_json::to_string(&self.project) {
                self.collab.send_canvas_if_changed(&json, force_push);
            }
        }
    }

    pub fn new_document(&mut self) {
        let before = snapshot_project(&self.project);
        let after = Document::new_empty_project();
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection.clear();
        self.project_save_path = None;
        self.viewport.pan = egui::vec2(48.0, 48.0);
        self.viewport.zoom = 0.85;
        self.status_message = "New A4 document".into();
        self.ui_anim.replay_intro();
    }

    pub fn request_open_svg(&mut self) {
        self.pending_open_svg = true;
    }

    pub fn request_open_project(&mut self) {
        self.pending_open_project = true;
    }

    pub fn request_import_image(&mut self) {
        #[cfg(target_os = "android")]
        {
            self.status_message = "Image import from files is not available on Android yet".into();
            return;
        }
        #[cfg(not(target_os = "android"))]
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            if let Ok(bytes) = std::fs::read(&path) {
                // Place near view "center" (rough, user can drag)
                let cx = 200.0;
                let cy = 150.0;
                let w = 320.0;
                let h = 240.0;
                self.insert_image(cx - w / 2.0, cy - h / 2.0, w, h, bytes);
            }
        }
    }
    pub fn request_save_project(&mut self) {
        self.pending_save_project = true;
    }
    pub fn request_export_svg(&mut self) {
        self.pending_export_svg = true;
    }

    pub fn request_export_image(&mut self) {
        self.pending_export_image = true;
    }

    pub fn apply_media_duration_from_path(&mut self, layer_index: usize, path: &str) {
        if layer_index >= self.project.document.layers.len() || path.is_empty() {
            return;
        }
        let Some(dur) = crate::video_decode::probe_media_duration_secs(path) else {
            log::warn!("Could not probe media duration for {}", path);
            return;
        };
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(layer) = after.layers.get_mut(layer_index) {
            layer.media_source_duration = Some(dur);
            layer.video_play_length = dur;
            if layer.video_start_offset > dur {
                layer.video_start_offset = 0.0;
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    /// Apply probed file duration: always refresh `media_source_duration`; only replace
    /// `video_play_length` when it is still the default placeholder (≥ 3599s).
    fn apply_probed_media_duration(layer: &mut crate::document::Layer, dur: f32) {
        let dur = dur.max(0.0);
        layer.media_source_duration = Some(dur);
        if layer.video_play_length >= 3599.0 {
            layer.video_play_length = dur;
        } else if layer.video_play_length > dur {
            layer.video_play_length = dur;
        }
        if layer.video_start_offset > dur {
            layer.video_start_offset = 0.0;
        }
    }

    /// Re-probe stale video/audio layers without pushing undo history (video editor UI).
    pub fn sync_stale_media_layer_durations(&mut self) {
        for layer in &mut self.project.document.layers {
            if layer.kind != crate::document::LayerKind::AV
            {
                continue;
            }
            if layer.video_path.is_empty() {
                continue;
            }
            let needs_probe = layer.video_play_length >= 3599.0 || layer.media_source_duration.is_none();
            if !needs_probe {
                continue;
            }
            let Some(dur) = crate::video_decode::probe_media_duration_secs(&layer.video_path) else {
                continue;
            };
            Self::apply_probed_media_duration(layer, dur);
        }
    }

    /// Re-probe video/audio layers (e.g. after load when play length was still default 3600s).
    pub fn refresh_all_media_layer_durations(&mut self) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let mut changed = false;
        for layer in &mut after.layers {
            if layer.kind != crate::document::LayerKind::AV
            {
                continue;
            }
            if layer.video_path.is_empty() {
                continue;
            }
            let Some(dur) = crate::video_decode::probe_media_duration_secs(&layer.video_path) else {
                log::warn!("Could not probe media duration for {}", layer.video_path);
                continue;
            };
            let cap_stale = layer
                .media_source_duration
                .is_none_or(|stored| (stored - dur).abs() > 0.05);
            let play_is_placeholder = layer.video_play_length >= 3599.0;
            let play_exceeds_file = layer.video_play_length > dur + 0.05;
            if !cap_stale && !play_is_placeholder && !play_exceeds_file {
                continue;
            }
            Self::apply_probed_media_duration(layer, dur);
            changed = true;
        }
        if changed {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchDocument { before, after },
            );
        }
    }

    pub fn do_undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.restore_selection_after_history();
            self.clear_transient_tool_state();
            self.invalidate_image_textures();
            self.status_message = "Undo".into();
            self.sync_inspector_from_selection();
            self.sync_flowchart_paths_if_active_layer();
        }
    }

    pub fn do_redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.restore_selection_after_history();
            self.clear_transient_tool_state();
            self.invalidate_image_textures();
            self.status_message = "Redo".into();
            self.sync_inspector_from_selection();
            self.sync_flowchart_paths_if_active_layer();
        }
    }

    /// Keep selection when objects still exist (so weight-flow / path edit stay active after undo).
    fn restore_selection_after_history(&mut self) {
        let prev = self.selection.clone();
        self.selection = prev
            .into_iter()
            .filter(|id| self.project.nodes.get(*id).is_some())
            .collect();
        // Drop sticky only if everything vanished.
        if self.selection.is_empty() {
            self.selection_sticky = false;
        }
    }

    fn clear_transient_tool_state(&mut self) {
        self.tools.drag_shape = None;
        self.tools.select.drag_mode = None;
        self.tools.select.marquee = None;
        self.tools.select.drag_snapshot.clear();
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
        self.tools.canvas_pan_drag = false;
        // Abort mid-stroke sim, but keep weight-flow *enabled* so undo doesn't "close" the brush.
        self.cancel_weight_flow_stroke(false);
        self.dismiss_on_page_text_edit_without_history();
        self.abort_raster_stroke_uncommitted();
    }

    /// Drop in-progress raster stroke; clear GPU/CPU cache so next paint reloads from `Image.bytes`.
    fn abort_raster_stroke_uncommitted(&mut self) {
        let id = self.tools.raster.target.take();
        self.tools.raster.before_bytes = None;
        self.tools.raster.live_rgba = None;
        self.tools.raster.live_w = 0;
        self.tools.raster.live_h = 0;
        self.tools.raster.painting = false;
        self.tools.raster.dirty = false;
        self.tools.raster.tex_dirty = false;
        self.tools.raster.last_px = None;
        self.tools.raster.stable_px = None;
        self.tools.raster.sample_hist.clear();
        self.tools.raster.spacing_carry = 0.0;
        // Mid-stroke only mutates the texture cache (not node bytes until commit),
        // so dropping the cache restores the committed image on next ensure_*.
        if let Some(id) = id {
            self.image_textures.remove(&id);
            self.image_pixel_cache.remove(&id);
        }
    }

    /// Drop on-page editor without pushing undo history (e.g. after undo/redo).
    fn dismiss_on_page_text_edit_without_history(&mut self) {
        self.restore_text_focus_pan();
        self.on_page_text_edit = None;
        self.on_page_text_before = None;
        self.on_page_text_focus_pending = false;
        self.on_page_text_newly_created = false;
    }

    pub fn set_selection(&mut self, ids: Vec<NodeId>) {
        self.selection = ids;
        self.gradient_editor_focus = crate::gradient_ui::GradientEditorFocus::None;
        self.sync_inspector_from_selection();
    }

    pub fn try_delete_focused_gradient_stop(&mut self) -> bool {
        use crate::document::normalize_stops;
        use crate::gradient_ui::GradientEditorFocus;
        if self.action_tab != ui::ActionTab::ColorStroke {
            return false;
        }
        match self.gradient_editor_focus {
            GradientEditorFocus::Fill if self.ui_fill_stops.len() > 2 => {
                let i = self
                    .ui_fill_stop_sel
                    .min(self.ui_fill_stops.len().saturating_sub(1));
                self.ui_fill_stops.remove(i);
                normalize_stops(&mut self.ui_fill_stops);
                self.ui_fill_stop_sel = self
                    .ui_fill_stop_sel
                    .min(self.ui_fill_stops.len().saturating_sub(1));
                self.apply_fill_to_selection();
                true
            }
            GradientEditorFocus::Stroke if self.ui_stroke_stops.len() > 2 => {
                let i = self
                    .ui_stroke_stop_sel
                    .min(self.ui_stroke_stops.len().saturating_sub(1));
                self.ui_stroke_stops.remove(i);
                normalize_stops(&mut self.ui_stroke_stops);
                self.ui_stroke_stop_sel = self
                    .ui_stroke_stop_sel
                    .min(self.ui_stroke_stops.len().saturating_sub(1));
                self.apply_stroke_to_selection();
                true
            }
            _ => false,
        }
    }

    fn sync_inspector_from_selection(&mut self) {
        if let Some(id) = self.selection.first() {
            if let Some(n) = self.project.nodes.get(*id) {
                if !matches!(n.kind, NodeKind::Path { .. }) {
                    self.tools.select.clear_path_point_selection();
                }
                self.ui_fill_stops = n.style.fill.stops();
                self.ui_fill_stop_sel = 0;
                self.ui_fill_kind = n.style.fill.kind();
                self.ui_gradient_angle = n.style.fill.linear_angle_deg();
                let (lx0, ly0, lx1, ly1) = n.style.fill.linear_line();
                self.ui_fill_line_x0 = lx0;
                self.ui_fill_line_y0 = ly0;
                self.ui_fill_line_x1 = lx1;
                self.ui_fill_line_y1 = ly1;
                if n.style.fill.kind() == FillKind::LinearGradient {
                    let line_angle =
                        crate::document::linear_angle_from_line(lx0, ly0, lx1, ly1);
                    let len = (lx1 - lx0).hypot(ly1 - ly0);
                    if len < 0.2
                        || (line_angle - self.ui_gradient_angle).abs() > 2.0
                            && (lx0 - 0.5).hypot(ly0 - 0.5) < 0.05
                    {
                        let span =
                            crate::document::linear_line_spanning_bbox(self.ui_gradient_angle);
                        self.ui_fill_line_x0 = span.0;
                        self.ui_fill_line_y0 = span.1;
                        self.ui_fill_line_x1 = span.2;
                        self.ui_fill_line_y1 = span.3;
                    }
                }
                let (rcx, rcy) = n.style.fill.radial_center();
                self.ui_radial_cx = rcx;
                self.ui_radial_cy = rcy;
                self.ui_stroke_stops = n.style.stroke.style.stops();
                self.ui_stroke_stop_sel = 0;
                self.ui_stroke_kind = n.style.stroke.style.kind();
                self.ui_stroke_angle = n.style.stroke.style.linear_angle_deg();
                let (sx0, sy0, sx1, sy1) = n.style.stroke.style.linear_line();
                self.ui_stroke_line_x0 = sx0;
                self.ui_stroke_line_y0 = sy0;
                self.ui_stroke_line_x1 = sx1;
                self.ui_stroke_line_y1 = sy1;
                if n.style.stroke.style.kind() == FillKind::LinearGradient {
                    let line_angle =
                        crate::document::linear_angle_from_line(sx0, sy0, sx1, sy1);
                    let len = (sx1 - sx0).hypot(sy1 - sy0);
                    if len < 0.2
                        || (line_angle - self.ui_stroke_angle).abs() > 2.0
                            && (sx0 - 0.5).hypot(sy0 - 0.5) < 0.05
                    {
                        let span =
                            crate::document::linear_line_spanning_bbox(self.ui_stroke_angle);
                        self.ui_stroke_line_x0 = span.0;
                        self.ui_stroke_line_y0 = span.1;
                        self.ui_stroke_line_x1 = span.2;
                        self.ui_stroke_line_y1 = span.3;
                    }
                }
                let (scx, scy) = n.style.stroke.style.radial_center();
                self.ui_stroke_radial_cx = scx;
                self.ui_stroke_radial_cy = scy;
                // Zero-stroke objects should not erase the reusable stroke width for new tools.
                if !matches!(n.kind, NodeKind::BrushStroke { .. }) {
                    if n.style.stroke.width > 0.01 {
                        self.ui_stroke_width = n.style.stroke.width;
                    }
                    self.stroke_enabled = n.style.stroke.width > 0.01;
                }
                self.ui_stroke_line_join = n.style.stroke.line_join;
                self.ui_stroke_line_cap = n.style.stroke.line_cap;
                self.ui_stroke_paint_order = n.style.stroke.paint_order;
                self.ui_marker_start = n.style.stroke.start_marker.clone();
                self.ui_marker_mid = n.style.stroke.mid_marker.clone();
                self.ui_marker_end = n.style.stroke.end_marker.clone();
                self.fill_enabled = n.style.fill.is_visible();
                if let NodeKind::Polygon { sides, .. } = &n.kind {
                    self.polygon_sides = *sides;
                }
                if let NodeKind::Text { style, .. } = &n.kind {
                    self.ui_text_content = style.content.clone();
                    self.ui_text_font_size = style.font_size;
                    self.ui_text_width = style.width;
                    self.ui_text_font_family = style.font_family.clone();
                    self.ui_text_bold = style.bold;
                    self.ui_text_italic = style.italic;
                }
            }
        }
        self.sync_on_path_ui_from_selection();
    }

    pub fn inspector_opacity(&self) -> f32 {
        self.selection
            .first()
            .and_then(|id| self.project.nodes.get(*id))
            .map(|n| n.style.opacity)
            .unwrap_or(1.0)
    }

    pub fn apply_fill_to_selection(&mut self) {
        let new_fill = self.build_ui_fill();
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.fill = new_fill.clone();
            if let NodeKind::Path { path } = &mut after.kind {
                if self.fill_enabled && !path.is_closed() && path.points.len() >= 3 {
                    path.set_closed(true);
                }
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            if let Some(track) = self.project.anim_timeline.nodes.get_mut(&id) {
                track.base_fill = Some(new_fill.clone());
            }
        }
    }

    pub fn reverse_path(&mut self, id: NodeId) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.reverse();
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = "Reversed path".into();
        }
    }

    pub fn set_all_path_anchors_smooth(&mut self, id: NodeId, smooth: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_all_anchors_smooth(smooth);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = if smooth {
                "Smoothed all corners".into()
            } else {
                "Sharpened all corners".into()
            };
        }
    }

    pub fn simplify_path(&mut self, id: NodeId) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.simplify_collinear(0.5);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = "Simplified path".into();
        }
    }

    pub fn set_path_closed(&mut self, id: NodeId, closed: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_closed(closed);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_circle_geometry(&mut self, id: NodeId, cx: f64, cy: f64, radius: f64) {
        self.set_ellipse_geometry(id, cx, cy, radius.max(0.5), radius.max(0.5));
    }

    pub fn set_polygon_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        r: f64,
        sides: u32,
        rotation_deg: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Polygon {
            cx: pcx,
            cy: pcy,
            r: pr,
            sides: ps,
            rotation_rad,
        } = &mut after.kind
        {
            *pcx = cx;
            *pcy = cy;
            *pr = r.max(1.0);
            *ps = sides.max(3);
            *rotation_rad = rotation_deg.to_radians();
            after.transform.rotation_rad = *rotation_rad;
            after.name = format!("Polygon ({})", *ps);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn build_ui_fill(&self) -> Fill {
        Fill::build(
            self.ui_fill_kind,
            self.fill_enabled,
            &self.ui_fill_stops,
            self.ui_gradient_angle,
            self.ui_fill_line_x0,
            self.ui_fill_line_y0,
            self.ui_fill_line_x1,
            self.ui_fill_line_y1,
            self.ui_radial_cx,
            self.ui_radial_cy,
        )
    }

    pub fn build_ui_stroke(&self) -> Stroke {
        Stroke {
            style: Fill::build(
                self.ui_stroke_kind,
                self.stroke_enabled,
                &self.ui_stroke_stops,
                self.ui_stroke_angle,
                self.ui_stroke_line_x0,
                self.ui_stroke_line_y0,
                self.ui_stroke_line_x1,
                self.ui_stroke_line_y1,
                self.ui_stroke_radial_cx,
                self.ui_stroke_radial_cy,
            ),
            width: if self.stroke_enabled {
                self.ui_stroke_width.max(0.5)
            } else {
                0.0
            },
            line_join: self.ui_stroke_line_join,
            line_cap: self.ui_stroke_line_cap,
            paint_order: self.ui_stroke_paint_order,
            start_marker: self.ui_marker_start.clone(),
            mid_marker: self.ui_marker_mid.clone(),
            end_marker: self.ui_marker_end.clone(),
        }
    }

    pub fn build_brush_fill(&self) -> Fill {
        Fill::build(
            self.tools.brush.fill_kind,
            true,
            &self.tools.brush.fill_stops,
            self.tools.brush.gradient_angle,
            self.tools.brush.fill_line_x0,
            self.tools.brush.fill_line_y0,
            self.tools.brush.fill_line_x1,
            self.tools.brush.fill_line_y1,
            self.tools.brush.radial_cx,
            self.tools.brush.radial_cy,
        )
    }

    pub fn apply_stroke_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            let ui = self.build_ui_stroke();
            after.style.stroke.style = ui.style;
            after.style.stroke.width = ui.width;
            after.style.stroke.line_join = ui.line_join;
            after.style.stroke.line_cap = ui.line_cap;
            after.style.stroke.paint_order = ui.paint_order;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn apply_path_markers_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke.start_marker = self.ui_marker_start.clone();
            after.style.stroke.mid_marker = self.ui_marker_mid.clone();
            after.style.stroke.end_marker = self.ui_marker_end.clone();
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn apply_stroke_width_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke.width = self.ui_stroke_width;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn apply_no_stroke_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke.width = 0.0;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_selection_opacity(&mut self, opacity: f32) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.opacity = opacity;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn rename_node(&mut self, id: NodeId, name: String) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        after.name = name;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_rect_geometry(
        &mut self,
        id: NodeId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        rx: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Rect {
            x: rx0,
            y: ry0,
            w: rw,
            h: rh,
            rx: rrx,
        } = &mut after.kind
        {
            *rx0 = x;
            *ry0 = y;
            *rw = w.max(1.0);
            *rh = h.max(1.0);
            *rrx = rx.max(0.0);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.sync_anim_transform_from_node(id);
    }

    pub fn set_ellipse_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Ellipse {
            cx: ecx,
            cy: ecy,
            rx: erx,
            ry: ery,
        } = &mut after.kind
        {
            *ecx = cx;
            *ecy = cy;
            *erx = rx.max(0.5);
            *ery = ry.max(0.5);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.sync_anim_transform_from_node(id);
    }

    pub fn set_line_geometry(
        &mut self,
        id: NodeId,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.points = vec![[x0, y0], [x1, y1]];
            path.verbs = vec![0, 1];
            path.closed = false;
            path.smooth_anchors.clear();
            path.handle_out_offset.clear();
            path.handle_in_offset.clear();
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    /// Move a path so its first anchor sits at `(ox, oy)` (Geometry origin for open/closed paths).
    pub fn set_path_origin(&mut self, id: NodeId, ox: f64, oy: f64) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        let (cx, cy) = match &after.kind {
            NodeKind::Path { path } if !path.points.is_empty() => {
                (path.points[0][0], path.points[0][1])
            }
            NodeKind::FlowchartPath { path } if !path.points.is_empty() => {
                (path.points[0].0, path.points[0].1)
            }
            _ => return,
        };
        let dx = ox - cx;
        let dy = oy - cy;
        if dx.abs() < 1e-12 && dy.abs() < 1e-12 {
            return;
        }
        after.translate(dx, dy);
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.sync_anim_transform_from_node(id);
    }

    pub fn set_plotter_geometry(
        &mut self,
        id: NodeId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        expr: String,
        ref_axis: crate::document::PlotterRef,
        domain_min: f64,
        domain_max: f64,
        range_min: f64,
        range_max: f64,
        auto_range: bool,
        margin_pct: f64,
        plot_stroke_width: f32,
        plot_stroke_rgba: [f32; 4],
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Plotter {
            x: px,
            y: py,
            w: pw,
            h: ph,
            expr: pexpr,
            ref_axis: pra,
            domain_min: pd0,
            domain_max: pd1,
            range_min: pr0,
            range_max: pr1,
            auto_range: pauto,
            margin_pct: pm,
            plot_stroke_width: psw,
            plot_stroke_rgba: pcol,
            ..
        } = &mut after.kind
        {
            *px = x;
            *py = y;
            *pw = w.max(1.0);
            *ph = h.max(1.0);
            *pexpr = expr;
            *pra = ref_axis;
            *pd0 = domain_min;
            *pd1 = domain_max;
            *pr0 = range_min;
            *pr1 = range_max;
            *pauto = auto_range;
            *pm = margin_pct.clamp(0.0, 200.0);
            *psw = plot_stroke_width.max(0.0);
            *pcol = plot_stroke_rgba;
        } else {
            return;
        }
        if before == after {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.sync_anim_transform_from_node(id);
    }

    /// Live-update plotter expression (no history) so the curve redraws while typing.
    pub fn set_plotter_expr_live(&mut self, id: NodeId, expr: String) {
        let Some(node) = self.project.nodes.get_mut(id) else {
            return;
        };
        if let NodeKind::Plotter { expr: pe, .. } = &mut node.kind {
            if *pe != expr {
                *pe = expr;
            }
        }
    }

    /// Begin expression edit undo snapshot if not already open for this node.
    pub fn begin_plotter_expr_edit(&mut self, id: NodeId) {
        if matches!(self.plotter_expr_edit_before.as_ref(), Some((nid, _)) if *nid == id) {
            return;
        }
        if let Some(node) = self.project.nodes.get(id).cloned() {
            self.plotter_expr_edit_before = Some((id, node));
        }
    }

    /// Commit one undo step for expression edits since [`begin_plotter_expr_edit`].
    pub fn commit_plotter_expr_edit(&mut self, id: NodeId) {
        let Some((bid, before)) = self.plotter_expr_edit_before.take() else {
            return;
        };
        if bid != id {
            self.plotter_expr_edit_before = Some((bid, before));
            return;
        }
        let Some(after) = self.project.nodes.get(id).cloned() else {
            return;
        };
        if before == after {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    /// Cancel live expression edit and restore pre-edit node (e.g. dialog Cancel).
    pub fn cancel_plotter_expr_edit(&mut self, id: NodeId) {
        if let Some((bid, before)) = self.plotter_expr_edit_before.take() {
            if bid == id {
                if let Some(n) = self.project.nodes.get_mut(id) {
                    *n = before;
                }
                return;
            }
            self.plotter_expr_edit_before = Some((bid, before));
        }
    }

    pub fn set_arc_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle_deg: f64,
        sweep_angle_deg: f64,
        join: crate::document::ArcJoin,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Arc {
            cx: acx,
            cy: acy,
            radius: ar,
            start_angle_rad,
            sweep_angle_rad,
            join: ajoin,
        } = &mut after.kind
        {
            *acx = cx;
            *acy = cy;
            *ar = radius.max(0.5);
            *start_angle_rad = start_angle_deg.to_radians();
            *sweep_angle_rad = sweep_angle_deg.to_radians();
            *ajoin = join;
            after.name = match join {
                crate::document::ArcJoin::NoJoin => "Arc".into(),
                crate::document::ArcJoin::Chord => "Chord".into(),
                crate::document::ArcJoin::ToOrigin => "Pie".into(),
            };
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_flowchart_node_label(
        &mut self,
        id: crate::document::NodeId,
        label: String,
        label_font_size: f64,
        label_align: crate::document::TextAlign,
        label_font_family: String,
        label_bold: bool,
        label_italic: bool,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let crate::document::NodeKind::FlowchartNode {
            label: l,
            label_font_size: fs,
            label_align: al,
            label_font_family: fam,
            label_bold: b,
            label_italic: i,
            ..
        } = &mut after.kind
        {
            *l = label;
            *fs = label_font_size;
            *al = label_align;
            *fam = label_font_family;
            *b = label_bold;
            *i = label_italic;
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_flowchart_path_props(
        &mut self,
        id: crate::document::NodeId,
        corner_radius: f64,
        endpoint_marker_size: f32,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let crate::document::NodeKind::FlowchartPath { path } = &mut after.kind {
            path.corner_radius = corner_radius;
            path.endpoint_marker_size = endpoint_marker_size;
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_document_title(&mut self, title: String) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.title = title;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn set_page_size(&mut self, width: f64, height: f64) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.width = width;
        after.height = height;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    /// Highest content frame (keyframes / AV) — does **not** grow with the playhead.
    /// Used for loop length so scrubbing/play doesn't expand the span mid-play.
    pub fn get_content_max_animation_frame(&self) -> usize {
        let mut max_f = 0usize;
        // Only living nodes/layers — deleted objects leave orphan tracks otherwise.
        for (id, anim) in &self.project.anim_timeline.nodes {
            if !self.project.owns_animation_id(*id) {
                continue;
            }
            let tracks = [
                &anim.pos_x, &anim.pos_y, &anim.rotation, &anim.opacity,
                &anim.color_r, &anim.color_g, &anim.color_b, &anim.color_a,
                &anim.stroke_width, &anim.stroke_r, &anim.stroke_g, &anim.stroke_b, &anim.stroke_a,
            ];
            for t in tracks {
                if let Some(last) = t.keyframes.last() {
                    max_f = max_f.max(last.frame);
                }
            }
            for gt in &anim.geom_tracks {
                if let Some(last) = gt.keyframes.last() {
                    max_f = max_f.max(last.frame);
                }
            }
            for pt in anim.param_tracks.values() {
                if let Some(last) = pt.keyframes.last() {
                    max_f = max_f.max(last.frame);
                }
            }
            // Stack animation spans must be reachable by playback.
            for sf in &anim.stack_functions {
                max_f = max_f.max(sf.end_frame());
            }
        }

        let fps = self.anim_fps.max(1) as f32;
        // Node Editor Output Object `run_till` (seconds) extends / caps the timeline.
        for l in &self.project.document.layers {
            if l.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            if let Some(g) = l.node_graph.as_ref() {
                let run_till = g.output_run_till_secs();
                if run_till > 1e-6 {
                    let end_frame = (run_till as f32 * fps).ceil().max(0.0) as usize;
                    max_f = max_f.max(end_frame);
                }
            }
        }
        for l in &self.project.document.layers {
            if l.kind != crate::document::LayerKind::AV {
                continue;
            }
            // Include the full media queue (not only legacy primary path).
            if !l.av_clips.is_empty() || !l.music_clips.is_empty() {
                for c in &l.av_clips {
                    let end_frame = (c.timeline_end_secs() * fps).ceil().max(0.0) as usize;
                    max_f = max_f.max(end_frame);
                }
                for m in &l.music_clips {
                    let end_frame = (m.end_sec() * fps).ceil().max(0.0) as usize;
                    max_f = max_f.max(end_frame);
                }
            } else if !l.video_path.is_empty() {
                let end_frame = (l.timeline_end_secs() * fps).ceil().max(0.0) as usize;
                max_f = max_f.max(end_frame);
            }
        }

        // Empty project: short scrub room only (NOT 300 — that made play loop to 300).
        if max_f == 0 {
            max_f = 60; // ~1s @60fps / 2s @30fps default
        }
        max_f
    }

    pub fn get_max_animation_frame(&self) -> usize {
        // Timeline end = content only. Do not grow with playhead scrub (that
        // stretched loops to 300 when the user dragged past the last keyframe).
        self.get_content_max_animation_frame()
    }

    /// Drop animation tracks for nodes/layers that no longer exist.
    pub fn prune_orphan_animation_tracks(&mut self) -> usize {
        self.project.prune_orphan_animation_tracks()
    }

    pub fn animation_content_duration_secs(&self) -> f32 {
        let max_frame = self.get_max_animation_frame();
        (max_frame + 1) as f32 / self.anim_fps.max(1) as f32
    }

    pub fn cache_current_project_for_open(&mut self) {
        let label = self.project.document.title.clone();
        self.cached_project = Some(crate::history::snapshot_project(&self.project));
        self.cached_project_label = Some(label);
        let cache_path = std::env::temp_dir().join(".vadadee-berry-open-cache.vadadee-berry.json");
        if let Err(e) = crate::io::save_project(&cache_path, self.cached_project.as_ref().unwrap()) {
            log::warn!("Could not write project open cache: {e}");
        }
    }

    /// When the user moves/edits an object that already has position (or rotation) keyframes,
    /// keep those keyframes in sync so the next `apply_animation_for_frame` does not snap back.
    /// At the start of the timeline (frame ≤ first keyframe), the first keyframe is updated.
    pub fn sync_anim_transform_from_node(&mut self, id: NodeId) {
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let (x, y) = node.get_pos();
        let rot = node.get_rotation();
        let frame = self.anim_current_frame;
        let Some(entry) = self.project.anim_timeline.nodes.get_mut(&id) else {
            return;
        };
        if !entry.pos_x.keyframes.is_empty() {
            Self::write_anim_keyframe_at_edit(&mut entry.pos_x, frame, x);
        }
        if !entry.pos_y.keyframes.is_empty() {
            Self::write_anim_keyframe_at_edit(&mut entry.pos_y, frame, y);
        }
        if !entry.rotation.keyframes.is_empty() {
            Self::write_anim_keyframe_at_edit(&mut entry.rotation, frame, rot);
        }
    }

    fn write_anim_keyframe_at_edit(track: &mut KeyframeTrack, frame: usize, value: f64) {
        if track.keyframes.is_empty() {
            return;
        }
        if track.keyframes.iter().any(|k| k.frame == frame) {
            track.insert(frame, value);
            return;
        }
        // Hold-before-first: editing at the beginning updates the first keyframe
        // (otherwise interpolate(frame) keeps returning the old first value forever).
        let first = track.keyframes[0].frame;
        if frame <= first {
            track.insert(first, value);
            return;
        }
        let last = track.keyframes[track.keyframes.len() - 1].frame;
        if frame >= last {
            track.insert(last, value);
            return;
        }
        // Between keys: insert a new key at the scrubbed frame.
        track.insert(frame, value);
    }

    pub fn apply_animation_for_frame(&mut self, frame: usize) {
        // Ignore (and drop) tracks whose object no longer exists.
        let _ = self.project.prune_orphan_animation_tracks();
        // Collect node ids first so we can mutably sample stack functions (records formula errors).
        let node_ids: Vec<NodeId> = self.project.anim_timeline.nodes.keys().copied().collect();
        let mut updates: Vec<(
            NodeId,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f32>,
            Option<[f32; 4]>,
            Option<f32>,
            Option<[f32; 4]>,
            Option<Vec<f64>>,
        )> = Vec::with_capacity(node_ids.len());
        for node_id in node_ids {
            let Some(track) = self.project.anim_timeline.nodes.get_mut(&node_id) else {
                continue;
            };
            let x = track.sample_mut("pos_x", frame);
            let y = track.sample_mut("pos_y", frame);
            let rot = track.sample_mut("rotation", frame);
            let opacity = track.sample_mut("opacity", frame).map(|o| o as f32);
            let r = track.sample_mut("color_r", frame);
            let g = track.sample_mut("color_g", frame);
            let b = track.sample_mut("color_b", frame);
            let a = track.sample_mut("color_a", frame);
            let color = if let (Some(r), Some(g), Some(b), Some(a)) = (r, g, b, a) {
                Some([r as f32, g as f32, b as f32, a as f32])
            } else {
                None
            };
            let stroke_w = track.sample_mut("stroke_width", frame).map(|w| w as f32);
            let sr = track.sample_mut("stroke_r", frame);
            let sg = track.sample_mut("stroke_g", frame);
            let sb = track.sample_mut("stroke_b", frame);
            let sa = track.sample_mut("stroke_a", frame);
            let stroke_color = if let (Some(r), Some(g), Some(b), Some(a)) = (sr, sg, sb, sa) {
                Some([r as f32, g as f32, b as f32, a as f32])
            } else {
                None
            };
            // Skip geom apply when no geom keyframes exist (avoids rewriting heavy paths every frame).
            let geom = if track.geom_tracks.iter().any(|t| !t.keyframes.is_empty()) {
                let current_geom = self
                    .project
                    .nodes
                    .get(node_id)
                    .map(|n| n.get_geom_floats())
                    .unwrap_or_default();
                let mut g_vals = Vec::with_capacity(track.geom_tracks.len().max(current_geom.len()));
                let n = track.geom_tracks.len().max(current_geom.len());
                for idx in 0..n {
                    let def_val = current_geom.get(idx).copied().unwrap_or(0.0);
                    if idx < track.geom_tracks.len() {
                        let lbl = format!("geom_{idx}");
                        g_vals.push(track.sample_mut(&lbl, frame).unwrap_or(def_val));
                    } else {
                        g_vals.push(def_val);
                    }
                }
                Some(g_vals)
            } else {
                None
            };
            updates.push((node_id, x, y, rot, opacity, color, stroke_w, stroke_color, geom));
        }

        for (
            node_id,
            target_x,
            target_y,
            target_rot,
            target_op,
            target_color,
            target_stroke_w,
            target_stroke_col,
            target_geom,
        ) in updates
        {
            if let Some(node) = self.project.nodes.get_mut(node_id) {
                // Apply position
                let (curr_x, curr_y) = node.get_pos();
                let dx = target_x.map(|tx| tx - curr_x).unwrap_or(0.0);
                let dy = target_y.map(|ty| ty - curr_y).unwrap_or(0.0);
                if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
                    node.translate(dx, dy);
                }
                
                // Apply rotation
                if let Some(rot) = target_rot {
                    node.set_rotation(rot);
                }
                
                // Apply opacity
                if let Some(op) = target_op {
                    node.set_opacity(op);
                }
                
                // Apply fill color
                if let Some(color) = target_color {
                    let mut base_fill = self.project.anim_timeline.nodes.get(&node_id)
                        .and_then(|track| track.base_fill.clone());
                    
                    if base_fill.is_none() {
                        base_fill = Some(node.style.fill.clone());
                        if let Some(track) = self.project.anim_timeline.nodes.get_mut(&node_id) {
                            track.base_fill = base_fill.clone();
                        }
                    }

                    if let Some(mut bf) = base_fill {
                        match &mut bf {
                            Fill::Solid(paint) => {
                                paint.rgba = color;
                                node.style.fill = Fill::Solid(*paint);
                            }
                            Fill::LinearGradient { stops, .. } |
                            Fill::RadialGradient { stops, .. } => {
                                for stop in stops {
                                    stop.color.rgba = [
                                        stop.color.rgba[0] * color[0],
                                        stop.color.rgba[1] * color[1],
                                        stop.color.rgba[2] * color[2],
                                        stop.color.rgba[3] * color[3],
                                    ];
                                }
                                node.style.fill = bf;
                            }
                            Fill::None => {}
                        }
                    } else {
                        node.set_color(color);
                    }
                }

                // Apply stroke width
                if let Some(sw) = target_stroke_w {
                    node.set_stroke_width(sw);
                }

                // Apply stroke color
                if let Some(color) = target_stroke_col {
                    let mut base_stroke = self
                        .project
                        .anim_timeline
                        .nodes
                        .get(&node_id)
                        .and_then(|track| track.base_stroke.clone());
                    if base_stroke.is_none() {
                        base_stroke = Some(node.style.stroke.style.clone());
                        if let Some(track) = self.project.anim_timeline.nodes.get_mut(&node_id) {
                            track.base_stroke = base_stroke.clone();
                        }
                    }
                    if let Some(mut bs) = base_stroke {
                        match &mut bs {
                            Fill::Solid(paint) => {
                                paint.rgba = color;
                                node.style.stroke.style = Fill::Solid(*paint);
                            }
                            Fill::LinearGradient { stops, .. }
                            | Fill::RadialGradient { stops, .. } => {
                                for stop in stops {
                                    stop.color.rgba = [
                                        stop.color.rgba[0] * color[0],
                                        stop.color.rgba[1] * color[1],
                                        stop.color.rgba[2] * color[2],
                                        stop.color.rgba[3] * color[3],
                                    ];
                                }
                                node.style.stroke.style = bs;
                            }
                            Fill::None => {
                                node.set_stroke_color(color);
                            }
                        }
                    } else {
                        node.set_stroke_color(color);
                    }
                }

                // Apply geometry
                if let Some(geom) = target_geom {
                    self.set_node_geom_floats(node_id, &geom);
                }
            } else if let Some(layer) = self.project.document.layers.iter_mut().find(|l| l.id == node_id && l.kind == crate::document::LayerKind::AV) {
                if let Some(x) = target_x {
                    layer.x = x as f32;
                }
                if let Some(y) = target_y {
                    layer.y = y as f32;
                }
                if let Some(rot) = target_rot {
                    layer.rotation = rot as f32;
                }
            }
        }

        // Node Editor graph parameters (layer id → param_* tracks).
        self.apply_node_editor_param_animation(frame);
    }

    /// Sample `param:{uuid}` tracks into GraphParam values for Node Editor layers.
    fn apply_node_editor_param_animation(&mut self, frame: usize) {
        let layer_ids: Vec<uuid::Uuid> = self
            .project
            .document
            .layers
            .iter()
            .filter(|l| l.kind == crate::document::LayerKind::NodeEditor)
            .map(|l| l.id)
            .collect();
        for layer_id in layer_ids {
            let Some(anim) = self.project.anim_timeline.nodes.get(&layer_id) else {
                continue;
            };
            // Collect (param_id, component, value) samples without holding graph mut.
            let mut samples: Vec<(uuid::Uuid, Option<usize>, f64)> = Vec::new();
            let param_meta: Vec<(uuid::Uuid, crate::document::GraphParamKind, f64, f64, f64, f64)> = {
                let Some(layer) = self
                    .project
                    .document
                    .layers
                    .iter()
                    .find(|l| l.id == layer_id)
                else {
                    continue;
                };
                let Some(g) = layer.node_graph.as_ref() else {
                    continue;
                };
                g.parameters
                    .iter()
                    .map(|p| (p.id, p.kind, p.v0, p.v1, p.v2, p.v3))
                    .collect()
            };
            for (pid, kind, v0, v1, v2, v3) in param_meta {
                let labels = match kind {
                    crate::document::GraphParamKind::Real => {
                        vec![(format!("param:{pid}"), None, v0)]
                    }
                    crate::document::GraphParamKind::Color => vec![
                        (format!("param:{pid}:0"), Some(0usize), v0),
                        (format!("param:{pid}:1"), Some(1), v1),
                        (format!("param:{pid}:2"), Some(2), v2),
                        (format!("param:{pid}:3"), Some(3), v3),
                    ],
                    crate::document::GraphParamKind::Position => vec![
                        (format!("param:{pid}:0"), Some(0usize), v0),
                        (format!("param:{pid}:1"), Some(1), v1),
                    ],
                };
                for (lbl, comp, def) in labels {
                    if let Some(v) = anim.sample(&lbl, frame) {
                        samples.push((pid, comp, v));
                    } else if anim.get_track(&lbl).is_some_and(|t| !t.keyframes.is_empty()) {
                        // sample returned None only if empty; keep default
                        let _ = def;
                    }
                }
            }
            if samples.is_empty() {
                continue;
            }
            let Some(layer) = self
                .project
                .document
                .layers
                .iter_mut()
                .find(|l| l.id == layer_id)
            else {
                continue;
            };
            let Some(g) = layer.node_graph.as_mut() else {
                continue;
            };
            for (pid, comp, v) in samples {
                if let Some(p) = g.parameters.iter_mut().find(|p| p.id == pid) {
                    match comp {
                        None => p.v0 = v,
                        Some(0) => p.v0 = v,
                        Some(1) => p.v1 = v,
                        Some(2) => p.v2 = v,
                        Some(3) => p.v3 = v,
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn get_node_geom_floats(&self, id: NodeId) -> Vec<f64> {
        let mut v = if let Some(node) = self.project.nodes.get(id) {
            node.get_geom_floats()
        } else {
            return Vec::new();
        };

        if let Some(tiling) = self.project.document.tiling_effects.values().find(|e| e.source_id == id) {
            v.push(tiling.gap_x);
            v.push(tiling.gap_y);
            v.push(tiling.count_x as f64);
            v.push(tiling.count_y as f64);
            v.push(tiling.offset_x);
            v.push(tiling.offset_y);
            v.push(tiling.row_rotation);
            v.push(tiling.col_rotation);
            v.push(tiling.row_scale);
            v.push(tiling.col_scale);
        }

        if let Some(circ) = self.project.document.circular_effects.values().find(|e| e.source_id == id) {
            v.push(circ.origin_x);
            v.push(circ.origin_y);
            v.push(circ.radius);
            v.push(circ.copies as f64);
            v.push(circ.angle_offset);
            v.push(circ.base_x);
            v.push(circ.base_y);
        }

        if let Some(oop) = self.project.document.path_effects.values().find(|e| e.source_id == id) {
            v.push(oop.gap);
            v.push(oop.count as f64);
            v.push(oop.start_offset);
            v.push(oop.loft_end_scale as f64);
            v.push(oop.loft_end_opacity as f64);
        }

        v
    }

    pub fn set_node_geom_floats(&mut self, id: NodeId, floats: &[f64]) {
        let base_len = if let Some(node) = self.project.nodes.get(id) {
            node.get_geom_floats().len()
        } else {
            0
        };

        if base_len > 0 && floats.len() >= base_len {
            if let Some(node) = self.project.nodes.get_mut(id) {
                node.set_geom_floats(&floats[..base_len]);
            }
        }

        let mut idx = base_len;

        let mut has_tiling = false;
        if let Some(tiling_id) = self.project.document.tiling_effects.values()
            .find(|e| e.source_id == id)
            .map(|e| e.id)
        {
            if floats.len() >= idx + 10 {
                if let Some(tiling) = self.project.document.tiling_effects.get_mut(&tiling_id) {
                    tiling.gap_x = floats[idx];
                    tiling.gap_y = floats[idx + 1];
                    tiling.count_x = floats[idx + 2].round().max(1.0) as usize;
                    tiling.count_y = floats[idx + 3].round().max(1.0) as usize;
                    tiling.offset_x = floats[idx + 4];
                    tiling.offset_y = floats[idx + 5];
                    tiling.row_rotation = floats[idx + 6];
                    tiling.col_rotation = floats[idx + 7];
                    tiling.row_scale = floats[idx + 8];
                    tiling.col_scale = floats[idx + 9];
                    has_tiling = true;
                }
                idx += 10;
            }
        }

        let mut has_circular = false;
        if let Some(circ_id) = self.project.document.circular_effects.values()
            .find(|e| e.source_id == id)
            .map(|e| e.id)
        {
            if floats.len() >= idx + 7 {
                if let Some(circ) = self.project.document.circular_effects.get_mut(&circ_id) {
                    circ.origin_x = floats[idx];
                    circ.origin_y = floats[idx + 1];
                    circ.radius = floats[idx + 2];
                    circ.copies = floats[idx + 3].round().max(1.0) as usize;
                    circ.angle_offset = floats[idx + 4];
                    circ.base_x = floats[idx + 5];
                    circ.base_y = floats[idx + 6];
                    has_circular = true;
                }
                idx += 7;
            }
        }

        let mut has_oop = false;
        if let Some(oop_id) = self.project.document.path_effects.values()
            .find(|e| e.source_id == id)
            .map(|e| e.id)
        {
            if floats.len() >= idx + 5 {
                if let Some(oop) = self.project.document.path_effects.get_mut(&oop_id) {
                    oop.gap = floats[idx];
                    oop.count = floats[idx + 1].round().max(1.0) as usize;
                    oop.start_offset = floats[idx + 2];
                    oop.loft_end_scale = floats[idx + 3] as f32;
                    oop.loft_end_opacity = floats[idx + 4] as f32;
                    has_oop = true;
                }
                idx += 5;
            }
        }

        if has_tiling {
            self.sync_tiling_ui_from_selection();
        }
        if has_circular {
            self.sync_circular_ui_from_selection();
        }
        if has_oop {
            self.sync_on_path_ui_from_selection();
        }
    }

    pub fn convert_rect_to_path(&mut self, id: NodeId) {
        let Some(node) = self.project.nodes.get_mut(id) else {
            return;
        };
        
        let (x, y, w, h) = match &node.kind {
            NodeKind::Rect { x, y, w, h, .. } => (*x, *y, *w, *h),
            _ => return, // Not a rect
        };
        
        // Convert the NodeKind to Path
        let corners = [
            (x, y),
            (x + w, y),
            (x + w, y + h),
            (x, y + h),
        ];
        let path = crate::document::PathData::from_anchor_data(
            &corners,
            &[],
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            true,
        );
        node.kind = NodeKind::Path { path };
        
        // Now convert its timeline tracks in self.anim_timeline
        if let Some(entry) = self.project.anim_timeline.nodes.get_mut(&id) {
            // We need to convert geom_tracks from Rect (3 tracks) to Path (24 tracks)
            let mut frames = std::collections::BTreeSet::new();
            for t in &entry.geom_tracks {
                for kf in &t.keyframes {
                    frames.insert(kf.frame);
                }
            }
            
            // Create 24 empty tracks for Path geometry
            let mut new_geom_tracks = vec![KeyframeTrack::default(); 24];
            
            // For each keyframe frame, calculate the 24 path geometry values from the interpolated rect values at that frame
            for f in frames {
                let w_val = entry.geom_tracks.get(0).and_then(|t| t.interpolate(f)).unwrap_or(w);
                let h_val = entry.geom_tracks.get(1).and_then(|t| t.interpolate(f)).unwrap_or(h);
                
                let c = [
                    (x, y),
                    (x + w_val, y),
                    (x + w_val, y + h_val),
                    (x, y + h_val),
                ];
                
                for i in 0..4 {
                    let base = i * 6;
                    new_geom_tracks[base].insert(f, c[i].0);
                    new_geom_tracks[base + 1].insert(f, c[i].1);
                    new_geom_tracks[base + 2].insert(f, 0.0);
                    new_geom_tracks[base + 3].insert(f, 0.0);
                    new_geom_tracks[base + 4].insert(f, 0.0);
                    new_geom_tracks[base + 5].insert(f, 0.0);
                }
            }
            
            entry.geom_tracks = new_geom_tracks;
        }
        
        // Also update anim_last_applied_states if it exists, to match the new geom_floats format/length
        if let Some(last) = self.anim_last_applied_states.get_mut(&id) {
            let gf = node.get_geom_floats();
            last.geom_floats = gf;
        }
    }

    pub fn toggle_keyframing_mode(&mut self) {
        self.anim_keyframing_mode = !self.anim_keyframing_mode;
        if self.anim_keyframing_mode {
            // Seed baselines so the first path-point move is detected against pre-edit geom.
            self.anim_last_applied_states.clear();
            for id in self.selection.clone() {
                if let Some(node) = self.project.nodes.get(id) {
                    let gf = self.get_node_geom_floats(id);
                    self.anim_last_applied_states.insert(
                        id,
                        AnimAppliedState {
                            pos: node.get_pos(),
                            rotation: node.get_rotation(),
                            opacity: node.get_opacity(),
                            color: node.get_color(),
                            stroke_width: node.get_stroke_width(),
                            stroke_color: node.get_stroke_color(),
                            geom_floats: gf,
                            fill: node.style.fill.clone(),
                        },
                    );
                }
            }
            self.status_message = format!(
                "Recording keyframes (Frame {}) — move object or path points",
                self.anim_current_frame
            );
        } else {
            self.status_message = "Keyframing stopped".into();
        }
    }

    pub fn add_layer(&mut self, name: &str) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_layer(name);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn add_empty_av_layer(&mut self, name: &str) {
        self.add_empty_av_layer_with_role(name, crate::document::AvRole::Video);
    }

    pub fn add_empty_av_layer_with_role(&mut self, name: &str, role: crate::document::AvRole) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_empty_av_layer_with_role(name, role);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn add_shading_layer(&mut self, name: &str) {
        self.add_shading_layer_with_preset(name, "blackhole");
    }

    pub fn add_flowchart_layer(&mut self, name: &str) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_flowchart_layer(name);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn add_node_editor_layer(&mut self, name: &str) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_node_editor_layer(name);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        // P6b: create Output proxy Image so the object is immediately selectable.
        if let Some(layer) = self.project.document.layers.get_mut(idx) {
            let _ = layer.ensure_ne_output_proxy(&mut self.project.nodes);
            let lid = layer.id;
            let proxy = layer.ne_output_proxy;
            self.selection = vec![proxy.unwrap_or(lid)];
            self.node_editor_ui.open(lid);
            // Geometry: Output is a canvas object (order/transform), not a graph Parameter.
            crate::ui::promote_action_tab(self, crate::ui::ActionTab::Geometry);
            self.action_tab = crate::ui::ActionTab::Geometry;
        }
        self.status_message = "Node Editor layer created".into();
    }

    pub fn add_screen_record_layer(&mut self, name: &str) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_screen_record_layer(name);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        if let Some(layer) = self.project.document.layers.get(idx) {
            self.selection = vec![layer.id];
        }
        self.status_message =
            "Screen Record layer — press Record (needs ffmpeg + DISPLAY)".into();
    }

    /// Start OS screen capture for a Screen Record layer.
    #[cfg(not(target_os = "android"))]
    pub fn start_screen_record(&mut self, layer_idx: usize) {
        let Some(layer) = self.project.document.layers.get(layer_idx).cloned() else {
            return;
        };
        if layer.kind != crate::document::LayerKind::ScreenRecord {
            self.status_message = "Not a Screen Record layer".into();
            return;
        }
        if self.screen_captures.contains_key(&layer.id) {
            self.status_message = "Already recording this layer".into();
            return;
        }
        // Prefer capture_dir (folder); fall back to parent of last septic file / default cache.
        let dir_hint = if !layer.capture_dir.trim().is_empty() {
            layer.capture_dir.clone()
        } else if !layer.septic_path.trim().is_empty() {
            let p = std::path::Path::new(layer.septic_path.trim());
            if p.is_dir() {
                layer.septic_path.clone()
            } else {
                p.parent()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }
        } else {
            String::new()
        };
        let sepscrr =
            crate::screen_capture::resolve_sepscrr_for_record(&dir_hint, &layer.name);
        match crate::screen_capture::ScreenCaptureSession::start(
            crate::screen_capture::ScreenCaptureStart {
                layer_id: layer.id,
                sepscrr_path: sepscrr.clone(),
                capture_cursor: layer.capture_cursor,
                capture_audio: layer.capture_audio,
                fps: layer.capture_fps.max(1).min(120),
                bitrate_kbps: layer.capture_bitrate_kbps,
            },
        ) {
            Ok(session) => {
                if let Some(l) = self.project.document.layers.get_mut(layer_idx) {
                    l.septic_path = sepscrr.to_string_lossy().into_owned();
                    if l.capture_dir.trim().is_empty() {
                        if let Some(parent) = sepscrr.parent() {
                            l.capture_dir = parent.to_string_lossy().into_owned();
                        }
                    }
                    l.screen_recording = true;
                }
                self.screen_captures.insert(layer.id, session);
                self.status_message = format!(
                    "Recording screen → {}",
                    sepscrr.file_name().and_then(|s| s.to_str()).unwrap_or("…")
                );
            }
            Err(e) => {
                self.status_message = status_one_line(&format!("Record failed: {e}"));
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn start_screen_record(&mut self, _layer_idx: usize) {
        self.status_message = "Screen record not available on Android".into();
    }

    /// Stop capture and write `.sepscrr` + sibling `.mp4`.
    #[cfg(not(target_os = "android"))]
    pub fn stop_screen_record(&mut self, layer_idx: usize) {
        let Some(layer) = self.project.document.layers.get(layer_idx) else {
            return;
        };
        let id = layer.id;
        let Some(session) = self.screen_captures.remove(&id) else {
            if let Some(l) = self.project.document.layers.get_mut(layer_idx) {
                l.screen_recording = false;
            }
            self.status_message = "Not recording".into();
            return;
        };
        let elapsed = session.elapsed_sec();
        let n = session.sample_count();
        match session.stop() {
            Ok(path) => {
                if let Some(l) = self.project.document.layers.get_mut(layer_idx) {
                    l.septic_path = path.to_string_lossy().into_owned();
                    l.screen_recording = false;
                    l.media_source_duration = Some(elapsed as f32);
                }
                self.status_message = status_one_line(&format!(
                    "Saved {:.1}s · {n} mouse → {}",
                    elapsed,
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("sepscrr")
                ));
            }
            Err(e) => {
                if let Some(l) = self.project.document.layers.get_mut(layer_idx) {
                    l.screen_recording = false;
                }
                self.status_message = status_one_line(&format!("Stop failed: {e}"));
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn stop_screen_record(&mut self, _layer_idx: usize) {
        self.status_message = "Screen record not available on Android".into();
    }

    /// True if this layer id has an active capture.
    pub fn is_screen_recording(&self, layer_id: uuid::Uuid) -> bool {
        #[cfg(not(target_os = "android"))]
        {
            self.screen_captures.contains_key(&layer_id)
        }
        #[cfg(target_os = "android")]
        {
            let _ = layer_id;
            false
        }
    }

    pub fn add_graph_node_to_active(&mut self, kind: crate::document::GraphNodeKind) {
        let idx = self.project.document.active_layer_index;
        let Some(layer) = self.project.document.layers.get_mut(idx) else {
            return;
        };
        if layer.kind != crate::document::LayerKind::NodeEditor {
            self.status_message = "Select a Node Editor layer".into();
            return;
        }
        layer.ensure_node_graph();
        let Some(g) = layer.node_graph.as_mut() else {
            return;
        };
        let n = g.nodes.len() as f32;
        let id = g.add_node(kind, 40.0 + n * 12.0, 40.0 + n * 18.0);
        self.node_editor_ui.selected = Some(id);
        self.status_message = "Node added".into();
    }

    fn rebalance_active_flowchart_layer_if_any(&mut self) {
        let doc = &self.project.document;
        if let Some(layer) = doc.layers.get(doc.active_layer_index) {
            if layer.kind == crate::document::LayerKind::Flowchart {
                let ids: Vec<crate::document::NodeId> = layer.nodes.clone();
                crate::document::flowchart::rebalance_flowchart_edge_anchors(
                    &mut self.project.nodes,
                    &ids,
                );
            }
        }
    }

    pub fn add_shading_layer_with_preset(&mut self, name: &str, preset: &str) {
        use crate::document::ShadingPass;
        let pass: ShadingPass = match preset.to_ascii_lowercase().as_str() {
            "crt" => ShadingPass::crt_preset(),
            "vignette" => ShadingPass::vignette_preset(),
            _ => ShadingPass::blackhole_preset(),
        };
        self.add_shading_layer_with_passes(name, vec![pass]);
    }

    /// MCP / API: shading layer from runtime WGSL source (no built-in preset).
    pub fn add_shading_layer_with_wgsl(
        &mut self,
        layer_name: &str,
        pass_name: &str,
        wgsl: &str,
        uniforms: Option<Vec<f32>>,
    ) -> Result<(), String> {
        use crate::document::ShadingPass;
        let src = wgsl.trim();
        if src.is_empty() {
            return Err("wgsl must not be empty".into());
        }
        // Static + GPU probe before mutating the document (no blank white page).
        crate::shading::probe_compile_shading_wgsl(self.wgpu_render.as_ref(), src)?;
        let mut pass = ShadingPass::new_preset(pass_name, src);
        if let Some(u) = uniforms {
            pass.uniforms = u;
        }
        if let Ok(mut err) = pass.compile_error.lock() {
            *err = None;
        }
        self.add_shading_layer_with_passes(layer_name, vec![pass]);
        Ok(())
    }

    /// Replace active shading pass WGSL from a `.wgsl` file path (dynamic load, not a preset).
    pub fn load_shading_wgsl_from_path(
        &mut self,
        layer_index: usize,
        path: &std::path::Path,
    ) -> Result<(), String> {
        let src = crate::shading::load_wgsl_file(path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Custom")
            .to_string();
        self.set_shading_wgsl(layer_index, &src, Some(&stem), None)
    }

    /// MCP / API: replace WGSL on an existing shading layer (no new layer).
    ///
    /// Runs static validation + GPU pipeline compile probe when wgpu is available.
    /// On failure, the document is left unchanged and the error is returned to MCP.
    pub fn set_shading_wgsl(
        &mut self,
        layer_index: usize,
        wgsl: &str,
        pass_name: Option<&str>,
        uniforms: Option<Vec<f32>>,
    ) -> Result<(), String> {
        let src = wgsl.trim();
        if src.is_empty() {
            return Err("wgsl must not be empty".into());
        }
        // Must pass GPU compile before history patch — broken WGSL used to paint white.
        crate::shading::probe_compile_shading_wgsl(self.wgpu_render.as_ref(), src)?;
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let layer = after
            .layers
            .get_mut(layer_index)
            .ok_or("Layer not found")?;
        if layer.kind != crate::document::LayerKind::Shading {
            return Err("Layer is not a shading layer".into());
        }
        if layer.shading_passes.is_empty() {
            layer
                .shading_passes
                .push(crate::document::ShadingPass::custom_template());
        }
        let pass = &mut layer.shading_passes[0];
        pass.load_wgsl_source(src, pass_name);
        if let Some(u) = uniforms {
            pass.uniforms = u;
        }
        // Prefer full-page under content by default for procedural galaxy/stars.
        pass.stack = crate::document::ShadingStack::Behind;
        if let Ok(mut err) = pass.compile_error.lock() {
            *err = None;
        }
        after.active_layer_index = layer_index;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        Ok(())
    }

    fn add_shading_layer_with_passes(
        &mut self,
        name: &str,
        passes: Vec<crate::document::ShadingPass>,
    ) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_shading_layer(name);
        after.active_layer_index = idx;
        if let Some(layer) = after.layers.get_mut(idx) {
            // Replace — do not extend. Layer used to ship with a default vignette pass;
            // extend left custom/MCP shaders as pass[1], then UI truncate kept only vignette
            // (compose on empty input → solid black).
            layer.shading_passes = passes;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    /// Infer AV role from media path. Images → Video. Audio → Audio. Video → Video.
    fn av_role_for_media_path(path: &str) -> Option<crate::document::AvRole> {
        use crate::document::AvClip;
        if AvClip::path_is_audio_only(path) {
            Some(crate::document::AvRole::Audio)
        } else if AvClip::path_is_visual_media(path) {
            Some(crate::document::AvRole::Video)
        } else {
            None
        }
    }

    /// Push media onto the active AV layer when role matches; otherwise refuse (no cross-type).
    /// If active is not a matching AV layer, find/create the correct role layer.
    pub fn add_av_layer(&mut self, name: &str, media_path: String) {
        let clean_name = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name)
            .to_string();
        match self.push_media_clip(&clean_name, media_path, false) {
            Ok(msg) => self.status_message = msg,
            Err(e) => self.status_message = e,
        }
    }

    /// Queue a media file onto the correct Video/Audio layer (role-enforced).
    /// `require_active_role` — if true, active layer must already match (Layer tab "Add track").
    pub fn push_media_clip(
        &mut self,
        name: &str,
        media_path: String,
        require_active_role: bool,
    ) -> Result<String, String> {
        use crate::document::{AvClip, AvRole, LayerKind};
        let Some(role) = Self::av_role_for_media_path(&media_path) else {
            return Err("Unsupported media type (use video/image for Video layer, audio for Audio)"
                .into());
        };
        let default_name = match role {
            AvRole::Audio => "Audio",
            AvRole::Video => "Video",
            AvRole::Daw => "DAW",
        };
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();

        if require_active_role {
            let active = after
                .active_layer()
                .ok_or_else(|| "No active layer".to_string())?;
            if active.kind != LayerKind::AV {
                return Err("Select a Video or Audio layer first".into());
            }
            if active.av_role != role {
                return Err(match active.av_role {
                    AvRole::Video => {
                        "This is a Video layer — add audio on an Audio layer".into()
                    }
                    AvRole::Audio => {
                        "This is an Audio layer — add video/image on a Video layer".into()
                    }
                    AvRole::Daw => "This is a DAW layer — use Video/Audio layers for media".into()
                });
            }
        }

        let idx = if require_active_role {
            after.active_layer_index
        } else {
            // Prefer active when matching; else correct role layer; else create.
            if let Some(l) = after.active_layer() {
                if l.kind == LayerKind::AV && l.av_role == role {
                    after.active_layer_index
                } else {
                    after.ensure_av_role_layer(role, default_name)
                }
            } else {
                after.ensure_av_role_layer(role, default_name)
            }
        };

        let path_for_extract = media_path.clone();
        let is_image = AvClip::path_is_still_image(&media_path);
        if let Some(layer) = after.layers.get_mut(idx) {
            layer.av_role = role;
            layer.ensure_av_clips();
            let empty = layer.av_clips.is_empty() && layer.video_path.is_empty();
            let timeline_start = if empty {
                0.0
            } else {
                crate::av_ui::queue_append_start_sec(layer)
            };
            let clip_name = if name.is_empty() {
                default_name.to_string()
            } else {
                name.to_string()
            };
            if empty {
                layer.video_path = media_path.clone();
                if !name.is_empty() {
                    layer.name = clip_name.clone();
                }
            }
            let mut clip =
                AvClip::new_from_media(clip_name, media_path.clone(), timeline_start);
            if empty {
                clip.id = layer.id;
            }
            clip.track_row = 0;
            if is_image {
                // Still image: default 5s hold unless probe later overrides.
                clip.media_source_duration = Some(5.0);
                clip.video_play_length = 5.0;
                if empty {
                    layer.media_source_duration = Some(5.0);
                    layer.video_play_length = 5.0;
                }
            } else if let Some(dur) =
                crate::video_decode::probe_media_duration_secs(&media_path)
            {
                clip.media_source_duration = Some(dur);
                clip.video_play_length = dur;
                if empty {
                    layer.media_source_duration = Some(dur);
                    layer.video_play_length = dur;
                }
            }
            layer.av_clips.push(clip);
            layer.sync_legacy_from_primary_clip();
        }
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        if !is_image {
            spawn_video_audio_extract(
                &path_for_extract,
                &self.audio_extract_status,
                &self.audio_pcm_cache,
            );
        }
        Ok(format!(
            "Added {} track to {} layer",
            if is_image {
                "image"
            } else if role == AvRole::Audio {
                "audio"
            } else {
                "video"
            },
            default_name
        ))
    }

    /// Rasterize selection / image object into a PNG and queue on the active Video layer.
    pub fn push_selection_as_av_image_clip(&mut self) -> Result<String, String> {
        use crate::document::{AvRole, LayerKind};
        let active = self
            .project
            .document
            .active_layer()
            .ok_or("No active layer")?;
        if active.kind != LayerKind::AV || active.av_role != AvRole::Video {
            return Err("Select a Video layer to add an image track from object".into());
        }
        if self.selection.is_empty() {
            return Err("Select an Image object (or any drawable) first".into());
        }

        let source_ids: Vec<uuid::Uuid> = self.selection.clone();
        let tmp_dir = std::env::temp_dir().join("vadadee-berry-av");
        std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
        let staging = tmp_dir.join(format!("obj_stage_{}.png", uuid::Uuid::new_v4().as_simple()));

        self.rasterize_nodes_to_png(&source_ids, &staging)?;

        let name = self
            .selection
            .first()
            .and_then(|id| self.project.nodes.get(*id))
            .map(|n| n.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Object".into());
        let path_str = staging.to_string_lossy().into_owned();
        self.push_media_clip(&name, path_str, true)?;
        // Attach live object link; stabilize path to clip id for overwrite refresh.
        if let Some(layer) = self.project.document.active_layer_mut() {
            if let Some(clip) = layer.av_clips.last_mut() {
                let stable = tmp_dir.join(format!("obj_{}.png", clip.id.as_simple()));
                let _ = std::fs::rename(&staging, &stable)
                    .or_else(|_| std::fs::copy(&staging, &stable).map(|_| ()));
                clip.media_path = stable.to_string_lossy().into_owned();
                clip.source_node_ids = source_ids;
                clip.name = name;
            }
            layer.sync_legacy_from_primary_clip();
        }
        Ok("Added live object track (updates when object changes)".into())
    }

    /// Rasterize one or more document nodes into a PNG path (for object-linked AV tracks).
    fn rasterize_nodes_to_png(
        &self,
        node_ids: &[uuid::Uuid],
        out_path: &std::path::Path,
    ) -> Result<(), String> {
        use crate::document::NodeKind;
        if node_ids.is_empty() {
            return Err("No objects to rasterize".into());
        }
        // Single Image node → write raw bytes (fast path).
        if node_ids.len() == 1 {
            if let Some(node) = self.project.nodes.get(node_ids[0]) {
                if let NodeKind::Image { bytes, .. } = &node.kind {
                    std::fs::write(out_path, bytes).map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
        let mut bounds: Option<kurbo::Rect> = None;
        for id in node_ids {
            let Some(node) = self.project.nodes.get(*id) else {
                continue;
            };
            let b = node.bounds();
            bounds = Some(match bounds {
                Some(acc) => acc.union(b),
                None => b,
            });
        }
        let Some(bounds) = bounds else {
            return Err("Could not compute object bounds".into());
        };
        crate::io::export_selection_raster(
            &self.project,
            node_ids,
            bounds,
            crate::io::ExportImageFormat::Png,
            1.0,
            out_path,
        )
        .map_err(|e| format!("Rasterize object failed: {e}"))
    }

    /// Content fingerprint for live object→track updates (geometry, name, image, anim frame).
    /// Not limited to history revision so drag / playback update frame-by-frame.
    fn object_link_content_sig(&self, node_ids: &[uuid::Uuid]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.anim_current_frame.hash(&mut h);
        self.history.revision().hash(&mut h);
        for id in node_ids {
            id.hash(&mut h);
            let Some(node) = self.project.nodes.get(*id) else {
                0u8.hash(&mut h);
                continue;
            };
            node.name.hash(&mut h);
            let b = node.bounds();
            b.x0.to_bits().hash(&mut h);
            b.y0.to_bits().hash(&mut h);
            b.x1.to_bits().hash(&mut h);
            b.y1.to_bits().hash(&mut h);
            let (px, py) = node.get_pos();
            px.to_bits().hash(&mut h);
            py.to_bits().hash(&mut h);
            node.get_rotation().to_bits().hash(&mut h);
            node.get_opacity().to_bits().hash(&mut h);
            if let crate::document::NodeKind::Image { bytes, width, height, .. } = &node.kind {
                width.to_bits().hash(&mut h);
                height.to_bits().hash(&mut h);
                bytes.len().hash(&mut h);
                // Sample ends so paint changes invalidate without hashing whole blob every frame.
                if let Some(b) = bytes.first() {
                    b.hash(&mut h);
                }
                if let Some(b) = bytes.last() {
                    b.hash(&mut h);
                }
                if bytes.len() > 64 {
                    bytes[bytes.len() / 2].hash(&mut h);
                }
            }
        }
        h.finish()
    }

    /// Delete object-linked tracks whose sources are gone; re-bake remaining every content change.
    fn refresh_object_linked_av_clips(&mut self, ctx: &Context) {
        // --- 1) Orphan tracks: source object(s) deleted → remove the track ---
        let mut orphan_clips: Vec<(usize, uuid::Uuid)> = Vec::new();
        for (li, layer) in self.project.document.layers.iter().enumerate() {
            if layer.kind != crate::document::LayerKind::AV {
                continue;
            }
            for clip in &layer.av_clips {
                if clip.source_node_ids.is_empty() {
                    continue;
                }
                let any_alive = clip
                    .source_node_ids
                    .iter()
                    .any(|id| self.project.nodes.get(*id).is_some());
                if !any_alive {
                    orphan_clips.push((li, clip.id));
                }
            }
        }
        if !orphan_clips.is_empty() {
            let before = snapshot_document(&self.project.document);
            let mut after = before.clone();
            let mut removed = 0usize;
            // Process high indices first so layer indices stay valid if we ever delete layers.
            orphan_clips.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (li, clip_id) in &orphan_clips {
                if let Some(layer) = after.layers.get_mut(*li) {
                    let n0 = layer.av_clips.len();
                    layer.av_clips.retain(|c| c.id != *clip_id);
                    if layer.av_clips.len() != n0 {
                        removed += 1;
                        if layer.av_clips.is_empty() {
                            layer.video_path.clear();
                            layer.media_source_duration = None;
                        } else {
                            layer.sync_legacy_from_primary_clip();
                        }
                    }
                    self.video_layers.remove(clip_id);
                }
                self.selection.retain(|id| id != clip_id);
                if self.piano_roll_clip == Some(*clip_id) {
                    self.piano_roll_clip = None;
                }
            }
            if removed > 0 {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchDocument { before, after },
                );
                self.status_message = if removed == 1 {
                    "Deleted track (source object removed)".into()
                } else {
                    format!("Deleted {removed} tracks (source objects removed)")
                };
                ctx.request_repaint();
            }
        }

        // --- 2) Prune dead ids from multi-source links; refresh living ones frame-by-frame ---
        let mut jobs: Vec<(usize, uuid::Uuid, Vec<uuid::Uuid>, String, u64)> = Vec::new();
        for (li, layer) in self.project.document.layers.iter().enumerate() {
            if layer.kind != crate::document::LayerKind::AV {
                continue;
            }
            for clip in &layer.av_clips {
                if clip.source_node_ids.is_empty() {
                    continue;
                }
                let living: Vec<uuid::Uuid> = clip
                    .source_node_ids
                    .iter()
                    .copied()
                    .filter(|id| self.project.nodes.get(*id).is_some())
                    .collect();
                if living.is_empty() {
                    continue; // will be removed next frame if race
                }
                let sig = self.object_link_content_sig(&living);
                let stale = self
                    .video_layers
                    .get(&clip.id)
                    .and_then(|s| s.object_link_rev)
                    .map(|r| r != sig)
                    .unwrap_or(true);
                if stale {
                    jobs.push((
                        li,
                        clip.id,
                        living,
                        clip.media_path.clone(),
                        sig,
                    ));
                }
            }
        }

        // Drop dead source ids on multi-links (keep track).
        for layer in &mut self.project.document.layers {
            if layer.kind != crate::document::LayerKind::AV {
                continue;
            }
            for clip in &mut layer.av_clips {
                if clip.source_node_ids.is_empty() {
                    continue;
                }
                clip.source_node_ids
                    .retain(|id| self.project.nodes.get(*id).is_some());
            }
        }

        if jobs.is_empty() {
            // Still repaint while any live-linked clip exists and user is dragging / playing,
            // so content sig can catch mid-drag geometry without waiting for pointer release.
            let any_linked = self.project.document.layers.iter().any(|l| {
                l.kind == crate::document::LayerKind::AV
                    && l.av_clips.iter().any(|c| !c.source_node_ids.is_empty())
            });
            if any_linked
                && (self.anim_is_playing
                    || self.tools.select.drag_mode.is_some()
                    || self.tools.select.drag_snapshot.is_empty() == false)
            {
                ctx.request_repaint();
            }
            return;
        }

        for (li, clip_id, source_ids, path, sig) in jobs {
            let out = if path.is_empty() {
                let tmp = std::env::temp_dir()
                    .join("vadadee-berry-av")
                    .join(format!("obj_{}.png", clip_id.as_simple()));
                let _ = std::fs::create_dir_all(tmp.parent().unwrap_or(std::path::Path::new(".")));
                tmp
            } else {
                std::path::PathBuf::from(&path)
            };
            if let Err(e) = self.rasterize_nodes_to_png(&source_ids, &out) {
                log::warn!("object-linked AV refresh failed for {clip_id}: {e}");
                continue;
            }
            let new_name = source_ids
                .iter()
                .find_map(|id| self.project.nodes.get(*id))
                .map(|n| n.name.clone())
                .filter(|n| !n.is_empty());
            let out_str = out.to_string_lossy().into_owned();
            if let Some(layer) = self.project.document.layers.get_mut(li) {
                if let Some(clip) = layer.av_clips.iter_mut().find(|c| c.id == clip_id) {
                    if let Some(n) = new_name {
                        clip.name = n;
                    }
                    clip.media_path = out_str;
                    clip.source_node_ids = source_ids;
                }
            }
            if let Some(state) = self.video_layers.get_mut(&clip_id) {
                state.texture = None;
                state.cached_frame = None;
                state.cached_source_frame = None;
                state.object_link_rev = Some(sig);
            }
            if self
                .video_frame_cache
                .as_ref()
                .is_some_and(|c| c.layer_id == clip_id)
            {
                self.video_frame_cache = None;
            }
            ctx.request_repaint();
        }
    }

    pub fn delete_av_clip(&mut self, layer_idx: usize, clip_id: uuid::Uuid) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let Some(layer) = after.layers.get_mut(layer_idx) else {
            return;
        };
        if layer.kind != crate::document::LayerKind::AV {
            return;
        }
        layer.ensure_av_clips();
        let n0 = layer.av_clips.len() + layer.music_clips.len();
        layer.av_clips.retain(|c| c.id != clip_id);
        layer.music_clips.retain(|c| c.id != clip_id);
        if layer.av_clips.len() + layer.music_clips.len() == n0 {
            return;
        }
        if layer.av_clips.is_empty() {
            layer.video_path.clear();
            layer.media_source_duration = None;
        } else {
            layer.sync_legacy_from_primary_clip();
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        self.selection.retain(|id| *id != clip_id);
        if self.piano_roll_clip == Some(clip_id) {
            self.piano_roll_clip = None;
        }
        self.status_message = "Deleted track".into();
    }

    // Back-compat
    pub fn add_video_layer(&mut self, name: &str, video_path: String) {
        self.add_av_layer(name, video_path)
    }
    pub fn add_audio_layer(&mut self, name: &str, audio_path: String) {
        self.add_av_layer(name, audio_path)
    }


    pub fn set_active_layer(&mut self, index: usize) {
        if index >= self.project.document.layers.len() {
            return;
        }
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.active_layer_index = index;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        self.selection.clear();
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.visible = visible;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn set_layer_locked(&mut self, index: usize, locked: bool) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.locked = locked;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn rename_layer(&mut self, index: usize, name: String) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.name = name;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    fn live_action_status(&self, ctx: &Context) -> Option<String> {
        if self.tools.space_pan || self.tools.canvas_pan_drag {
            return Some("Panning".into());
        }
        if let Some(drag) = &self.tools.drag_shape {
            if let Some(kind) = drag.kind {
                return Some(format!("Creating {}", kind.label()));
            }
        }
        if self.tools.active == ToolKind::Pen {
            if !self.tools.pen.is_empty() {
                return Some("Creating path".into());
            }
            return Some("Click to place path points".into());
        }
        if self.on_page_text_edit.is_some() && ctx.text_edit_focused() {
            return Some("Editing text".into());
        }
        if self.tools.select.node_drag_active {
            if let Some(target) = self.tools.select.node_edit_target {
                let what = match target {
                    PathEditTarget::Anchor(i) => format!("point {i}"),
                    PathEditTarget::HandleOut(i) => format!("handle out {i}"),
                    PathEditTarget::HandleIn(i) => format!("handle in {i}"),
                    PathEditTarget::MidCtrl1(i) => format!("mid ctrl1 seg {i}"),
                    PathEditTarget::MidCtrl2(i) => format!("mid ctrl2 seg {i}"),
                };
                return Some(format!("Dragging {what}"));
            }
        }
        if let Some(mode) = self.tools.select.drag_mode {
            return Some(match mode {
                SelectDrag::Move => {
                    if self.selection.len() == 1 {
                        if let Some(id) = self.selection.first() {
                            if let Some(n) = self.project.nodes.get(*id) {
                                return Some(format!("Moving {}", n.name));
                            }
                        }
                    }
                    "Moving selection".into()
                }
                SelectDrag::Resize(_) => "Resizing".into(),
                SelectDrag::Rotate => "Rotating".into(),
                SelectDrag::TilingGizmo(_) | SelectDrag::CircularGizmo(_) => "Editing effect".into(),
            });
        }
        if self.tools.select.marquee.is_some() {
            return Some("Selecting".into());
        }
        None
    }

    pub(crate) fn is_ephemeral_status_event(msg: &str) -> bool {
        // Sticky: screen-record outcomes must stay until the next real status change.
        if msg.starts_with("Recording")
            || msg.starts_with("Record ")
            || msg.starts_with("Saved screen")
            || msg.starts_with("Stop ")
            || msg.starts_with("Already recording")
            || msg.starts_with("Not recording")
            || msg.starts_with("Not a Screen Record")
            || msg.starts_with("Screen record")
        {
            return false;
        }
        // Unfocused is sticky while the window stays blurred; Switched is a one-shot flash.
        if msg == "Unfocused" {
            return false;
        }
        msg == "Undo"
            || msg == "Redo"
            || msg == "Pasted"
            || msg == "Pasted image"
            || msg == "Nothing to paste"
            || msg == "Layer locked"
            || msg == "Switched"
            || msg.starts_with("Copied")
            || msg.starts_with("Cut ")
            || msg.starts_with("Open")
            || msg.starts_with("Save")
            || msg.starts_with("Export")
            || msg.starts_with("New ")
            || msg.contains("failed")
            || msg.contains("Failed")
            || msg.starts_with("Pen cancelled")
            || msg.starts_with("Removed point")
            || msg.starts_with("Polyline cleared")
    }

    /// Track OS/window focus: status **Unfocused** when leaving, **Switched** when returning.
    /// Keeps the event loop alive while blurred so MCP tools still answer.
    pub fn update_window_focus_status(&mut self, ctx: &Context) {
        let focused = ctx.input(|i| i.focused);
        if self.window_was_focused && !focused {
            self.status_message = "Unfocused".into();
        } else if !self.window_was_focused && focused {
            // User switched back (workspace / alt-tab / other app).
            self.status_message = "Switched".into();
        }
        self.window_was_focused = focused;
        if self.video_export.rendering {
            // Hybrid export uses shared wgpu on the worker. If we throttle paints while
            // unfocused/switched workspace, device work stalls → export falls below 1fps.
            ctx.request_repaint();
        } else if !focused {
            // Without this, paint/MCP poll stops while unfocused → tool timeouts.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Second status-bar segment: live action, short event line, else **Idle**.
    pub fn derive_action_status(&self, ctx: &Context) -> String {
        if let Some(progress) = &self.paste_progress {
            return progress.label.clone();
        }
        if self.anim_is_playing {
            return format!("Playing animation (Frame {})", self.anim_current_frame);
        }
        if self.anim_keyframing_mode {
            return format!("Recording keyframes (Frame {})", self.anim_current_frame);
        }
        // Sticky / important messages (recording, hard errors) must outrank idle tools.
        if !self.status_message.is_empty()
            && (self.status_message.starts_with("Recording")
                || self.status_message.starts_with("Record ")
                || self.status_message.starts_with("Saved screen")
                || self.status_message.starts_with("Stop ")
                || self.status_message.contains("failed")
                || self.status_message.contains("Failed"))
        {
            return self.status_message.clone();
        }
        if !self.screen_captures.is_empty() {
            let n = self.screen_captures.len();
            return if n == 1 {
                "Recording screen…".into()
            } else {
                format!("Recording {n} screens…")
            };
        }
        // Window / workspace not active — show before tool live status.
        if !ctx.input(|i| i.focused) {
            return "Unfocused".into();
        }
        if let Some(live) = self.live_action_status(ctx) {
            return live;
        }
        if Self::is_ephemeral_status_event(&self.status_message) {
            return self.status_message.clone();
        }
        if !self.status_message.is_empty() {
            return self.status_message.clone();
        }
        "Idle".into()
    }

    pub fn selection_bounds(&self) -> Option<kurbo::Rect> {
        if self.selection.is_empty() {
            return None;
        }
        let mut union_rect: Option<kurbo::Rect> = None;
        for id in &self.selection {
            if let Some(node) = self.project.nodes.get(*id) {
                let bounds = node.bounds_with_store(&self.project.nodes);
                if let Some(ref mut u) = union_rect {
                    *u = u.union(bounds);
                } else {
                    union_rect = Some(bounds);
                }
            }
        }
        union_rect
    }

    pub fn resize_to_selection(&mut self) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };
        
        let before = snapshot_project(&self.project);
        
        // Translate all nodes
        let dx = -bounds.x0;
        let dy = -bounds.y0;
        for node in self.project.nodes.map.values_mut() {
            node.translate(dx, dy);
        }
        
        // Resize document
        self.project.document.width = bounds.width().round();
        self.project.document.height = bounds.height().round();
        
        let after = snapshot_project(&self.project);
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );

        // Adjust viewport pan so that coordinates visually stay in the same place
        self.viewport.pan.x -= dx as f32 * self.viewport.zoom;
        self.viewport.pan.y -= dy as f32 * self.viewport.zoom;
        
        self.status_message = format!(
            "Resized canvas to selected bounds: {}x{}",
            self.project.document.width, self.project.document.height
        );
    }

    pub fn copy_selection_as_png(&mut self, dpi_scale: f32) {
        let Some(bounds) = self.selection_bounds() else {
            self.status_message = "Copy PNG failed: no object selected".into();
            return;
        };

        let Some((w, h, bytes)) =
            io::rasterize_selection_rgba(&self.project, &self.selection, bounds, dpi_scale)
        else {
            self.status_message = "Copy PNG failed: rasterization error".into();
            return;
        };
        
        // 3. Set image to system clipboard
        #[cfg(not(target_os = "android"))]
        {
            match arboard::Clipboard::new() {
                Ok(mut cb) => {
                    let img = arboard::ImageData {
                        width: w as usize,
                        height: h as usize,
                        bytes: std::borrow::Cow::from(bytes),
                    };
                    if let Err(e) = cb.set_image(img) {
                        self.status_message = format!("Clipboard copy failed: {e}");
                    } else {
                        self.status_message = format!("Copied selection as PNG ({}x{}) to clipboard", w, h);
                    }
                }
                Err(e) => {
                    self.status_message = format!("Clipboard error: {e}");
                }
            }
        }
        #[cfg(target_os = "android")]
        {
            self.status_message = "Clipboard image copy not supported on Android".into();
        }
    }

    pub fn request_video_export(&mut self, ctx: egui::Context) {
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            let ext = self.video_export.format.extension();
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("animation.{ext}"))
                .add_filter("Video", &[ext])
                .save_file()
            {
                self.begin_video_export(path, ctx);
            }
        }
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            self.status_message = "Video export is only available on desktop".into();
        }
    }

    pub fn begin_video_export(&mut self, output: std::path::PathBuf, ctx: egui::Context) {
        // Refresh media caps only; do not reset user Play Duration (e.g. 10s trim).
        self.sync_stale_media_layer_durations();
        let anim_fps = self.anim_fps.max(1);
        let export_fps = self.video_export.fps.max(1);
        let content_secs = self.animation_content_duration_secs();
        let cycles = self.video_export.export_cycles.max(1);
        // User Duration (e.g. 10s) is exact — never clamp up to full media length.
        let plan = crate::export_worker::plan_export_duration(
            self.video_export.export_duration_secs,
            content_secs,
            cycles,
            anim_fps,
            export_fps,
        );
        let temp =
            std::env::temp_dir().join(format!("vadadee_video_{}", uuid::Uuid::new_v4().as_simple()));
        let _ = std::fs::create_dir_all(&temp);
        let restore = self.anim_current_frame;

        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let job = crate::export_worker::ExportJobConfig {
            output_path: output,
            work_dir: temp.clone(),
            fps: self.video_export.fps,
            resolution_pct: self.video_export.resolution_pct,
            bitrate_kbps: self.video_export.bitrate_kbps,
            format: self.video_export.format,
            power: self.video_export.power_level,
            fx_quality: self.video_export.fx_quality,
            total_frames: plan.total_frames,
            anim_fps,
            max_anim_frame: plan.max_anim_frame,
            cycle_frame_count: plan.cycle_frame_count,
            export_cycles: cycles,
        };

        crate::export_worker::spawn_export_worker(
            self.project.clone(),
            job,
            cancel.clone(),
            tx,
            self.wgpu_render.clone(),
            self.video_export.renderer_reclaim.clone(),
        );

        self.video_export.restore_anim_frame = restore;
        self.video_export.frame_done = 0;
        self.video_export.worker_frame_done = 0;
        self.video_export.total_frames = plan.total_frames;
        self.video_export.frames_dir = Some(temp);
        self.video_export.rendering = true;
        self.video_export.progress = Some(0.0);
        self.video_export.progress_target = 0.0;
        self.video_export.progress_smooth = 0.0;
        self.video_export.progress_visible = true;
        self.video_export.export_start_time = Some(std::time::Instant::now());
        self.video_export.export_rx = Some(rx);
        self.video_export.export_cancel = Some(cancel);
        self.video_export.status_msg = format!(
            "Rendering {} frames ({:.1}s) @ {} fps, {}% · {} cycle(s)…",
            plan.total_frames,
            plan.cycle_secs * cycles as f32,
            self.video_export.fps,
            self.video_export.resolution_pct,
            cycles
        );
        self.status_message = "Video export started (background)".into();

        // Initialize joke and system stats:
        self.video_export.last_frame_time = Some(std::time::Instant::now());
        self.video_export.sec_per_frame = 0.0;
        self.video_export.last_joke_update = std::time::Instant::now();
        self.video_export.last_stats_update = std::time::Instant::now();
        self.video_export.sys_stats.update();
        let is_mobile = cfg!(target_os = "android");
        self.video_export.joke_cycle = 0;
        self.video_export.current_joke = crate::sys_stats::choose_joke(
            &self.video_export.joke_rules,
            self.video_export.sys_stats.cpu_usage,
            self.video_export.sys_stats.ram_sys_used_gb,
            self.video_export.sec_per_frame,
            self.video_export.sys_stats.cpu_temp,
            is_mobile,
            self.video_export.joke_cycle,
        );
    }

    pub fn cancel_video_export(&mut self) {
        if let Some(cancel) = &self.video_export.export_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.video_export.status_msg = "Cancelling…".into();
        self.status_message = "Cancelling video export…".into();
    }

    fn finish_video_export_ui(&mut self, cancelled: bool) {
        self.video_export.rendering = false;
        self.video_export.progress = None;
        self.video_export.export_rx = None;
        self.video_export.export_cancel = None;
        self.video_export.export_start_time = None;
        if let Some(dir) = self.video_export.frames_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
        self.apply_animation_for_frame(self.video_export.restore_anim_frame);
        if cancelled {
            self.video_export.status_msg = "Cancelled.".into();
            self.status_message = "Video export cancelled".into();
        }
    }

    pub fn poll_video_export(&mut self, ctx: &Context) {
        if !self.video_export.rendering {
            return;
        }
        let Some(rx) = &self.video_export.export_rx else {
            return;
        };
        let mut done: Option<(bool, String)> = None;
        // Drain all worker events; speed comes from worker EMA, not UI receive gaps.
        while let Ok(ev) = rx.try_recv() {
            match ev {
                crate::export_worker::ExportWorkerEvent::Phase(phase) => {
                    self.video_export.status_msg = match phase {
                        crate::export_worker::ExportPhase::Preparing => {
                            "Preparing export…".into()
                        }
                        crate::export_worker::ExportPhase::Encoding => {
                            format!(
                                "Encoding {} frames @ {} fps…",
                                self.video_export.total_frames, self.video_export.fps
                            )
                        }
                        crate::export_worker::ExportPhase::Finalizing => {
                            "Finalizing video file…".into()
                        }
                    };
                }
                crate::export_worker::ExportWorkerEvent::Progress {
                    frame_done,
                    total,
                    message,
                    sec_per_frame,
                    ..
                } => {
                    self.video_export.worker_frame_done = frame_done;
                    self.video_export.frame_done = frame_done;
                    self.video_export.total_frames = total;
                    let target = frame_done as f32 / total.max(1) as f32;
                    self.video_export.progress_target = target.clamp(0.0, 1.0);
                    // Keep `progress` for anything that still reads it; UI uses smooth.
                    self.video_export.progress = Some(self.video_export.progress_target);
                    // Prefer worker-measured rate (immune to UI batching stalls).
                    if sec_per_frame > 1e-6 {
                        if self.video_export.sec_per_frame < 1e-6 {
                            self.video_export.sec_per_frame = sec_per_frame;
                        } else {
                            // Light UI-side smooth so the label doesn't flicker.
                            self.video_export.sec_per_frame =
                                self.video_export.sec_per_frame * 0.7 + sec_per_frame * 0.3;
                        }
                    }
                    // Don't thrash status with every frame once we have a stable line.
                    if frame_done == 0
                        || frame_done == total
                        || message.starts_with("Export path=")
                        || message.starts_with("Muxing")
                        || frame_done % 15 == 0
                    {
                        self.video_export.status_msg = message;
                    }
                }
                crate::export_worker::ExportWorkerEvent::Finished { success, message } => {
                    done = Some((success, message));
                }
            }
        }

        // P7a: ease progress bar toward worker target every UI tick (~smooth, no jumps).
        let dt = ctx.input(|i| i.unstable_dt).clamp(1.0 / 120.0, 0.1);
        let target = self.video_export.progress_target;
        let cur = self.video_export.progress_smooth;
        // Catch up in ~0.2s when behind; never overshoot.
        let t = (dt / 0.2).min(1.0);
        self.video_export.progress_smooth = cur + (target - cur) * t;
        if (self.video_export.progress_smooth - target).abs() < 0.0005 {
            self.video_export.progress_smooth = target;
        }
        self.video_export.progress = Some(self.video_export.progress_smooth);

        // Periodic updates:
        let now = std::time::Instant::now();
        if now.duration_since(self.video_export.last_stats_update) >= std::time::Duration::from_secs(1) {
            self.video_export.sys_stats.update();
            self.video_export.last_stats_update = now;
        }

        if now.duration_since(self.video_export.last_joke_update) >= std::time::Duration::from_secs(10) {
            let is_mobile = cfg!(target_os = "android");
            self.video_export.joke_cycle = self.video_export.joke_cycle.wrapping_add(1);
            self.video_export.current_joke = crate::sys_stats::choose_joke(
                &self.video_export.joke_rules,
                self.video_export.sys_stats.cpu_usage,
                self.video_export.sys_stats.ram_sys_used_gb,
                self.video_export.sec_per_frame,
                self.video_export.sys_stats.cpu_temp,
                is_mobile,
                self.video_export.joke_cycle,
            );
            self.video_export.last_joke_update = now;
        }

        if let Some((success, message)) = done {
            // Drain any renderers the export thread shipped back for safe GPU teardown.
            if let Ok(mut q) = self.video_export.renderer_reclaim.lock() {
                q.clear(); // drops here, on the main GL-context thread
            }
            let cancelled = !success && message.contains("Cancelled");

            if success {
                self.video_export.progress_target = 1.0;
                self.video_export.progress_smooth = 1.0;
                self.video_export.progress = Some(1.0);
                self.video_export.status_msg = message.clone();
                self.status_message = message.clone();
            } else if !cancelled {
                self.video_export.status_msg = message.clone();
                self.status_message = message;
            }
            self.finish_video_export_ui(cancelled);
        }
        if self.video_export.rendering {
            // Keep the egui/wgpu event loop alive even when the window is unfocused /
            // on another workspace. Hybrid export blits shading on the shared device;
            // if the main thread sleeps, device.poll stalls and export collapses to <1fps.
            ctx.request_repaint();
        }
    }

    pub fn copy_selection(&mut self) {
        log::info!("CLIPBOARD: copy_selection called, selection.len()={}", self.selection.len());
        if self.selection.is_empty() {
            log::info!("CLIPBOARD: copy skipped, empty selection");
            return;
        }
        self.clipboard = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .collect();
        let n = self.clipboard.len();
        self.status_message = if n == 1 {
            format!("Copied {}", self.clipboard[0].name)
        } else {
            format!("Copied {n} objects")
        };
        log::info!("CLIPBOARD: copied {} objects to internal clipboard", n);
    }

    pub fn cut_selection(&mut self) {
        log::info!("CLIPBOARD: cut_selection called, selection.len()={}", self.selection.len());
        if self.selection.is_empty() {
            log::info!("CLIPBOARD: cut skipped, empty selection");
            return;
        }
        let names: Vec<String> = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id))
            .map(|n| n.name.clone())
            .collect();
        self.clipboard = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .collect();
        self.delete_selection();
        self.status_message = if names.len() == 1 {
            format!("Cut {}", names[0])
        } else {
            format!("Cut {} objects", names.len())
        };
        log::info!("CLIPBOARD: cut {} objects", names.len());
    }

    fn image_paste_doc_center(&self) -> (f64, f64) {
        if let Some((cx, cy)) = self.cursor_doc {
            return (cx, cy);
        }
        if let (Some(rect), origin) = (self.canvas_screen_rect, self.canvas_origin) {
            let center_screen = rect.center();
            return tools::doc_point_from_screen(
                center_screen,
                origin,
                self.viewport.pan,
                self.viewport.zoom,
            );
        }
        (180.0, 120.0)
    }

    fn object_paste_offset(&self) -> (f64, f64) {
        /// Offset so the first object's position lands on a grid cell (no “half-step gap”).
        let snap_offset = |dx: f64, dy: f64, origin: (f64, f64)| -> (f64, f64) {
            if !self.viewport.snap_grid {
                return (dx, dy);
            }
            let g = self.viewport.grid_step as f64;
            if g <= 0.0 {
                return (dx, dy);
            }
            let tx = ((origin.0 + dx) / g).round() * g;
            let ty = ((origin.1 + dy) / g).round() * g;
            (tx - origin.0, ty - origin.1)
        };

        if let Some((cx, cy)) = self.cursor_doc {
            if let Some(first) = self.clipboard.first() {
                let b = first.bounds();
                let (fx, fy) = (b.x0, b.y0);
                // Nudge one grid step so paste is not on top of the original.
                let g = if self.viewport.snap_grid {
                    self.viewport.grid_step as f64
                } else {
                    16.0
                };
                let raw = (cx - fx + g, cy - fy + g);
                return snap_offset(raw.0, raw.1, (fx, fy));
            }
            let g = if self.viewport.snap_grid {
                self.viewport.grid_step as f64
            } else {
                24.0
            };
            return (g, g);
        }
        if let (Some(rect), origin) = (self.canvas_screen_rect, self.canvas_origin) {
            let center_screen = rect.center();
            let (cx, cy) =
                tools::doc_point_from_screen(center_screen, origin, self.viewport.pan, self.viewport.zoom);
            if let Some(first) = self.clipboard.first() {
                let b = first.bounds();
                let (fx, fy) = (b.x0, b.y0);
                let g = if self.viewport.snap_grid {
                    self.viewport.grid_step as f64
                } else {
                    16.0
                };
                let raw = (cx - fx + g, cy - fy + g);
                return snap_offset(raw.0, raw.1, (fx, fy));
            }
            let g = if self.viewport.snap_grid {
                self.viewport.grid_step as f64
            } else {
                24.0
            };
            return (g, g);
        }
        let g = if self.viewport.snap_grid {
            self.viewport.grid_step as f64
        } else {
            24.0
        };
        (g, g)
    }

    fn begin_system_image_paste(&mut self) {
        self.paste_progress = Some(PasteProgress {
            label: "Pasting… 1/3 reading clipboard".into(),
            task: PasteTask::SystemImage {
                step: 1,
                rgba: None,
                png: None,
                placement: None,
            },
        });
    }

    fn begin_object_paste(&mut self, offset: (f64, f64)) {
        let nodes = self.clipboard.clone();
        let total = nodes.len();
        self.paste_progress = Some(PasteProgress {
            label: format!("Pasting… 0/{total} objects"),
            task: PasteTask::Objects {
                nodes,
                offset,
                index: 0,
                new_sel: Vec::new(),
            },
        });
    }

    fn finish_paste(&mut self, message: String) {
        self.paste_progress = None;
        self.status_message = message;
    }

    fn advance_paste_operation(&mut self, ctx: &Context) {
        let Some(mut progress) = self.paste_progress.take() else {
            return;
        };

        match &mut progress.task {
            PasteTask::SystemImage {
                step,
                rgba,
                png,
                placement,
            } => match *step {
                1 => {
                    log::info!("CLIPBOARD: paste step 1/3 reading clipboard");
                    if !self.layer_editable() {
                        self.finish_paste("Layer locked".into());
                        return;
                    }
                    #[cfg(target_os = "android")]
                    {
                        self.finish_paste("System image paste is not available on Android".into());
                        return;
                    }
                    #[cfg(not(target_os = "android"))]
                    {
                        let Ok(mut cb) = arboard::Clipboard::new() else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let Ok(img) = cb.get_image() else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let w = img.width as u32;
                        let h = img.height as u32;
                        if w == 0 || h == 0 {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let Some(rgba_img) =
                            image::RgbaImage::from_raw(w, h, img.bytes.into_owned())
                        else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let (cx, cy) = self.image_paste_doc_center();
                        let disp_w = (w as f64).min(400.0);
                        let disp_h = disp_w * (h as f64 / w.max(1) as f64);
                        *rgba = Some(rgba_img);
                        *placement = Some(ImagePastePlacement {
                            x: cx - disp_w / 2.0,
                            y: cy - disp_h / 2.0,
                            width: disp_w,
                            height: disp_h,
                        });
                        *step = 2;
                        progress.label = "Pasting… 2/3 processing image".into();
                        self.paste_progress = Some(progress);
                        ctx.request_repaint();
                    }
                }
                2 => {
                    log::info!("CLIPBOARD: paste step 2/3 processing image");
                    let Some(rgba_img) = rgba.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    let mut out = Vec::new();
                    let ok = rgba_img
                        .write_to(
                            &mut std::io::Cursor::new(&mut out),
                            image::ImageFormat::Png,
                        )
                        .is_ok()
                        && !out.is_empty();
                    if !ok {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    }
                    *png = Some(out);
                    *step = 3;
                    progress.label = "Pasting… 3/3 placing on canvas".into();
                    self.paste_progress = Some(progress);
                    ctx.request_repaint();
                }
                3 => {
                    log::info!("CLIPBOARD: paste step 3/3 placing on canvas");
                    let Some(bytes) = png.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    let Some(place) = placement.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    self.insert_image(place.x, place.y, place.width, place.height, bytes);
                    self.finish_paste("Pasted image".into());
                    log::info!("CLIPBOARD: pasted image from system clipboard");
                    ctx.request_repaint();
                }
                _ => {
                    self.finish_paste("Nothing to paste".into());
                }
            },
            PasteTask::Objects {
                nodes,
                offset,
                index,
                new_sel,
            } => {
                let total = nodes.len();
                if *index < total {
                    let mut node = nodes[*index].clone();
                    node.translate(offset.0, offset.1);
                    let dup = node.duplicate();
                    let id = dup.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node: dup });
                    new_sel.push(id);
                    *index += 1;
                    progress.label = format!("Pasting… {}/{total} objects", *index);
                    if *index >= total {
                        self.selection = new_sel.clone();
                        let done = if total == 1 {
                            "Pasted".into()
                        } else {
                            format!("Pasted {total} objects")
                        };
                        self.finish_paste(done);
                        log::info!("CLIPBOARD: pasted {total} objects from internal clipboard");
                    } else {
                        self.paste_progress = Some(progress);
                    }
                    ctx.request_repaint();
                } else {
                    self.finish_paste("Nothing to paste".into());
                }
            }
        }
    }

    pub fn is_pasting(&self) -> bool {
        self.paste_progress.is_some()
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn system_clipboard_has_image(&self) -> bool {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_image().ok())
            .is_some_and(|img| img.width > 0 && img.height > 0)
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn system_clipboard_has_image(&self) -> bool {
        false
    }

    /// `prefer_system_image`: true when egui-winit did not deliver Paste (image-only OS clipboard).
    pub fn paste_clipboard(&mut self, prefer_system_image: bool) {
        if self.paste_progress.is_some() {
            return;
        }
        log::info!(
            "CLIPBOARD: paste_clipboard called, internal={} prefer_system_image={}",
            self.clipboard.len(),
            prefer_system_image
        );
        if !self.layer_editable() {
            self.status_message = "Layer locked".into();
            log::info!("CLIPBOARD: paste blocked, layer not editable");
            return;
        }
        if prefer_system_image && self.system_clipboard_has_image() {
            self.begin_system_image_paste();
            return;
        }
        if !self.clipboard.is_empty() {
            let offset = self.object_paste_offset();
            self.begin_object_paste(offset);
            return;
        }
        if self.system_clipboard_has_image() {
            self.begin_system_image_paste();
        } else {
            self.status_message = "Nothing to paste".into();
        }
    }

    /// Group selected nodes. Children keep absolute coords; group transform is
    /// the parent origin (union center). Moving/rotating the group updates children.
    pub fn group_selection(&mut self) {
        if self.selection.len() < 2 {
            self.status_message = "Select 2+ objects to group".into();
            return;
        }
        if !self.layer_editable() {
            return;
        }
        let mut child_ids: Vec<NodeId> = Vec::new();
        let mut union: Option<kurbo::Rect> = None;
        for &id in &self.selection {
            if let Some(n) = self.project.nodes.get(id) {
                if matches!(n.kind, NodeKind::Group { .. }) {
                    continue; // skip nesting groups for now
                }
                child_ids.push(id);
                let b = n.bounds();
                union = Some(match union {
                    Some(u) => u.union(b),
                    None => b,
                });
            }
        }
        if child_ids.len() < 2 {
            self.status_message = "Need 2+ non-group objects".into();
            return;
        }
        let u = union.unwrap();
        let ox = (u.x0 + u.x1) * 0.5;
        let oy = (u.y0 + u.y1) * 0.5;
        // Convert children to parent-relative positions (subtract group origin).
        let mut patches = Vec::new();
        for &cid in &child_ids {
            let Some(before) = self.project.nodes.get(cid).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.translate(-ox, -oy);
            patches.push((cid, before, after));
        }
        let mut group = Node::group(child_ids.clone(), "Group");
        group.transform.translation = [ox, oy];
        group.transform.rotation_rad = 0.0;
        let gid = group.id;
        if !patches.is_empty() {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNodes { patches },
            );
        }
        self.history
            .push(&mut self.project, ProjectEdit::InsertNode { node: group });
        // Remove children from layer list order? Keep them but typically groups hold children
        // that remain in the node store; layer.nodes may still list them — filter draw of
        // nested children via hidden when parent is a group on the layer.
        self.selection = vec![gid];
        self.status_message = format!("Grouped {} objects (relative to parent)", child_ids.len());
    }

    pub fn ungroup_selection(&mut self) {
        let sel = self.selection.clone();
        let mut restored = Vec::new();
        for &id in &sel {
            let Some(group) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let NodeKind::Group { children } = &group.kind else {
                continue;
            };
            let (tx, ty) = (
                group.transform.translation[0],
                group.transform.translation[1],
            );
            let rot = group.transform.rotation_rad;
            let cos = rot.cos();
            let sin = rot.sin();
            let mut patches = Vec::new();
            for &cid in children {
                let Some(before) = self.project.nodes.get(cid).cloned() else {
                    continue;
                };
                let mut after = before.clone();
                // Local → world: rotate about origin then translate by group origin.
                let (lx, ly) = after.get_pos();
                let wx = tx + lx * cos - ly * sin;
                let wy = ty + lx * sin + ly * cos;
                let (cx, cy) = after.get_pos();
                after.translate(wx - cx, wy - cy);
                after.set_rotation(before.get_rotation() + rot);
                patches.push((cid, before, after));
                restored.push(cid);
            }
            if !patches.is_empty() {
                self.history
                    .push(&mut self.project, ProjectEdit::PatchNodes { patches });
            }
            // Delete group node
            self.delete_nodes(&[id]);
        }
        if !restored.is_empty() {
            self.selection = restored;
            self.status_message = "Ungrouped".into();
        } else {
            self.status_message = "No group in selection".into();
        }
    }

    pub fn duplicate_selection(&mut self) {
        let copies: Vec<Node> = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .map(|mut n| {
                n.translate(24.0, 24.0);
                n.duplicate()
            })
            .collect();
        let mut new_sel = Vec::new();
        for node in copies {
            let id = node.id;
            self.history
                .push(&mut self.project, ProjectEdit::InsertNode { node });
            new_sel.push(id);
        }
        self.selection = new_sel;
    }

    pub fn delete_layer(&mut self, index: usize) {
        if self.project.document.layers.len() <= 1 {
            return;
        }
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.layers.remove(index);
        if after.active_layer_index >= after.layers.len() {
            after.active_layer_index = after.layers.len() - 1;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        self.selection.clear();
    }

    pub fn nudge_layer_order(&mut self, index: usize, delta: isize) {
        let len = self.project.document.layers.len();
        let target = (index as isize + delta).clamp(0, len as isize - 1) as usize;
        if target == index {
            return;
        }
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let layer = after.layers.remove(index);
        after.layers.insert(target, layer);
        after.active_layer_index = target;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    /// Layer index that owns the first selected image-layer node, if any.
    fn layer_index_for_node_selection(&self) -> Option<usize> {
        for id in &self.selection {
            if self.project.nodes.get(*id).is_none() {
                continue;
            }
            for (i, layer) in self.project.document.layers.iter().enumerate() {
                if layer.kind == crate::document::LayerKind::Image && layer.nodes.contains(id) {
                    return Some(i);
                }
            }
        }
        None
    }

    fn nudge_nodes_within_layer(&mut self, layer_index: usize, delta: isize) -> bool {
        let before = self
            .project
            .document
            .layers
            .get(layer_index)
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut after = before.clone();
        let mut changed = false;
        for id in self.selection.clone() {
            if let Some(pos) = after.iter().position(|n| *n == id) {
                let new_pos =
                    (pos as isize + delta).clamp(0, after.len() as isize - 1) as usize;
                if new_pos != pos {
                    let item = after.remove(pos);
                    after.insert(new_pos, item);
                    changed = true;
                }
            }
        }
        if changed && after != before {
            self.history.push(
                &mut self.project,
                ProjectEdit::ReorderNodes {
                    layer_index,
                    before,
                    after,
                },
            );
            return true;
        }
        false
    }

    /// Raise / lower selection in the stack (vs video/audio layers) or within an image layer.
    /// Kind of the sole selected layer, if selection is exactly one layer id.
    pub fn selected_layer_kind(&self) -> Option<crate::document::LayerKind> {
        if self.selection.len() != 1 {
            return None;
        }
        let id = self.selection[0];
        self.project
            .document
            .layers
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.kind)
    }

    pub fn nudge_z_order(&mut self, delta: isize) {
        let len = self.project.document.layers.len();
        if len == 0 {
            return;
        }

        for id in self.selection.clone() {
            if let Some(pos) = self.project.document.layers.iter().position(|l| l.id == id) {
                self.nudge_layer_order(pos, delta);
                return;
            }
        }

        if let Some(layer_idx) = self.layer_index_for_node_selection() {
            let target = (layer_idx as isize + delta).clamp(0, len as isize - 1) as usize;
            if target != layer_idx {
                self.nudge_layer_order(layer_idx, delta);
                return;
            }
            let _ = self.nudge_nodes_within_layer(layer_idx, delta);
            return;
        }

        let idx = self.project.document.active_layer_index;
        let _ = self.nudge_nodes_within_layer(idx, delta);
    }

    /// Flip all selected nodes horizontally (if `horizontal`) or vertically.
    /// Multi-selection flips about the **shared** selection centre so relative layout mirrors.
    pub fn flip_selection(&mut self, horizontal: bool) {
        if self.selection.is_empty() || !self.layer_editable() {
            return;
        }
        // Shared flip axis from union of selected bounds (incl. group children).
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &id in &self.selection {
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            let b = node.bounds_with_store(&self.project.nodes);
            if b.width() <= 0.0 && b.height() <= 0.0 {
                continue;
            }
            min_x = min_x.min(b.x0);
            min_y = min_y.min(b.y0);
            max_x = max_x.max(b.x1);
            max_y = max_y.max(b.y1);
        }
        if !min_x.is_finite() {
            return;
        }
        let axis_x = (min_x + max_x) * 0.5;
        let axis_y = (min_y + max_y) * 0.5;

        let ids = self.selection.clone();
        for &id in &ids {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            // Expand groups: flip each child about the shared axis.
            if let NodeKind::Group { children } = &before.kind {
                for &cid in children {
                    let Some(cb) = self.project.nodes.get(cid).cloned() else {
                        continue;
                    };
                    let mut ca = cb.clone();
                    if horizontal {
                        ca.flip_h_about(axis_x);
                    } else {
                        ca.flip_v_about(axis_y);
                    }
                    if cb != ca {
                        self.history.push(
                            &mut self.project,
                            ProjectEdit::PatchNode {
                                id: cid,
                                before: cb,
                                after: ca,
                            },
                        );
                    }
                }
                continue;
            }
            let mut after = before.clone();
            if horizontal {
                after.flip_h_about(axis_x);
            } else {
                after.flip_v_about(axis_y);
            }
            if before == after {
                continue;
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode {
                    id,
                    before,
                    after,
                },
            );
        }
        self.status_message = if horizontal {
            "Flipped horizontal".into()
        } else {
            "Flipped vertical".into()
        };
    }

    fn layer_editable(&self) -> bool {

        self.project
            .document
            .active_layer()
            .is_some_and(|l| l.visible && !l.locked)
    }

    fn process_file_dialogs(&mut self) {
        #[cfg(target_os = "android")]
        {
            if self.pending_open_svg
                || self.pending_open_project
                || self.pending_save_project
                || self.pending_export_svg
                || self.pending_export_image
            {
                self.pending_open_svg = false;
                self.pending_open_project = false;
                self.pending_save_project = false;
                #[cfg(not(target_os = "android"))]
                {
                    self.pending_mcp_bulk_rects.clear();
                    self.mcp_bulk_staging.clear();
                }
                self.pending_export_svg = false;
                self.pending_export_image = false;
                self.status_message =
                    "Project/SVG file dialogs are not available on Android yet".into();
            }
            return;
        }
        #[cfg(not(target_os = "android"))]
        {
            if self.pending_open_project {
                self.pending_open_project = false;
                self.cache_current_project_for_open();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Vadadee Berry project", &[io::PROJECT_FILE_EXTENSION])
                    .pick_file()
                {
                    match io::load_project(&path) {
                        Ok(p) => {
                            self.project = p;
                            self.project_save_path = Some(path.clone());
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                self.project.document.title = stem.to_string();
                            }
                            self.selection.clear();
                            self.history.clear();
                            self.viewport.pan = egui::vec2(48.0, 48.0);
                            self.viewport.zoom = 0.85;
                            self.refresh_all_media_layer_durations();
                            self.status_message = format!("Opened project: {}", path.display());
                        }
                        Err(e) => self.status_message = format!("Open project failed: {e}"),
                    }
                }
            }
            if self.pending_open_svg {
                self.pending_open_svg = false;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .pick_file()
                {
                    match io::import_svg(&path) {
                        Ok(mut p) => {
                            let before = snapshot_project(&self.project);
                            p.document.title = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("SVG")
                                .to_string();
                            let after = p;
                            self.history.push(
                                &mut self.project,
                                ProjectEdit::SetDocument { before, after },
                            );
                            self.selection.clear();
                            self.status_message = format!("Opened {}", path.display());
                        }
                        Err(e) => self.status_message = format!("Open failed: {e}"),
                    }
                }
            }
            if self.pending_save_project {
                self.pending_save_project = false;
                if let Some(path) = self.project_save_path.clone() {
                    match self.save_project_to_path(&path) {
                        Ok(()) => self.status_message = format!("Saved {}", path.display()),
                        Err(e) => self.status_message = format!("Save failed: {e}"),
                    }
                } else {
                    let default_name = io::default_project_filename(&self.project.document.title);
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(&default_name)
                        .add_filter("Vadadee Berry project", &[io::PROJECT_FILE_EXTENSION])
                        .save_file()
                    {
                        match self.save_project_to_path(&path) {
                            Ok(()) => self.status_message = format!("Saved {}", path.display()),
                            Err(e) => self.status_message = format!("Save failed: {e}"),
                        }
                    }
                }
            }
            if self.pending_export_svg {
                self.pending_export_svg = false;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .save_file()
                {
                    match io::export_svg(&path, &self.project) {
                        Ok(()) => self.status_message = format!("Exported {}", path.display()),
                        Err(e) => self.status_message = format!("Export failed: {e}"),
                    }
                }
            }
            if self.pending_export_image {
                self.pending_export_image = false;
                let fmt = self.export_image_format;
                let ext = fmt.extension();
                let scale = 1.0f32;
                if self.export_image_selection_only {
                    let Some(bounds) = self.selection_bounds() else {
                        self.status_message = "Export image: nothing selected".into();
                        return;
                    };
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("selection.{ext}"))
                        .add_filter("Image", &[ext])
                        .save_file()
                    {
                        match io::export_selection_raster(
                            &self.project,
                            &self.selection,
                            bounds,
                            fmt,
                            scale,
                            &path,
                        ) {
                            Ok(()) => self.status_message = format!("Exported {}", path.display()),
                            Err(e) => self.status_message = format!("Export failed: {e}"),
                        }
                    }
                } else if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!("export.{ext}"))
                    .add_filter("Image", &[ext])
                    .save_file()
                {
                    match io::export_document_raster(&self.project, fmt, scale, &path) {
                        Ok(()) => self.status_message = format!("Exported {}", path.display()),
                        Err(e) => self.status_message = format!("Export failed: {e}"),
                    }
                }
            }
        }
    }

    fn object_clipboard_blocked(&self, ctx: &Context) -> bool {
        self.on_page_text_edit.is_some()
            || ctx.text_edit_focused()
            || (self.show_shader_editor_window.is_some() && ctx.memory(|mem| mem.has_focus(egui::Id::new("shader_editor_text"))))
            || ctx.memory(|mem| mem.has_focus(egui::Id::new("sidebar_shader_editor_text")))
    }

    fn handle_text_paste_fallback(&mut self, ctx: &Context) {
        if !self.object_clipboard_blocked(ctx) {
            return;
        }

        ctx.input_mut(|i| {
            let has_cmd = i.modifiers.command || i.modifiers.ctrl;
            let already_has_paste = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
            if already_has_paste {
                return;
            }

            let mut paste_pressed = false;
            for event in &i.events {
                if let egui::Event::Key { key: egui::Key::V, pressed: true, .. } = event {
                    if has_cmd {
                        paste_pressed = true;
                        break;
                    }
                }
            }

            // Never inject paste for Ctrl+Shift+V (reserved for flip when selection exists).
            if paste_pressed && !i.modifiers.shift {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(text) = cb.get_text() {
                        i.events.push(egui::Event::Paste(text));
                        let _ = i.consume_key(egui::Modifiers::COMMAND, egui::Key::V);
                        let _ = i.consume_key(egui::Modifiers::CTRL, egui::Key::V);
                    }
                }
            }
        });
    }

    /// Called early in chrome (right after menubar) so that state changes from
    /// keyboard C/V/X are visible in the same frame's status_bar and canvas_ui,
    /// exactly like when the user clicks the menubar items.
    /// Returns `true` when paste was triggered from an egui input event this frame.
    pub fn handle_object_clipboard_shortcuts(&mut self, ctx: &Context) -> bool {
        if self.object_clipboard_blocked(ctx) {
            return false;
        }

        let has_selection = !self.selection.is_empty();

        // egui-winit turns Ctrl+C/V/X into Event::Copy/Cut/Paste (not Event::Key), so we must
        // listen for both. Ctrl+D/Z still arrive as Key events, which is why those worked.
        //
        // Ctrl+Shift+V with a selection is **flip vertical**, never paste.
        // Ctrl+Shift+H with a selection is **flip horizontal**.
        let (want_copy, want_copy_png, want_cut, want_paste, want_flip_h, want_flip_v) =
            ctx.input(|i| {
                let has_cmd = i.modifiers.command || i.modifiers.ctrl;
                let has_shift = i.modifiers.shift;
                let mut copy = false;
                let mut copy_png = false;
                let mut copy_png_from_key = false;
                let mut cut = false;
                let mut paste = false;
                let mut flip_h = false;
                let mut flip_v = false;
                for event in &i.events {
                    match event {
                        Event::Copy => {
                            if has_shift || copy_png_from_key {
                                copy_png = true;
                            } else {
                                copy = true;
                            }
                        }
                        Event::Cut => cut = true,
                        Event::Paste(_) => {
                            // OS may emit Paste for Ctrl+Shift+V — never paste when
                            // selection is non-empty and Shift is held (flip instead).
                            if has_shift && has_selection {
                                flip_v = true;
                            } else if !has_shift {
                                paste = true;
                            }
                            // Shift + no selection: ignore (do not paste)
                        }
                        Event::Key {
                            key: Key::C,
                            pressed: true,
                            ..
                        } if has_cmd => {
                            if has_shift {
                                copy_png = true;
                                copy_png_from_key = true;
                            } else {
                                copy = true;
                            }
                        }
                        Event::Key {
                            key: Key::X,
                            pressed: true,
                            ..
                        } if has_cmd => cut = true,
                        Event::Key {
                            key: Key::V,
                            pressed: true,
                            ..
                        } if has_cmd => {
                            if has_shift && has_selection {
                                flip_v = true;
                            } else if !has_shift {
                                paste = true;
                            }
                            // Ctrl+Shift+V, no selection → neither paste nor flip
                        }
                        Event::Key {
                            key: Key::H,
                            pressed: true,
                            ..
                        } if has_cmd && has_shift && has_selection => {
                            flip_h = true;
                        }
                        _ => {}
                    }
                }
                // Also catch held modifiers + key_pressed when events were already filtered.
                if has_cmd && has_shift && has_selection {
                    if i.key_pressed(Key::V) {
                        flip_v = true;
                        paste = false;
                    }
                    if i.key_pressed(Key::H) {
                        flip_h = true;
                    }
                }
                (copy, copy_png, cut, paste, flip_h, flip_v)
            });

        if !(want_copy
            || want_copy_png
            || want_cut
            || want_paste
            || want_flip_h
            || want_flip_v)
        {
            return false;
        }

        ctx.input_mut(|i| {
            i.events.retain(|event| {
                !matches!(
                    event,
                    Event::Copy | Event::Cut | Event::Paste(_)
                )
            });
            if want_copy {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::C);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::C);
            }
            if want_copy_png {
                let _ = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::C);
                let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::C);
            }
            if want_cut {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::X);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::X);
            }
            if want_paste {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::V);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::V);
            }
            if want_flip_v {
                let _ = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::V);
                let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::V);
            }
            if want_flip_h {
                let _ = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::H);
                let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::H);
            }
        });

        // Flip takes priority over paste when selection is non-empty.
        if want_flip_h {
            self.flip_selection(true);
            ctx.request_repaint();
            return false;
        }
        if want_flip_v {
            self.flip_selection(false);
            ctx.request_repaint();
            return false;
        }

        if want_copy_png && !self.selection.is_empty() {
            log::info!("CLIPBOARD: detected copy PNG shortcut");
            self.copy_selection_as_png(ctx.pixels_per_point());
            ctx.request_repaint();
            return false;
        }
        if want_copy && !want_copy_png {
            log::info!("CLIPBOARD: detected copy shortcut");
            self.copy_selection();
            let txt = if self.clipboard.len() == 1 {
                self.clipboard[0].name.clone()
            } else {
                format!("{} objects", self.clipboard.len())
            };
            ctx.output_mut(|o| {
                o.commands.push(egui::OutputCommand::CopyText(txt));
            });
            ctx.request_repaint();
            return false;
        }
        if want_cut {
            log::info!("CLIPBOARD: detected cut shortcut");
            self.cut_selection();
            let txt = if self.clipboard.len() == 1 {
                self.clipboard[0].name.clone()
            } else {
                format!("{} objects", self.clipboard.len())
            };
            ctx.output_mut(|o| {
                o.commands.push(egui::OutputCommand::CopyText(txt));
            });
            ctx.request_repaint();
            return false;
        }
        if want_paste {
            log::info!("CLIPBOARD: detected paste shortcut");
            self.paste_clipboard(false);
            ctx.request_repaint();
            return true;
        }
        false
    }

    /// egui-winit drops Ctrl+V when the clipboard has only image/png (no text), so no
    /// Event::Paste or Key::V reaches egui. Poll the physical hotkey as a fallback.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn handle_paste_hotkey_fallback(&mut self, ctx: &Context, events_handled_paste: bool) {
        if !ctx.input(|i| i.focused) {
            return;
        }

        use device_query::{DeviceQuery, DeviceState, Keycode};

        let keys = DeviceState::new().get_keys();
        let ctrl = keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl);
        let shift = keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
        let v = keys.contains(&Keycode::V);
        // Only plain Ctrl+V is paste. Ctrl+Shift+V is flip when objects are selected.
        let down = v && ctrl && !shift;
        let edge = down && !self.paste_hotkey_was_down;
        self.paste_hotkey_was_down = down;

        if events_handled_paste || self.object_clipboard_blocked(ctx) {
            return;
        }
        if edge {
            log::info!("CLIPBOARD: paste hotkey fallback (image-only system clipboard)");
            self.paste_clipboard(true);
            ctx.request_repaint();
        }
    }

    fn keyboard_shortcuts(&mut self, ctx: &Context) {
        let text_focused = ctx.text_edit_focused();
        let mut bubble_keys_handled = false;
        ctx.input_mut(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            if cmd && self.collab.is_connected() {
                if i.key_pressed(Key::M) {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::M);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::M);
                    self.cursor_bubble_edit = !self.cursor_bubble_edit;
                    if self.cursor_bubble_edit {
                        self.ensure_cursor_doc_for_collab_bubble(ctx);
                        self.cursor_bubble_focus_pending = true;
                    } else {
                        self.cursor_bubble_text.clear();
                    }
                    self.collab_last_cursor_sent = None;
                    bubble_keys_handled = true;
                }
                if i.key_pressed(Key::Backspace) && self.cursor_bubble_edit {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Backspace);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Backspace);
                    self.cursor_bubble_text.clear();
                    self.collab_last_cursor_sent = None;
                    bubble_keys_handled = true;
                }
            }
        });
        if bubble_keys_handled {
            ctx.request_repaint();
        }
        if self.cursor_bubble_edit {
            return;
        }
        ctx.input_mut(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            if cmd {
                if i.modifiers.shift && i.key_pressed(Key::R) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::R);
                    let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::R);
                    self.resize_to_selection();
                }
                if i.modifiers.shift && i.key_pressed(Key::Z) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Z);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Z);
                    self.do_redo();
                } else if i.key_pressed(Key::Z) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Z);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Z);
                    if self.tools.active == ToolKind::Pen && !self.tools.pen.is_empty() {
                        self.tools.pen.pop_anchor();
                        self.status_message = if self.tools.pen.is_empty() {
                            "Polyline cleared — Esc to exit pen".into()
                        } else {
                            format!(
                                "Removed point ({} remaining)",
                                self.tools.pen.len()
                            )
                        };
                    } else {
                        self.do_undo();
                    }
                }
                if i.key_pressed(Key::Y) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Y);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Y);
                    self.do_redo();
                }
                if i.key_pressed(Key::O) {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::O);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::O);
                    self.request_open_project();
                }
                if i.key_pressed(Key::S) {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::S);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::S);
                    self.request_save_project();
                }
                if i.key_pressed(Key::N) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::N);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::N);
                    self.new_document();
                }
                if i.key_pressed(Key::D) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::D);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::D);
                    self.duplicate_selection();
                }
                // Flip: Ctrl+Shift+H / Ctrl+Shift+V (not Ctrl+V paste — requires Shift).
                if i.modifiers.shift && i.key_pressed(Key::H) && !text_focused {
                    let _ = i.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        Key::H,
                    );
                    let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::H);
                    self.flip_selection(true);
                }
                if i.modifiers.shift && i.key_pressed(Key::V) && !text_focused {
                    let _ = i.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        Key::V,
                    );
                    let _ = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::V);
                    self.flip_selection(false);
                }
            }
            if i.key_pressed(Key::Enter) && self.tools.active == ToolKind::Pen {
                self.finish_pen_path(self.tools.pen.was_closed);
            } else if i.key_pressed(Key::Escape) {
                // Prefer closing modal dialogs over tool cancel / deselect.
                if self.hit_pick_menu.is_some() {
                    self.hit_pick_menu = None;
                } else if self.object_rename_dialog.is_some() {
                    self.object_rename_dialog = None;
                } else if self.anim_stack_formula_dialog.is_some() {
                    self.anim_stack_formula_dialog = None;
                    self.anim_stack_formula_draft.clear();
                } else if self.plotter_formula_dialog.is_some() {
                    self.plotter_formula_dialog = None;
                    self.plotter_formula_draft.clear();
                } else if self.show_shader_editor_window.is_some() {
                    self.show_shader_editor_window = None;
                } else if self.video_export.progress_visible {
                    // Don't abort render mid-export; only hide the dialog if idle.
                    if !self.video_export.rendering {
                        self.video_export.progress_visible = false;
                    }
                } else if self.on_page_text_edit.is_some() {
                    self.finish_on_page_text_edit();
                } else if self.node_editor_ui.open_layer_id.is_some() {
                    // Node Editor owns Esc (unfocus text field, then close dialog).
                    // Do not cancel tools / deselect canvas objects underneath.
                } else if self.tools.weight_flow.stroke.is_some() {
                    // Cancel in-progress sculpt (restore path) without full deselect.
                    self.cancel_weight_flow_stroke(true);
                    self.status_message = "Weight flow stroke cancelled".into();
                } else if self.tools.weight_flow.enabled {
                    // Keyboard exit from weight-flow mode so Select works again.
                    self.tools.weight_flow.enabled = false;
                    self.tools.weight_flow.cancel_stroke();
                    self.status_message = "Weight flow brush off".into();
                } else {
                    self.cancel_tool_to_select();
                }
            } else if (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
                && self.tools.active == ToolKind::Pen
                && !self.tools.pen.is_empty()
                && !text_focused
            {
                self.tools.pen.pop_anchor();
                self.status_message = if self.tools.pen.is_empty() {
                    "Polyline cleared — Esc to exit pen".into()
                } else {
                    format!(
                        "Removed point ({} remaining)",
                        self.tools.pen.len()
                    )
                };
            } else if (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) && !text_focused
            {
                // Node Editor dialog handles Delete/Backspace for graph nodes itself.
                if self.node_editor_ui.open_layer_id.is_some() {
                    // leave key for node_editor_ui
                } else if let Some((node_id, track_lbl, frame)) = self.anim_selected_keyframe.clone() {
                    self.delete_keyframe(node_id, &track_lbl, frame);
                } else if self.tools.active == ToolKind::Node
                    && !self.tools.select.selected_path_points.is_empty()
                    && self.remove_selected_path_points()
                {
                    // removed path anchors
                } else if !self.try_delete_focused_gradient_stop() {
                    self.delete_selection();
                }
            }

            if !text_focused {
                // Play/pause on Space
                if i.key_pressed(Key::Space) {
                    self.anim_is_playing = !self.anim_is_playing;
                    if self.anim_is_playing {
                        let now = std::time::Instant::now();
                        self.anim_playback_wall = Some(now);
                        self.anim_play_origin = Some((now, self.anim_current_frame));
                        self.anim_time_accumulator = 0.0;
                    } else {
                        self.anim_playback_wall = None;
                        self.anim_play_origin = None;
                        self.stop_all_video_streams();
                    }
                    let _ = i.consume_key(egui::Modifiers::NONE, Key::Space);
                }
                
                // Back to start on Ctrl + Left Arrow
                if cmd && i.key_pressed(Key::ArrowLeft) {
                    self.anim_current_frame = 0;
                    self.anim_is_playing = false;
                    self.anim_playback_wall = None;
                    self.anim_play_origin = None;
                    self.stop_all_video_streams();
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::ArrowLeft);
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::ArrowLeft);
                }

                // Ctrl+Up / Ctrl+Down: raise / lower object (or layer) z-order
                if cmd && i.key_pressed(Key::ArrowUp) {
                    self.nudge_z_order(1);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::ArrowUp);
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::ArrowUp);
                }
                if cmd && i.key_pressed(Key::ArrowDown) {
                    self.nudge_z_order(-1);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::ArrowDown);
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::ArrowDown);
                }

                // Arrow keys nudging selected objects / points
                let mut nudge_dx: f64 = 0.0;
                let mut nudge_dy: f64 = 0.0;
                let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                
                if i.key_pressed(Key::ArrowLeft) && !cmd {
                    nudge_dx = -step;
                    let _ = i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                    let _ = i.consume_key(egui::Modifiers::SHIFT, Key::ArrowLeft);
                }
                if i.key_pressed(Key::ArrowRight) && !cmd {
                    nudge_dx = step;
                    let _ = i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                    let _ = i.consume_key(egui::Modifiers::SHIFT, Key::ArrowRight);
                }
                if i.key_pressed(Key::ArrowUp) && !cmd {
                    nudge_dy = -step;
                    let _ = i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                    let _ = i.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp);
                }
                if i.key_pressed(Key::ArrowDown) && !cmd {
                    nudge_dy = step;
                    let _ = i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                    let _ = i.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown);
                }

                if nudge_dx.abs() > 1e-5 || nudge_dy.abs() > 1e-5 {
                    if self.tools.active == ToolKind::Node && !self.tools.select.selected_path_points.is_empty() {
                        // Nudge selected path points
                        for (id, pi) in self.tools.select.selected_path_points.clone() {
                            if let Some(before) = self.project.nodes.get(id).cloned() {
                                let mut after = before.clone();
                                if let NodeKind::Path { path } = &mut after.kind {
                                    path.move_anchors_by(&[pi], nudge_dx, nudge_dy);
                                }
                                if before != after {
                                    if let Some(node_mut) = self.project.nodes.get_mut(id) {
                                        *node_mut = after.clone();
                                    }
                                    self.history.push(
                                        &mut self.project,
                                        ProjectEdit::PatchNode { id, before, after },
                                    );
                                }
                            }
                        }
                    } else if !self.selection.is_empty() {
                        // Nudge selected objects
                        for id in self.selection.clone() {
                            if let Some(before) = self.project.nodes.get(id).cloned() {
                                let mut after = before.clone();
                                let child_ids = if let NodeKind::Group { children } = &after.kind {
                                    Some(children.clone())
                                } else {
                                    None
                                };
                                if let Some(kids) = child_ids {
                                    for cid in kids {
                                        if let Some(c_before) = self.project.nodes.get(cid).cloned() {
                                            let mut c_after = c_before.clone();
                                            c_after.translate(nudge_dx, nudge_dy);
                                            if c_before != c_after {
                                                if let Some(c_mut) = self.project.nodes.get_mut(cid) {
                                                    *c_mut = c_after.clone();
                                                }
                                                self.history.push(
                                                    &mut self.project,
                                                    ProjectEdit::PatchNode { id: cid, before: c_before, after: c_after },
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    after.translate(nudge_dx, nudge_dy);
                                    if before != after {
                                        if let Some(node_mut) = self.project.nodes.get_mut(id) {
                                            *node_mut = after.clone();
                                        }
                                        self.history.push(
                                            &mut self.project,
                                            ProjectEdit::PatchNode { id, before, after },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Cancel current tool interaction and switch to Select.
    /// For Pen: zero the points (cancel polyline immediately).
    pub fn cancel_tool_to_select(&mut self) {
        let was_pen = self.tools.active == ToolKind::Pen;
        if was_pen {
            self.tools.pen.anchors.clear();
            self.tools.pen.smooth_anchors.clear();
            self.tools.pen.handle_out_offset.clear();
            self.tools.pen.handle_in_offset.clear();
            self.tools.pen.curve_adjust = None;
        }
        self.tools.pen = Default::default();
        self.tools.drag_shape = None;
        self.tools.select.marquee = None;
        self.tools.select.drag_snapshot.clear();
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
        self.tools.select.drag_mode = None;
        // Always leave weight-flow so Select is usable after keyboard cancel.
        if self.tools.weight_flow.enabled || self.tools.weight_flow.stroke.is_some() {
            self.cancel_weight_flow_stroke(true);
            self.tools.weight_flow.enabled = false;
        }
        if self.tools.active != ToolKind::Select {
            if self.tools.active != ToolKind::Eyedropper {
                self.tools.last_active_tool = self.tools.active;
            }
            self.tools.active = ToolKind::Select;
            self.status_message = if was_pen {
                "Pen cancelled".into()
            } else {
                "Select".into()
            };
        } else {
            // Select tool + Esc: clear sticky selection so another object can be picked.
            self.selection.clear();
            self.selection_sticky = false;
            self.hit_pick_menu = None;
            self.tools.select.select_rotation_mode = false;
            self.status_message = "Deselected".into();
        }
    }

    /// Abort weight-flow stroke; if `restore`, put path back to pre-stroke snapshot.
    fn cancel_weight_flow_stroke(&mut self, restore: bool) {
        let Some(stroke) = self.tools.weight_flow.stroke.take() else {
            return;
        };
        if restore {
            if let Some(node) = self.project.nodes.get_mut(stroke.node_id) {
                *node = stroke.before;
            }
        }
    }

    pub fn delete_keyframe(&mut self, node_id: NodeId, track_lbl: &str, frame: usize) {
        let before_timeline = self.project.anim_timeline.clone();
        let mut updated = false;
        if let Some(anim) = self.project.anim_timeline.nodes.get_mut(&node_id) {
            if let Some(track) = anim.get_track_mut(track_lbl) {
                track.keyframes.retain(|kf| kf.frame != frame);
                if let Some((sel_node_id, ref sel_track_lbl, sel_frame)) = self.anim_selected_keyframe {
                    if sel_node_id == node_id && sel_track_lbl == track_lbl && sel_frame == frame {
                        self.anim_selected_keyframe = None;
                    }
                }
                self.apply_animation_for_frame(self.anim_current_frame);
                updated = true;
            }
        }
        if updated {
            let after_timeline = self.project.anim_timeline.clone();
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchTimeline { before: before_timeline, after: after_timeline },
            );
        }
    }

    pub fn get_node_geom_track_name(&self, id: NodeId, idx: usize) -> String {
        let Some(node) = self.project.nodes.get(id) else {
            return format!("Geom {}", idx);
        };
        let base_len = match &node.kind {
            NodeKind::Rect { .. } => 3,
            NodeKind::Ellipse { .. } => 2,
            NodeKind::Polygon { .. } => 3,
            NodeKind::Arc { .. } => 3,
            NodeKind::Path { path } => path.anchor_positions().len() * 6,
            NodeKind::BrushStroke { points } => points.len() * 3,
            _ => 0,
        };
        if idx < base_len {
            match &node.kind {
                NodeKind::Rect { .. } => match idx {
                    0 => "Width".to_string(),
                    1 => "Height".to_string(),
                    2 => "Corner Rad".to_string(),
                    _ => format!("Geom {}", idx),
                },
                NodeKind::Ellipse { .. } => match idx {
                    0 => "Radius X".to_string(),
                    1 => "Radius Y".to_string(),
                    _ => format!("Geom {}", idx),
                },
                NodeKind::Polygon { .. } => match idx {
                    0 => "Radius".to_string(),
                    1 => "Sides".to_string(),
                    2 => "Rotation".to_string(),
                    _ => format!("Geom {}", idx),
                },
                NodeKind::Arc { .. } => match idx {
                    0 => "Radius".to_string(),
                    1 => "Start Ang".to_string(),
                    2 => "Sweep Ang".to_string(),
                    _ => format!("Geom {}", idx),
                },
                NodeKind::Path { .. } => {
                    let pt_idx = idx / 6;
                    match idx % 6 {
                        0 => format!("Pt {} X", pt_idx),
                        1 => format!("Pt {} Y", pt_idx),
                        2 => format!("Pt {} Out X", pt_idx),
                        3 => format!("Pt {} Out Y", pt_idx),
                        4 => format!("Pt {} In X", pt_idx),
                        5 => format!("Pt {} In Y", pt_idx),
                        _ => unreachable!(),
                    }
                }
                NodeKind::BrushStroke { .. } => {
                    let pt_idx = idx / 3;
                    match idx % 3 {
                        0 => format!("Stroke {} X", pt_idx),
                        1 => format!("Stroke {} Y", pt_idx),
                        _ => format!("Stroke {} W", pt_idx),
                    }
                }
                _ => format!("Geom {}", idx),
            }
        } else {
            let floats = node.get_geom_floats();
            if idx < floats.len() {
                let marker = floats[base_len];
                if marker == 1.0 {
                    let local = idx - base_len;
                    match local {
                        0 => "Fill Mode".to_string(),
                        1 => "Grad Angle".to_string(),
                        2 => "Grad X0".to_string(),
                        3 => "Grad Y0".to_string(),
                        4 => "Grad X1".to_string(),
                        5 => "Grad Y1".to_string(),
                        6 => "Grad Stops Count".to_string(),
                        _ => {
                            let stop_idx = (local - 7) / 5;
                            match (local - 7) % 5 {
                                0 => format!("Stop {} Pos", stop_idx),
                                1 => format!("Stop {} R", stop_idx),
                                2 => format!("Stop {} G", stop_idx),
                                3 => format!("Stop {} B", stop_idx),
                                4 => format!("Stop {} A", stop_idx),
                                _ => unreachable!(),
                            }
                        }
                    }
                } else if marker == 2.0 {
                    let local = idx - base_len;
                    match local {
                        0 => "Fill Mode".to_string(),
                        1 => "Grad Center X".to_string(),
                        2 => "Grad Center Y".to_string(),
                        3 => "Grad Stops Count".to_string(),
                        _ => {
                            let stop_idx = (local - 4) / 5;
                            match (local - 4) % 5 {
                                0 => format!("Stop {} Pos", stop_idx),
                                1 => format!("Stop {} R", stop_idx),
                                2 => format!("Stop {} G", stop_idx),
                                3 => format!("Stop {} B", stop_idx),
                                4 => format!("Stop {} A", stop_idx),
                                _ => unreachable!(),
                            }
                        }
                    }
                } else {
                    format!("Geom {}", idx)
                }
            } else {
                format!("Geom {}", idx)
            }
        }
    }

    pub fn delete_selection_public(&mut self) {
        self.delete_selection();
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }

        // Handle deleting AV clips or Music clips inside layers
        let mut clip_removed = false;
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        for layer in &mut after_doc.layers {
            let initial_av_len = layer.av_clips.len();
            layer.av_clips.retain(|c| !self.selection.contains(&c.id));
            if layer.av_clips.len() != initial_av_len {
                clip_removed = true;
                layer.sync_legacy_from_primary_clip();
            }

            let initial_music_len = layer.music_clips.len();
            layer.music_clips.retain(|c| !self.selection.contains(&c.id));
            if layer.music_clips.len() != initial_music_len {
                clip_removed = true;
            }
        }
        if clip_removed {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchDocument {
                    before: before_doc,
                    after: after_doc,
                },
            );
            self.selection.clear();
            self.sync_inspector_from_selection();
            return;
        }

        let mut layer_positions: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|id| {
                self.project
                    .document
                    .layers
                    .iter()
                    .position(|l| l.id == *id)
            })
            .collect();
        layer_positions.sort_unstable_by(|a, b| b.cmp(a));
        let mut layer_deleted = false;
        for pos in layer_positions {
            self.delete_layer(pos);
            layer_deleted = true;
        }
        if layer_deleted {
            self.selection.clear();
            self.sync_inspector_from_selection();
            return;
        }

        if !self.layer_editable() {
            return;
        }
        let layer_index = self.project.document.active_layer_index;
        let layer_nodes_before = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut removed = Vec::new();
        let mut removed_anims = Vec::new();
        for id in &self.selection {
            if let Some(node) = self.project.nodes.get(*id).cloned() {
                removed.push((*id, node));
            }
            if let Some(anim) = self.project.anim_timeline.nodes.get(id).cloned() {
                removed_anims.push((*id, anim));
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::RemoveNodes {
                removed,
                removed_anims,
                layer_index,
                layer_nodes_before,
                ne_proxy_before: Vec::new(),
            },
        );
        self.selection.clear();
        self.sync_flowchart_paths_if_active_layer();
    }

    fn insert_node(&mut self, node: Node) {
        let id = node.id;
        self.history
            .push(&mut self.project, ProjectEdit::InsertNode { node });
        self.selection = vec![id];
        self.sync_inspector_from_selection();
        // Ensure flowchart connectors re-route + slots rebalanced when new nodes/paths added
        self.rebalance_active_flowchart_layer_if_any();
    }

    /// Bulk insert many nodes as a *single* history entry.
    /// Dramatically faster and less UI churn than thousands of individual inserts.
    fn insert_nodes_batch(&mut self, nodes: Vec<Node>) {
        if nodes.is_empty() {
            return;
        }
        self.history
            .push(&mut self.project, ProjectEdit::InsertNodes { nodes: nodes.clone() });
        // Select the last one (consistent with single insert behavior)
        if let Some(last) = nodes.last() {
            self.selection = vec![last.id];
        }
        self.sync_inspector_from_selection();
    }

    fn mcp_bulk_active(&self) -> bool {
        #[cfg(not(target_os = "android"))]
        {
            !self.pending_mcp_bulk_rects.is_empty() || !self.mcp_bulk_staging.is_empty()
        }
        #[cfg(target_os = "android")]
        {
            false
        }
    }

    fn apply_nodes_live(project: &mut ProjectFile, nodes: &[Node]) {
        for node in nodes {
            let id = project.nodes.insert(node.clone());
            project.document.append_to_active_layer(id);
        }
    }

    fn rebuild_spatial_index(&mut self) {
        let revision = self.history.revision();
        let hidden = self.hidden_canvas_sources();
        self.spatial_index =
            crate::spatial_index::SpatialIndex::rebuild(&self.project, &hidden, revision);
        self.cached_draw_order = if self.spatial_index.is_enabled() {
            self.spatial_index.flat_order().to_vec()
        } else {
            self.project.document.ordered_node_ids()
        };
        self.cached_draw_order_revision = revision;
    }

    fn draw_order_cached(&mut self) -> &[NodeId] {
        let revision = self.history.revision();
        if self.cached_draw_order_revision != revision {
            self.rebuild_spatial_index();
        }
        &self.cached_draw_order
    }

    pub fn is_bulk_selection(&self) -> bool {
        self.selection.len() >= crate::perf::BULK_SELECTION_THRESHOLD
    }

    fn sync_inspector_if_needed(&mut self) {
        if self.is_bulk_selection() {
            self.status_message = format!(
                "{} objects selected — bulk mode (single undo on move)",
                self.selection.len()
            );
            return;
        }
        self.sync_inspector_from_selection();
    }

    fn setup_bulk_drag_if_needed(&mut self, selection: &[NodeId]) {
        if selection.len() < crate::perf::BULK_SELECTION_THRESHOLD {
            return;
        }
        let mut ids = Vec::with_capacity(selection.len());
        let mut origins = Vec::with_capacity(selection.len());
        for &id in selection {
            if let Some(node) = self.project.nodes.get(id) {
                let b = node.bounds();
                ids.push(id);
                origins.push((b.x0, b.y0));
            }
        }
        self.tools.select.bulk_drag = Some(crate::tools::BulkDrag {
            ids,
            origins,
            preview_dx: 0.0,
            preview_dy: 0.0,
        });
        self.tools.select.drag_snapshot.clear();
    }

    fn apply_bulk_move_preview(&mut self, dx: f64, dy: f64) {
        let Some(bulk) = self.tools.select.bulk_drag.as_mut() else {
            return;
        };
        bulk.preview_dx = dx;
        bulk.preview_dy = dy;
        for (i, &id) in bulk.ids.iter().enumerate() {
            let Some((ox, oy)) = bulk.origins.get(i).copied() else {
                continue;
            };
            if let Some(node) = self.project.nodes.get_mut(id) {
                let b = node.bounds();
                let w = b.width();
                let h = b.height();
                node.set_bounds(kurbo::Rect::new(ox + dx, oy + dy, ox + dx + w, oy + dy + h));
            }
        }
        // Lively update attached flowchart lines during bulk node drag preview
        self.sync_flowchart_paths_if_active_layer();
    }

    fn revert_bulk_move_preview(&mut self) {
        let Some(bulk) = self.tools.select.bulk_drag.as_ref() else {
            return;
        };
        for (i, &id) in bulk.ids.iter().enumerate() {
            let Some((ox, oy)) = bulk.origins.get(i).copied() else {
                continue;
            };
            if let Some(node) = self.project.nodes.get_mut(id) {
                let b = node.bounds();
                let w = b.width();
                let h = b.height();
                node.set_bounds(kurbo::Rect::new(ox, oy, ox + w, oy + h));
            }
        }
        // Restore connector routes based on reverted node positions
        self.sync_flowchart_paths_if_active_layer();
    }

    fn commit_bulk_drag(&mut self, dx: f64, dy: f64) {
        let Some(bulk) = self.tools.select.bulk_drag.take() else {
            return;
        };
        if dx.hypot(dy) < 0.001 {
            return;
        }
        let mut patches = Vec::with_capacity(bulk.ids.len());
        for (i, &id) in bulk.ids.iter().enumerate() {
            let Some((ox, oy)) = bulk.origins.get(i).copied() else {
                continue;
            };
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            let b = node.bounds();
            let w = b.width();
            let h = b.height();
            let mut before = node.clone();
            before.set_bounds(kurbo::Rect::new(ox, oy, ox + w, oy + h));
            let mut after = before.clone();
            after.translate(dx, dy);
            if before != after {
                patches.push((id, before, after));
            }
        }
        if !patches.is_empty() {
            let ids: Vec<NodeId> = patches.iter().map(|(id, _, _)| *id).collect();
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNodes { patches },
            );
            for id in ids {
                self.sync_anim_transform_from_node(id);
            }
        }
        self.sync_flowchart_paths_if_active_layer();
    }

    pub fn split_active_av_clip_at_playhead(&mut self) {
        const MIN_SPLIT_SEC: f32 = 0.1;
        let idx = self.project.document.active_layer_index;
        let play_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
        let Some(layer) = self.project.document.layers.get_mut(idx) else {
            return;
        };
        if layer.kind != crate::document::LayerKind::AV {
            self.status_message = "Select an AV layer to split".into();
            return;
        }
        layer.ensure_av_clips();
        let clip_pos = layer.av_clips.iter().position(|c| {
            play_sec > c.video_timeline_start + MIN_SPLIT_SEC
                && play_sec < c.timeline_end_secs() - MIN_SPLIT_SEC
        });
        let Some(clip_idx) = clip_pos else {
            self.status_message = "Playhead must be inside a clip".into();
            return;
        };
        let clip = layer.av_clips[clip_idx].clone();
        let left_len = play_sec - clip.video_timeline_start;
        let right_start = play_sec;
        let right_offset = clip.video_start_offset + left_len;
        let right_len = clip.timeline_play_secs() - left_len;

        let mut right = clip.clone();
        right.id = uuid::Uuid::new_v4();
        right.name = format!("{} (split)", clip.name);
        right.video_timeline_start = right_start;
        right.video_start_offset = right_offset;
        right.video_play_length = right_len.max(MIN_SPLIT_SEC);
        right.track_row = clip.track_row;

        if let Some(c) = layer.av_clips.get_mut(clip_idx) {
            c.video_play_length = left_len.max(MIN_SPLIT_SEC);
        }
        layer.av_clips.insert(clip_idx + 1, right);
        layer.sync_legacy_from_primary_clip();
        self.status_message = format!("Split clip at {:.2}s", play_sec);
    }

    pub fn create_music_clip_at_playhead(&mut self) {
        self.create_daw_clip_at_playhead();
    }

    /// Create a 1s DAW node on a DAW-role AV layer (creates the layer if needed).
    pub fn create_daw_clip_at_playhead(&mut self) {
        let play_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let n = after
            .layers
            .iter()
            .filter(|l| l.kind == crate::document::LayerKind::AV && l.av_role == crate::document::AvRole::Daw)
            .map(|l| l.music_clips.len())
            .sum::<usize>()
            + 1;
        let idx = after.ensure_av_role_layer(
            crate::document::AvRole::Daw,
            &format!("DAW {}", after.layers.iter().filter(|l| l.av_role == crate::document::AvRole::Daw).count().max(1)),
        );
        let Some(layer) = after.layers.get_mut(idx) else {
            return;
        };
        layer.av_role = crate::document::AvRole::Daw;
        layer.ensure_av_clips();
        // Append to end of DAW queue (after last media/DAW on this layer).
        let start = crate::av_ui::queue_append_start_sec(layer).max(play_sec);
        let mut clip =
            crate::document::MusicClip::new_empty(format!("DAW {n}"), start, 1.0);
        clip.track_row = 0;
        let id = clip.id;
        layer.music_clips.push(clip);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        self.piano_roll_clip = Some(id);
        self.status_message = format!("Created 1s DAW node at {:.2}s", play_sec);
    }

    /// Pick topmost node at `doc`. Ghosts (boolean/clip hidden sources) are skipped unless
    /// `include_ghosts` (Ctrl+Shift+click or Objects tab).
    /// Apply a choice from the multi-hit object picker overlay.
    pub fn select_from_hit_picker(&mut self, id: NodeId) {
        self.selection = vec![id];
        self.selection_sticky = true;
        self.tools.select.select_rotation_mode = false;
        self.hit_pick_menu = None;
        self.sync_inspector_from_selection();
        self.status_message =
            "Object selected (sticky — Esc or empty click to deselect)".into();
    }

    fn pick_node_at(&self, doc: (f64, f64), slop: f64) -> Option<NodeId> {
        self.pick_node_at_opts(doc, slop, false)
    }

    fn pick_node_at_opts(&self, doc: (f64, f64), slop: f64, include_ghosts: bool) -> Option<NodeId> {
        // Normal pick: treat visible clip composites as selectable (source is hidden ghost).
        if !include_ghosts {
            if let Some((source, _mask)) = self.pick_clip_mask_at(doc, slop) {
                return Some(source);
            }
        }
        let (hit, _) = self.pick_node_at_with_bbox_fallback_opts(doc, slop, include_ghosts);
        hit
    }

    /// Hit-test clip-mask solid faces (mask shape in doc space). Returns (source, mask).
    fn pick_clip_mask_at(&self, doc: (f64, f64), slop: f64) -> Option<(NodeId, NodeId)> {
        use kurbo::Shape;
        let pt = kurbo::Point::new(doc.0, doc.1);
        // Later effects draw on top — search in reverse insertion order.
        for cm in self.project.document.clip_masks.values().rev() {
            let Some(mask) = self.project.nodes.get(cm.mask_id) else {
                continue;
            };
            let bez = mask.bez_path();
            let hit_mask = if bez.elements().is_empty() {
                mask.bounds().inflate(slop, slop).contains(pt)
            } else {
                bez.contains(pt) || mask.bounds().inflate(slop, slop).contains(pt)
            };
            // Also accept hits on the source image rect (before texture is warm the
            // composite may be invisible, but the image footprint is still pickable).
            let hit_src = self
                .project
                .nodes
                .get(cm.source_id)
                .map(|s| s.bounds().inflate(slop, slop).contains(pt))
                .unwrap_or(false);
            if hit_mask || hit_src {
                return Some((cm.source_id, cm.mask_id));
            }
        }
        None
    }

    /// If `id` is part of a clip mask, return (source, mask) for unit selection.
    fn clip_pair_for(&self, id: NodeId) -> Option<(NodeId, NodeId)> {
        self.project
            .document
            .clip_masks
            .values()
            .find(|cm| cm.source_id == id || cm.mask_id == id)
            .map(|cm| (cm.source_id, cm.mask_id))
    }

    fn pick_node_at_with_bbox_fallback(
        &self,
        doc: (f64, f64),
        slop: f64,
    ) -> (Option<NodeId>, Option<NodeId>) {
        self.pick_node_at_with_bbox_fallback_opts(doc, slop, false)
    }

    fn pick_node_at_with_bbox_fallback_opts(
        &self,
        doc: (f64, f64),
        slop: f64,
        include_ghosts: bool,
    ) -> (Option<NodeId>, Option<NodeId>) {
        let all = self.pick_all_nodes_at(doc, slop, include_ghosts);
        let hit = all.first().copied();
        (hit, None)
    }

    /// All nodes under the pointer (topmost first). Used for multi-object hit picker.
    fn pick_all_nodes_at(
        &self,
        doc: (f64, f64),
        slop: f64,
        include_ghosts: bool,
    ) -> Vec<NodeId> {
        let hidden = if include_ghosts {
            std::collections::HashSet::new()
        } else {
            self.hidden_canvas_sources()
        };
        let mut precise: Vec<NodeId> = Vec::new();
        let mut bbox_only: Vec<NodeId> = Vec::new();
        // Topmost first (paint order reversed).
        for id in self.project.document.ordered_node_ids().into_iter().rev() {
            if hidden.contains(&id)
                && !crate::document::is_pickable_effect_source(&self.project.document, id)
            {
                continue;
            }
            if let Some(node) = self.project.nodes.get(id) {
                if !self.hit_test_node_for_pick(id, node, doc, slop) {
                    continue;
                }
                if self.precise_hit_for_pick(id, node, doc, slop) {
                    precise.push(id);
                } else if !matches!(node.kind, NodeKind::Image { .. }) {
                    bbox_only.push(id);
                }
            }
        }
        if precise.is_empty() {
            bbox_only
        } else {
            precise
        }
    }

    /// Load (or reload) texture for an Image node from its embedded bytes.
    fn ensure_image_texture(&mut self, id: NodeId, bytes: &[u8], ctx: &Context) {
        if self.image_textures.contains_key(&id) && self.image_pixel_cache.contains_key(&id) {
            return;
        }
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let rgba = dyn_img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            let handle = ctx.load_texture(
                format!("vadadee-berry-img-{}", id),
                color_image.clone(),
                egui::TextureOptions::LINEAR,
            );
            self.image_textures.insert(id, handle);
            self.image_pixel_cache.insert(id, color_image);
        }
    }

    /// Drop GPU/CPU image caches so they re-decode from `Image.bytes` (undo/redo / external edit).
    fn invalidate_image_textures(&mut self) {
        self.image_textures.clear();
        self.image_pixel_cache.clear();
    }

    /// Push RGBA8 into egui texture + pixel cache (paint preview / commit).
    fn sync_image_texture_from_rgba(
        &mut self,
        id: NodeId,
        w: u32,
        h: u32,
        rgba: &[u8],
        ctx: &Context,
    ) {
        if w == 0 || h == 0 || rgba.len() < (w as usize) * (h as usize) * 4 {
            return;
        }
        let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba);
        if let Some(tex) = self.image_textures.get_mut(&id) {
            tex.set(ci.clone(), egui::TextureOptions::LINEAR);
        } else {
            let handle = ctx.load_texture(
                format!("vadadee-berry-img-{}", id),
                ci.clone(),
                egui::TextureOptions::LINEAR,
            );
            self.image_textures.insert(id, handle);
        }
        self.image_pixel_cache.insert(id, ci);
    }

    /// Public for brush tip preview UI (same color as paint stamps).
    pub fn raster_paint_preview_color(&self) -> [u8; 4] {
        self.raster_paint_rgba()
    }

    /// Current solid paint color as RGBA8 (from fill inspector).
    fn raster_paint_rgba(&self) -> [u8; 4] {
        let p = match self.ui_fill_kind {
            crate::document::FillKind::Solid => self
                .ui_fill_stops
                .first()
                .map(|s| s.color)
                .unwrap_or_else(|| crate::document::Paint::from_hex(0x1a1f2e, 1.0)),
            _ => self
                .ui_fill_stops
                .first()
                .map(|s| s.color)
                .unwrap_or_else(|| crate::document::Paint::from_hex(0x1a1f2e, 1.0)),
        };
        [
            (p.rgba[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (p.rgba[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (p.rgba[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            (p.rgba[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }

    /// Ensure a paint target Image exists; create a transparent full-page image if needed.
    fn ensure_raster_paint_target(&mut self, ctx: &Context) -> Option<NodeId> {
        // Prefer selected Image.
        if let Some(&id) = self.selection.first() {
            let bytes_opt = self.project.nodes.get(id).and_then(|n| match &n.kind {
                NodeKind::Image { bytes, .. } => Some(bytes.clone()),
                _ => None,
            });
            if let Some(bytes) = bytes_opt {
                self.ensure_image_texture(id, &bytes, ctx);
                return Some(id);
            }
        }
        // Create new transparent paint surface (cap pixel size for interactive brush FPS).
        let doc_w = self.project.document.width.max(1.0);
        let doc_h = self.project.document.height.max(1.0);
        // Prefer ≤1536 on the long edge so live paint stays interactive on typical pages.
        const MAX_PAINT_EDGE: f64 = 1536.0;
        let long = doc_w.max(doc_h);
        let scale = if long > MAX_PAINT_EDGE {
            MAX_PAINT_EDGE / long
        } else {
            1.0
        };
        let pw = (doc_w * scale).round().clamp(1.0, MAX_PAINT_EDGE) as u32;
        let ph = (doc_h * scale).round().clamp(1.0, MAX_PAINT_EDGE) as u32;
        let bytes = crate::raster::RasterBuffer::transparent_png(pw, ph)?;
        // Image is still placed at full document size; pixels are lower-res paint buffer.
        let node = self.styled_shape_node(Node::image(0.0, 0.0, doc_w, doc_h, bytes));
        let id = node.id;
        self.insert_node(node);
        self.selection = vec![id];
        let bytes = self
            .project
            .nodes
            .get(id)
            .and_then(|n| match &n.kind {
                NodeKind::Image { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })?;
        self.ensure_image_texture(id, &bytes, ctx);
        self.status_message = format!("New paint layer {pw}×{ph}px");
        Some(id)
    }

    fn tool_raster_paint(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        pressure: Option<f32>,
        erase: bool,
        ctx: &Context,
    ) {
        if pressed {
            let Some(id) = self.ensure_raster_paint_target(ctx) else {
                self.status_message = "Could not create paint surface".into();
                return;
            };
            let snap = self.project.nodes.get(id).and_then(|node| {
                if let NodeKind::Image {
                    x,
                    y,
                    width,
                    height,
                    bytes,
                    ..
                } = &node.kind
                {
                    Some((*x, *y, *width, *height, bytes.clone()))
                } else {
                    None
                }
            });
            let Some((x, y, width, height, bytes)) = snap else {
                return;
            };
            if !self.image_pixel_cache.contains_key(&id) {
                self.ensure_image_texture(id, &bytes, ctx);
            }
            // Build live RGBA once for the whole stroke (no Color32 ↔ raw every move).
            let live = if let Some(ci) = self.image_pixel_cache.get(&id) {
                let mut raw = Vec::with_capacity(ci.pixels.len() * 4);
                for p in &ci.pixels {
                    raw.push(p.r());
                    raw.push(p.g());
                    raw.push(p.b());
                    raw.push(p.a());
                }
                (
                    ci.size[0] as u32,
                    ci.size[1] as u32,
                    raw,
                )
            } else if let Some(buf) = crate::raster::RasterBuffer::from_png_bytes(&bytes) {
                (buf.width, buf.height, buf.rgba)
            } else {
                return;
            };
            self.tools.raster.target = Some(id);
            self.tools.raster.before_bytes = Some(bytes);
            self.tools.raster.before_x = x;
            self.tools.raster.before_y = y;
            self.tools.raster.before_w = width;
            self.tools.raster.before_h = height;
            self.tools.raster.live_w = live.0;
            self.tools.raster.live_h = live.1;
            self.tools.raster.live_rgba = Some(live.2);
            self.tools.raster.last_px = None;
            self.tools.raster.stable_px = None;
            self.tools.raster.sample_hist.clear();
            self.tools.raster.spacing_carry = 0.0;
            self.tools.raster.painting = true;
            self.tools.raster.dirty = false;
            self.tools.raster.tex_dirty = false;
            self.tools.raster.last_tex_upload = 0.0;
            self.raster_stamp_at(doc, pressure, erase, true, ctx);
        } else if down && self.tools.raster.painting {
            self.raster_stamp_at(doc, pressure, erase, false, ctx);
        }
        if released && self.tools.raster.painting {
            self.finish_raster_stroke(ctx);
        }
    }

    fn raster_stamp_at(
        &mut self,
        doc: (f64, f64),
        pressure: Option<f32>,
        erase: bool,
        force_first: bool,
        ctx: &Context,
    ) {
        let Some(id) = self.tools.raster.target else {
            return;
        };
        let meta = self.project.nodes.get(id).and_then(|node| {
            if let NodeKind::Image {
                x,
                y,
                width,
                height,
                ..
            } = &node.kind
            {
                Some((*x, *y, *width, *height, node.transform.rotation_rad))
            } else {
                None
            }
        });
        let Some((x, y, width, height, rot)) = meta else {
            return;
        };
        let uv = crate::document::image_doc_to_uv(x, y, width, height, rot, doc.0, doc.1);
        let (pw, ph) = (self.tools.raster.live_w, self.tools.raster.live_h);
        if pw == 0 || ph == 0 || self.tools.raster.live_rgba.is_none() {
            return;
        }
        let (px, py) = if let Some((u, v)) = uv {
            ((u * pw as f64) as f32, (v * ph as f64) as f32)
        } else {
            let cx = x + width * 0.5;
            let cy = y + height * 0.5;
            let (lx, ly) = if rot.abs() < 1e-12 {
                (doc.0, doc.1)
            } else {
                let c = rot.cos();
                let s = rot.sin();
                let dx = doc.0 - cx;
                let dy = doc.1 - cy;
                (cx + dx * c + dy * s, cy - dx * s + dy * c)
            };
            let u = ((lx - x) / width).clamp(0.0, 1.0);
            let v = ((ly - y) / height).clamp(0.0, 1.0);
            ((u * pw as f64) as f32, (v * ph as f64) as f32)
        };

        // Streamline: pull stabilized tip toward pointer (CSP-style continuous pen).
        let stab = self.tools.raster.stabilizer.clamp(0.0, 1.0);
        let (px, py) = if force_first || stab < 1e-3 {
            self.tools.raster.stable_px = Some((px, py));
            (px, py)
        } else {
            let (sx, sy) = self.tools.raster.stable_px.unwrap_or((px, py));
            // Higher stabilizer → stronger lag (smaller follow fraction).
            let follow = (1.0 - stab * 0.92).clamp(0.05, 1.0);
            let nx = sx + (px - sx) * follow;
            let ny = sy + (py - sy) * follow;
            self.tools.raster.stable_px = Some((nx, ny));
            (nx, ny)
        };

        // Skip near-duplicate samples (micro jitter) so history stays useful.
        if let Some(&(lx, ly)) = self.tools.raster.sample_hist.last() {
            let d = (px - lx).hypot(py - ly);
            if d < 0.35 && !force_first {
                return;
            }
        }

        let press = pressure.unwrap_or(1.0).clamp(0.05, 1.0);
        self.tools.raster.sample_hist.push((px, py));
        // Keep full stroke history (no tiny ring). Bound only pathological cases.
        if self.tools.raster.sample_hist.len() > 16_384 {
            let drop_n = self.tools.raster.sample_hist.len() - 16_384;
            self.tools.raster.sample_hist.drain(0..drop_n);
        }

        // Incremental bake like Krita/CSP: stamp only the *new* segment into live_rgba.
        let radius = crate::raster::doc_size_to_pixel_radius(
            self.tools.raster.size * press,
            width,
            height,
            pw,
            ph,
        );
        let spacing = (radius * self.tools.raster.spacing.max(0.04)).max(0.25);
        let hardness = self.tools.raster.hardness;
        let opacity = self.tools.raster.opacity * if erase { 1.0 } else { press };
        let color = self.raster_paint_rgba();
        let force = force_first || self.tools.raster.sample_hist.len() <= 1;
        let (stamps, carry) = crate::raster::stamps_for_new_sample(
            &self.tools.raster.sample_hist,
            spacing,
            self.tools.raster.spacing_carry,
            force,
        );
        self.tools.raster.spacing_carry = carry;
        let prev_px = self.tools.raster.last_px;
        self.tools.raster.last_px = Some((px, py));
        let clip = self.raster_paint_clip_px(id, x, y, width, height, rot, pw, ph);
        let poly_comps = self.raster_poly_masks_px(x, y, width, height, rot, pw, ph);
        let sticky_rect = self.tools.raster.sticky_mask_doc;
        let rect_mask = if let Some((x0, y0, x1, y1)) = sticky_rect {
            self.doc_aabb_to_image_clip(x, y, width, height, rot, pw, ph, x0, y0, x1, y1)
        } else {
            None
        };
        // Pixel-accurate mask (eyedrop/magnetic) — only for this Image.
        let (pix_mask, pix_mw) = self
            .tools
            .raster
            .sticky_pixel_mask
            .as_ref()
            .filter(|m| m.node_id == id && m.width == pw && m.height == ph)
            .map(|m| (Some(m.mask.as_slice()), m.width))
            .unwrap_or((None, 0));
        let smudge = self.tools.active == ToolKind::Smudge;
        let alpha_lock = self.tools.raster.alpha_lock && !erase;
        let smudge_str = self.tools.raster.smudge_strength * press;
        let (ox, oy) = self.raster_sym_origin_px(id, x, y, width, height, rot, pw, ph);
        let divs = self.tools.raster.sym_divisions.max(1);
        let off = self.tools.raster.sym_offset_deg.to_radians();
        let all_stamps =
            crate::raster::expand_circular_symmetry(&stamps, (ox, oy), divs, off);

        if !all_stamps.is_empty() {
            if let Some(rgba) = self.tools.raster.live_rgba.as_mut() {
                let mut buf = crate::raster::RasterBuffer {
                    width: pw,
                    height: ph,
                    rgba: std::mem::take(rgba),
                };
                if smudge {
                    let dir = if let Some((lx, ly)) = prev_px {
                        (px - lx, py - ly)
                    } else {
                        (1.0, 0.0)
                    };
                    for (sx, sy) in &all_stamps {
                        if !poly_comps.is_empty() {
                            let in_poly = poly_comps.iter().any(|poly| {
                                crate::raster::point_in_polygon(*sx as f64, *sy as f64, poly)
                            });
                            if !in_poly {
                                continue;
                            }
                        }
                        if let Some(m) = pix_mask {
                            let ix = sx.round() as i32;
                            let iy = sy.round() as i32;
                            if ix < 0
                                || iy < 0
                                || m.get(iy as usize * pix_mw as usize + ix as usize)
                                    .copied()
                                    .unwrap_or(0)
                                    == 0
                            {
                                continue;
                            }
                        }
                        let ddx = sx - ox;
                        let ddy = sy - oy;
                        let base_dx = px - ox;
                        let base_dy = py - oy;
                        let d = if base_dx * base_dx + base_dy * base_dy > 1e-6
                            && ddx * ddx + ddy * ddy > 1e-6
                        {
                            let a0 = base_dy.atan2(base_dx);
                            let a1 = ddy.atan2(ddx);
                            let da = a1 - a0;
                            let (c, s) = (da.cos(), da.sin());
                            (dir.0 * c - dir.1 * s, dir.0 * s + dir.1 * c)
                        } else {
                            dir
                        };
                        buf.smudge_circle(*sx, *sy, radius, hardness, smudge_str, d, clip);
                    }
                } else {
                    let poly_ref = if poly_comps.is_empty() {
                        None
                    } else {
                        Some(poly_comps.as_slice())
                    };
                    for (sx, sy) in all_stamps {
                        buf.stamp_circle_masked(
                            sx,
                            sy,
                            radius,
                            hardness,
                            color,
                            opacity,
                            erase,
                            clip,
                            rect_mask,
                            alpha_lock,
                            poly_ref,
                            pix_mask,
                            pix_mw,
                        );
                    }
                }
                *rgba = buf.rgba;
            }
            self.tools.raster.dirty = true;
            self.tools.raster.tex_dirty = true;
        }

        // Throttled full-texture upload (pro apps update dirty tiles; we update whole
        // layer ~20×/s — cheap once PNG clone-per-frame is gone).
        let now = ctx.input(|i| i.time);
        if self.tools.raster.tex_dirty
            && now - self.tools.raster.last_tex_upload >= (1.0 / 20.0)
        {
            self.flush_raster_texture(ctx, false);
        }
        ctx.request_repaint();
    }

    /// Live stroke preview: continuous thick polyline through **all** samples (never freezes
    /// mid-spiral). Tip always tracks the cursor even on long strokes.
    fn draw_raster_stroke_overlay(&self, painter: &egui::Painter, origin: Pos2) {
        let Some(id) = self.tools.raster.target else {
            return;
        };
        // If texture is already showing live paint, still draw a light tip ring only.
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let NodeKind::Image {
            x,
            y,
            width,
            height,
            ..
        } = &node.kind
        else {
            return;
        };
        let rot = node.transform.rotation_rad;
        let pw = self.tools.raster.live_w.max(1) as f64;
        let ph = self.tools.raster.live_h.max(1) as f64;
        let hist = &self.tools.raster.sample_hist;
        if hist.is_empty() {
            return;
        }
        let erase = self.tools.active == ToolKind::Eraser;
        let rgba = self.raster_paint_rgba();
        let color = if erase {
            egui::Color32::from_rgba_unmultiplied(220, 60, 60, 140)
        } else {
            egui::Color32::from_rgba_unmultiplied(
                rgba[0],
                rgba[1],
                rgba[2],
                (rgba[3] as f32 * 0.55) as u8,
            )
        };
        let stroke_w = (self.tools.raster.size * self.viewport.zoom).max(1.5);
        let r_screen = stroke_w * 0.5;

        // Pixel → document for every sample (cheap). Downsample body for PathShape only.
        let to_doc = |px: f32, py: f32| -> (f64, f64) {
            let u = px as f64 / pw;
            let v = py as f64 / ph;
            let lx = *x + u * *width;
            let ly = *y + v * *height;
            if rot.abs() < 1e-12 {
                (lx, ly)
            } else {
                let cx = *x + *width * 0.5;
                let cy = *y + *height * 0.5;
                let c = rot.cos();
                let s = rot.sin();
                let rx = lx - cx;
                let ry = ly - cy;
                (cx + rx * c - ry * s, cy + rx * s + ry * c)
            }
        };

        let n = hist.len();
        // Always keep tip accurate: last ~64 samples at full density; older path thinned.
        let tip_keep = 64usize;
        let body_end = n.saturating_sub(tip_keep);
        let stride = if body_end > 400 {
            (body_end / 400).max(1)
        } else {
            1
        };

        let mut screen_pts: Vec<egui::Pos2> = Vec::with_capacity(n.min(500) + tip_keep);
        for i in (0..body_end).step_by(stride) {
            let (dx, dy) = to_doc(hist[i].0, hist[i].1);
            screen_pts.push(self.viewport.doc_to_screen((dx, dy), origin));
        }
        let tip_start = body_end;
        for i in tip_start..n {
            let (dx, dy) = to_doc(hist[i].0, hist[i].1);
            screen_pts.push(self.viewport.doc_to_screen((dx, dy), origin));
        }

        if screen_pts.len() >= 2 {
            painter.add(egui::Shape::line(
                screen_pts,
                egui::Stroke::new(stroke_w, color),
            ));
        } else if let Some(&p) = screen_pts.first() {
            painter.circle_filled(p, r_screen, color);
        }

        // Tip disc so the brush never “sticks” when body is thinned.
        if let Some(&(px, py)) = hist.last() {
            let (dx, dy) = to_doc(px, py);
            let tip = self.viewport.doc_to_screen((dx, dy), origin);
            painter.circle_filled(tip, r_screen, color);
            painter.circle_stroke(
                tip,
                r_screen,
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 140)),
            );
        }
    }

    /// Upload live paint buffer to GPU. `force` = always (stroke end).
    fn flush_raster_texture(&mut self, ctx: &Context, force: bool) {
        if !self.tools.raster.tex_dirty && !force {
            return;
        }
        let Some(id) = self.tools.raster.target else {
            return;
        };
        let w = self.tools.raster.live_w;
        let h = self.tools.raster.live_h;
        // Take buffer temporarily so we can mutably update texture maps.
        let Some(rgba) = self.tools.raster.live_rgba.take() else {
            return;
        };
        self.sync_image_texture_from_rgba(id, w, h, &rgba, ctx);
        self.tools.raster.live_rgba = Some(rgba);
        self.tools.raster.tex_dirty = false;
        self.tools.raster.last_tex_upload = ctx.input(|i| i.time);
    }

    fn finish_raster_stroke(&mut self, ctx: &Context) {
        // Pixels were stamped incrementally while dragging — only flush + commit PNG.
        self.flush_raster_texture(ctx, true);

        let dirty = self.tools.raster.dirty;
        let erase = self.tools.active == ToolKind::Eraser;
        let Some(id) = self.tools.raster.target.take() else {
            self.tools.raster.before_bytes = None;
            self.tools.raster.live_rgba = None;
            self.tools.raster.painting = false;
            self.tools.raster.sample_hist.clear();
            return;
        };
        let before_bytes = self.tools.raster.before_bytes.take();
        let live_w = self.tools.raster.live_w;
        let live_h = self.tools.raster.live_h;
        let live_rgba = self.tools.raster.live_rgba.take();
        self.tools.raster.sample_hist.clear();
        self.tools.raster.painting = false;
        self.tools.raster.last_px = None;
        self.tools.raster.stable_px = None;
        self.tools.raster.spacing_carry = 0.0;
        self.tools.raster.tex_dirty = false;
        if !dirty {
            return;
        }
        let Some(before_bytes) = before_bytes else {
            return;
        };
        let Some(rgba) = live_rgba else {
            return;
        };
        let buf = crate::raster::RasterBuffer {
            width: live_w,
            height: live_h,
            rgba,
        };
        let Some(png) = buf.encode_png() else {
            self.status_message = "Paint encode failed".into();
            return;
        };
        // Ensure GPU matches committed buffer.
        self.sync_image_texture_from_rgba(id, live_w, live_h, &buf.rgba, ctx);

        let Some(before_node) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before_node.clone();
        if let NodeKind::Image { bytes, .. } = &mut after.kind {
            *bytes = png;
        } else {
            return;
        }
        let mut before = before_node;
        if let NodeKind::Image { bytes, .. } = &mut before.kind {
            *bytes = before_bytes;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode {
                id,
                before,
                after,
            },
        );
        self.status_message = if self.tools.active == ToolKind::Smudge {
            "Smudged".into()
        } else if erase {
            "Erased".into()
        } else {
            "Painted".into()
        };
    }

    /// Symmetry origin in image-pixel space.
    fn raster_sym_origin_px(
        &self,
        id: NodeId,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
    ) -> (f32, f32) {
        let _ = id;
        if let Some((dx, dy)) = self.tools.raster.sym_origin_doc {
            if let Some((u, v)) =
                crate::document::image_doc_to_uv(img_x, img_y, img_w, img_h, rot, dx, dy)
            {
                return ((u * pw as f64) as f32, (v * ph as f64) as f32);
            }
        }
        (pw as f32 * 0.5, ph as f32 * 0.5)
    }

    /// Flood-fill Image at document point with water-drop expand animation.
    fn tool_bucket_fill(&mut self, doc: (f64, f64), pressed: bool, ctx: &Context) {
        if !pressed {
            return;
        }
        if self.flood_fill_anim.is_some() {
            return; // wait for current anim
        }
        let Some(id) = self.ensure_raster_paint_target(ctx) else {
            self.status_message = "Could not create paint surface".into();
            return;
        };
        let snap = self.project.nodes.get(id).and_then(|node| {
            if let NodeKind::Image {
                x,
                y,
                width,
                height,
                bytes,
                ..
            } = &node.kind
            {
                Some((
                    *x,
                    *y,
                    *width,
                    *height,
                    node.transform.rotation_rad,
                    bytes.clone(),
                ))
            } else {
                None
            }
        });
        let Some((x, y, width, height, rot, before_bytes)) = snap else {
            return;
        };
        if !self.image_pixel_cache.contains_key(&id) {
            self.ensure_image_texture(id, &before_bytes, ctx);
        }
        let Some(ci) = self.image_pixel_cache.get(&id) else {
            return;
        };
        let pw = ci.size[0] as u32;
        let ph = ci.size[1] as u32;
        if pw == 0 || ph == 0 {
            return;
        }
        let Some((u, v)) =
            crate::document::image_doc_to_uv(x, y, width, height, rot, doc.0, doc.1)
        else {
            self.status_message = "Click inside the image to fill".into();
            return;
        };
        let sx = (u * pw as f64).floor() as i32;
        let sy = (v * ph as f64).floor() as i32;
        let mut base = Vec::with_capacity(ci.pixels.len() * 4);
        for p in &ci.pixels {
            base.push(p.r());
            base.push(p.g());
            base.push(p.b());
            base.push(p.a());
        }
        let clip = self.raster_paint_clip_px(id, x, y, width, height, rot, pw, ph);
        if let Some((x0, y0, x1, y1)) = clip {
            if sx < x0 || sy < y0 || sx >= x1 || sy >= y1 {
                self.status_message = "Click inside paint mask / selection to fill".into();
                return;
            }
        }
        let mut after = base.clone();
        let fill = self.raster_paint_rgba();
        let n = crate::raster::flood_fill(
            &mut after,
            pw,
            ph,
            sx,
            sy,
            fill,
            self.tools.raster.fill_tolerance,
        );
        if n == 0 {
            self.status_message = "Nothing to fill".into();
            return;
        }
        if let Some((x0, y0, x1, y1)) = clip {
            for yy in 0..ph as i32 {
                for xx in 0..pw as i32 {
                    if xx < x0 || yy < y0 || xx >= x1 || yy >= y1 {
                        let i = (yy as usize * pw as usize + xx as usize) * 4;
                        after[i..i + 4].copy_from_slice(&base[i..i + 4]);
                    }
                }
            }
        }
        // Build filled-pixel list with distance from seed for radial reveal.
        let mut filled = Vec::with_capacity(n);
        let mut max_dist = 1.0f32;
        for yy in 0..ph as i32 {
            for xx in 0..pw as i32 {
                let i = (yy as usize * pw as usize + xx as usize) * 4;
                if after[i..i + 4] != base[i..i + 4] {
                    let d = ((xx - sx) as f32).hypot((yy - sy) as f32);
                    max_dist = max_dist.max(d);
                    filled.push((xx, yy, d));
                }
            }
        }
        // Duration scales slightly with area (0.35s–1.1s).
        let duration = (0.35 + (n as f32).sqrt() * 0.008).clamp(0.35, 1.1);
        self.flood_fill_anim = Some(FloodFillAnim {
            id,
            before_bytes,
            after_rgba: after,
            width: pw,
            height: ph,
            seed_x: sx,
            seed_y: sy,
            max_dist,
            filled,
            start: std::time::Instant::now(),
            duration_secs: duration,
            base_rgba: base,
        });
        self.status_message = "Filling…".into();
        ctx.request_repaint();
    }

    /// Advance water-drop flood animation; commits history when done.
    fn tick_flood_fill_anim(&mut self, ctx: &Context) {
        let Some(anim) = self.flood_fill_anim.as_ref() else {
            return;
        };
        let t = anim.start.elapsed().as_secs_f32() / anim.duration_secs.max(0.05);
        let ease = 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3); // ease-out
        let radius = anim.max_dist * ease + 2.0;
        let id = anim.id;
        let w = anim.width;
        let h = anim.height;
        let mut frame = anim.base_rgba.clone();
        for &(x, y, d) in &anim.filled {
            if d <= radius {
                let i = (y as usize * w as usize + x as usize) * 4;
                frame[i..i + 4].copy_from_slice(&anim.after_rgba[i..i + 4]);
            }
        }
        // Soft glow ring near the wavefront
        let ring_w = (anim.max_dist * 0.04).clamp(2.0, 12.0);
        let glow = self.raster_paint_rgba();
        for &(x, y, d) in &anim.filled {
            let dd = (d - radius).abs();
            if dd < ring_w {
                let a = (1.0 - dd / ring_w) * 0.45;
                let i = (y as usize * w as usize + x as usize) * 4;
                for c in 0..3 {
                    let g = glow[c] as f32;
                    let cur = frame[i + c] as f32;
                    frame[i + c] = (cur + (g - cur) * a).clamp(0.0, 255.0) as u8;
                }
            }
        }
        self.sync_image_texture_from_rgba(id, w, h, &frame, ctx);

        if t >= 1.0 {
            let anim = self.flood_fill_anim.take().unwrap();
            let Some(png) = (crate::raster::RasterBuffer {
                width: anim.width,
                height: anim.height,
                rgba: anim.after_rgba.clone(),
            })
            .encode_png() else {
                self.status_message = "Fill encode failed".into();
                return;
            };
            self.sync_image_texture_from_rgba(
                anim.id,
                anim.width,
                anim.height,
                &anim.after_rgba,
                ctx,
            );
            let Some(before_node) = self.project.nodes.get(anim.id).cloned() else {
                return;
            };
            let mut after = before_node.clone();
            if let NodeKind::Image { bytes, .. } = &mut after.kind {
                *bytes = png;
            }
            let mut before = before_node;
            if let NodeKind::Image { bytes, .. } = &mut before.kind {
                *bytes = anim.before_bytes;
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode {
                    id: anim.id,
                    before,
                    after,
                },
            );
            self.status_message = format!("Filled {} px", anim.filled.len());
        } else {
            ctx.request_repaint();
        }
    }

    /// Prefer sticky AABB mask; poly masks use AABB of poly for coarse clip + point-in-poly.
    fn raster_paint_clip_px(
        &self,
        target_id: NodeId,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
    ) -> Option<(i32, i32, i32, i32)> {
        if let Some(poly) = self.tools.raster.sticky_mask_poly.as_ref() {
            if poly.len() >= 3 {
                let mut x0 = f64::MAX;
                let mut y0 = f64::MAX;
                let mut x1 = f64::MIN;
                let mut y1 = f64::MIN;
                for &(px, py) in poly {
                    x0 = x0.min(px);
                    y0 = y0.min(py);
                    x1 = x1.max(px);
                    y1 = y1.max(py);
                }
                return self.doc_aabb_to_image_clip(
                    img_x, img_y, img_w, img_h, rot, pw, ph, x0, y0, x1, y1,
                );
            }
        }
        if let Some((x0, y0, x1, y1)) = self.tools.raster.sticky_mask_doc {
            return self.doc_aabb_to_image_clip(
                img_x,
                img_y,
                img_w,
                img_h,
                rot,
                pw,
                ph,
                x0,
                y0,
                x1,
                y1,
            );
        }
        self.raster_selection_clip_px(target_id, img_x, img_y, img_w, img_h, rot, pw, ph)
    }

    /// All sticky lasso polygons mapped into image pixel space.
    fn raster_poly_masks_px(
        &self,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
    ) -> Vec<Vec<(f64, f64)>> {
        let mut components: Vec<&[(f64, f64)]> = Vec::new();
        if let Some(p) = self.tools.raster.sticky_mask_poly.as_ref() {
            if p.len() >= 3 {
                components.push(p.as_slice());
            }
        }
        for p in &self.tools.raster.sticky_mask_polys {
            if p.len() >= 3 {
                components.push(p.as_slice());
            }
        }
        let mut out = Vec::new();
        for poly in components {
            let mut px = Vec::with_capacity(poly.len());
            for &(dx, dy) in poly {
                if let Some((u, v)) =
                    crate::document::image_doc_to_uv(img_x, img_y, img_w, img_h, rot, dx, dy)
                {
                    px.push((u * pw as f64, v * ph as f64));
                }
            }
            if px.len() >= 3 {
                out.push(px);
            }
        }
        out
    }

    fn raster_poly_mask_px(
        &self,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
    ) -> Option<Vec<(f64, f64)>> {
        // Backward-compatible single poly (first component) for call sites needing one list.
        self.raster_poly_masks_px(img_x, img_y, img_w, img_h, rot, pw, ph)
            .into_iter()
            .next()
    }

    /// True if pixel is inside sticky rect and/or any sticky poly (OR = union mask).
    fn raster_pixel_in_sticky_mask_px(
        &self,
        px: f64,
        py: f64,
        clip: Option<(i32, i32, i32, i32)>,
        polys: &[Vec<(f64, f64)>],
    ) -> bool {
        let has_rect = clip.is_some();
        let has_poly = !polys.is_empty();
        if !has_rect && !has_poly {
            return true; // no sticky mask
        }
        let mut ok = false;
        if let Some((x0, y0, x1, y1)) = clip {
            // Only treat sticky_mask_doc as sticky when polys empty OR both present for union.
            // Coarse AABB from poly is always present when poly exists — don't double-count.
            // Callers pass clip from raster_paint_clip_px which includes poly AABB.
            // For poly-only, clip is AABB of poly: still need point-in-poly for real clip.
            if has_poly {
                // AABB already applied by stamp loop bounds; poly decides membership.
            } else if px >= x0 as f64 && px < x1 as f64 && py >= y0 as f64 && py < y1 as f64 {
                ok = true;
            }
        }
        if has_poly {
            for poly in polys {
                if crate::raster::point_in_polygon(px, py, poly) {
                    ok = true;
                    break;
                }
            }
        } else if has_rect {
            // handled above
        }
        if has_poly && !has_rect {
            return ok;
        }
        if has_rect && !has_poly {
            return ok;
        }
        // Both: OR
        if has_rect && has_poly {
            let in_rect = clip
                .map(|(x0, y0, x1, y1)| {
                    px >= x0 as f64 && px < x1 as f64 && py >= y0 as f64 && py < y1 as f64
                })
                .unwrap_or(false);
            let in_poly = polys
                .iter()
                .any(|p| crate::raster::point_in_polygon(px, py, p));
            return in_rect || in_poly;
        }
        ok
    }

    fn doc_aabb_to_image_clip(
        &self,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Option<(i32, i32, i32, i32)> {
        let corners = [
            (x0, y0),
            (x1, y0),
            (x1, y1),
            (x0, y1),
            ((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            ((x0 + x1) * 0.5, y0),
            ((x0 + x1) * 0.5, y1),
            (x0, (y0 + y1) * 0.5),
            (x1, (y0 + y1) * 0.5),
        ];
        let mut min_u = 1.0f64;
        let mut min_v = 1.0f64;
        let mut max_u = 0.0f64;
        let mut max_v = 0.0f64;
        let mut any = false;
        for (dx, dy) in corners {
            if let Some((u, v)) =
                crate::document::image_doc_to_uv(img_x, img_y, img_w, img_h, rot, dx, dy)
            {
                any = true;
                min_u = min_u.min(u);
                min_v = min_v.min(v);
                max_u = max_u.max(u);
                max_v = max_v.max(v);
            }
        }
        if !any {
            return Some((0, 0, 0, 0));
        }
        let ix0 = (min_u * pw as f64).floor() as i32;
        let iy0 = (min_v * ph as f64).floor() as i32;
        let ix1 = (max_u * pw as f64).ceil() as i32;
        let iy1 = (max_v * ph as f64).ceil() as i32;
        Some((
            ix0.clamp(0, pw as i32),
            iy0.clamp(0, ph as i32),
            ix1.clamp(0, pw as i32),
            iy1.clamp(0, ph as i32),
        ))
    }

    /// Map selection AABB → Image pixel clip when "clip to selection" is on and
    /// selection is not only the paint target.
    fn raster_selection_clip_px(
        &self,
        target_id: NodeId,
        img_x: f64,
        img_y: f64,
        img_w: f64,
        img_h: f64,
        rot: f64,
        pw: u32,
        ph: u32,
    ) -> Option<(i32, i32, i32, i32)> {
        if !self.tools.raster.clip_to_selection || self.selection.is_empty() {
            return None;
        }
        if self.selection.len() == 1 && self.selection[0] == target_id {
            return None;
        }
        let b = self.selection_bounds()?;
        if b.width() < 1e-6 || b.height() < 1e-6 {
            return None;
        }
        self.doc_aabb_to_image_clip(
            img_x,
            img_y,
            img_w,
            img_h,
            rot,
            pw,
            ph,
            b.x0,
            b.y0,
            b.x1,
            b.y1,
        )
    }

    /// Capture current selection AABB as a sticky paint mask (or clear).
    pub fn raster_set_sticky_mask_from_selection(&mut self) {
        if let Some(b) = self.selection_bounds() {
            if b.width() > 1.0 && b.height() > 1.0 {
                self.tools.raster.sticky_mask_doc = Some((b.x0, b.y0, b.x1, b.y1));
                self.status_message = "Paint mask set from selection".into();
                return;
            }
        }
        self.status_message = "Select something first to set paint mask".into();
    }

    pub fn raster_clear_sticky_mask(&mut self) {
        self.tools.raster.sticky_mask_doc = None;
        self.tools.raster.sticky_mask_poly = None;
        self.tools.raster.sticky_mask_polys.clear();
        self.tools.raster.sticky_pixel_mask = None;
        self.tools.raster.mask_tool = crate::tools::PaintMaskTool::Off;
        self.tools.raster.mask_rect_origin = None;
        self.tools.raster.mask_drag_current = None;
        self.tools.raster.mask_lasso_pts.clear();
        self.tools.raster_select.rect_origin = None;
        self.tools.raster_select.drag_current = None;
        self.tools.raster_select.lasso_pts.clear();
        self.status_message = "Paint mask cleared".into();
    }

    fn raster_apply_rect_mask(&mut self, a: (f64, f64), b: (f64, f64), union: bool) {
        let x0 = a.0.min(b.0);
        let y0 = a.1.min(b.1);
        let x1 = a.0.max(b.0);
        let y1 = a.1.max(b.1);
        if (x1 - x0).abs() < 2.0 || (y1 - y0).abs() < 2.0 {
            return;
        }
        if union {
            if let Some((ox0, oy0, ox1, oy1)) = self.tools.raster.sticky_mask_doc {
                self.tools.raster.sticky_mask_doc =
                    Some((x0.min(ox0), y0.min(oy0), x1.max(ox1), y1.max(oy1)));
            } else {
                self.tools.raster.sticky_mask_doc = Some((x0, y0, x1, y1));
            }
            // keep polys + pixel mask for union OR
        } else {
            self.tools.raster.sticky_mask_doc = Some((x0, y0, x1, y1));
            self.tools.raster.sticky_mask_poly = None;
            self.tools.raster.sticky_mask_polys.clear();
            self.tools.raster.sticky_pixel_mask = None;
        }
        self.status_message = if union {
            "Rect mask (union)".into()
        } else {
            "Rect mask set".into()
        };
    }

    fn raster_apply_poly_mask(&mut self, mut pts: Vec<(f64, f64)>, union: bool) {
        if pts.len() < 3 {
            return;
        }
        if let Some(first) = pts.first().copied() {
            let last = *pts.last().unwrap();
            if (first.0 - last.0).hypot(first.1 - last.1) > 1.0 {
                pts.push(first);
            }
        }
        if union {
            if self.tools.raster.sticky_mask_poly.is_none()
                && self.tools.raster.sticky_mask_polys.is_empty()
            {
                self.tools.raster.sticky_mask_poly = Some(pts);
            } else {
                self.tools.raster.sticky_mask_polys.push(pts);
            }
            // keep sticky_mask_doc + pixel mask for union OR
        } else {
            self.tools.raster.sticky_mask_poly = Some(pts);
            self.tools.raster.sticky_mask_polys.clear();
            self.tools.raster.sticky_mask_doc = None;
            self.tools.raster.sticky_pixel_mask = None;
        }
        self.status_message = if union {
            "Lasso mask (union)".into()
        } else {
            "Lasso mask set".into()
        };
    }

    /// Magnetic / eyedrop: flood similar **connected** pixels → pixel mask (not AABB).
    /// Holes of a different color inside the seed region are **not** selected.
    fn raster_select_color_region(
        &mut self,
        doc: (f64, f64),
        ctx: &Context,
        glow: bool,
    ) {
        use crate::tools::StickyPixelMask;
        let Some(id) = self.selection.first().copied().filter(|&id| {
            self.project
                .nodes
                .get(id)
                .map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
        }) else {
            self.status_message = "Select an Image first".into();
            return;
        };
        let snap = self.project.nodes.get(id).and_then(|node| {
            if let NodeKind::Image {
                x,
                y,
                width,
                height,
                bytes,
                ..
            } = &node.kind
            {
                Some((
                    *x,
                    *y,
                    *width,
                    *height,
                    node.transform.rotation_rad,
                    bytes.clone(),
                ))
            } else {
                None
            }
        });
        let Some((ix, iy, iw, ih, rot, bytes)) = snap else {
            return;
        };
        self.ensure_image_texture(id, &bytes, ctx);
        let Some(ci) = self.image_pixel_cache.get(&id) else {
            return;
        };
        let pw = ci.size[0] as u32;
        let ph = ci.size[1] as u32;
        let Some((u, v)) =
            crate::document::image_doc_to_uv(ix, iy, iw, ih, rot, doc.0, doc.1)
        else {
            self.status_message = "Click on the Image".into();
            return;
        };
        let sx = (u * pw as f64).floor() as i32;
        let sy = (v * ph as f64).floor() as i32;
        let mut raw = Vec::with_capacity(ci.pixels.len() * 4);
        for p in &ci.pixels {
            raw.push(p.r());
            raw.push(p.g());
            raw.push(p.b());
            raw.push(p.a());
        }
        // Flood only connected matching pixels (tolerance 0 ⇒ exact color, no leak into holes).
        let mut work = raw.clone();
        let tol = self.tools.raster_select.tolerance;
        let n = crate::raster::flood_fill(
            &mut work,
            pw,
            ph,
            sx,
            sy,
            [1, 2, 3, 255],
            tol,
        );
        if n == 0 {
            self.status_message = "Nothing selected".into();
            return;
        }
        // Build mask only from flood result (not AABB) — hole colors stay unselected.
        let mut mask = vec![0u8; (pw * ph) as usize];
        for yy in 0..ph as usize {
            let row = yy * pw as usize;
            for xx in 0..pw as usize {
                let i = (row + xx) * 4;
                if work[i..i + 4] != raw[i..i + 4] {
                    mask[row + xx] = 255;
                }
            }
        }
        // Glow: expand outward from selected pixels only (cheap). Cap px so UI never freezes.
        if glow {
            let scale = ((pw as f64 / iw.max(1.0)) + (ph as f64 / ih.max(1.0))) * 0.5;
            // Doc glow_radius → image pixels, hard-capped (was 64 → multi-second freezes).
            let glow_px = (self.tools.raster_select.glow_radius as f64 * scale)
                .ceil()
                .clamp(0.0, 16.0) as i32;
            if glow_px > 0 {
                crate::raster::dilate_mask(&mut mask, pw, ph, glow_px);
            }
        }
        let mut pixel = StickyPixelMask {
            node_id: id,
            width: pw,
            height: ph,
            mask,
        };
        let union = self.tools.raster_select.union_mask;
        if union {
            if let Some(existing) = self.tools.raster.sticky_pixel_mask.as_mut() {
                if existing.node_id == id && existing.width == pw && existing.height == ph {
                    existing.or_with(&pixel);
                } else {
                    *existing = pixel;
                }
            } else {
                self.tools.raster.sticky_pixel_mask = Some(pixel);
            }
            // Keep rect/poly for union OR with geometric masks.
        } else {
            self.tools.raster.sticky_pixel_mask = Some(pixel);
            self.tools.raster.sticky_mask_doc = None;
            self.tools.raster.sticky_mask_poly = None;
            self.tools.raster.sticky_mask_polys.clear();
        }
        let count = self
            .tools
            .raster
            .sticky_pixel_mask
            .as_ref()
            .map(|m| m.mask.iter().filter(|&&v| v != 0).count())
            .unwrap_or(0);
        self.status_message = if glow {
            format!("Pixel mask + glow ({count} px, flood {n})")
        } else {
            format!("Pixel mask ({count} px, exact flood)")
        };
    }

    /// Raster Select tool interaction (only meaningful with Image selected).
    fn tool_raster_select(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        ctx: &Context,
    ) {
        use crate::tools::RasterSelectMode;
        if !self.selection_is_single_image() {
            if pressed {
                self.status_message = "Raster Select: select an Image object first".into();
            }
            return;
        }
        let union = self.tools.raster_select.union_mask;
        match self.tools.raster_select.mode {
            RasterSelectMode::Rect => {
                if pressed {
                    self.tools.raster_select.rect_origin = Some(doc);
                    self.tools.raster_select.drag_current = Some(doc);
                } else if down && self.tools.raster_select.rect_origin.is_some() {
                    self.tools.raster_select.drag_current = Some(doc);
                } else if released {
                    if let (Some(o), Some(c)) = (
                        self.tools.raster_select.rect_origin.take(),
                        self.tools.raster_select.drag_current.take(),
                    ) {
                        self.raster_apply_rect_mask(o, c, union);
                    }
                }
            }
            RasterSelectMode::Lasso => {
                if pressed {
                    self.tools.raster_select.lasso_pts.clear();
                    self.tools.raster_select.lasso_pts.push(doc);
                    self.tools.raster_select.drag_current = Some(doc);
                } else if down {
                    if let Some(last) = self.tools.raster_select.lasso_pts.last().copied() {
                        if (doc.0 - last.0).hypot(doc.1 - last.1) > 1.5 {
                            self.tools.raster_select.lasso_pts.push(doc);
                        }
                    }
                    self.tools.raster_select.drag_current = Some(doc);
                } else if released {
                    let pts = std::mem::take(&mut self.tools.raster_select.lasso_pts);
                    self.tools.raster_select.drag_current = None;
                    self.raster_apply_poly_mask(pts, union);
                }
            }
            RasterSelectMode::Magnetic | RasterSelectMode::Eyedrop => {
                if pressed {
                    let glow = self.tools.raster_select.mode == RasterSelectMode::Eyedrop
                        && self.tools.raster_select.glow_select;
                    self.raster_select_color_region(doc, ctx, glow);
                }
            }
        }
    }

    pub fn selection_is_single_image(&self) -> bool {
        if self.selection.len() != 1 {
            return false;
        }
        self.project
            .nodes
            .get(self.selection[0])
            .map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
    }

    pub fn raster_reset_sym_origin(&mut self) {
        self.tools.raster.sym_origin_doc = None;
        self.tools.raster.sym_dragging_origin = false;
        self.status_message = "Symmetry origin → image center".into();
    }

    /// Draw rect/lasso mask while `mask_tool` is active. Returns true if event consumed.
    fn handle_paint_mask_draw(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
    ) -> bool {
        use crate::tools::PaintMaskTool;
        match self.tools.raster.mask_tool {
            PaintMaskTool::Off => false,
            PaintMaskTool::Rect => {
                if pressed {
                    self.tools.raster.mask_rect_origin = Some(doc);
                    self.tools.raster.mask_drag_current = Some(doc);
                    return true;
                }
                if down && self.tools.raster.mask_rect_origin.is_some() {
                    self.tools.raster.mask_drag_current = Some(doc);
                    return true;
                }
                if released {
                    if let (Some(o), Some(c)) = (
                        self.tools.raster.mask_rect_origin.take(),
                        self.tools.raster.mask_drag_current.take(),
                    ) {
                        let x0 = o.0.min(c.0);
                        let y0 = o.1.min(c.1);
                        let x1 = o.0.max(c.0);
                        let y1 = o.1.max(c.1);
                        if (x1 - x0).abs() > 2.0 && (y1 - y0).abs() > 2.0 {
                            self.tools.raster.sticky_mask_doc = Some((x0, y0, x1, y1));
                            self.tools.raster.sticky_mask_poly = None;
                            self.status_message = "Rect paint mask set".into();
                        }
                    }
                    self.tools.raster.mask_tool = PaintMaskTool::Off;
                    return true;
                }
                self.tools.raster.mask_rect_origin.is_some()
            }
            PaintMaskTool::Lasso => {
                if pressed {
                    self.tools.raster.mask_lasso_pts.clear();
                    self.tools.raster.mask_lasso_pts.push(doc);
                    self.tools.raster.mask_drag_current = Some(doc);
                    return true;
                }
                if down {
                    if let Some(last) = self.tools.raster.mask_lasso_pts.last().copied() {
                        let d = (doc.0 - last.0).hypot(doc.1 - last.1);
                        if d > 1.5 {
                            self.tools.raster.mask_lasso_pts.push(doc);
                        }
                    } else {
                        self.tools.raster.mask_lasso_pts.push(doc);
                    }
                    self.tools.raster.mask_drag_current = Some(doc);
                    return true;
                }
                if released {
                    let mut pts = std::mem::take(&mut self.tools.raster.mask_lasso_pts);
                    self.tools.raster.mask_drag_current = None;
                    if pts.len() >= 3 {
                        // Close poly
                        if let Some(first) = pts.first().copied() {
                            let last = *pts.last().unwrap();
                            if (first.0 - last.0).hypot(first.1 - last.1) > 1.0 {
                                pts.push(first);
                            }
                        }
                        self.tools.raster.sticky_mask_poly = Some(pts);
                        self.tools.raster.sticky_mask_doc = None;
                        self.status_message = "Lasso paint mask set".into();
                    }
                    self.tools.raster.mask_tool = PaintMaskTool::Off;
                    return true;
                }
                !self.tools.raster.mask_lasso_pts.is_empty()
            }
        }
    }

    /// Marching-ants style dashed polyline (screen space).
    fn draw_dashed_polyline(
        painter: &egui::Painter,
        pts: &[Pos2],
        closed: bool,
        color_a: egui::Color32,
        color_b: egui::Color32,
        phase: f32,
    ) {
        if pts.len() < 2 {
            return;
        }
        let dash = 7.0f32;
        let gap = 5.0f32;
        let period = dash + gap;
        let mut segs: Vec<(Pos2, Pos2)> = Vec::new();
        let n = if closed { pts.len() } else { pts.len() - 1 };
        for i in 0..n {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            segs.push((a, b));
        }
        let mut dist_along = phase.rem_euclid(period);
        for (a, b) in segs {
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.5 {
                continue;
            }
            let ux = dx / len;
            let uy = dy / len;
            let mut t = 0.0f32;
            while t < len {
                let in_dash = dist_along < dash;
                let remain = if in_dash {
                    dash - dist_along
                } else {
                    period - dist_along
                };
                let step = remain.min(len - t);
                if in_dash {
                    let p0 = Pos2::new(a.x + ux * t, a.y + uy * t);
                    let p1 = Pos2::new(a.x + ux * (t + step), a.y + uy * (t + step));
                    // Double stroke: black under / yellow on top for contrast on any bg.
                    painter.line_segment(
                        [p0, p1],
                        egui::Stroke::new(3.0, color_b),
                    );
                    painter.line_segment(
                        [p0, p1],
                        egui::Stroke::new(1.6, color_a),
                    );
                }
                t += step;
                dist_along = (dist_along + step).rem_euclid(period);
            }
        }
    }

    fn draw_paint_mask_overlay(&self, painter: &egui::Painter, origin: Pos2) {
        let phase = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f32() * 14.0)
                .unwrap_or(0.0)
        };
        let yellow = egui::Color32::from_rgb(255, 220, 60);
        let black = egui::Color32::from_rgb(20, 20, 20);
        // Yellow shade **only** while Raster Select is active.
        let shade = self.tools.active == ToolKind::RasterSelect;
        let fill = egui::Color32::from_rgba_unmultiplied(255, 210, 40, 55);

        // Rect mask — dashed always; shade only on select tool
        if let Some((x0, y0, x1, y1)) = self.tools.raster.sticky_mask_doc {
            let p0 = self.viewport.doc_to_screen((x0, y0), origin);
            let p1 = self.viewport.doc_to_screen((x1, y1), origin);
            let rect = egui::Rect::from_two_pos(p0, p1);
            if shade {
                painter.rect_filled(rect, 0.0, fill);
            }
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            Self::draw_dashed_polyline(painter, &corners, true, yellow, black, phase);
        }
        // Lasso polys — dashed outline only (shade via soft fill omitted for non-convex)
        let mut polys: Vec<&[(f64, f64)]> = Vec::new();
        if let Some(p) = self.tools.raster.sticky_mask_poly.as_ref() {
            if p.len() >= 2 {
                polys.push(p.as_slice());
            }
        }
        for p in &self.tools.raster.sticky_mask_polys {
            if p.len() >= 2 {
                polys.push(p.as_slice());
            }
        }
        for poly in polys {
            let pts: Vec<_> = poly
                .iter()
                .map(|&p| self.viewport.doc_to_screen(p, origin))
                .collect();
            Self::draw_dashed_polyline(painter, &pts, true, yellow, black, phase);
        }
        // Pixel mask: dashed boundaries (incl. holes); soft yellow texture only on select tool
        self.draw_pixel_mask_selection_overlay(painter, origin, phase, yellow, black, shade);

        // In-progress paint-panel rect/lasso mask tools
        if let (Some(o), Some(c)) = (
            self.tools.raster.mask_rect_origin,
            self.tools.raster.mask_drag_current,
        ) {
            let p0 = self.viewport.doc_to_screen(o, origin);
            let p1 = self.viewport.doc_to_screen(c, origin);
            let rect = egui::Rect::from_two_pos(p0, p1);
            if shade {
                painter.rect_filled(rect, 0.0, fill);
            }
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            Self::draw_dashed_polyline(painter, &corners, true, yellow, black, phase);
        }
        if self.tools.raster.mask_lasso_pts.len() >= 2 {
            let pts: Vec<_> = self
                .tools
                .raster
                .mask_lasso_pts
                .iter()
                .map(|&p| self.viewport.doc_to_screen(p, origin))
                .collect();
            Self::draw_dashed_polyline(painter, &pts, false, yellow, black, phase);
        }
    }

    /// Pixel mask: **dashed edges including holes**; optional yellow shade only on Raster Select.
    fn draw_pixel_mask_selection_overlay(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        phase: f32,
        yellow: egui::Color32,
        black: egui::Color32,
        shade: bool,
    ) {
        let Some(pm) = self.tools.raster.sticky_pixel_mask.as_ref() else {
            return;
        };
        let Some(node) = self.project.nodes.get(pm.node_id) else {
            return;
        };
        let NodeKind::Image {
            x,
            y,
            width,
            height,
            ..
        } = &node.kind
        else {
            return;
        };
        let rot = node.transform.rotation_rad as f32;
        let pw = pm.width.max(1);
        let ph = pm.height.max(1);
        let is_on = |xx: i32, yy: i32| -> bool {
            if xx < 0 || yy < 0 || xx >= pw as i32 || yy >= ph as i32 {
                return false;
            }
            pm.mask[yy as usize * pw as usize + xx as usize] != 0
        };

        // Soft yellow shade via downscaled texture (no per-pixel squares).
        if shade {
            const MAX_SIDE: u32 = 256;
            let scale = (pw.max(ph) as f32 / MAX_SIDE as f32).max(1.0);
            let tw = ((pw as f32 / scale).ceil() as u32).max(1);
            let th = ((ph as f32 / scale).ceil() as u32).max(1);
            let mut rgba = vec![0u8; (tw * th * 4) as usize];
            for ty in 0..th {
                for tx in 0..tw {
                    let ix = ((tx as f32 + 0.5) * scale) as i32;
                    let iy = ((ty as f32 + 0.5) * scale) as i32;
                    if is_on(ix.min(pw as i32 - 1), iy.min(ph as i32 - 1)) {
                        let i = (ty * tw + tx) as usize * 4;
                        rgba[i] = 255;
                        rgba[i + 1] = 210;
                        rgba[i + 2] = 40;
                        rgba[i + 3] = 70;
                    }
                }
            }
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [tw as usize, th as usize],
                &rgba,
            );
            let tex = painter.ctx().load_texture(
                "raster_pixel_mask_shade",
                img,
                egui::TextureOptions::LINEAR,
            );
            let tl = self.viewport.doc_to_screen((*x, *y), origin);
            let br = self
                .viewport
                .doc_to_screen((*x + *width, *y + *height), origin);
            let rect = egui::Rect::from_two_pos(tl, br);
            crate::render::paint_image_rotated(
                painter,
                tex.id(),
                rect,
                rot,
                1.0,
            );
        }

        // Edge segments (includes inner holes): dashed yellow path.
        // Map pixel corner to screen.
        let to_screen = |ix: f64, iy: f64| -> Pos2 {
            let lx = *x + ix / pw as f64 * *width;
            let ly = *y + iy / ph as f64 * *height;
            let (dx, dy) = if (rot as f64).abs() < 1e-12 {
                (lx, ly)
            } else {
                let cx = *x + *width * 0.5;
                let cy = *y + *height * 0.5;
                let c = (rot as f64).cos();
                let s = (rot as f64).sin();
                let rx = lx - cx;
                let ry = ly - cy;
                (cx + rx * c - ry * s, cy + rx * s + ry * c)
            };
            self.viewport.doc_to_screen((dx, dy), origin)
        };

        // Subsample scan for large images to keep FPS, but denser than before.
        let scan = ((pw.max(ph) as usize) / 512).max(1);
        let mut segs: Vec<(Pos2, Pos2)> = Vec::new();
        const MAX_SEGS: usize = 6000;
        'outer: for yy in (0..ph as i32).step_by(scan) {
            for xx in (0..pw as i32).step_by(scan) {
                if !is_on(xx, yy) {
                    continue;
                }
                // Top edge of this cell (if above is off)
                if !is_on(xx, yy - 1) {
                    segs.push((
                        to_screen(xx as f64, yy as f64),
                        to_screen((xx + scan as i32) as f64, yy as f64),
                    ));
                }
                // Left edge
                if !is_on(xx - 1, yy) {
                    segs.push((
                        to_screen(xx as f64, yy as f64),
                        to_screen(xx as f64, (yy + scan as i32) as f64),
                    ));
                }
                // Right edge of strip
                if !is_on(xx + scan as i32, yy) {
                    segs.push((
                        to_screen((xx + scan as i32) as f64, yy as f64),
                        to_screen((xx + scan as i32) as f64, (yy + scan as i32) as f64),
                    ));
                }
                // Bottom
                if !is_on(xx, yy + scan as i32) {
                    segs.push((
                        to_screen(xx as f64, (yy + scan as i32) as f64),
                        to_screen((xx + scan as i32) as f64, (yy + scan as i32) as f64),
                    ));
                }
                if segs.len() >= MAX_SEGS {
                    break 'outer;
                }
            }
        }
        // Dash each edge segment (shows outer contour **and** inner holes).
        let dash = 6.0f32;
        let gap = 4.0f32;
        let period = dash + gap;
        for (i, (a, b)) in segs.iter().enumerate() {
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.25 {
                continue;
            }
            let ux = dx / len;
            let uy = dy / len;
            let mut t = 0.0f32;
            let mut dist = (phase + i as f32 * 0.7).rem_euclid(period);
            while t < len {
                let in_dash = dist < dash;
                let remain = if in_dash {
                    dash - dist
                } else {
                    period - dist
                };
                let step = remain.min(len - t);
                if in_dash {
                    let p0 = Pos2::new(a.x + ux * t, a.y + uy * t);
                    let p1 = Pos2::new(a.x + ux * (t + step), a.y + uy * (t + step));
                    painter.line_segment([p0, p1], egui::Stroke::new(2.8, black));
                    painter.line_segment([p0, p1], egui::Stroke::new(1.5, yellow));
                }
                t += step;
                dist = (dist + step).rem_euclid(period);
            }
        }
    }

    fn draw_raster_select_overlay(&self, painter: &egui::Painter, origin: Pos2) {
        if self.tools.active != ToolKind::RasterSelect {
            return;
        }
        let phase = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f32() * 14.0)
                .unwrap_or(0.0)
        };
        let yellow = egui::Color32::from_rgb(255, 220, 60);
        let black = egui::Color32::from_rgb(20, 20, 20);
        let fill = egui::Color32::from_rgba_unmultiplied(255, 210, 40, 50);
        if let (Some(o), Some(c)) = (
            self.tools.raster_select.rect_origin,
            self.tools.raster_select.drag_current,
        ) {
            let p0 = self.viewport.doc_to_screen(o, origin);
            let p1 = self.viewport.doc_to_screen(c, origin);
            let rect = egui::Rect::from_two_pos(p0, p1);
            painter.rect_filled(rect, 0.0, fill);
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            Self::draw_dashed_polyline(painter, &corners, true, yellow, black, phase);
        }
        if self.tools.raster_select.lasso_pts.len() >= 2 {
            let pts: Vec<_> = self
                .tools
                .raster_select
                .lasso_pts
                .iter()
                .map(|&p| self.viewport.doc_to_screen(p, origin))
                .collect();
            Self::draw_dashed_polyline(painter, &pts, false, yellow, black, phase);
        }
    }

    /// Fill Image with transparent (clear paint layer) — undoable.
    pub fn raster_clear_layer(&mut self, ctx: &egui::Context) {
        let id = self
            .selection
            .first()
            .copied()
            .or(self.tools.raster.target)
            .filter(|&id| {
                self.project
                    .nodes
                    .get(id)
                    .map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
            });
        let Some(id) = id else {
            self.status_message = "Select an Image to clear".into();
            return;
        };
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let NodeKind::Image { bytes, .. } = &node.kind else {
            return;
        };
        let before_bytes = bytes.clone();
        let Some(buf) = crate::raster::RasterBuffer::from_png_bytes(&before_bytes) else {
            self.status_message = "Could not decode image".into();
            return;
        };
        let cleared = crate::raster::RasterBuffer::new(buf.width, buf.height);
        let Some(png) = cleared.encode_png() else {
            return;
        };
        self.sync_image_texture_from_rgba(id, cleared.width, cleared.height, &cleared.rgba, ctx);
        let Some(before_node) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before_node.clone();
        if let NodeKind::Image { bytes, .. } = &mut after.kind {
            *bytes = png;
        }
        let mut before = before_node;
        if let NodeKind::Image { bytes, .. } = &mut before.kind {
            *bytes = before_bytes;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode {
                id,
                before,
                after,
            },
        );
        self.tools.raster.live_rgba = None;
        self.image_textures.remove(&id);
        self.image_pixel_cache.remove(&id);
        self.status_message = "Paint layer cleared".into();
    }

    /// Drag circular-symmetry origin (circle + plus). Returns true if event was consumed.
    fn handle_symmetry_origin_gizmo(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        canvas_origin: Pos2,
    ) -> bool {
        if self.tools.raster.sym_divisions < 2 || self.tools.raster.sym_locked {
            self.tools.raster.sym_dragging_origin = false;
            return false;
        }
        let hit_r_doc = (12.0 / self.viewport.zoom as f64).max(6.0);
        let origin_doc = self.tools.raster.sym_origin_doc.unwrap_or_else(|| {
            // Default to active image center if available.
            if let Some(&id) = self.selection.first().or(self.tools.raster.target.as_ref()) {
                if let Some(n) = self.project.nodes.get(id) {
                    if let NodeKind::Image { x, y, width, height, .. } = &n.kind {
                        return (*x + *width * 0.5, *y + *height * 0.5);
                    }
                }
            }
            (
                self.project.document.width * 0.5,
                self.project.document.height * 0.5,
            )
        });
        if pressed {
            let d = (doc.0 - origin_doc.0).hypot(doc.1 - origin_doc.1);
            if d <= hit_r_doc {
                self.tools.raster.sym_dragging_origin = true;
                self.tools.raster.sym_origin_doc = Some(doc);
                return true;
            }
        }
        if self.tools.raster.sym_dragging_origin {
            if down {
                self.tools.raster.sym_origin_doc = Some(doc);
                return true;
            }
            if released {
                self.tools.raster.sym_dragging_origin = false;
                return true;
            }
        }
        let _ = canvas_origin;
        false
    }

    /// Blue radial guide lines + origin handle for circular symmetry.
    fn draw_circular_symmetry_guides(&self, painter: &egui::Painter, origin: Pos2) {
        let n = self.tools.raster.sym_divisions.max(1);
        if n < 2 {
            return;
        }
        let locked = self.tools.raster.sym_locked;
        let line_a = if locked { 50u8 } else { 160u8 };
        let color = egui::Color32::from_rgba_unmultiplied(80, 160, 255, line_a);
        let origin_doc = self.tools.raster.sym_origin_doc.unwrap_or_else(|| {
            if let Some(&id) = self.selection.first().or(self.tools.raster.target.as_ref()) {
                if let Some(nd) = self.project.nodes.get(id) {
                    if let NodeKind::Image { x, y, width, height, .. } = &nd.kind {
                        return (*x + *width * 0.5, *y + *height * 0.5);
                    }
                }
            }
            (
                self.project.document.width * 0.5,
                self.project.document.height * 0.5,
            )
        });
        let center = self.viewport.doc_to_screen(origin_doc, origin);
        // Ray length in screen px
        let ray = 4000.0f32;
        let offset = self.tools.raster.sym_offset_deg.to_radians() as f32;
        let step = std::f32::consts::TAU / n as f32;
        for i in 0..n {
            let a = offset + step * i as f32;
            let dir = egui::vec2(a.cos(), a.sin());
            let p0 = center - dir * ray;
            let p1 = center + dir * ray;
            painter.line_segment(
                [p0, p1],
                egui::Stroke::new(1.25, color),
            );
        }
        // Origin handle: circle + plus
        let r = if locked { 7.0 } else { 9.0 };
        let handle_col = if locked {
            egui::Color32::from_rgba_unmultiplied(80, 160, 255, 90)
        } else {
            egui::Color32::from_rgb(80, 160, 255)
        };
        painter.circle_stroke(center, r, egui::Stroke::new(1.5, handle_col));
        painter.circle_filled(
            center,
            2.5,
            egui::Color32::from_rgba_unmultiplied(80, 160, 255, if locked { 80 } else { 200 }),
        );
        let arm = r * 0.65;
        painter.line_segment(
            [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
            egui::Stroke::new(1.25, handle_col),
        );
        painter.line_segment(
            [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
            egui::Stroke::new(1.25, handle_col),
        );
    }

    /// Sample Image (or canvas) color into Fill swatch while painting (Alt).
    fn raster_pick_color_at(&mut self, doc: (f64, f64)) {
        let color = self.color_at_doc_pos(doc);
        let r = color.r() as f32 / 255.0;
        let g = color.g() as f32 / 255.0;
        let b = color.b() as f32 / 255.0;
        let a = color.a() as f32 / 255.0;
        let paint = crate::document::Paint {
            rgba: [r, g, b, a],
        };
        if let Some(stop) = self.ui_fill_stops.first_mut() {
            stop.color = paint;
        } else {
            self.ui_fill_stops
                .push(crate::document::GradientStop { pos: 0.0, color: paint });
        }
        self.ui_fill_kind = crate::document::FillKind::Solid;
        self.fill_enabled = true;
        self.status_message = format!(
            "Picked #{:02X}{:02X}{:02X}",
            color.r(),
            color.g(),
            color.b()
        );
    }

    /// Load a filesystem image (or video frame when `video_time_sec` is set) for NE paths.
    pub fn ensure_graph_path_texture(&mut self, path: &str, ctx: &Context) -> Option<egui::TextureHandle> {
        self.ensure_graph_path_texture_at(path, None, ctx)
    }

    pub fn ensure_graph_path_texture_at(
        &mut self,
        path: &str,
        video_time_sec: Option<f64>,
        ctx: &Context,
    ) -> Option<egui::TextureHandle> {
        let key = match video_time_sec {
            Some(t) => format!("{path}|t{t:.3}"),
            None => path.to_string(),
        };
        if let Some(t) = self.graph_path_textures.get(&key) {
            return Some(t.clone());
        }
        let last_key = format!("{path}|last");
        // While WAV extract holds LIBAV_LOCK, skip UI video decode for this path —
        // competing for the lock freezes the app until extract finishes (~frame 200+).
        if self.ne_audio_extract_busy(path) {
            if let Some(t) = self.graph_path_textures.get(&last_key) {
                return Some(t.clone());
            }
            // One cheap frame-0 still so the canvas isn't empty before extract.
            if video_time_sec.is_some() {
                if let Some(t) = self.graph_path_textures.get(path) {
                    return Some(t.clone());
                }
            }
        }
        let fps = self.anim_fps as f32;
        let rgba = match crate::document::load_graph_media_rgba(path, video_time_sec, fps) {
            Some(img) => img,
            None => {
                // Decode miss (libav busy / seek fail): keep last good frame — no white flash.
                if let Some(t) = self.graph_path_textures.get(&last_key) {
                    return Some(t.clone());
                }
                return None;
            }
        };
        let (w, h) = rgba.dimensions();
        let pixels = rgba.into_raw();
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
        let handle = ctx.load_texture(
            format!("vadadee-berry-graph-path-{}", key),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.graph_path_textures.insert(key, handle.clone());
        self.graph_path_textures
            .insert(last_key, handle.clone());
        // Cap timed-frame cache; never drop `|last` slots (prevents white flashes).
        if self.graph_path_textures.len() > 64 {
            let drop: Vec<String> = self
                .graph_path_textures
                .keys()
                .filter(|k| k.contains("|t") && !k.ends_with("|last"))
                .take(16)
                .cloned()
                .collect();
            for k in drop {
                self.graph_path_textures.remove(&k);
            }
        }
        Some(handle)
    }

    /// Public wrapper for Node Editor preview popup.
    pub fn ensure_graph_fx_texture_public(
        &mut self,
        path: &str,
        eval: &crate::document::GraphOutputEval,
        ctx: &Context,
    ) -> Option<egui::TextureHandle> {
        self.ensure_graph_fx_texture(path, eval, ctx)
    }

    pub fn graph_path_texture_id(&self, key: &str) -> Option<egui::TextureId> {
        self.graph_path_textures.get(key).map(|t| t.id())
    }

    pub fn image_texture_id(&self, id: NodeId) -> Option<egui::TextureId> {
        self.image_textures.get(&id).map(|t| t.id())
    }

    /// Load path texture with FX. **Blur is GPU-baked** (native texture, no CPU readback).
    /// Brightness-only is paint-time tint (no per-frame rebake — Param anim stays smooth).
    fn ensure_graph_fx_texture(
        &mut self,
        path: &str,
        eval: &crate::document::GraphOutputEval,
        ctx: &Context,
    ) -> Option<egui::TextureHandle> {
        let animating = self.anim_is_playing;
        let q = eval.quantized_for_cache(animating);

        let media_key = q.media_cache_key(path);

        // Identity **or brightness-only** → sharp base texture (tint applied when painting).
        if !q.needs_texture_bake() {
            self.invalidate_graph_gpu_live(&media_key);
            if q.brightness < 0.99 || q.brightness > 1.01 {
                // Still drop soft GPU mips when leaving blur.
                self.invalidate_graph_gpu_path_prefix(&media_key);
            }
            return self.ensure_graph_path_texture_at(path, q.video_time_sec, ctx);
        }

        // Always key by full FX state (incl. blur). Live-only key caused: blur→0 still
        // painting the previous blurred GPU texture when only brightness changed.
        let key = q.fx_cache_key(path);
        let live_key = format!("{media_key}|live");

        let br = q.blur_px.clamp(0.0, 64.0) as f32;
        let color_key = format!(
            "b{:.3}|c{:.3}|s{:.3}|h{:.2}",
            q.brightness, q.contrast, q.saturation, q.hue_shift
        );

        // --- Sharp path with contrast/sat/hue (still no blur) ---
        if br < 0.05 {
            self.invalidate_graph_gpu_live(&media_key);
            self.invalidate_graph_gpu_path_prefix(&media_key);
            if let Some(t) = self.graph_path_textures.get(&key) {
                return Some(t.clone());
            }
            if !self.graph_base_rgba.contains_key(&media_key) {
                let fps = self.anim_fps as f32;
                let rgba = crate::document::load_graph_media_rgba(path, q.video_time_sec, fps)?;
                self.graph_base_rgba.insert(media_key.clone(), rgba);
            }
            // Medium res for rare sat/hue/contrast bakes (not brightness-only).
            let max_side = if animating { 512u32 } else { 1024u32 };
            let base = self.graph_base_rgba.get(&media_key)?;
            let mut rgba = crate::document::downscale_rgba_max_side(base, max_side);
            let mut color_only = q.clone();
            color_only.blur_px = 0.0;
            crate::document::apply_graph_image_fx(&mut rgba, &color_only);
            let (w, h) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            let handle = ctx.load_texture(
                format!("vadadee-berry-gfx-sharp-{key}"),
                color_image,
                egui::TextureOptions::LINEAR,
            );
            self.graph_path_textures.insert(key, handle.clone());
            return Some(handle);
        }

        // --- Blurred path ---
        if let Some(e) = self.graph_gpu_fx.get(&key) {
            if e.color_key == color_key && (e.blur_px - br).abs() < 0.015 {
                return None; // paint uses graph_gpu_fx
            }
        }
        // Also check live slot only if it matches this blur+color exactly.
        if animating {
            if let Some(e) = self.graph_gpu_fx.get(&live_key) {
                if e.color_key == color_key && (e.blur_px - br).abs() < 0.015 {
                    return None;
                }
            }
        }

        if !self.graph_base_rgba.contains_key(&media_key) {
            let fps = self.anim_fps as f32;
            let rgba = crate::document::load_graph_media_rgba(path, q.video_time_sec, fps)?;
            self.graph_base_rgba.insert(media_key.clone(), rgba);
        }

        let max_side = if animating { 128u32 } else { 256u32 };
        let preview_key = format!("{media_key}|{max_side}");
        if !self.graph_preview_rgba.contains_key(&preview_key) {
            let base = self.graph_base_rgba.get(&media_key)?;
            let small = crate::document::downscale_rgba_max_side(base, max_side);
            self.graph_preview_rgba.insert(preview_key.clone(), small);
        }
        let preview = self.graph_preview_rgba.get(&preview_key)?.clone();

        let color_cache_key = format!("{media_key}|{color_key}|{max_side}");
        let rgba = if let Some(c) = self.graph_color_rgba.get(&color_cache_key) {
            c.clone()
        } else {
            let mut color_only = q.clone();
            color_only.blur_px = 0.0;
            let mut rgba = preview;
            crate::document::apply_graph_image_fx(&mut rgba, &color_only);
            self.graph_color_rgba.insert(color_cache_key, rgba.clone());
            if self.graph_color_rgba.len() > 32 {
                self.graph_color_rgba.clear();
            }
            rgba
        };

        let gpu_key = if animating { live_key.clone() } else { key.clone() };

        if let Some(rs) = self.wgpu_render.clone() {
            if let Some((tex, view, w, h)) =
                crate::shading::graph_blur::GraphBlurEngine::blur_to_texture(
                    &rs.device,
                    &rs.queue,
                    &rgba,
                    br,
                )
            {
                let existing = self.graph_gpu_fx.get(&gpu_key).map(|e| e.id);
                if let Some(id) =
                    crate::shading::graph_blur::register_or_update_native(&rs, &view, existing)
                {
                    if !animating {
                        let prefix = format!("{media_key}|");
                        let mut drop_keys: Vec<String> = self
                            .graph_gpu_fx
                            .keys()
                            .filter(|k| {
                                k.starts_with(&prefix) && *k != &gpu_key && !k.ends_with("|live")
                            })
                            .cloned()
                            .collect();
                        if drop_keys.len() > 48 {
                            drop_keys.sort();
                            let n = drop_keys.len() - 48;
                            for old in drop_keys.into_iter().take(n) {
                                if let Some(e) = self.graph_gpu_fx.remove(&old) {
                                    crate::shading::graph_blur::free_native_texture(&rs, e.id);
                                }
                            }
                        }
                    }
                    self.graph_gpu_fx.insert(
                        gpu_key,
                        GraphGpuFxEntry {
                            id,
                            size: [w as usize, h as usize],
                            blur_px: br,
                            color_key,
                            _tex: std::sync::Arc::new(tex),
                        },
                    );
                    return None;
                }
            }
        }

        // CPU fallback.
        let mut rgba = rgba;
        crate::document::continuous_preview_blur_rgba(&mut rgba, br);
        let (w, h) = rgba.dimensions();
        let pixels = rgba.into_raw();
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
        let handle = ctx.load_texture(
            format!(
                "vadadee-berry-gfx-{}",
                key.len().wrapping_mul(2654435761) ^ (br.to_bits() as usize)
            ),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.graph_path_textures.insert(key, handle.clone());
        Some(handle)
    }

    /// Drop GPU live blur for a path so sharp frames are not painted from stale soft mips.
    fn invalidate_graph_gpu_live(&mut self, path: &str) {
        let live = format!("{path}|live");
        if let Some(e) = self.graph_gpu_fx.remove(&live) {
            if let Some(rs) = self.wgpu_render.as_ref() {
                crate::shading::graph_blur::free_native_texture(rs, e.id);
            }
        }
    }

    /// Free every GPU FX texture for this file path (blur keys + live).
    fn invalidate_graph_gpu_path_prefix(&mut self, path: &str) {
        let prefix = format!("{path}|");
        let keys: Vec<String> = self
            .graph_gpu_fx
            .keys()
            .filter(|k| k.starts_with(&prefix) || *k == path)
            .cloned()
            .collect();
        for k in keys {
            if let Some(e) = self.graph_gpu_fx.remove(&k) {
                if let Some(rs) = self.wgpu_render.as_ref() {
                    crate::shading::graph_blur::free_native_texture(rs, e.id);
                }
            }
        }
    }

    /// Resolve a graph file (+FX) texture for canvas paint: GPU native or egui handle.
    /// Brightness-only uses the sharp base texture; multiply is applied in paint.
    fn graph_fx_paint_tex(
        &self,
        path: &str,
        eval: &crate::document::GraphOutputEval,
    ) -> Option<(egui::TextureId, [usize; 2])> {
        let animating = self.anim_is_playing;
        let q = eval.quantized_for_cache(animating);
        let media_key = q.media_cache_key(path);
        if !q.needs_texture_bake() {
            if let Some(t) = self.graph_path_textures.get(&media_key) {
                return Some((t.id(), t.size()));
            }
            // Keep last good frame if this exact time key was evicted / not ready.
            let last = format!("{}|last", path);
            let t = self.graph_path_textures.get(&last)?;
            return Some((t.id(), t.size()));
        }
        let br = q.blur_px.clamp(0.0, 64.0) as f32;
        let key = q.fx_cache_key(path);
        let live_key = format!("{media_key}|live");

        // Sharp non-brightness FX (sat/hue/contrast): CPU cache; never stale GPU blur.
        if br < 0.05 {
            if let Some(t) = self.graph_path_textures.get(&key) {
                return Some((t.id(), t.size()));
            }
            return self
                .graph_path_textures
                .get(&media_key)
                .map(|t| (t.id(), t.size()));
        }

        // Blurred: prefer matching live slot while playing, else exact key.
        if animating {
            if let Some(e) = self.graph_gpu_fx.get(&live_key) {
                if (e.blur_px - br).abs() < 0.02 {
                    return Some((e.id, e.size));
                }
            }
        }
        if let Some(e) = self.graph_gpu_fx.get(&key) {
            return Some((e.id, e.size));
        }
        if let Some(t) = self.graph_path_textures.get(&key) {
            return Some((t.id(), t.size()));
        }
        self.graph_path_textures
            .get(&media_key)
            .map(|t| (t.id(), t.size()))
    }

fn run_video_decode_thread(
    rx_cmd: std::sync::mpsc::Receiver<VideoCommand>,
    tx_frame: std::sync::mpsc::Sender<(usize, usize, u32, u32, Vec<u8>)>,
) {
    let mut current_path: Option<String> = None;
    let mut libav_stream: Option<crate::video_decode::VideoStream> = None;

    while let Ok(cmd) = rx_cmd.recv() {
        let mut latest_cmd = cmd;
        while let Ok(next_cmd) = rx_cmd.try_recv() {
            if matches!(next_cmd, VideoCommand::Stop) {
                latest_cmd = next_cmd;
                break;
            }
            latest_cmd = next_cmd;
        }

        match latest_cmd {
            VideoCommand::Stop => break,
            VideoCommand::StopStream => {
                libav_stream = None;
            }
            VideoCommand::GetFrame {
                timeline_frame,
                source_frame,
                fps,
                path,
                sequential: _,
            } => {
                if !crate::video_decode::is_libav_available() {
                    continue;
                }
                let path_changed = current_path.as_ref() != Some(&path);
                if path_changed {
                    current_path = Some(path.clone());
                    libav_stream = None;
                }
                if libav_stream.is_none() {
                    libav_stream = crate::video_decode::VideoStream::open(&path);
                }
                let decoded = if let Some(ref mut stream) = libav_stream {
                    stream
                        .get_frame(source_frame, fps)
                        .map(|(w, h, rgba)| (w, h, rgba))
                } else {
                    None
                };
                if let Some((w, h, rgba)) = decoded {
                    let _ = tx_frame.send((timeline_frame, source_frame, w, h, rgba));
                } else if let Some((w, h, rgba)) =
                    crate::video_decode::decode_frame(&path, source_frame, fps)
                {
                    let _ = tx_frame.send((timeline_frame, source_frame, w, h, rgba));
                }
            }
        }
    }
}

    pub fn stop_all_video_streams(&mut self) {
        for state in self.video_layers.values_mut() {
            if state.stream_active {
                let _ = state.tx_cmd.send(VideoCommand::StopStream);
                state.stream_active = false;
                state.requested_frame = None;
            }
        }
    }

    /// Load (or reload) texture for a video layer at the current frame.
    /// Tick the per-layer video decode system. Call once per frame.
    /// - Collects any completed background decodes and uploads textures.
    /// - Kicks off a new background decode if the current frame differs from cached.
    /// - Never blocks the UI thread.
    fn tick_video_layers(&mut self, ctx: &Context) {
        // Export holds LIBAV_LOCK on the worker — skip UI decode so we don't serialize
        // and stall both threads (was a major export slowdown).
        if self.video_export.rendering {
            return;
        }
        let fps = self.anim_fps as f32;
        let current_frame = self.anim_current_frame;

        // Collect clip metadata (video clips only) without borrowing self.video_layers yet.
        // `active` is false when playhead is outside the clip span — no freeze-frame before/after.
        let t_sec = current_frame as f32 / fps;
        let mut layers_info: Vec<(uuid::Uuid, String, f32, f32, f32, f32, f32, f32, f32, bool)> =
            Vec::new();
        for l in &self.project.document.layers {
            if !l.visible || l.kind != crate::document::LayerKind::AV {
                continue;
            }
            let mut layer = l.clone();
            layer.ensure_av_clips();
            for c in &layer.av_clips {
                if c.is_audio_only() || c.media_path.is_empty() {
                    continue;
                }
                let active = c.contains_timeline_sec(t_sec);
                layers_info.push((
                    c.id,
                    c.media_path.clone(),
                    l.hue,
                    l.saturation,
                    l.brightness,
                    l.contrast,
                    c.video_start_offset,
                    c.timeline_play_secs(),
                    c.video_timeline_start,
                    active,
                ));
            }
        }

        // Clean up deleted/inactive video layers to terminate their channels and background processes
        let active_ids: std::collections::HashSet<uuid::Uuid> = layers_info.iter().map(|info| info.0).collect();
        self.video_layers.retain(|id, _| active_ids.contains(id));

        for (
            layer_id,
            video_path,
            hue,
            sat,
            bright,
            contrast,
            start_offset,
            play_length,
            timeline_start,
            active,
        ) in &layers_info
        {
            let state = self.video_layers.entry(*layer_id).or_insert_with(|| {
                let (tx_cmd, rx_cmd) = std::sync::mpsc::channel();
                let (tx_frame, rx_frame) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    Self::run_video_decode_thread(rx_cmd, tx_frame);
                });
                VideoLayerState {
                    texture: None,
                    cached_frame: None,
                    cached_source_frame: None,
                    tx_cmd,
                    rx_frame,
                    requested_frame: None,
                    stream_active: false,
                    last_req_time: None,
                    object_link_rev: None,
                }
            });

            // Outside clip window: drop texture so canvas stays blank (no freeze first/last frame).
            // Still images keep the last texture while active only.
            if !*active {
                if state.stream_active {
                    let _ = state.tx_cmd.send(VideoCommand::StopStream);
                    state.stream_active = false;
                }
                while state.rx_frame.try_recv().is_ok() {}
                // Drop video frames when inactive; still images also clear so they don't linger.
                state.texture = None;
                state.cached_frame = None;
                state.cached_source_frame = None;
                state.requested_frame = None;
                if self
                    .video_frame_cache
                    .as_ref()
                    .is_some_and(|c| c.layer_id == *layer_id)
                {
                    self.video_frame_cache = None;
                }
                continue;
            }

            // Static / animated-image files: load into texture (no FFmpeg).
            // Object-linked tracks clear texture when sources change (see refresh_object_linked_av_clips).
            if crate::document::AvClip::path_is_still_image(video_path) {
                if state.texture.is_none() {
                    if let Ok(dyn_img) = image::open(video_path) {
                        let rgba = dyn_img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let mut pixels = rgba.into_raw();
                        if *hue != 0.0 || *sat != 1.0 || *bright != 1.0 || *contrast != 1.0 {
                            let mut img =
                                image::RgbaImage::from_raw(w, h, pixels).unwrap_or_default();
                            apply_color_controls(&mut img, *hue, *sat, *bright, *contrast);
                            pixels = img.into_raw();
                        }
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        );
                        // Unique texture name so egui doesn't keep a stale GPU handle.
                        let handle = ctx.load_texture(
                            format!(
                                "vadadee-img-{}-{}",
                                layer_id.as_simple(),
                                self.history.revision()
                            ),
                            color_image,
                            egui::TextureOptions::default(),
                        );
                        state.texture = Some(handle.clone());
                        state.cached_frame = Some(current_frame);
                        // Keep object_link_rev from refresh_object_linked_av_clips (content sig).
                        // Do not overwrite with history revision — that would re-bake every frame.
                        self.video_frame_cache = Some(VideoFrameCache {
                            layer_id: *layer_id,
                            frame: current_frame,
                            texture: handle,
                        });
                    }
                } else {
                    state.cached_frame = Some(current_frame);
                }
                continue;
            }

            let mut latest_frame = None;
            while let Ok(data) = state.rx_frame.try_recv() {
                latest_frame = Some(data);
            }

            if let Some((decoded_frame, decoded_source_frame, w, h, mut rgba)) = latest_frame {
                if *hue != 0.0 || *sat != 1.0 || *bright != 1.0 || *contrast != 1.0 {
                    let mut img = image::RgbaImage::from_raw(w, h, rgba).unwrap_or_default();
                    apply_color_controls(&mut img, *hue, *sat, *bright, *contrast);
                    rgba = img.into_raw();
                }
                let color_image =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                let handle = ctx.load_texture(
                    format!("vadadee-vid-{}-{}", layer_id.as_simple(), decoded_frame),
                    color_image,
                    egui::TextureOptions::default(),
                );

                self.video_frame_cache = Some(VideoFrameCache {
                    layer_id: *layer_id,
                    frame: decoded_frame,
                    texture: handle.clone(),
                });
                state.texture = Some(handle);
                state.cached_frame = Some(decoded_frame);
                state.cached_source_frame = Some(decoded_source_frame);
                state.requested_frame = None;
                ctx.request_repaint();
            }

            // Cleanly terminate the sequential stream if the animation has been paused
            if !self.anim_is_playing && state.stream_active {
                let _ = state.tx_cmd.send(VideoCommand::StopStream);
                state.stream_active = false;
            }

            let elapsed_time = (t_sec - timeline_start).clamp(0.0, *play_length);
            // Strictly inside span already; still clamp so source never past play length.
            if elapsed_time >= *play_length && *play_length > 0.0 {
                // At exact end boundary — treat as inactive (half-open interval).
                state.texture = None;
                continue;
            }
            let source_time = start_offset + elapsed_time;
            let source_frame_idx = (source_time * fps) as usize;

            let mut already_cached = state.cached_frame == Some(current_frame);
            if !already_cached && state.cached_source_frame == Some(source_frame_idx) {
                state.cached_frame = Some(current_frame);
                already_cached = true;
            }
            let already_requested = state.requested_frame == Some(current_frame);

            let now = ctx.input(|i| i.time);
            let throttle = if self.anim_is_playing {
                false
            } else if let Some(last) = state.last_req_time {
                (now - last) < 0.080 // limit to ~12.5 fps when scrubbing
            } else {
                false
            };

            if !already_cached && !already_requested && !throttle {
                let _ = state.tx_cmd.send(VideoCommand::GetFrame {
                    timeline_frame: current_frame,
                    source_frame: source_frame_idx,
                    fps,
                    path: video_path.clone(),
                    sequential: self.anim_is_playing,
                });
                state.requested_frame = Some(current_frame);
                state.stream_active = self.anim_is_playing;
                state.last_req_time = Some(now);
            }
        }

        // Rare memory prune only — never while playing (must not delete live WAV under rodio).
        // Time-throttle: frame-0 every paused tick used to wipe extract WAVs mid-session.
        if !self.anim_is_playing {
            use std::time::{Duration, Instant};
            static LAST_CLEAN: std::sync::OnceLock<std::sync::Mutex<Option<Instant>>> =
                std::sync::OnceLock::new();
            let slot = LAST_CLEAN.get_or_init(|| std::sync::Mutex::new(None));
            let mut last = slot.lock().unwrap_or_else(|e| e.into_inner());
            let due = last
                .map(|t| t.elapsed() >= Duration::from_secs(120))
                .unwrap_or(true);
            if due {
                *last = Some(Instant::now());
                drop(last);
                self.cleanup_unused_audio_caches();
            }
        }
    }

    pub fn cleanup_unused_audio_caches(&mut self) {
        let mut active_source_paths = std::collections::HashSet::new();
        // Paths currently streamed by rodio (keep their WAVs even if layer was just hidden).
        for p in self.audio_player_media_path.values() {
            if !p.is_empty() {
                active_source_paths.insert(p.clone());
            }
        }
        for layer in &self.project.document.layers {
            if layer.kind == crate::document::LayerKind::AV {
                let mut layer_clone = layer.clone();
                layer_clone.ensure_av_clips();
                for clip in &layer_clone.av_clips {
                    if !clip.media_path.is_empty() {
                        active_source_paths.insert(clip.media_path.clone());
                    }
                }
                if !layer.video_path.is_empty() {
                    active_source_paths.insert(layer.video_path.clone());
                }
            }
            // Node Editor: graph media is what extract keys on — omitting this deleted
            // Ready WAVs and left speakers silent after hide/show or short play.
            // Keep paths for *all* NE layers (including hidden) so toggling visibility
            // does not wipe the septic/session extract cache.
            if layer.kind == crate::document::LayerKind::NodeEditor {
                if let Some(g) = layer.node_graph.as_ref() {
                    for node in g.nodes.values() {
                        match &node.kind {
                            crate::document::GraphNodeKind::ObjectImage { path }
                            | crate::document::GraphNodeKind::ObjectVideo { path }
                            | crate::document::GraphNodeKind::ObjectAudio { path }
                            | crate::document::GraphNodeKind::ObjectSeptic { path }
                            | crate::document::GraphNodeKind::ObjectMouse { path } => {
                                if !path.is_empty() {
                                    active_source_paths.insert(path.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(p) = g.resolve_output_sound().path() {
                        if !p.is_empty() {
                            active_source_paths.insert(p.to_string());
                        }
                    }
                    if let crate::document::GraphImageSource::FilePath(p) =
                        g.resolve_output_image().image
                    {
                        if !p.is_empty() {
                            active_source_paths.insert(p);
                        }
                    }
                }
            }
        }

        let cache_dir = dirs_next_audio_cache_dir();
        let mut active_wav_paths = std::collections::HashSet::new();
        if let Ok(mut status_map) = self.audio_extract_status.lock() {
            // Keep *every* Ready WAV still on disk — only drop Failed / missing entries.
            // Never delete extract files just because a layer is currently invisible.
            for (source_path, status) in status_map.iter() {
                match status {
                    AudioExtractStatus::Ready(wav_path) => {
                        if crate::audio_extract::wav_is_playable(wav_path) {
                            active_wav_paths.insert(wav_path.to_string_lossy().to_string());
                            active_source_paths.insert(source_path.clone());
                        }
                    }
                    AudioExtractStatus::Extracting { .. } => {
                        active_source_paths.insert(source_path.clone());
                    }
                    AudioExtractStatus::Failed => {}
                }
            }
            status_map.retain(|source_path, status| match status {
                AudioExtractStatus::Ready(wav_path) => {
                    if crate::audio_extract::wav_is_playable(wav_path) {
                        true
                    } else {
                        // Stale Ready (file deleted externally) — allow re-extract.
                        log::debug!(
                            "audio cache: clearing stale Ready for {source_path} (wav missing)"
                        );
                        false
                    }
                }
                AudioExtractStatus::Extracting { .. } => true,
                AudioExtractStatus::Failed => {
                    // Keep Failed so we don't spin forever; still drop if source gone.
                    active_source_paths.contains(source_path)
                }
            });
        }

        if let Ok(mut pcm_cache) = self.audio_pcm_cache.lock() {
            pcm_cache.retain(|key, _| {
                active_source_paths.contains(key) || active_wav_paths.contains(key)
            });
        }

        // Soft disk sweep: only remove truly orphan extract WAVs that status no longer
        // knows about AND that are older than 24h (never mid-session delete after extract).
        if let Ok(rd) = std::fs::read_dir(&cache_dir) {
            let now = std::time::SystemTime::now();
            for ent in rd.flatten() {
                let path = ent.path();
                if !path.is_file() {
                    continue;
                }
                let is_wav = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("wav"))
                    .unwrap_or(false);
                if !is_wav {
                    continue;
                }
                let key = path.to_string_lossy().to_string();
                if active_wav_paths.contains(&key) {
                    continue;
                }
                let old_enough = ent
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| now.duration_since(t).ok())
                    .map(|d| d.as_secs() > 24 * 3600)
                    .unwrap_or(false);
                if old_enough {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }



    pub fn insert_image(&mut self, x: f64, y: f64, width: f64, height: f64, bytes: Vec<u8>) {
        let node = self.styled_shape_node(Node::image(x, y, width, height, bytes));
        self.insert_node(node);
        ui::promote_action_tab(self, ui::ActionTab::ColorStroke);
    }

    fn finish_pen_path(&mut self, close: bool) {
        let pen = self.tools.pen.clone();
        if pen.anchors.len() < 2 {
            self.tools.pen = Default::default();
            return;
        }
        let path = PathData::from_anchor_data(
            &pen.anchors,
            &pen.smooth_anchors,
            pen.handle_out_offset,
            pen.handle_in_offset,
            close,
        );
        if let Some(id) = pen.continue_node {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                self.tools.pen = Default::default();
                return;
            };
            let mut after = before.clone();
            after.kind = NodeKind::Path { path };
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.selection = vec![id];
            self.status_message = "Path updated".into();
        } else {
            let mut node = Node::path_from_bez(path.to_bez(), "Path");
            node.style.fill = self.build_ui_fill();
            node.style.stroke = self.build_ui_stroke();
            node.kind = NodeKind::Path { path };
            self.insert_node(node);
        }
        self.tools.pen = Default::default();
    }

    fn sync_pen_continue_from_selection(&mut self) {
        if !self.tools.pen.is_empty() || self.tools.pen.continue_node.is_some() {
            return;
        }
        if self.selection.len() != 1 {
            return;
        }
        let id = self.selection[0];
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let NodeKind::Path { path } = &node.kind else {
            return;
        };
        if path.anchor_positions().len() < 2 {
            return;
        }
        let anchors = path.anchor_positions();
        self.tools.pen.anchors = anchors;
        self.tools.pen.smooth_anchors = path.smooth_anchors.clone();
        self.tools.pen.handle_out_offset = path.handle_out_offset.clone();
        self.tools.pen.handle_in_offset = path.handle_in_offset.clone();
        self.tools.pen.continue_node = Some(id);
        self.tools.pen.extend_from_start = false;
        self.tools.pen.join_anchor = None;
        self.tools.pen.was_closed = path.is_closed();
        self.status_message = if path.is_closed() {
            "Pen: add points to closed path, or click near start to re-close".into()
        } else {
            "Pen: click an end point to continue, or add points".into()
        };
    }

    pub fn canvas_ui(&mut self, ui: &mut Ui) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let origin = rect.min;
        self.canvas_screen_rect = Some(rect);
        self.canvas_origin = origin;

        if response.clicked() || response.drag_started() {
            ui.ctx().memory_mut(|mem| mem.request_focus(response.id));
        }
        self.canvas_focused = ui.ctx().memory(|mem| mem.has_focus(response.id));

        // Handle dropped files (png/jpeg/project) -> create Image node or load project
        let drops: Vec<_> = ui.input(|i| i.raw.dropped_files.clone());
        for f in drops {
            let bytes: Vec<u8> = if let Some(b) = &f.bytes {
                b.to_vec()
            } else if let Some(p) = &f.path {
                std::fs::read(p).ok().unwrap_or_default()
            } else {
                vec![]
            };
            if bytes.is_empty() { continue; }
            let name = f.name.to_lowercase();
            if name.ends_with(".vadadee-berry.json") {
                let path_to_load = f.path.clone();
                if let Some(p) = path_to_load {
                    match io::load_project(&p) {
                        Ok(loaded_proj) => {
                            self.project = loaded_proj;
                            self.selection.clear();
                            self.history.clear();
                            self.viewport.pan = egui::vec2(48.0, 48.0);
                            self.viewport.zoom = 0.85;
                            self.status_message = format!("Loaded project: {}", p.display());
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to load project: {e}");
                        }
                    }
                } else {
                    if let Ok(loaded_proj) = serde_json::from_slice::<ProjectFile>(&bytes) {
                        self.project = loaded_proj;
                        self.selection.clear();
                        self.history.clear();
                        self.viewport.pan = egui::vec2(48.0, 48.0);
                        self.viewport.zoom = 0.85;
                        self.status_message = format!("Loaded project: {}", f.name);
                    }
                }
                continue;
            }
            if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")
                || bytes.starts_with(b"\x89PNG") || bytes.starts_with(b"\xFF\xD8")
            {
                let pos = rect.center();
                let doc = tools::doc_point_from_screen(pos, origin, self.viewport.pan, self.viewport.zoom);
                let disp_w = 320.0;
                let disp_h = 240.0;
                self.insert_image(doc.0 - disp_w / 2.0, doc.1 - disp_h / 2.0, disp_w, disp_h, bytes);
            }
        }

        self.viewport.page_width = self.project.document.width as f32;
        self.viewport.page_height = self.project.document.height as f32;
        let page = self.viewport.page_rect(
            origin,
            self.project.document.width as f32,
            self.project.document.height as f32,
        );

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::colors::CANVAS_BG);
            // Page fill first, then grid on top (grid used to paint under the opaque page).
            render::draw_page_shadow(&painter, page, self.project.document.page_color_egui());
            render::draw_grid(&painter, &self.viewport, origin, page);
            if self.pixel_art_mode {
                let cell = self.pixel_cell_size as f64;
                let mut x = (page.min.x as f64 / cell).floor() * cell;
                while x < page.max.x as f64 {
                    let p1 = self.viewport.doc_to_screen((x, page.min.y as f64), origin);
                    let p2 = self.viewport.doc_to_screen((x, page.max.y as f64), origin);
                    painter.line_segment([p1, p2], egui::Stroke::new(0.5, egui::Color32::from_rgb(80, 80, 80)));
                    x += cell;
                }
                let mut y = (page.min.y as f64 / cell).floor() * cell;
                while y < page.max.y as f64 {
                    let p1 = self.viewport.doc_to_screen((page.min.x as f64, y), origin);
                    let p2 = self.viewport.doc_to_screen((page.max.x as f64, y), origin);
                    painter.line_segment([p1, p2], egui::Stroke::new(0.5, egui::Color32::from_rgb(80, 80, 80)));
                    y += cell;
                }
            }

            let order = self.draw_order_cached().to_vec();
            let ctx = ui.ctx().clone();
            for id in &order {
                if let Some(node) = self.project.nodes.get(*id) {
                    if let NodeKind::Text { style, .. } = &node.kind {
                        self.fonts.ensure_loaded(&ctx, &style.font_family);
                    }
                }
            }
            self.fonts
                .ensure_loaded(&ctx, &self.ui_text_font_family);
            // Ensure textures for any Image nodes (decode from embedded bytes if needed).
            // CRITICAL: never clone PNG bytes when texture already warm — that was multi-MB/frame lag.
            let image_ids: Vec<_> = order
                .iter()
                .copied()
                .filter(|id| {
                    self.project
                        .nodes
                        .get(*id)
                        .map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
                })
                .collect();
            // While on-page editing a text, suppress its normal draw so the in-place editor provides
            // the visible glyphs + caret with no duplicate/offset.
            let draw_order: Vec<NodeId> = if let Some(edit_id) = self.on_page_text_edit {
                order.into_iter().filter(|&iid| iid != edit_id).collect()
            } else {
                order
            };
            for id in image_ids {
                if self.image_textures.contains_key(&id) && self.image_pixel_cache.contains_key(&id)
                {
                    continue;
                }
                // Clone only when we actually need to decode.
                let bytes = self.project.nodes.get(id).and_then(|n| match &n.kind {
                    NodeKind::Image { bytes, .. } => Some(bytes.clone()),
                    _ => None,
                });
                if let Some(bytes) = bytes {
                    self.ensure_image_texture(id, &bytes, &ctx);
                }
            }

            let freehand_busy = self.tools.raster.painting
                || (self.tools.active == ToolKind::Brush && !self.tools.brush.points.is_empty());
            // While brushing, skip AV/NE warm-up so the UI can hit max pointer sample rate.
            if !freehand_busy {
                // Async video decode: non-blocking poll + kick off background decode if needed
                self.refresh_object_linked_av_clips(&ctx);
                self.tick_video_layers(&ctx);

                // P6b: ensure Output Object canvas proxies exist (selectable Image nodes).
                {
                    let layer_indices: Vec<usize> = self
                        .project
                        .document
                        .layers
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.kind == crate::document::LayerKind::NodeEditor)
                        .map(|(i, _)| i)
                        .collect();
                    for i in layer_indices {
                        let Some(layer) = self.project.document.layers.get_mut(i) else {
                            continue;
                        };
                        let _ = layer.ensure_ne_output_proxy(&mut self.project.nodes);
                    }
                }

                // Start video→WAV extract ASAP (not only when Play is pressed). Contending
                // with per-frame UI video decode delayed audio until ~hundreds of frames.
                self.warm_ne_video_audio_extract();
            }

            // Warm Node Editor Output Object textures (include blur bake when not playing).
            let graph_evals: Vec<(String, crate::document::GraphOutputEval)> = self
                .project
                .document
                .layers
                .iter()
                .filter(|l| l.kind == crate::document::LayerKind::NodeEditor && l.visible)
                .filter_map(|l| l.node_graph.as_ref())
                .filter_map(|g| {
                    let eval = g.resolve_output_image();
                    match &eval.image {
                        crate::document::GraphImageSource::FilePath(p) => {
                            Some((p.clone(), eval))
                        }
                        _ => None,
                    }
                })
                .collect();
            // While extract is busy, only show cached frames (no new libav seeks).
            for (path, eval) in &graph_evals {
                if eval.video_time_sec.is_some() && self.ne_audio_extract_busy(path) {
                    // Touch last texture only — skip FX rebake that would decode.
                    let _ = self.ensure_graph_path_texture_at(
                        path,
                        eval.video_time_sec,
                        &ctx,
                    );
                    continue;
                }
                let _ = self.ensure_graph_fx_texture(path, eval, &ctx);
            }
            // Fit still-default proxies to natural image size (once).
            {
                let page_w = self.project.document.width;
                let page_h = self.project.document.height;
                let fit_jobs: Vec<(usize, u32, u32)> = self
                    .project
                    .document
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.kind == crate::document::LayerKind::NodeEditor && l.visible)
                    .filter_map(|(i, l)| {
                        let g = l.node_graph.as_ref()?;
                        let eval = g.resolve_output_image();
                        let path = match &eval.image {
                            crate::document::GraphImageSource::FilePath(p) => p.as_str(),
                            _ => return None,
                        };
                        let size = self
                            .graph_fx_paint_tex(path, &eval)
                            .map(|(_, s)| s)
                            .or_else(|| {
                                self.graph_path_textures
                                    .get(path)
                                    .map(|t| t.size())
                            })?;
                        Some((i, size[0] as u32, size[1] as u32))
                    })
                    .collect();
                for (i, iw, ih) in fit_jobs {
                    if let Some(layer) = self.project.document.layers.get_mut(i) {
                        layer.fit_ne_output_proxy_to_image(
                            &mut self.project.nodes,
                            iw,
                            ih,
                            page_w,
                            page_h,
                        );
                    }
                }
            }

            let mut hidden_sources = self.hidden_canvas_sources();
            hidden_sources.extend(path_effect_form_node_ids(
                &self.project.document.path_effects,
            ));
            let dragging_objects = !self.tools.select.drag_snapshot.is_empty();
            let revision = self.history.revision();
            let anim_frame = self.anim_current_frame;
            let loft_paths: std::collections::HashSet<NodeId> = self.project.document.path_effects.values()
                .filter(|e| e.mode == OnPathMode::Loft)
                .map(|e| e.path_id)
                .collect();

            // Draw layer by layer (stack order = document.layers, bottom → top)
            for layer in &self.project.document.layers {
                if !layer.visible {
                    continue;
                }
                match layer.kind {
                    crate::document::LayerKind::Image => {
                        let use_raster_cache = self.enable_layer_raster_cache
                            && !dragging_objects
                            && self.on_page_text_edit.is_none()
                            && crate::layer_cache::should_cache_layer(
                                &self.project,
                                layer,
                                &hidden_sources,
                                self.enable_layer_raster_cache,
                                dragging_objects,
                                self.on_page_text_edit.is_some(),
                                self.anim_is_playing,
                                self.mcp_bulk_active(),
                            )
                            && self
                                .layer_raster_cache
                                .get(&layer.id)
                                .is_some_and(|e| {
                                    crate::layer_cache::cache_entry_valid(e, revision, anim_frame)
                                });

                        if use_raster_cache {
                            if let Some(entry) = self.layer_raster_cache.get(&layer.id) {
                                let page = self.viewport.page_rect(
                                    origin,
                                    self.project.document.width as f32,
                                    self.project.document.height as f32,
                                );
                                painter.image(
                                    entry.texture.id(),
                                    page,
                                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                    egui::Color32::WHITE,
                                );
                            }
                            let skip_overlay = self.is_bulk_selection()
                                && self.selection.len() >= crate::perf::BULK_OVERLAY_SKIP;
                            let layer_set: std::collections::HashSet<NodeId> =
                                layer.nodes.iter().copied().collect();
                            let selection_overlay: Vec<NodeId> = self
                                .selection
                                .iter()
                                .copied()
                                .filter(|id| layer_set.contains(id))
                                .collect();
                            if !skip_overlay && !selection_overlay.is_empty() {
                                render::draw_nodes_ex(
                                    &painter,
                                    &self.project.nodes,
                                    &selection_overlay,
                                    &self.viewport,
                                    origin,
                                    self.project.document.width as f32,
                                    self.project.document.height as f32,
                                    &self.selection,
                                    &hidden_sources,
                                    &loft_paths,
                                    &self.fonts,
                                    &self.image_textures,
                                    self.project.document.page_color_egui(),
                                );
                            }
                        } else {
                            // O(n) membership — layer.nodes is Vec; .contains() per id was O(n²).
                            let layer_set: std::collections::HashSet<NodeId> =
                                layer.nodes.iter().copied().collect();
                            let layer_draw_order: Vec<NodeId> = draw_order
                                .iter()
                                .copied()
                                .filter(|id| layer_set.contains(id))
                                .collect();
                            render::draw_nodes_ex(
                                &painter,
                                &self.project.nodes,
                                &layer_draw_order,
                                &self.viewport,
                                origin,
                                self.project.document.width as f32,
                                self.project.document.height as f32,
                                &self.selection,
                                &hidden_sources,
                                &loft_paths,
                                &self.fonts,
                                &self.image_textures,
                                self.project.document.page_color_egui(),
                            );
                        }
                    }
                    crate::document::LayerKind::AV => {
                        if !layer.has_canvas_video() {
                            continue;
                        }
                        let t_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
                        // Hide video before clip start and after clip end (no freeze-frame).
                        if !layer.shows_video_at(t_sec) {
                            continue;
                        }
                        let mut layer_clips = layer.clone();
                        layer_clips.ensure_av_clips();
                        let primary_id = layer
                            .video_clip_at_time(t_sec)
                            .map(|(id, _, _, _, _)| id)
                            .or_else(|| {
                                layer_clips
                                    .av_clips
                                    .iter()
                                    .find(|c| !c.is_audio_only())
                                    .map(|c| c.id)
                            })
                            .unwrap_or(layer.id);
                        let tex = self.video_layers.get(&primary_id)
                            .and_then(|s| s.texture.as_ref())
                            .cloned()
                            .or_else(|| self.video_layers.get(&layer.id).and_then(|s| s.texture.as_ref()).cloned())
                            .or_else(|| {
                                self.video_frame_cache.as_ref()
                                    .filter(|c| c.layer_id == primary_id || c.layer_id == layer.id)
                                    .map(|c| c.texture.clone())
                            });
                        if let Some(texture) = tex {
                            let mut opacity = 1.0;
                            let mut dx = layer.x as f64;
                            let mut dy = layer.y as f64;
                            let mut rot = layer.rotation as f64;
                            if let Some(track) = self.project.anim_timeline.nodes.get(&layer.id) {
                                if let Some(o) = track.opacity.interpolate(self.anim_current_frame) {
                                    opacity = o as f32;
                                }
                                if let Some(x) = track.pos_x.interpolate(self.anim_current_frame) {
                                    dx = x;
                                }
                                if let Some(y) = track.pos_y.interpolate(self.anim_current_frame) {
                                    dy = y;
                                }
                                if let Some(r) = track.rotation.interpolate(self.anim_current_frame) {
                                    rot = r;
                                }
                            }
                            
                            let tex_w = texture.size()[0] as f32;
                            let tex_h = texture.size()[1] as f32;
                            let aspect = if tex_h > 0.0 { tex_w / tex_h } else { 1.0 };
                            
                            let mut w = layer.width;
                            let mut h = layer.height;
                            if layer.aspect_ratio_locked {
                                if w / h > aspect {
                                    w = h * aspect;
                                } else {
                                    h = w / aspect;
                                }
                            }
                            
                            let tl = self.viewport.doc_to_screen((dx, dy), origin);
                            let br = self.viewport.doc_to_screen(
                                (dx + w as f64, dy + h as f64),
                                origin
                            );
                            let rect = egui::Rect::from_min_max(tl, br);
                            let rot_rad = (rot as f32).to_radians();
                            
                            paint_rotated_image(
                                &painter,
                                texture.id(),
                                rect,
                                rot_rad,
                                opacity,
                            );
                            
                            // Selection highlight outline
                            if self.selection.contains(&layer.id) || self.selection.contains(&primary_id) {
                                let mut points = [
                                    rect.left_top(),
                                    rect.right_top(),
                                    rect.right_bottom(),
                                    rect.left_bottom(),
                                ];
                                if rot_rad != 0.0 {
                                    let center = rect.center();
                                    let cos = rot_rad.cos();
                                    let sin = rot_rad.sin();
                                    for pt in &mut points {
                                        let d = *pt - center;
                                        let rx = d.x * cos - d.y * sin;
                                        let ry = d.x * sin + d.y * cos;
                                        *pt = center + egui::vec2(rx, ry);
                                    }
                                }
                                let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 120, 215));
                                painter.line_segment([points[0], points[1]], stroke);
                                painter.line_segment([points[1], points[2]], stroke);
                                painter.line_segment([points[2], points[3]], stroke);
                                painter.line_segment([points[3], points[0]], stroke);
                            }
                        }
                    }
                    crate::document::LayerKind::Shading => {
                        // Full document page under content in stack order. Galaxy/stars
                        // procedural is edge-weighted so glow shows around the video, not under it.
                        let page = self.viewport.page_rect(
                            origin,
                            self.project.document.width as f32,
                            self.project.document.height as f32,
                        );
                        let shade_time = ctx.input(|i| i.time) as f32;
                        let gpu = self.gpu_shading.then(|| self.wgpu_render.as_ref()).flatten();
                        if let Some(rs) = gpu {
                            if crate::shading::shading_passes_need_input(&layer.shading_passes) {
                                let view = io::default_document_view(&self.project);
                                if let Some((w, h, rgba)) = io::rasterize_document_view(
                                    &self.project,
                                    view,
                                    50.0,
                                    self.anim_current_frame,
                                    &std::collections::HashMap::new(),
                                ) {
                                    crate::shading::queue_shading_input(rs, w, h, rgba);
                                }
                            }
                        }
                        // Always paint live shaders (including during export). Event loop stays
                        // warm via request_repaint() while rendering so hybrid export + canvas share GPU.
                        crate::shading::draw_shading_passes(
                            &painter,
                            page,
                            &layer.shading_passes,
                            shade_time,
                            gpu,
                        );
                        if layer.shading_passes.iter().any(|p| p.enabled)
                            && !self.video_export.rendering
                        {
                            ctx.request_repaint_after(std::time::Duration::from_millis(33));
                        }
                    }
                    crate::document::LayerKind::ScreenRecord => {
                        // Capture layer — no canvas draw (session is for Septic Player).
                    }
                    crate::document::LayerKind::NodeEditor => {
                        // P2/P4: composite Output Object + effect chain onto the canvas.
                        if let Some(g) = &layer.node_graph {
                            let eval = g.resolve_output_image();
                            match &eval.image {
                                crate::document::GraphImageSource::AppObjects(ids) => {
                                    if !ids.is_empty() {
                                        let order: Vec<NodeId> = draw_order
                                            .iter()
                                            .copied()
                                            .filter(|id| ids.contains(id))
                                            .chain(
                                                ids.iter()
                                                    .copied()
                                                    .filter(|id| !draw_order.contains(id)),
                                            )
                                            .collect();
                                        if !order.is_empty() {
                                            let mut hide = hidden_sources.clone();
                                            for id in &order {
                                                hide.remove(id);
                                            }
                                            // Soft multi-pass blur for app objects (continuous radius).
                                            let blur = eval.blur_px.clamp(0.0, 12.0) as f32;
                                            if blur > 0.05 {
                                                let step = blur; // frame-continuous, not max(1.0) steps
                                                let offsets = [
                                                    (0.0, 0.0),
                                                    (step, 0.0),
                                                    (-step, 0.0),
                                                    (0.0, step),
                                                    (0.0, -step),
                                                    (step * 0.7, step * 0.7),
                                                    (-step * 0.7, step * 0.7),
                                                    (step * 0.7, -step * 0.7),
                                                    (-step * 0.7, -step * 0.7),
                                                ];
                                                // Primary draw once; offset hints only as faint overlay.
                                                render::draw_nodes_ex(
                                                    &painter,
                                                    &self.project.nodes,
                                                    &order,
                                                    &self.viewport,
                                                    origin,
                                                    self.project.document.width as f32,
                                                    self.project.document.height as f32,
                                                    &self.selection,
                                                    &hide,
                                                    &loft_paths,
                                                    &self.fonts,
                                                    &self.image_textures,
                                                    self.project.document.page_color_egui(),
                                                );
                                                let _ = offsets; // full re-draw offset not available without node translate
                                            } else {
                                                render::draw_nodes_ex(
                                                    &painter,
                                                    &self.project.nodes,
                                                    &order,
                                                    &self.viewport,
                                                    origin,
                                                    self.project.document.width as f32,
                                                    self.project.document.height as f32,
                                                    &self.selection,
                                                    &hide,
                                                    &loft_paths,
                                                    &self.fonts,
                                                    &self.image_textures,
                                                    self.project.document.page_color_egui(),
                                                );
                                            }
                                            // Effect chip for live algebra-driven params.
                                            if eval.effects_on_path {
                                                let chip = format!(
                                                    "FX b={:.2} sat={:.2} hue={:.0} blur={:.1}",
                                                    eval.brightness,
                                                    eval.saturation,
                                                    eval.hue_shift,
                                                    eval.blur_px
                                                );
                                                let pos =
                                                    self.viewport.doc_to_screen((12.0, 28.0), origin);
                                                painter.text(
                                                    pos,
                                                    egui::Align2::LEFT_TOP,
                                                    chip,
                                                    egui::FontId::proportional(11.0),
                                                    egui::Color32::from_rgb(200, 180, 90),
                                                );
                                            }
                                        }
                                    }
                                }
                                crate::document::GraphImageSource::FilePath(path) => {
                                    if let Some((tex_id, _tex_size)) =
                                        self.graph_fx_paint_tex(path, &eval)
                                    {
                                        // P6b: paint exactly the Output Object rect (user-resizable).
                                        // Full-frame UV unless a Zoom node is in the graph chain.
                                        let (dx, dy, w, h, rot_rad) = layer.ne_output_paint_geom(
                                            &self.project.nodes,
                                            &eval,
                                        );
                                        let mut layer_opacity = layer
                                            .ne_output_proxy
                                            .and_then(|pid| self.project.nodes.get(pid))
                                            .map(|n| n.get_opacity())
                                            .unwrap_or(1.0);
                                        // Opacity track (if any) wins when present.
                                        if let Some(pid) = layer.ne_output_proxy {
                                            if let Some(track) =
                                                self.project.anim_timeline.nodes.get(&pid)
                                            {
                                                if let Some(o) = track
                                                    .opacity
                                                    .interpolate(self.anim_current_frame)
                                                {
                                                    layer_opacity = o as f32;
                                                }
                                            }
                                        }
                                        let tl = self.viewport.doc_to_screen((dx, dy), origin);
                                        let br = self.viewport.doc_to_screen(
                                            (dx + w, dy + h),
                                            origin,
                                        );
                                        let rect = egui::Rect::from_min_max(tl, br);
                                        let mirror = eval.geo_mirror.round() as i32;
                                        // Brightness-only: free vertex tint (Param anim does not rebake).
                                        let mul = layer_opacity.clamp(0.0, 1.0);
                                        let rgb_mul = if eval.only_brightness_fx() {
                                            (eval.brightness as f32).clamp(0.0, 8.0)
                                        } else {
                                            1.0
                                        };
                                        let uv = if eval.has_zoom() {
                                            eval.zoom_uv_rect()
                                        } else {
                                            (0.0, 0.0, 1.0, 1.0)
                                        };
                                        paint_rotated_image_mirrored_tint_uv(
                                            &painter,
                                            tex_id,
                                            rect,
                                            rot_rad as f32,
                                            mul,
                                            rgb_mul,
                                            mirror & 1 != 0,
                                            mirror & 2 != 0,
                                            uv,
                                        );
                                        // Selection stroke when proxy is selected (not drawn via draw_nodes).
                                        if let Some(pid) = layer.ne_output_proxy {
                                            if self.selection.contains(&pid) {
                                                // Outline follows rotation (corners of rotated rect).
                                                let c = rect.center();
                                                let cos = (rot_rad as f32).cos();
                                                let sin = (rot_rad as f32).sin();
                                                let half = rect.size() * 0.5;
                                                let corners = [
                                                    egui::vec2(-half.x, -half.y),
                                                    egui::vec2(half.x, -half.y),
                                                    egui::vec2(half.x, half.y),
                                                    egui::vec2(-half.x, half.y),
                                                ]
                                                .map(|d| {
                                                    c + egui::vec2(
                                                        d.x * cos - d.y * sin,
                                                        d.x * sin + d.y * cos,
                                                    )
                                                });
                                                let stroke = egui::Stroke::new(
                                                    1.0,
                                                    egui::Color32::from_rgb(0, 120, 215),
                                                );
                                                for i in 0..4 {
                                                    painter.line_segment(
                                                        [corners[i], corners[(i + 1) % 4]],
                                                        stroke,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                crate::document::GraphImageSource::Empty => {}
                            }
                            if let Some(err) = &g.root_error {
                                let pos = self.viewport.doc_to_screen((12.0, 12.0), origin);
                                painter.text(
                                    pos,
                                    egui::Align2::LEFT_TOP,
                                    format!("Node Editor: {err}"),
                                    egui::FontId::proportional(12.0),
                                    egui::Color32::from_rgb(255, 120, 120),
                                );
                            }
                        }
                    }
                    crate::document::LayerKind::Flowchart => {
                        // Draw flowchart using nodes (rounded rects for nodes, paths for lines)
                        let layer_set: std::collections::HashSet<NodeId> =
                            layer.nodes.iter().copied().collect();
                        let layer_draw_order: Vec<NodeId> = draw_order
                            .iter()
                            .copied()
                            .filter(|id| layer_set.contains(id))
                            .collect();
                        crate::render::draw_nodes_ex(
                            &painter,
                            &self.project.nodes,
                            &layer_draw_order,
                            &self.viewport,
                            origin,
                            self.project.document.width as f32,
                            self.project.document.height as f32,
                            &self.selection,
                            &hidden_sources,
                            &loft_paths,
                            &self.fonts,
                            &self.image_textures,
                            self.project.document.page_color_egui(),
                        );
                        // Flowchart uses orthogonal routed paths via FlowchartPathData + rounded render
                    }
                }
            }

            // Draw large selection outline for Tiling/Circular sources using effective bounds
            for &id in &self.selection {
                if self.node_uses_extended_bounds(id) {
                    if let Some(node) = self.project.nodes.get(id) {
                        let eb = crate::document::get_effective_bounds(
                            node,
                            &self.project.document,
                            &self.project.nodes,
                        );
                        let tl = self.viewport.doc_to_screen((eb.x0, eb.y0), origin);
                        let br = self.viewport.doc_to_screen((eb.x1, eb.y1), origin);
                        let r = egui::Rect::from_min_max(tl, br);
                        painter.rect_stroke(
                            r.expand(2.0),
                            0.0,
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 120, 215)),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
            }
            render::draw_path_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.path_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            render::draw_tiling_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.tiling_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            render::draw_circular_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.circular_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            render::draw_clip_mask_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.clip_masks,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            if self.tools.active == ToolKind::Select && self.tools.select.marquee.is_none() {
                if self.selection.len() == 1 {
                    if let Some(id) = self.selection.first() {
                        if let Some(node) = self.project.nodes.get(*id) {
                            // Clip source selected alone: handles follow mask bounds (visible clip).
                            let eb = if let Some(cm) = self
                                .project
                                .document
                                .clip_masks
                                .values()
                                .find(|cm| cm.source_id == *id)
                            {
                                self.project
                                    .nodes
                                    .get(cm.mask_id)
                                    .map(|m| m.bounds())
                                    .unwrap_or_else(|| {
                                        crate::document::get_effective_bounds(
                                            node,
                                            &self.project.document,
                                            &self.project.nodes,
                                        )
                                    })
                            } else {
                                crate::document::get_effective_bounds(
                                    node,
                                    &self.project.document,
                                    &self.project.nodes,
                                )
                            };
                            let tl = self.viewport.doc_to_screen((eb.x0, eb.y0), origin);
                            let br = self.viewport.doc_to_screen((eb.x1, eb.y1), origin);
                            let mut sr = egui::Rect::from_min_max(tl, br);
                            if sr.width() < 16.0 {
                                sr.min.x -= 8.0;
                                sr.max.x += 8.0;
                            }
                            if sr.height() < 16.0 {
                                sr.min.y -= 8.0;
                                sr.max.y += 8.0;
                            }
                            render::draw_transform_handles(
                                &painter,
                                sr,
                                self.tools.select.select_rotation_mode,
                            );
                        } else {
                            let mut layer_found = None;
                            for layer in &self.project.document.layers {
                                if layer.id == *id {
                                    layer_found = Some(layer);
                                    break;
                                }
                                if layer.kind == crate::document::LayerKind::AV {
                                    let mut l_clips = layer.clone();
                                    l_clips.ensure_av_clips();
                                    if l_clips.av_clips.iter().any(|c| c.id == *id) {
                                        layer_found = Some(layer);
                                        break;
                                    }
                                }
                            }
                            if let Some(l) = layer_found {
                                if l.kind == crate::document::LayerKind::AV {
                                    let mut dx = l.x as f64;
                                    let mut dy = l.y as f64;
                                    if let Some(track) = self.project.anim_timeline.nodes.get(&l.id) {
                                        if let Some(x) = track.pos_x.interpolate(self.anim_current_frame) {
                                            dx = x;
                                        }
                                        if let Some(y) = track.pos_y.interpolate(self.anim_current_frame) {
                                            dy = y;
                                        }
                                    }
                                    let t_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
                                    let mut l_clips = l.clone();
                                    l_clips.ensure_av_clips();
                                    let primary_id = l
                                        .video_clip_at_time(t_sec)
                                        .map(|(cid, _, _, _, _)| cid)
                                        .or_else(|| {
                                            l_clips
                                                .av_clips
                                                .iter()
                                                .find(|c| !c.is_audio_only())
                                                .map(|c| c.id)
                                        })
                                        .unwrap_or(l.id);

                                    let aspect = self.video_layers.get(&primary_id)
                                        .or_else(|| self.video_layers.get(&l.id))
                                        .and_then(|s| s.texture.as_ref())
                                        .map(|tex| {
                                            let tex_w = tex.size()[0] as f32;
                                            let tex_h = tex.size()[1] as f32;
                                            if tex_h > 0.0 { (tex_w / tex_h) as f64 } else { 1.0 }
                                        })
                                        .or_else(|| {
                                            self.video_frame_cache.as_ref()
                                                .filter(|c| c.layer_id == primary_id || c.layer_id == l.id)
                                                .map(|c| {
                                                    let tex_w = c.texture.size()[0] as f32;
                                                    let tex_h = c.texture.size()[1] as f32;
                                                    if tex_h > 0.0 { (tex_w / tex_h) as f64 } else { 1.0 }
                                                })
                                        })
                                        .unwrap_or(1.0);
                                    let mut w = l.width as f64;
                                    let mut h = l.height as f64;
                                    if l.aspect_ratio_locked {
                                        if w / h > aspect {
                                            w = h * aspect;
                                        } else {
                                            h = w / aspect;
                                        }
                                    }
                                    let tl = self.viewport.doc_to_screen((dx, dy), origin);
                                    let br = self.viewport.doc_to_screen((dx + w, dy + h), origin);
                                    let sr = egui::Rect::from_min_max(tl, br);
                                    render::draw_transform_handles(&painter, sr, self.tools.select.select_rotation_mode);
                                }
                            }
                        }
                    }
                } else if self.selection.len() > 1 {
                    if let Some(sr) = render::selection_union_screen_rect(
                        &self.project.nodes,
                        &self.selection,
                        &self.viewport,
                        origin,
                        &self.project.document.tiling_effects,
                        &self.project.document.circular_effects,
                        &self.project.document.clip_masks,
                    ) {
                        render::draw_group_selection_bounds(&painter, sr);
                        if self.is_bulk_selection() {
                            let label = format!("{} objects", self.selection.len());
                            painter.text(
                                sr.center(),
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::proportional(11.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            }

            if let Some(m) = &self.tools.select.marquee {
                if tools::marquee_is_drag(m.origin_doc, m.current_doc) {
                    render::draw_marquee_rect(
                        &painter,
                        &self.viewport,
                        origin,
                        m.origin_doc,
                        m.current_doc,
                    );
                }
            }

            if self.tools.active == ToolKind::Node && !self.is_bulk_selection() {
                for id in &self.selection {
                    if let Some(node) = self.project.nodes.get(*id) {
                        render::draw_node_handles(
                            &painter,
                            node,
                            &self.viewport,
                            origin,
                            &self.tools.select.selected_path_points,
                            self.tools.select.selected_path_segment,
                        );
                    }
                }
            }

            let is_flowchart_layer = self.project.document.layers
                .get(self.project.document.active_layer_index)
                .map_or(false, |l| l.kind == crate::document::LayerKind::Flowchart);

            if is_flowchart_layer && (
                self.tools.active == ToolKind::Line
                || self.tools.active == ToolKind::Node
                || self.tools.drag_shape.as_ref().map_or(false, |d| d.kind == Some(ToolKind::Line))
                || self.tools.select.node_drag_active
            ) {
                let active_idx = self.project.document.active_layer_index;
                if let Some(layer) = self.project.document.layers.get(active_idx) {
                    let store = &self.project.nodes;
                    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 120, 215));
                    let fill_color = egui::Color32::from_rgb(220, 240, 255);
                    for &nid in &layer.nodes {
                        if let Some(nd) = store.get(nid) {
                            if let Some(geom) = crate::document::flowchart::node_as_flowchart_geom(&nd.kind) {
                                let sides = [
                                    crate::document::flowchart::FlowchartAnchor::edge(crate::document::flowchart::FlowchartEdgeSide::Top, 0, 1),
                                    crate::document::flowchart::FlowchartAnchor::edge(crate::document::flowchart::FlowchartEdgeSide::Bottom, 0, 1),
                                    crate::document::flowchart::FlowchartAnchor::edge(crate::document::flowchart::FlowchartEdgeSide::Left, 0, 1),
                                    crate::document::flowchart::FlowchartAnchor::edge(crate::document::flowchart::FlowchartEdgeSide::Right, 0, 1),
                                ];
                                for anc in sides {
                                    let doc_pos = geom.anchor_position(anc);
                                    let screen_pos = self.viewport.doc_to_screen(doc_pos, origin);
                                    painter.circle(screen_pos, 4.0, fill_color, stroke);
                                }
                            }
                        }
                    }
                }
            }

            if self.action_tab == ui::ActionTab::ColorStroke && self.selection.len() == 1 {
                if let Some(id) = self.selection.first() {
                    if let Some(node) = self.project.nodes.get(*id) {
                        let bounds = node.bounds();
                        if self.ui_fill_edit_gradient_line
                            && self.fill_enabled
                            && matches!(
                                self.ui_fill_kind,
                                FillKind::LinearGradient | FillKind::RadialGradient
                            )
                        {
                            render::draw_gradient_flow_overlay(
                                &painter,
                                &self.viewport,
                                origin,
                                bounds,
                                self.ui_fill_kind,
                                (
                                    self.ui_fill_line_x0,
                                    self.ui_fill_line_y0,
                                    self.ui_fill_line_x1,
                                    self.ui_fill_line_y1,
                                ),
                                self.ui_radial_cx,
                                self.ui_radial_cy,
                                &self.ui_fill_stops,
                            );
                        }
                        if self.ui_stroke_edit_gradient_line
                            && self.stroke_enabled
                            && matches!(
                                self.ui_stroke_kind,
                                FillKind::LinearGradient | FillKind::RadialGradient
                            )
                        {
                            render::draw_gradient_flow_overlay(
                                &painter,
                                &self.viewport,
                                origin,
                                bounds,
                                self.ui_stroke_kind,
                                (
                                    self.ui_stroke_line_x0,
                                    self.ui_stroke_line_y0,
                                    self.ui_stroke_line_x1,
                                    self.ui_stroke_line_y1,
                                ),
                                self.ui_stroke_radial_cx,
                                self.ui_stroke_radial_cy,
                                &self.ui_stroke_stops,
                            );
                        }
                    }
                }
            }

            if let Some(drag) = &self.tools.drag_shape {
                let ctrl_angle = ui.ctx().input(|i| i.modifiers.ctrl || i.modifiers.command);
                match drag.kind {
                    Some(ToolKind::Rectangle) | Some(ToolKind::Plotter) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        render::draw_preview_rect(&painter, &self.viewport, origin, x, y, w, h);
                    }
                    Some(ToolKind::Circle) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let side = w.min(h);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        render::draw_preview_ellipse(
                            &painter, &self.viewport, origin, cx, cy, r, r,
                        );
                    }
                    Some(ToolKind::Ellipse) | Some(ToolKind::Arc) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        render::draw_preview_ellipse(
                            &painter, &self.viewport, origin, cx, cy, w / 2.0, h / 2.0,
                        );
                    }
                    Some(ToolKind::Line) => {
                        let is_flowchart = self.project.document.layers
                            .get(self.project.document.active_layer_index)
                            .map_or(false, |l| l.kind == crate::document::LayerKind::Flowchart);
                        if is_flowchart {
                            let origin_pt = drag.origin_doc;
                            let current_pt = if ctrl_angle {
                                tools::snap_angle_15deg(drag.origin_doc, drag.current_doc)
                            } else {
                                drag.current_doc
                            };
                            let active_idx = self.project.document.active_layer_index;
                            if let Some(layer) = self.project.document.layers.get(active_idx) {
                                let store = &self.project.nodes;
                                let anchor_slop = 80.0f64;
                                let mut best_start_d = anchor_slop;
                                let mut best_end_d = anchor_slop;
                                let mut start_node = None;
                                let mut start_anchor = None;
                                let mut end_node = None;
                                let mut end_anchor = None;
                                let mut points = vec![origin_pt, current_pt];

                                for &nid in &layer.nodes {
                                    if let Some(nd) = store.get(nid) {
                                        if let Some(geom) = crate::document::flowchart::node_as_flowchart_geom(&nd.kind) {
                                            // For start
                                            let anc_s = crate::document::flowchart::snap_anchor_for_point(&geom, origin_pt);
                                            let ap_s = geom.anchor_position(anc_s);
                                            let ds = (ap_s.0 - origin_pt.0).hypot(ap_s.1 - origin_pt.1);
                                            if ds < best_start_d {
                                                start_node = Some(nid);
                                                start_anchor = Some(anc_s);
                                                points[0] = ap_s;
                                                best_start_d = ds;
                                            }

                                            // For end
                                            let anc_e = crate::document::flowchart::snap_anchor_for_point(&geom, current_pt);
                                            let ap_e = geom.anchor_position(anc_e);
                                            let de = (ap_e.0 - current_pt.0).hypot(ap_e.1 - current_pt.1);
                                            if de < best_end_d {
                                                end_node = Some(nid);
                                                end_anchor = Some(anc_e);
                                                points[1] = ap_e;
                                                best_end_d = de;
                                            }
                                        }
                                    }
                                }

                                let mut path_data = crate::document::flowchart::FlowchartPathData {
                                    points,
                                    start_node,
                                    start_anchor,
                                    end_node,
                                    end_anchor,
                                    endpoint_marker_size: 12.0,
                                    corner_radius: 12.0,
                                };

                                let exclude: Vec<_> = [path_data.start_node, path_data.end_node].iter().filter_map(|x| *x).collect();
                                let obstacles = crate::document::flowchart::flowchart_routing_obstacles(store, &layer.nodes, &exclude);
                                crate::document::flowchart::sync_flowchart_path_endpoints(&mut path_data, store, &obstacles);

                                let bez = crate::document::flowchart::rounded_orthogonal_bez(&path_data.points, path_data.corner_radius);
                                render::draw_preview_bezier(&painter, &self.viewport, origin, &bez);
                            }
                        } else {
                            let end_pt = if ctrl_angle {
                                tools::snap_angle_15deg(drag.origin_doc, drag.current_doc)
                            } else {
                                drag.current_doc
                            };
                            render::draw_preview_line(
                                &painter,
                                &self.viewport,
                                origin,
                                drag.origin_doc,
                                end_pt,
                            );
                        }
                    }
                    Some(ToolKind::Polygon) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let side = w.min(h);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        render::draw_preview_polygon(
                            &painter,
                            &self.viewport,
                            origin,
                            cx,
                            cy,
                            side / 2.0,
                            self.polygon_sides,
                        );
                    }
                    _ => {}
                }
            }

            render::draw_pen_preview(
                &painter,
                &self.viewport,
                origin,
                &self.tools.pen,
                self.cursor_doc,
            );

            for guide in &self.live_snap_guides {
                let p1 = self.viewport.doc_to_screen(guide.start, origin);
                let p2 = self.viewport.doc_to_screen(guide.end, origin);
                let color = egui::Color32::from_rgb(255, 215, 0); // Yellow/Gold
                if guide.is_tangent {
                    painter.line_segment([p1, p2], egui::Stroke::new(2.5, color));
                    let contact = (
                        (guide.start.0 + guide.end.0) * 0.5,
                        (guide.start.1 + guide.end.1) * 0.5,
                    );
                    let cp = self.viewport.doc_to_screen(contact, origin);
                    painter.circle_filled(cp, 4.0, color);
                } else {
                    painter.line_segment([p1, p2], egui::Stroke::new(1.5, color));
                }
            }

            crate::left_dock::draw_local_cursor_bubble(self, ui, origin);
            crate::left_dock::draw_remote_cursors(self, ui, origin);

            if self.tools.active == ToolKind::Brush
                && (!self.tools.brush.points.is_empty()
                    || self.tools.brush.brush_type == crate::tools::BrushType::Pixel)
            {
                let erasing = !self.tools.brush.pixel_erase_before.is_empty();
                let stroke_color = if erasing {
                    egui::Color32::from_rgba_unmultiplied(220, 60, 60, 180)
                } else {
                    match &self.build_brush_fill() {
                        Fill::Solid(p) => p.to_egui(),
                        Fill::LinearGradient { stops, .. }
                        | Fill::RadialGradient { stops, .. } => {
                            if let Some(s) = stops.first() {
                                s.color.to_egui()
                            } else {
                                egui::Color32::from_rgb(0, 120, 215)
                            }
                        }
                        Fill::None => egui::Color32::from_rgb(0, 120, 215),
                    }
                };
                render::draw_brush_preview(
                    &painter,
                    &self.viewport,
                    origin,
                    &self.tools.brush.points,
                    stroke_color,
                    self.tools.brush.smoothness,
                    self.tools.brush.heavy,
                    self.cursor_doc,
                    self.tools.brush.brush_type,
                );
            }

            // Live raster paint preview (no full-texture reupload while dragging).
            if self.tools.raster.painting
                && matches!(
                    self.tools.active,
                    ToolKind::RasterBrush | ToolKind::Eraser | ToolKind::Smudge
                )
            {
                self.draw_raster_stroke_overlay(&painter, origin);
            }
            if matches!(
                self.tools.active,
                ToolKind::RasterBrush
                    | ToolKind::Eraser
                    | ToolKind::Smudge
                    | ToolKind::BucketFill
                    | ToolKind::RasterSelect
            ) {
                self.draw_paint_mask_overlay(&painter, origin);
                self.draw_raster_select_overlay(&painter, origin);
                // Animate marching-ants when any mask is active.
                let mask_live = self.tools.raster.sticky_mask_doc.is_some()
                    || self.tools.raster.sticky_mask_poly.is_some()
                    || !self.tools.raster.sticky_mask_polys.is_empty()
                    || self.tools.raster.sticky_pixel_mask.is_some()
                    || self.tools.raster.mask_rect_origin.is_some()
                    || !self.tools.raster.mask_lasso_pts.is_empty()
                    || self.tools.raster_select.rect_origin.is_some()
                    || !self.tools.raster_select.lasso_pts.is_empty();
                if mask_live {
                    ui.ctx().request_repaint();
                }
                if self.tools.raster.sym_divisions >= 2
                    && self.tools.active != ToolKind::RasterSelect
                {
                    self.draw_circular_symmetry_guides(&painter, origin);
                }
            }

            // Weight flow brush cursor (path sculpt)
            if self.tools.weight_flow.enabled {
                if let Some(doc) = self
                    .tools
                    .weight_flow
                    .cursor_doc
                    .or(self.cursor_doc)
                {
                    render::draw_weight_flow_cursor(
                        &painter,
                        &self.viewport,
                        origin,
                        doc,
                        &self.tools.weight_flow,
                        self.weight_flow_target_path().is_some(),
                    );
                }
            }
        }

        let mut path_rect = None;
        let mut pen_finished = false;
        let mut pen_cancelled = false;
        if self.tools.active == ToolKind::Pen && !self.tools.pen.is_empty() {
            let x = rect.center().x;
            let y = rect.max.y - 80.0;
            let overlay_pos = egui::pos2(x, y);
            
            egui::Area::new(egui::Id::new("path_drawing_overlay"))
                .fixed_pos(overlay_pos)
                .pivot(egui::Align2::CENTER_CENTER)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    let inner_resp = egui::Frame::NONE
                        .fill(egui::Color32::from_black_alpha(220))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(16, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let tick_btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✔")
                                            .color(egui::Color32::from_rgb(0, 230, 118))
                                            .strong()
                                            .size(20.0)
                                    )
                                    .frame(false)
                                );
                                if tick_btn.clicked() {
                                    pen_finished = true;
                                }
                                tick_btn.on_hover_text("Complete path drawing");

                                ui.add_space(16.0);

                                let cross_btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✖")
                                            .color(egui::Color32::from_rgb(255, 23, 68))
                                            .strong()
                                            .size(20.0)
                                    )
                                    .frame(false)
                                );
                                if cross_btn.clicked() {
                                    pen_cancelled = true;
                                }
                                cross_btn.on_hover_text("Cancel path drawing");
                            });
                        });
                    path_rect = Some(inner_resp.response.rect);
                });
        }
        self.path_overlay_rect = path_rect;
        if pen_finished {
            self.finish_pen_path(self.tools.pen.was_closed);
        }
        if pen_cancelled {
            self.tools.pen = Default::default();
            self.status_message = "Path cancelled".into();
        }

        ui::show_on_page_text_editor(self, ui, &response, origin);
        self.handle_canvas_input(&response, origin);
        response
    }

    fn handle_canvas_input(&mut self, response: &egui::Response, origin: Pos2) {
        if self.tools.active == ToolKind::Eyedropper || self.eyedropper_holding || self.eyedropper_releasing {
            let hover_pos = response.ctx.input(|i| i.pointer.hover_pos());
            let primary_pressed = response.ctx.input(|i| {
                i.pointer.button_pressed(egui::PointerButton::Primary)
            }) && response.contains_pointer();
            let primary_down = response.is_pointer_button_down_on() || response.ctx.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
            let primary_released_anywhere = response.ctx.input(|i| {
                i.pointer.button_released(egui::PointerButton::Primary)
            });

            let doc_pos = if let Some(hpos) = hover_pos {
                let mut d = self.viewport.screen_to_doc(hpos, origin);
                d = self.viewport.snap(d);
                Some(d)
            } else {
                self.eyedropper_target_pos
            };

            let dpos = doc_pos.unwrap_or((0.0, 0.0));
            self.tool_eyedropper_holding(
                &response.ctx,
                dpos,
                primary_pressed,
                primary_down,
                primary_released_anywhere,
            );

            if self.eyedropper_holding || self.eyedropper_releasing {
                if let Some(target) = self.eyedropper_target_pos {
                    let painter = response.ctx.layer_painter(response.layer_id);
                    let hovered_color = self.color_at_doc_pos(target);
                    render::draw_eyedropper_magnifier(
                        &painter,
                        &self.viewport,
                        origin,
                        target,
                        self.eyedropper_t,
                        hovered_color,
                    );
                }
            }
            return;
        }

        if response.ctx.input(|i| i.multi_touch().is_some()) {
            self.tools.brush.points.clear();
            return;
        }
        if let Some(editor_rect) = self.text_editor_rect {
            if let Some(pointer_pos) = response.ctx.input(|i| i.pointer.interact_pos()) {
                if editor_rect.contains(pointer_pos) {
                    return;
                }
            }
        }
        if let Some(overlay_rect) = self.path_overlay_rect {
            if let Some(pointer_pos) = response.ctx.input(|i| i.pointer.interact_pos()) {
                if overlay_rect.contains(pointer_pos) {
                    return;
                }
            }
        }
        self.update_cursor_doc_from_pointer(&response.ctx, response);
        self.live_snap_guides.clear();
        let primary_down = response.is_pointer_button_down_on();
        let primary_pressed = response.ctx.input(|i| {
            i.pointer.button_pressed(egui::PointerButton::Primary)
        }) && response.contains_pointer();
        let primary_released = response.ctx.input(|i| {
            i.pointer.button_released(egui::PointerButton::Primary)
        }) && response.contains_pointer();
        let primary_released_anywhere = response.ctx.input(|i| {
            i.pointer.button_released(egui::PointerButton::Primary)
        });
        let double_clicked = response.double_clicked()
            || (response.contains_pointer()
                && response.ctx.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                }));

        let pan_active = self.tools.space_pan
            || response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary);

        self.tools.canvas_pan_drag = pan_active;
        if pan_active {
            let delta = response.drag_delta();
            self.viewport.pan += delta;
            return;
        }

        let Some(doc_snapped) = self.cursor_doc else {
            self.gradient_flow_drag = None;
            return;
        };
        // Raw pointer (unsnapped) — required for CircularClone yellow handles under Snap to Grid.
        let (raw_screen, raw_doc) = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos())
            .or_else(|| response.ctx.input(|i| i.pointer.hover_pos()))
            .filter(|p| {
                self.canvas_screen_rect
                    .map(|r| r.contains(*p))
                    .unwrap_or(true)
            })
            .map(|p| (p, self.viewport.screen_to_doc(p, origin)))
            .unwrap_or_else(|| {
                (
                    self.viewport.doc_to_screen(doc_snapped, origin),
                    doc_snapped,
                )
            });
        let pos = self.viewport.doc_to_screen(doc_snapped, origin);

        if self.handle_gradient_flow_input(
            origin,
            pos,
            doc_snapped,
            primary_pressed,
            primary_down,
            primary_released,
        ) {
            return;
        }

        if !self.layer_editable() {
            return;
        }

        // Weight flow brush owns LMB when enabled + path selected (Select/Node).
        if self.tools.weight_flow.enabled {
            let dt = response.ctx.input(|i| i.stable_dt).max(1.0 / 240.0) as f32;
            if self.tool_weight_flow(
                doc_snapped,
                primary_pressed,
                primary_down,
                primary_released_anywhere,
                dt,
            ) {
                return;
            }
        }

        if self.tools.active == ToolKind::Pen {
            self.sync_pen_continue_from_selection();
        }

        let (shift, ctrl, ghost_pick) = response.ctx.input(|i| {
            let shift = i.modifiers.shift;
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            // Ctrl+Shift+click selects ghost (hidden boolean/clip operands).
            (shift, ctrl, shift && ctrl)
        });
        match self.tools.active {
            ToolKind::Select => self.tool_select(
                raw_screen,
                origin,
                raw_doc,
                shift || ctrl,
                ghost_pick,
                primary_pressed,
                primary_down,
                primary_released,
                double_clicked,
            ),
            ToolKind::Rectangle
            | ToolKind::Circle
            | ToolKind::Ellipse
            | ToolKind::Line
            | ToolKind::Polygon
            | ToolKind::Arc
            | ToolKind::Plotter => {
                let ctrl = response.ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
                self.tool_drag_shape(doc_snapped, primary_down, primary_released, ctrl);
            }
            ToolKind::Pen => {
                let ctrl = response.ctx.input(|i| i.modifiers.ctrl);
                let primary_released_pen = primary_released_anywhere;
                self.tool_pen(
                    doc_snapped,
                    primary_pressed,
                    primary_down,
                    primary_released_pen,
                    ctrl,
                );
            }
            ToolKind::Text => self.tool_text(doc_snapped, primary_pressed),
            ToolKind::Brush => {
                let time = response.ctx.input(|i| i.time);
                let pressure = response.ctx.input(|i| {
                    for event in &i.events {
                        if let egui::Event::Touch { force, .. } = event {
                            return *force;
                        }
                    }
                    None
                });
                // Freehand + stabilizer must use raw pointer — grid snap makes polygonal strokes.
                // Pixel brush still snaps to grid cells inside the tool.
                let brush_doc = if self.tools.brush.brush_type == crate::tools::BrushType::Pixel {
                    doc_snapped
                } else {
                    raw_doc
                };
                self.tool_brush(
                    brush_doc,
                    time,
                    primary_pressed,
                    primary_down,
                    primary_released_anywhere,
                    pressure,
                    shift,
                    ctrl,
                );
            }
            ToolKind::RasterBrush | ToolKind::Eraser | ToolKind::Smudge => {
                if self.handle_paint_mask_draw(
                    raw_doc,
                    primary_pressed,
                    primary_down,
                    primary_released_anywhere,
                ) {
                    // Drawing rect/lasso paint mask
                } else if self.handle_symmetry_origin_gizmo(
                    raw_doc,
                    primary_pressed,
                    primary_down,
                    primary_released_anywhere,
                    origin,
                ) {
                    // Symmetry origin drag
                } else {
                    let alt = response.ctx.input(|i| i.modifiers.alt);
                    if alt && self.tools.active != ToolKind::Smudge {
                        if primary_pressed || primary_down {
                            self.raster_pick_color_at(raw_doc);
                        }
                    } else {
                        let pressure = response.ctx.input(|i| {
                            for event in &i.events {
                                if let egui::Event::Touch { force, .. } = event {
                                    return *force;
                                }
                            }
                            None
                        });
                        let erase = self.tools.active == ToolKind::Eraser || shift;
                        self.tool_raster_paint(
                            raw_doc,
                            primary_pressed,
                            primary_down,
                            primary_released_anywhere,
                            pressure,
                            erase,
                            &response.ctx,
                        );
                    }
                }
            }
            ToolKind::BucketFill => {
                let alt = response.ctx.input(|i| i.modifiers.alt);
                if alt && (primary_pressed || primary_down) {
                    self.raster_pick_color_at(raw_doc);
                } else {
                    self.tool_bucket_fill(raw_doc, primary_pressed, &response.ctx);
                }
            }
            ToolKind::RasterSelect => {
                self.tool_raster_select(
                    raw_doc,
                    primary_pressed,
                    primary_down,
                    primary_released_anywhere,
                    &response.ctx,
                );
            }

            ToolKind::Node => self.tool_node(
                pos,
                origin,
                doc_snapped,
                shift,
                ctrl,
                primary_pressed,
                primary_down,
                primary_released,
                primary_released_anywhere,
                double_clicked,
            ),
            ToolKind::Eyedropper => {}
        }

        if primary_released_anywhere
            && self.tools.active == ToolKind::Node
            && !self.tools.select.drag_snapshot.is_empty()
        {
            self.commit_drag_edits();
        }
    }

    fn commit_drag_edits(&mut self) {
        if self.tools.select.node_drag_active {
            if let Some(target) = self.tools.select.node_edit_target {
                if let Some(&(id, _)) = self.tools.select.drag_snapshot.first() {
                    if self.tools.select.selected_path_points.len() <= 1 {
                        self.tools.select
                            .set_single_path_point(id, target.anchor_index());
                    }
                }
            }
        }
        let mut moved_ids = Vec::new();
        for (id, before) in self.tools.select.drag_snapshot.drain(..) {
            let Some(after) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            if before != after {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
                moved_ids.push(id);
            }
        }
        // Keep animation keyframes in sync so position/path geom does not snap back on apply.
        for id in moved_ids {
            self.sync_anim_transform_from_node(id);
            if self.anim_keyframing_mode && !self.anim_is_playing {
                // Ensure path-point / weight-adjacent geom is captured even if REC only saw one frame.
                self.record_geom_keyframes_for_node(id);
            } else {
                self.sync_anim_geom_from_node(id);
            }
            self.sync_circular_ui_from_effect_id(id);
        }
        self.tools.select.circular_ring_drag_start.clear();
        self.tools.select.drag_mode = None;
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
        self.tools.select.mid_curve_drag = None;
        self.sync_flowchart_paths_if_active_layer();
    }

    fn sync_flowchart_paths_if_active_layer(&mut self) {
        let doc = &self.project.document;
        if let Some(layer) = doc.layers.get(doc.active_layer_index) {
            if layer.kind != crate::document::LayerKind::Flowchart {
                return;
            }
            let layer_ids = layer.nodes.clone();
            let obstacles = {
                let store = &self.project.nodes;
                crate::document::flowchart::flowchart_routing_obstacles(store, &layer_ids, &[])
            };
            let store = &mut self.project.nodes;
            for &nid in &layer_ids {
                let path_data = if let Some(node) = store.get(nid) {
                    if let crate::document::NodeKind::FlowchartPath { path } = &node.kind {
                        Some(path.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(mut p) = path_data {
                    crate::document::flowchart::sync_flowchart_path_endpoints(&mut p, store, &obstacles);
                    if let Some(node) = store.get_mut(nid) {
                        if let crate::document::NodeKind::FlowchartPath { path } = &mut node.kind {
                            *path = p;
                        }
                    }
                }
            }
            // also rebalance slots after possible endpoint moves
            crate::document::flowchart::rebalance_flowchart_edge_anchors(store, &layer_ids);
        }
    }

    /// Lightweight: ensure source image textures exist. Viewport clip uses mesh UV mapping
    /// (no per-frame SVG/base64 re-encode — that was melting FPS).
    fn update_clip_mask_textures(&mut self, ctx: &egui::Context) {
        let mut active_clip_ids = std::collections::HashSet::new();
        let clip_masks: Vec<crate::document::ClipMaskEffect> =
            self.project.document.clip_masks.values().cloned().collect();

        for cm in &clip_masks {
            active_clip_ids.insert(cm.id);
            // Cheap signature — never Debug whole Node (Image bytes kill the frame).
            let sig = {
                let mask_b = self
                    .project
                    .nodes
                    .get(cm.mask_id)
                    .map(|n| n.bounds())
                    .unwrap_or_default();
                let src_b = self
                    .project
                    .nodes
                    .get(cm.source_id)
                    .map(|n| n.bounds())
                    .unwrap_or_default();
                format!(
                    "{}:{}:{:.1},{:.1},{:.1},{:.1}:{:.1},{:.1},{:.1},{:.1}",
                    cm.source_id.as_simple(),
                    cm.mask_id.as_simple(),
                    mask_b.x0,
                    mask_b.y0,
                    mask_b.x1,
                    mask_b.y1,
                    src_b.x0,
                    src_b.y0,
                    src_b.x1,
                    src_b.y1
                )
            };
            if self.clip_mask_signatures.get(&cm.id) == Some(&sig) {
                continue;
            }
            if let Some(source_node) = self.project.nodes.get(cm.source_id) {
                if let NodeKind::Image { bytes, .. } = &source_node.kind {
                    let bytes = bytes.clone();
                    self.ensure_image_texture(cm.source_id, &bytes, ctx);
                }
            }
            // Drop any stale baked clip texture; mesh path is authoritative for images.
            self.image_textures.remove(&cm.id);
            self.clip_mask_signatures.insert(cm.id, sig);
        }

        self.clip_mask_signatures
            .retain(|id, _| active_clip_ids.contains(id));
    }

    fn hidden_canvas_sources(&self) -> std::collections::HashSet<NodeId> {
        let mut hidden =
            crate::document::hidden_effect_sources(&self.project.document.path_effects);
        for e in self.project.document.tiling_effects.values() {
            if e.hide_source {
                hidden.insert(e.source_id);
            }
        }
        for e in self.project.document.circular_effects.values() {
            if e.hide_source {
                hidden.insert(e.source_id);
            }
        }
        for cm in self.project.document.clip_masks.values() {
            hidden.insert(cm.source_id);
            if cm.hide_mask {
                hidden.insert(cm.mask_id);
            }
        }
        for e in self.project.document.boolean_effects.values() {
            if e.hide_operands {
                hidden.insert(e.a_id);
                hidden.insert(e.b_id);
            }
        }
        // Group children are painted via the parent (relative transform).
        for n in self.project.nodes.map.values() {
            if let NodeKind::Group { children } = &n.kind {
                for &cid in children {
                    hidden.insert(cid);
                }
            }
        }
        // P6c: App objects feeding a visible Node Editor Output are drawn by the NE
        // composite only — hide originals so they don't double-draw on Image layers.
        for layer in &self.project.document.layers {
            if !layer.visible || layer.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            let Some(g) = layer.node_graph.as_ref() else {
                continue;
            };
            let eval = g.resolve_output_image();
            if let crate::document::GraphImageSource::AppObjects(ids) = &eval.image {
                for id in ids {
                    hidden.insert(*id);
                }
            }
        }
        hidden
    }

    fn update_layer_raster_cache(&mut self, ctx: &egui::Context) {
        use crate::layer_cache::{
            cache_entry_valid, install_cache_result, should_cache_layer, spawn_layer_raster_job,
        };

        if !self.enable_layer_raster_cache {
            while self.layer_cache_result_rx.try_recv().is_ok() {}
            self.layer_raster_cache.clear();
            self.layer_cache_pending.clear();
            return;
        }

        while let Ok(result) = self.layer_cache_result_rx.try_recv() {
            install_cache_result(
                &mut self.layer_raster_cache,
                &mut self.layer_cache_pending,
                ctx,
                result,
            );
        }

        let hidden = self.hidden_canvas_sources();
        let dragging = !self.tools.select.drag_snapshot.is_empty();
        let text_editing = self.on_page_text_edit.is_some();
        let revision = self.history.revision();
        let anim_frame = self.anim_current_frame;
        let anim_playing = self.anim_is_playing;

        let mut active_layer_ids = std::collections::HashSet::new();
        let layers: Vec<_> = self.project.document.layers.clone();

        for layer in &layers {
            if layer.kind != crate::document::LayerKind::Image {
                continue;
            }
            active_layer_ids.insert(layer.id);

            if !should_cache_layer(
                &self.project,
                layer,
                &hidden,
                self.enable_layer_raster_cache,
                dragging,
                text_editing,
                anim_playing,
                self.mcp_bulk_active(),
            ) {
                self.layer_raster_cache.remove(&layer.id);
                self.layer_cache_pending.remove(&layer.id);
                continue;
            }

            if self
                .layer_raster_cache
                .get(&layer.id)
                .is_some_and(|e| cache_entry_valid(e, revision, anim_frame))
            {
                continue;
            }
            if self.layer_cache_pending.contains(&layer.id) {
                continue;
            }

            self.layer_cache_pending.insert(layer.id);
            spawn_layer_raster_job(
                self.project.clone(),
                layer.clone(),
                hidden.clone(),
                revision,
                anim_frame,
                self.layer_cache_result_tx.clone(),
            );
        }

        self.layer_raster_cache
            .retain(|id, _| active_layer_ids.contains(id));
        self.layer_cache_pending
            .retain(|id| active_layer_ids.contains(id));
    }

    /// Returns true if the UI should repaint soon (audio prepare in flight only).
    pub fn sync_audio_playback(&mut self) -> bool {
        // Pause: keep players, only pause rodio (instant resume). Never full-file re-decode.
        if !self.anim_is_playing {
            for p in self.audio_players.values() {
                p.pause();
            }
            self.audio_prepare_rx.clear();
            return false;
        }

        // Ensure algebra (Time / Expr) is current before resolving VideoPlayer windows.
        self.eval_node_editor_graphs();

        let playhead_time = self.anim_current_frame as f32 / self.anim_fps as f32;
        // (path, timeline_start, file_offset, volume, bass, playback_rate)
        let mut active_clip_ids = std::collections::HashSet::new();
        let mut clip_info_map: std::collections::HashMap<
            uuid::Uuid,
            (String, f32, f32, f32, f32, f32),
        > = std::collections::HashMap::new();

        for layer in &self.project.document.layers {
            // P5: Node Editor Output Object sound.
            if layer.visible && layer.kind == crate::document::LayerKind::NodeEditor {
                if let Some(g) = layer.node_graph.as_ref() {
                    let snd = g.resolve_output_sound();
                    if let Some(path) = snd.path() {
                        let playable = crate::document::AvClip::path_is_audio_only(path)
                            || crate::document::AvClip::path_is_video_container(path)
                            || path.ends_with(".wav")
                            || path.ends_with(".WAV");
                        if playable && std::path::Path::new(path).is_file() {
                            let id = layer.id;
                            // Bass as mild gain boost (streaming path has no full-buffer EQ).
                            let bass_boost =
                                (1.0 + (snd.eq_bass as f32).clamp(-6.0, 12.0) * 0.08).max(0.05);
                            let vol = ((layer.volume as f64 * snd.volume) as f32 * bass_boost)
                                .clamp(0.0, 4.0);
                            // Never sticky-skip NE layers — extract/stream must be retriable.
                            self.audio_layers_skip.remove(&id);
                            active_clip_ids.insert(id);
                            // Timed VideoPlayer: seek media to media_time_sec.
                            // Untimed ObjectAudio/Video.sound: follow playhead from 0.
                            let (tl_start, file_off) = match snd.media_time_sec {
                                Some(t) => (playhead_time, (t as f32).max(0.0)),
                                None => (0.0_f32, 0.0_f32),
                            };
                            let rate = (snd.playback_rate as f32).clamp(0.05, 16.0);
                            clip_info_map.insert(
                                id,
                                (
                                    path.to_string(),
                                    tl_start,
                                    file_off,
                                    vol.max(0.05), // never fully mute via float noise
                                    snd.eq_bass as f32,
                                    rate,
                                ),
                            );
                        } else if !path.is_empty() {
                            log::debug!(
                                "NE sound path not playable yet: {path} (file={})",
                                std::path::Path::new(path).is_file()
                            );
                        }
                    }
                }
                continue;
            }
            if !layer.visible || layer.kind != crate::document::LayerKind::AV {
                continue;
            }
            let mut l = layer.clone();
            l.ensure_av_clips();
            for clip in &l.av_clips {
                if clip.media_path.is_empty() || clip.is_still_image() {
                    continue;
                }
                let path = &clip.media_path;
                let playable = crate::document::AvClip::path_is_audio_only(path)
                    || is_video_container_ext(path);
                if !playable {
                    continue;
                }
                let start = clip.video_timeline_start;
                let duration = clip.timeline_play_secs();
                if playhead_time >= start && playhead_time < start + duration {
                    active_clip_ids.insert(clip.id);
                    clip_info_map.insert(
                        clip.id,
                        (
                            clip.media_path.clone(),
                            start,
                            clip.video_start_offset,
                            layer.volume,
                            0.0,
                            1.0, // AV clips play 1× media rate
                        ),
                    );
                }
            }
        }

        if !self.ensure_audio_output() {
            return true;
        }
        let Some(device) = self.audio_device.as_ref() else {
            return true;
        };
        let mixer = device.mixer();
        let mut reset_audio_reason: Option<String> = None;

        // Drain any legacy prepare threads (no longer started for NE stream path).
        self.audio_prepare_rx.retain(|clip_id, rx| {
            match rx.try_recv() {
                Ok(_) | Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
                Err(std::sync::mpsc::TryRecvError::Empty) => active_clip_ids.contains(clip_id),
            }
        });

        for &clip_id in &active_clip_ids {
            let Some(&(ref media_path, start, start_offset, volume, _bass, play_rate)) =
                clip_info_map.get(&clip_id)
            else {
                continue;
            };
            if self.audio_layers_skip.contains(&clip_id) {
                continue;
            }

            let file_pos = ((playhead_time - start) + start_offset).max(0.0);
            // Scrub detection only — free-running rodio must NOT recreate every playhead skip.
            let jump_threshold = 1.25_f32 * play_rate.max(1.0);
            let mut need_create = !self.audio_players.contains_key(&clip_id);

            // Restart if rate changed (e.g. user edited Expr 1→2).
            if let Some(&prev_rate) = self.audio_player_playback_rate.get(&clip_id) {
                if (prev_rate - play_rate).abs() > 0.15 {
                    self.audio_players.remove(&clip_id);
                    self.audio_player_buffer_offset.remove(&clip_id);
                    self.audio_player_last_file_pos.remove(&clip_id);
                    self.audio_player_playback_rate.remove(&clip_id);
                    self.audio_player_media_path.remove(&clip_id);
                    need_create = true;
                }
            }
            // Septic / video file path changed mid-play → must not keep old audio stream.
            let path_changed = self
                .audio_player_media_path
                .get(&clip_id)
                .is_some_and(|p| p != media_path);
            if path_changed {
                let old = self.audio_player_media_path.remove(&clip_id);
                self.audio_players.remove(&clip_id);
                self.audio_player_buffer_offset.remove(&clip_id);
                self.audio_player_last_file_pos.remove(&clip_id);
                self.audio_player_playback_rate.remove(&clip_id);
                // Drop extract status for the old path so a new WAV is built.
                if let Some(prev_path) = old {
                    if let Ok(mut st) = self.audio_extract_status.lock() {
                        st.remove(&prev_path);
                    }
                }
                need_create = true;
                log::info!(
                    "audio media path changed for {clip_id}: reopening {}",
                    media_path
                );
            }

            if let Some(player) = self.audio_players.get(&clip_id) {
                let last_pos = self
                    .audio_player_last_file_pos
                    .get(&clip_id)
                    .copied()
                    .unwrap_or(file_pos);
                let delta = (file_pos - last_pos).abs();
                let scrubbed_back = file_pos + 0.35 < last_pos;
                let scrubbed_far = delta > jump_threshold;

                // Still has samples queued → just keep playing (do not thrash recreate).
                if !player.empty() && !scrubbed_back && !scrubbed_far {
                    player.set_volume(volume);
                    player.play();
                    // Advance tracked pos with wall clock estimate so small skips don't seek.
                    let est = last_pos
                        + (1.0 / self.anim_fps.max(1) as f32) * play_rate.max(0.05);
                    // Blend toward desired file_pos gently.
                    let blended = est * 0.7 + file_pos * 0.3;
                    self.audio_player_last_file_pos.insert(clip_id, blended);
                } else if player.empty() || scrubbed_back || scrubbed_far {
                    // Source finished or user scrubbed — rebuild from file_pos.
                    self.audio_players.remove(&clip_id);
                    self.audio_player_buffer_offset.remove(&clip_id);
                    self.audio_player_last_file_pos.remove(&clip_id);
                    self.audio_player_playback_rate.remove(&clip_id);
                    self.audio_player_media_path.remove(&clip_id);
                    need_create = true;
                }
            }

            if need_create {
                // Prefer cached WAV for video containers; pure audio / .wav stream as-is.
                let resolved = resolve_audio_path_for_rodio(
                    media_path,
                    &self.audio_extract_status,
                    &self.audio_pcm_cache,
                );
                if let Some(audio_path) = resolved {
                    if !audio_path.is_file() {
                        log::warn!("audio path missing: {}", audio_path.display());
                        self.status_message =
                            format!("Audio file missing: {}", audio_path.display());
                    } else {
                        let player = rodio::Player::connect_new(mixer);
                        match crate::audio_extract::stream_file_to_player_rate(
                            &player,
                            &audio_path,
                            file_pos,
                            volume,
                            play_rate,
                        ) {
                            Ok(()) => {
                                self.audio_player_last_file_pos.insert(clip_id, file_pos);
                                self.audio_player_buffer_offset.insert(clip_id, file_pos);
                                self.audio_player_playback_rate.insert(clip_id, play_rate);
                                self.audio_player_media_path
                                    .insert(clip_id, media_path.clone());
                                self.audio_players.insert(clip_id, player);
                                self.audio_layers_skip.remove(&clip_id);
                                // Surface once so silent graphs (Visualizer Level is synthetic) are diagnosable.
                                self.status_message = format!(
                                    "Audio → speakers @ {:.1}s ×{:.2}",
                                    file_pos, play_rate
                                );
                                log::info!(
                                    "audio playing {} @ {:.2}s rate={:.2} vol={:.2}",
                                    audio_path.display(),
                                    file_pos,
                                    play_rate,
                                    volume
                                );
                            }
                            Err(e) => {
                                log::warn!("audio stream failed {media_path}: {e}");
                                self.status_message = format!("Audio: {e}");
                                let msg = e.to_string().to_ascii_lowercase();
                                // Dead device / broken stream → reopen sink (no full app restart).
                                if msg.contains("device")
                                    || msg.contains("stream")
                                    || msg.contains("disconnected")
                                    || msg.contains("closed")
                                    || msg.contains("i/o")
                                    || msg.contains("io error")
                                {
                                    reset_audio_reason = Some(e.to_string());
                                } else if !self.project.document.layers.iter().any(|l| {
                                    l.id == clip_id
                                        && l.kind == crate::document::LayerKind::NodeEditor
                                }) {
                                    // Soft skip AV layers only; NE retries next frames.
                                    self.audio_layers_skip.insert(clip_id);
                                }
                            }
                        }
                    }
                } else {
                    // Video extract not ready — one background extract is enough.
                    spawn_video_audio_extract(
                        media_path,
                        &self.audio_extract_status,
                        &self.audio_pcm_cache,
                    );
                    if let Ok(map) = self.audio_extract_status.lock() {
                        match map.get(media_path) {
                            Some(AudioExtractStatus::Extracting { progress }) => {
                                self.status_message = format!(
                                    "Extracting video audio… {:.0}%",
                                    progress * 100.0
                                );
                            }
                            Some(AudioExtractStatus::Failed) => {
                                self.status_message =
                                    "Audio extract failed (no track or FFmpeg/libav issue)"
                                        .into();
                            }
                            Some(AudioExtractStatus::Ready(p)) if !p.is_file() => {
                                // Stale Ready — clear so extract can run again.
                                drop(map);
                                if let Ok(mut m) = self.audio_extract_status.lock() {
                                    m.remove(media_path);
                                }
                            }
                            _ => {
                                self.status_message =
                                    "Preparing audio…".into();
                            }
                        }
                    }
                }
            }
        }

        self.audio_players
            .retain(|id, _| active_clip_ids.contains(id));
        self.audio_player_buffer_offset
            .retain(|id, _| active_clip_ids.contains(id));
        self.audio_player_last_file_pos
            .retain(|id, _| active_clip_ids.contains(id));
        self.audio_player_playback_rate
            .retain(|id, _| active_clip_ids.contains(id));
        self.audio_player_media_path
            .retain(|id, _| active_clip_ids.contains(id));
        self.audio_prepare_rx
            .retain(|id, _| active_clip_ids.contains(id));

        if let Some(reason) = reset_audio_reason {
            self.reset_audio_output(&reason);
            return true; // reopen device next frame
        }

        // Only repaint for in-flight prepare (streaming path rarely needs this).
        !self.audio_prepare_rx.is_empty()
    }

    pub fn set_path_handle_mode(&mut self, id: NodeId, anchor_idx: usize, mode: BezierHandleMode) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_handle_mode(anchor_idx, mode);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_path_anchor_smooth(&mut self, id: NodeId, anchor_idx: usize, smooth: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let already = matches!(
            &before.kind,
            NodeKind::Path { path } if path.is_anchor_smooth(anchor_idx) == smooth
        );
        if already {
            return;
        }
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_anchor_smooth(anchor_idx, smooth);
        } else {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.status_message = if smooth {
            format!("Smooth point {}", anchor_idx + 1)
        } else {
            format!("Sharp point {}", anchor_idx + 1)
        };
    }

    /// Enable corner curve (LPE-style fillet) at the sharp vertex. Creates two yellow tangent points
    /// on the legs. Non-destructive. Yellows are always equidistant via D = R / tan(θ/2) formula.
    /// Drag either to adjust the radius R for a proper circular arc.
    pub fn make_corner_curve(&mut self, id: NodeId, corner_idx: usize) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            let anchors = path.anchor_positions();
            let n = anchors.len();
            if corner_idx >= n {
                return;
            }
            let p = anchors[corner_idx];
            // prev leg
            let prev = if corner_idx > 0 { corner_idx - 1 } else if path.is_closed() && n > 2 { n-1 } else { return };
            let pa = anchors[prev];
            let len_prev = ((p.0 - pa.0).powi(2) + (p.1 - pa.1).powi(2)).sqrt();
            // next leg
            let next = if corner_idx + 1 < n { corner_idx + 1 } else if path.is_closed() && n > 2 { 0 } else { return };
            let pb = anchors[next];
            let len_next = ((p.0 - pb.0).powi(2) + (p.1 - pb.1).powi(2)).sqrt();
            let d = 0.10 * len_prev.min(len_next).max(1.0);
            // Compute R such that initial D = 0.1*min_len  =>  R = D * tan(θ/2)
            let r = if let Some(theta) = path.corner_angle_at(corner_idx) {
                let h = theta / 2.0;
                let th = h.tan();
                if th > 1e-9 { d * th } else { d }
            } else {
                d
            };
            path.set_corner_fillet(corner_idx, r);
        } else {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.tools.select.selected_path_points.clear();
        self.tools.select.selected_path_points.push((id, corner_idx));
        self.tools.select.selected_path_segment = None;
        self.tools.select.node_edit_target = Some(PathEditTarget::Anchor(corner_idx));
        ui::promote_action_tab(self, ui::ActionTab::Geometry);
        self.status_message = "Corner curve enabled (two yellow points on legs)".into();
    }

    pub fn smooth_selected_path_points(&mut self) {
        let points = self.tools.select.selected_path_points.clone();
        if points.is_empty() {
            return;
        }
        let mut by_path: std::collections::HashMap<NodeId, Vec<usize>> =
            std::collections::HashMap::new();
        for (id, idx) in points {
            by_path.entry(id).or_default().push(idx);
        }
        for (id, indices) in by_path {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            if let NodeKind::Path { path } = &mut after.kind {
                for idx in indices {
                    path.set_anchor_smooth(idx, true);
                }
            } else {
                continue;
            }
            if before != after {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
            }
        }
        self.status_message = "Smooth curve on selected points".into();
    }

    pub fn remove_selected_path_points(&mut self) -> bool {
        let points = self.tools.select.selected_path_points.clone();
        if points.is_empty() {
            return false;
        }
        let mut by_path: std::collections::HashMap<NodeId, Vec<usize>> =
            std::collections::HashMap::new();
        for (id, idx) in points {
            by_path.entry(id).or_default().push(idx);
        }
        let mut removed_any = false;
        for (id, mut indices) in by_path {
            indices.sort_unstable();
            indices.dedup();
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            if let NodeKind::Path { path } = &mut after.kind {
                if path.remove_anchors(&indices) {
                    self.history.push(
                        &mut self.project,
                        ProjectEdit::PatchNode { id, before, after },
                    );
                    removed_any = true;
                }
            }
        }
        if removed_any {
            self.tools.select.clear_path_point_selection();
            self.status_message = "Removed path point(s)".into();
        }
        removed_any
    }

    pub fn selection_path_and_objects(&self) -> Option<(Vec<NodeId>, NodeId)> {
        if self.selection.len() == 1 {
            if let Some(eff) =
                path_effect_by_form_node(&self.project.document.path_effects, self.selection[0])
            {
                return Some((vec![eff.source_id], eff.path_id));
            }
        }
        let mut paths = Vec::new();
        let mut objects = Vec::new();
        for id in &self.selection {
            let Some(node) = self.project.nodes.get(*id) else {
                continue;
            };
            match &node.kind {
                NodeKind::Path { .. } => paths.push(*id),
                NodeKind::Group { .. } => {}
                _ => objects.push(*id),
            }
        }
        if paths.len() == 1 && !objects.is_empty() {
            Some((objects, paths[0]))
        } else {
            None
        }
    }

    pub fn selection_path_and_object(&self) -> Option<(NodeId, NodeId)> {
        self.selection_path_and_objects()
            .and_then(|(objs, path)| objs.first().copied().map(|o| (o, path)))
    }

    pub fn sync_on_path_ui_from_selection(&mut self) {
        // Prefer direct path+object selection
        if let Some((obj, path)) = self.selection_path_and_object() {
            if let Some(effect) =
                find_effect_for_pair(&self.project.document.path_effects, obj, path)
            {
                self.ui_on_path_mode = effect.mode;
                self.ui_on_path_gap = effect.gap;
                self.ui_on_path_count = effect.count;
                self.ui_on_path_cyclic = effect.cyclic;
                self.ui_on_path_rotate = effect.rotate_to_tangent;
                self.ui_on_path_loft_scale = effect.loft_end_scale;
                self.ui_on_path_loft_opacity = effect.loft_end_opacity;
                self.backfill_path_effect_forms_if_needed(path, &[obj]);
                return;
            }
        }
        // Fallback: path selected that already has effect(s) (panel context)
        if let Some((objs, path)) = self.object_on_path_panel_context() {
            if let Some(&obj) = objs.first() {
                if let Some(effect) =
                    find_effect_for_pair(&self.project.document.path_effects, obj, path)
                {
                    self.ui_on_path_mode = effect.mode;
                    self.ui_on_path_gap = effect.gap;
                    self.ui_on_path_count = effect.count;
                    self.ui_on_path_cyclic = effect.cyclic;
                    self.ui_on_path_rotate = effect.rotate_to_tangent;
                    self.ui_on_path_loft_scale = effect.loft_end_scale;
                    self.ui_on_path_loft_opacity = effect.loft_end_opacity;
                }
            }
            self.backfill_path_effect_forms_if_needed(path, &objs);
        }
    }

    pub fn sync_tiling_ui_from_selection(&mut self) {
        if let Some(&oid) = self.selection.first() {
            if let Some(effect) = self
                .project
                .document
                .tiling_effects
                .values()
                .find(|e| e.source_id == oid)
            {
                self.ui_tiling_rows = effect.count_y;
                self.ui_tiling_cols = effect.count_x;
                self.ui_tiling_offset_x = effect.offset_x;
                self.ui_tiling_offset_y = effect.offset_y;
                self.ui_tiling_row_rot = effect.row_rotation;
                self.ui_tiling_col_rot = effect.col_rotation;
                self.ui_tiling_row_scale = effect.row_scale;
                self.ui_tiling_col_scale = effect.col_scale;
                self.ui_tiling_gap_x = effect.gap_x;
                self.ui_tiling_gap_y = effect.gap_y;
            }
        }
    }

    pub fn sync_circular_ui_from_selection(&mut self) {
        if let Some(&oid) = self.selection.first() {
            if let Some(effect) = self
                .project
                .document
                .circular_effects
                .values()
                .find(|e| e.source_id == oid)
            {
                self.ui_circular_copies = effect.copies;
                self.ui_circular_angle_offset = effect.angle_offset;
                self.ui_circular_origin_x = effect.origin_x;
                self.ui_circular_origin_y = effect.origin_y;
                self.ui_circular_rotate_mode = effect.rotate_mode;
            }
        }
    }

    fn get_tiling_gizmo_points(&self, id: NodeId) -> Option<[(f64, f64); 3]> {
        if let Some(e) = self.project.document.tiling_effects.values().find(|e| e.source_id == id) {
            if let Some(node) = self.project.nodes.get(id) {
                let b = node.bounds();
                let p0 = (b.x0 + e.offset_x, b.y0 + e.offset_y);
                let p1 = (p0.0 + e.gap_x, p0.1);
                let p2 = (p0.0, p0.1 + e.gap_y);
                return Some([p0, p1, p2]);
            }
        }
        None
    }

    fn get_circular_gizmo_points(&self, id: NodeId) -> Option<[(f64, f64); 3]> {
        if let Some(e) = self.project.document.circular_effects.values().find(|e| e.source_id == id) {
            // 0 = base (first instance on ring), 1 = origin (center), 2 = angle tip (next copy).
            let p0 = (e.base_x, e.base_y);
            let p1 = (e.origin_x, e.origin_y);
            let p2 = e.placement_xy(1);
            return Some([p0, p1, p2]);
        }
        None
    }

    /// Hit circular gizmo in **screen space** (handles stay easy to grab at any zoom).
    /// Returns handle index: 0 base, 1 origin, 2 angle tip; or None.
    fn hit_circular_gizmo(
        &self,
        id: NodeId,
        screen: Pos2,
        origin: Pos2,
    ) -> Option<usize> {
        let pts = self.get_circular_gizmo_points(id)?;
        let slop = 14.0_f32; // px
        // Prefer points over lines (check closest point first).
        let mut best: Option<(usize, f32)> = None;
        for (i, &(px, py)) in pts.iter().enumerate() {
            let sp = self.viewport.doc_to_screen((px, py), origin);
            let d = screen.distance(sp);
            if d <= slop && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        if let Some((i, _)) = best {
            return Some(i);
        }
        // Lines: base↔origin (radius), origin↔angle tip
        let line_slop = 10.0_f32;
        let s0 = self.viewport.doc_to_screen(pts[0], origin);
        let s1 = self.viewport.doc_to_screen(pts[1], origin);
        let s2 = self.viewport.doc_to_screen(pts[2], origin);
        if Self::dist_point_to_segment_screen(screen, s0, s1) <= line_slop {
            // Near radius line: pick nearer endpoint.
            return if screen.distance(s0) <= screen.distance(s1) {
                Some(0)
            } else {
                Some(1)
            };
        }
        if Self::dist_point_to_segment_screen(screen, s1, s2) <= line_slop {
            return if screen.distance(s2) <= screen.distance(s1) {
                Some(2)
            } else {
                Some(1)
            };
        }
        None
    }

    fn dist_point_to_segment_screen(p: Pos2, a: Pos2, b: Pos2) -> f32 {
        let ab = b - a;
        let len_sq = ab.length_sq();
        if len_sq < 1e-8 {
            return p.distance(a);
        }
        let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
        let proj = a + ab * t;
        p.distance(proj)
    }

    fn sync_circular_ui_from_effect_id(&mut self, source_id: NodeId) {
        if let Some(effect) = self
            .project
            .document
            .circular_effects
            .values()
            .find(|e| e.source_id == source_id)
        {
            self.ui_circular_copies = effect.copies;
            self.ui_circular_angle_offset = effect.angle_offset;
            self.ui_circular_origin_x = effect.origin_x;
            self.ui_circular_origin_y = effect.origin_y;
            self.ui_circular_rotate_mode = effect.rotate_mode;
        }
    }

    /// Keep circular base/origin locked to the source object when it is translated.
    fn translate_circular_effect_for_source(&mut self, source_id: NodeId, dx: f64, dy: f64) {
        if dx.abs() < 1e-15 && dy.abs() < 1e-15 {
            return;
        }
        if let Some((_, e)) = self
            .project
            .document
            .circular_effects
            .iter_mut()
            .find(|(_, e)| e.source_id == source_id)
        {
            e.base_x += dx;
            e.base_y += dy;
            e.origin_x += dx;
            e.origin_y += dy;
            e.radius = (e.base_x - e.origin_x).hypot(e.base_y - e.origin_y).max(1.0);
        }
    }

    fn build_on_path_effect(&self, effect_id: uuid::Uuid, source_id: NodeId, path_id: NodeId) -> ObjectOnPathEffect {
        let gap = if self.ui_on_path_mode == OnPathMode::Loft {
            self
                .project
                .nodes
                .get(source_id)
                .map(default_loft_gap_for_node)
                .unwrap_or(2.0)
                .max(0.5)
        } else {
            self.ui_on_path_gap
        };
        ObjectOnPathEffect {
            id: effect_id,
            source_id,
            path_id,
            mode: self.ui_on_path_mode,
            gap,
            count: self.ui_on_path_count.max(2),
            start_offset: 0.0,
            rotate_to_tangent: self.ui_on_path_rotate,
            cyclic: self.ui_on_path_cyclic,
            loft_end_scale: self.ui_on_path_loft_scale,
            loft_end_opacity: self.ui_on_path_loft_opacity,
            hide_source: true,
            form_node_id: None,
        }
    }

    pub fn object_on_path_panel_context(&self) -> Option<(Vec<NodeId>, NodeId)> {
        if self.selection.len() == 1 {
            if let Some(eff) =
                path_effect_by_form_node(&self.project.document.path_effects, self.selection[0])
            {
                return Some((vec![eff.source_id], eff.path_id));
            }
        }
        if let Some(ctx) = self.selection_path_and_objects() {
            return Some(ctx);
        }
        if self.selection.len() != 1 {
            return None;
        }
        let path_id = self.selection[0];
        let path_node = self.project.nodes.get(path_id)?;
        if !matches!(path_node.kind, NodeKind::Path { .. }) {
            return None;
        }
        let mut objects = Vec::new();
        for effect_id in &path_node.path_effect_links {
            let Some(effect) = self.project.document.path_effects.get(effect_id) else {
                continue;
            };
            if effect.path_id == path_id && !objects.contains(&effect.source_id) {
                objects.push(effect.source_id);
            }
        }
        if objects.is_empty() {
            None
        } else {
            Some((objects, path_id))
        }
    }

    pub fn selection_has_object_on_path_effect(&self) -> bool {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return false;
        };
        has_effect_for_objects(&self.project.document.path_effects, &objects, path_id)
    }

    /// Shapes eligible for Tiling / CircularClone (includes Path; excludes Group).
    pub fn is_tiling_circular_source(node: &Node) -> bool {
        !matches!(node.kind, NodeKind::Group { .. })
            && !matches!(node.kind, NodeKind::Image { .. })
            && !matches!(node.kind, NodeKind::Text { .. })
            && !matches!(node.kind, NodeKind::BrushStroke { .. })
            && !matches!(node.kind, NodeKind::FlowchartNode { .. })
            && !matches!(node.kind, NodeKind::FlowchartPath { .. })
    }

    pub fn selection_tiling_circular_sources(&self) -> Vec<NodeId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| {
                self.project
                    .nodes
                    .get(*id)
                    .is_some_and(Self::is_tiling_circular_source)
            })
            .collect()
    }

    pub fn selection_has_tiling_effect(&self) -> bool {
        self.selection.iter().any(|&oid| {
            self.project
                .document
                .tiling_effects
                .values()
                .any(|e| e.source_id == oid)
        })
    }

    pub fn selection_has_circular_effect(&self) -> bool {
        self.selection.iter().any(|&oid| {
            self.project
                .document
                .circular_effects
                .values()
                .any(|e| e.source_id == oid)
        })
    }

    /// Convert selected shapes (rect/circle/ellipse/chord/polygon/arc/…) to editable paths.
    pub fn convert_selection_to_path(&mut self) {
        let ids: Vec<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| {
                self.project.nodes.get(*id).is_some_and(|n| {
                    !matches!(
                        n.kind,
                        NodeKind::Path { .. }
                            | NodeKind::Group { .. }
                            | NodeKind::Image { .. }
                            | NodeKind::Text { .. }
                            | NodeKind::BrushStroke { .. }
                            | NodeKind::FlowchartNode { .. }
                            | NodeKind::FlowchartPath { .. }
                    )
                })
            })
            .collect();
        if ids.is_empty() {
            self.status_message =
                "Select circle/rect/ellipse/chord/polygon/arc to convert to path".into();
            return;
        }
        let mut count = 0usize;
        for id in ids {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let bez = before.bez_path();
            if bez.elements().is_empty() {
                continue;
            }
            let mut after = before.clone();
            let name = before.name.clone();
            let style = before.style.clone();
            after.kind = NodeKind::Path {
                path: PathData::from_bez(&bez),
            };
            after.name = if name.is_empty() {
                "Path".into()
            } else {
                format!("{name} path")
            };
            after.style = style;
            // Ensure closed flag when shape was closed.
            if let NodeKind::Path { path } = &mut after.kind {
                if !path.is_closed() {
                    path.set_closed(true);
                }
            }
            if before != after {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
                count += 1;
            }
        }
        self.status_message = if count > 0 {
            format!("Converted {count} shape(s) to path")
        } else {
            "Nothing converted".into()
        };
    }

    /// Snap a free point (gizmo handle) to magnets then grid.
    fn snap_gizmo_point(&mut self, doc: (f64, f64), exclude: Option<NodeId>) -> (f64, f64) {
        let mut snapped = doc;
        let mut mag_x = false;
        let mut mag_y = false;
        self.live_snap_guides.clear();
        if self.snap_magnet {
            let threshold = (10.0 / self.viewport.zoom as f64).max(0.5);
            let mut target_pts = self.get_canvas_snap_points();
            for (id, node) in &self.project.nodes.map {
                if exclude == Some(*id) {
                    continue;
                }
                target_pts.extend(self.get_node_snap_points(node));
            }
            for e in self.project.document.circular_effects.values() {
                if exclude == Some(e.source_id) {
                    continue;
                }
                target_pts.push((e.base_x, e.base_y));
                target_pts.push((e.origin_x, e.origin_y));
                target_pts.push(e.placement_xy(1));
            }
            let mut best_dx = threshold;
            let mut best_dy = threshold;
            let mut snap_x = None;
            let mut snap_y = None;
            for &tpt in &target_pts {
                let dx = tpt.0 - doc.0;
                let dy = tpt.1 - doc.1;
                if dx.abs() < best_dx.abs() {
                    best_dx = dx;
                    snap_x = Some(tpt);
                }
                if dy.abs() < best_dy.abs() {
                    best_dy = dy;
                    snap_y = Some(tpt);
                }
            }
            if let Some(tpt) = snap_x {
                snapped.0 = tpt.0;
                mag_x = true;
                self.live_snap_guides.push(SnapGuide {
                    start: tpt,
                    end: (tpt.0, snapped.1),
                    is_tangent: false,
                });
            }
            if let Some(tpt) = snap_y {
                snapped.1 = tpt.1;
                mag_y = true;
                self.live_snap_guides.push(SnapGuide {
                    start: tpt,
                    end: (snapped.0, tpt.1),
                    is_tangent: false,
                });
            }
        }
        // Grid snap on free axes only (magnet wins when active).
        if self.viewport.snap_grid {
            let g = self.viewport.grid_step as f64;
            if g > 0.0 {
                if !mag_x {
                    snapped.0 = (snapped.0 / g).round() * g;
                }
                if !mag_y {
                    snapped.1 = (snapped.1 / g).round() * g;
                }
            }
        }
        if self.pixel_art_mode {
            let cell = self.pixel_cell_size as f64;
            snapped.0 = (snapped.0 / cell).round() * cell;
            snapped.1 = (snapped.1 / cell).round() * cell;
        }
        snapped
    }

    fn node_has_tiling_or_circular(&self, id: NodeId) -> bool {
        self.project.document.tiling_effects.values().any(|e| e.source_id == id)
            || self
                .project
                .document
                .circular_effects
                .values()
                .any(|e| e.source_id == id)
    }

    fn node_uses_extended_bounds(&self, id: NodeId) -> bool {
        node_uses_extended_pick_bounds(&self.project.document, id)
    }

    /// Hit-test including circular/tiling placement footprints (not only source bbox).
    fn hit_test_node_for_pick(
        &self,
        id: NodeId,
        node: &Node,
        doc: (f64, f64),
        slop: f64,
    ) -> bool {
        if let Some(e) = self
            .project
            .document
            .circular_effects
            .values()
            .find(|e| e.source_id == id)
        {
            return crate::document::hit_test_circular_clone(node, e, doc.0, doc.1, slop);
        }
        if self.node_uses_extended_bounds(id) {
            let eb = crate::document::get_effective_bounds(
                node,
                &self.project.document,
                &self.project.nodes,
            );
            let pt = kurbo::Point::new(doc.0, doc.1);
            return eb.inflate(slop, slop).contains(pt);
        }
        node.hit_test_with_store(&self.project.nodes, doc.0, doc.1, slop)
    }

    fn precise_hit_for_pick(
        &self,
        id: NodeId,
        node: &Node,
        doc: (f64, f64),
        slop: f64,
    ) -> bool {
        if self
            .project
            .document
            .circular_effects
            .values()
            .any(|e| e.source_id == id)
        {
            // hit_test_circular_clone already tested instances.
            return true;
        }
        if self.node_uses_extended_bounds(id) {
            return true;
        }
        let pt = kurbo::Point::new(doc.0, doc.1);
        node.bez_path().contains(pt)
            || matches!(node.kind, NodeKind::Text { .. })
            || matches!(node.kind, NodeKind::Image { .. })
            || node.hit_test_with_store(&self.project.nodes, doc.0, doc.1, slop)
    }

    fn expand_drag_ids_for_path_effects(&self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = Vec::new();
        for &id in ids {
            for bid in path_effect_move_bundle(&self.project.document, id) {
                if !out.contains(&bid) {
                    out.push(bid);
                }
            }
        }
        out
    }

    /// Commit object-on-path for the current path + object selection.
    pub fn apply_object_on_path_effect(&mut self) {
        let Some((objects, path_id)) = self.selection_path_and_objects() else {
            return;
        };
        let path_data = self.project.nodes.get(path_id).and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.clone()),
            _ => None,
        });
        let Some(path_data) = path_data else {
            return;
        };
        let tol = 0.5 / self.viewport.zoom as f64;
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created: Vec<(NodeId, uuid::Uuid, Option<Node>)> = Vec::new();
        for source_id in &objects {
            if has_effect_for_objects(&after_doc.path_effects, &[*source_id], path_id) {
                continue;
            }
            let effect_id = uuid::Uuid::new_v4();
            let mut effect = self.build_on_path_effect(effect_id, *source_id, path_id);
            let form_node = self.project.nodes.get(*source_id).and_then(|source| {
                build_path_effect_form_node(source, &effect, &path_data, tol)
            });
            if let Some(ref form) = form_node {
                effect.form_node_id = Some(form.id);
            }
            after_doc.path_effects.insert(effect_id, effect);
            created.push((*source_id, effect_id, form_node));
        }
        if created.is_empty() {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        let mut form_selection: Vec<NodeId> = Vec::new();
        for (source_id, effect_id, form_node) in created {
            if let Some(form) = form_node {
                let form_id = form.id;
                self.history
                    .push(&mut self.project, ProjectEdit::InsertNode { node: form });
                form_selection.push(form_id);
            }
            for id in [source_id, path_id] {
                let Some(before) = self.project.nodes.get(id).cloned() else {
                    continue;
                };
                if before.path_effect_links.contains(&effect_id) {
                    continue;
                }
                let mut after = before.clone();
                after.path_effect_links.push(effect_id);
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
            }
        }
        if form_selection.len() == 1 {
            self.selection = form_selection;
        }
        self.status_message = "Object on path applied — drag the combined form to move".into();
    }

    /// Update parameters on effects that are already applied (live, no undo step).
    /// Create missing pick/drag form proxies for effects saved before form nodes existed.
    fn backfill_path_effect_forms_if_needed(&mut self, path_id: NodeId, source_ids: &[NodeId]) {
        let path_data = self.project.nodes.get(path_id).and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.clone()),
            _ => None,
        });
        let Some(path_data) = path_data else {
            return;
        };
        let tol = 0.5 / self.viewport.zoom as f64;
        for &source_id in source_ids {
            let Some(existing) =
                find_effect_for_pair(&self.project.document.path_effects, source_id, path_id)
            else {
                continue;
            };
            if existing.form_node_id.is_some() {
                continue;
            }
            let Some(source) = self.project.nodes.get(source_id).cloned() else {
                continue;
            };
            let Some(form) = build_path_effect_form_node(&source, existing, &path_data, tol) else {
                continue;
            };
            let form_id = form.id;
            let effect_id = existing.id;
            let before_doc = snapshot_document(&self.project.document);
            let mut after_doc = before_doc.clone();
            if let Some(e) = after_doc.path_effects.get_mut(&effect_id) {
                e.form_node_id = Some(form_id);
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchDocument {
                    before: before_doc,
                    after: after_doc,
                },
            );
            self.history
                .push(&mut self.project, ProjectEdit::InsertNode { node: form });
        }
    }

    pub fn update_object_on_path_effects_live(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        self.backfill_path_effect_forms_if_needed(path_id, &objects);
        let path_data = self.project.nodes.get(path_id).and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.clone()),
            _ => None,
        });
        let Some(path_data) = path_data else {
            return;
        };
        let tol = 0.5 / self.viewport.zoom as f64;
        for source_id in objects {
            let Some(existing) =
                find_effect_for_pair(&self.project.document.path_effects, source_id, path_id)
            else {
                continue;
            };
            let form_id = existing.form_node_id;
            let mut effect = self.build_on_path_effect(existing.id, source_id, path_id);
            effect.form_node_id = form_id;
            self.project
                .document
                .path_effects
                .insert(existing.id, effect.clone());
            if let (Some(fid), Some(source)) = (
                form_id,
                self.project.nodes.get(source_id).cloned(),
            ) {
                if let Some(form) = self.project.nodes.get_mut(fid) {
                    sync_path_effect_form_geometry(
                        form,
                        &source,
                        &effect,
                        &path_data,
                        tol,
                    );
                }
            }
        }
    }

    pub fn update_tiling_effects_live(&mut self) {
        let objs = self.selection_tiling_circular_sources();
        for oid in objs {
            if let Some(existing) = self.project.document.tiling_effects.values().find(|e| e.source_id == oid).cloned() {
                let mut effect = existing;
                effect.count_y = self.ui_tiling_rows;
                effect.count_x = self.ui_tiling_cols;
                effect.offset_x = self.ui_tiling_offset_x;
                effect.offset_y = self.ui_tiling_offset_y;
                effect.row_rotation = self.ui_tiling_row_rot;
                effect.col_rotation = self.ui_tiling_col_rot;
                effect.row_scale = self.ui_tiling_row_scale;
                effect.col_scale = self.ui_tiling_col_scale;
                effect.gap_x = self.ui_tiling_gap_x;
                effect.gap_y = self.ui_tiling_gap_y;
                self.project.document.tiling_effects.insert(effect.id, effect);
            }
        }
    }

    pub fn update_circular_effects_live(&mut self) {
        let objs = self.selection_tiling_circular_sources();
        for oid in objs {
            if let Some(existing) = self.project.document.circular_effects.values().find(|e| e.source_id == oid).cloned() {
                let mut effect = existing;
                effect.copies = self.ui_circular_copies;
                effect.angle_offset = self.ui_circular_angle_offset;
                effect.origin_x = self.ui_circular_origin_x;
                effect.origin_y = self.ui_circular_origin_y;
                effect.rotate_mode = self.ui_circular_rotate_mode;
                self.project.document.circular_effects.insert(effect.id, effect);
            }
        }
    }

    pub fn remove_object_on_path_effect(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        for source_id in objects {
            self.remove_one_object_on_path_effect(source_id, path_id);
        }
    }

    fn remove_one_object_on_path_effect(&mut self, source_id: NodeId, path_id: NodeId) {
        let Some(effect) =
            find_effect_for_pair(&self.project.document.path_effects, source_id, path_id)
        else {
            return;
        };
        let effect_id = effect.id;
        let form_id = effect.form_node_id;
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        after_doc.path_effects.swap_remove(&effect_id);
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        if let Some(fid) = form_id {
            if let Some(node) = self.project.nodes.get(fid).cloned() {
                let layer_index = self.project.document.active_layer_index;
                let layer_nodes_before = self
                    .project
                    .document
                    .active_layer()
                    .map(|l| l.nodes.clone())
                    .unwrap_or_default();
                let removed_anims = self
                    .project
                    .anim_timeline
                    .nodes
                    .get(&fid)
                    .cloned()
                    .map(|a| vec![(fid, a)])
                    .unwrap_or_default();
                self.history.push(
                    &mut self.project,
                    ProjectEdit::RemoveNodes {
                        removed: vec![(fid, node)],
                        removed_anims,
                        layer_index,
                        layer_nodes_before,
                        ne_proxy_before: Vec::new(),
                    },
                );
                self.selection.retain(|id| *id != fid);
            }
        }
        for id in [source_id, path_id] {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            if !before.path_effect_links.contains(&effect_id) {
                continue;
            }
            let mut after = before.clone();
            after.path_effect_links.retain(|e| *e != effect_id);
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
        self.status_message = "Removed object-on-path effect".into();
    }

    pub fn bake_object_on_path_copies(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        self.update_object_on_path_effects_live();
        let path_data = self.project.nodes.get(path_id).and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.clone()),
            _ => None,
        });
        let Some(path) = path_data else {
            return;
        };
        let tol = 0.5 / self.viewport.zoom as f64;
        let mut child_ids = Vec::new();
        for source_id in &objects {
            let Some(effect) =
                find_effect_for_pair(&self.project.document.path_effects, *source_id, path_id)
                    .cloned()
            else {
                continue;
            };
            let Some(source) = self.project.nodes.get(*source_id).cloned() else {
                continue;
            };
            if effect.mode == OnPathMode::Loft {
                if let Some(mut node) = loft_sweep_node(&source, &effect, &path, tol) {
                    node.name = format!("{} loft", source.name);
                    let id = node.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node });
                    child_ids.push(id);
                }
            } else {
                let placements = effect_placements(&effect, &path as &dyn PathMagic, tol);
                for (i, placement) in placements.iter().enumerate() {
                    let mut node = node_at_placement(&source as &dyn FaceRenderable, placement);
                    node.name = format!("{} #{}", source.name, i + 1);
                    let id = node.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node });
                    child_ids.push(id);
                }
            }
        }
        if child_ids.is_empty() {
            self.status_message = "Nothing to bake — adjust path effect settings".into();
            return;
        }
        let group_name = if objects.len() == 1 {
            format!(
                "{} on path",
                self.project
                    .nodes
                    .get(objects[0])
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| "Object".into())
            )
        } else {
            "Objects on path".into()
        };
        let group = Node::group(child_ids.clone(), group_name);
        let group_id = group.id;
        self.history
            .push(&mut self.project, ProjectEdit::InsertNode { node: group });
        self.selection = vec![group_id];
        self.status_message = format!(
            "Baked {} instance(s) into group",
            child_ids.len()
        );
    }

    pub fn apply_tiling_magic(&mut self) {
        let objects = self.selection_tiling_circular_sources();
        if objects.is_empty() {
            self.status_message =
                "Select path/circle/rect/ellipse/chord/… to apply Tiling".into();
            return;
        }
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created = vec![];
        for &source_id in &objects {
            if after_doc.tiling_effects.values().any(|e| e.source_id == source_id) {
                continue;
            }
            let Some(source) = self.project.nodes.get(source_id) else { continue; };
            let b = source.bounds();
            let w = (b.x1 - b.x0).abs().max(1.0);
            let h = (b.y1 - b.y0).abs().max(1.0);
            let effect_id = uuid::Uuid::new_v4();
            let effect = TilingEffect {
                id: effect_id,
                source_id,
                gap_x: w,
                gap_y: h,
                count_x: 3,
                count_y: 3,
                offset_x: 0.0,  // top-left offset for first
                offset_y: 0.0,
                row_rotation: 0.0,
                col_rotation: 0.0,
                row_scale: 0.0,
                col_scale: 0.0,
                hide_source: true,
            };
            after_doc.tiling_effects.insert(effect_id, effect);
            created.push(source_id);
            // sync ui
            self.ui_tiling_gap_x = w;
            self.ui_tiling_gap_y = h;
            self.ui_tiling_rows = 3;
            self.ui_tiling_cols = 3;
            self.ui_tiling_offset_x = 0.0;
            self.ui_tiling_offset_y = 0.0;
            self.ui_tiling_row_rot = 0.0;
            self.ui_tiling_col_rot = 0.0;
            self.ui_tiling_row_scale = 0.0;
            self.ui_tiling_col_scale = 0.0;
        }
        if created.is_empty() {
            self.status_message = "No new Tiling effects".into();
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before: before_doc, after: after_doc },
        );
        self.status_message = format!("Enabled Tiling for {} object(s). Use container to bake.", created.len());
    }

    pub fn apply_circular_clone_magic(&mut self) {
        let objects = self.selection_tiling_circular_sources();
        if objects.is_empty() {
            self.status_message =
                "Select path/circle/rect/ellipse/chord/… to apply CircularClone".into();
            return;
        }
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created = vec![];
        for &source_id in &objects {
            if after_doc.circular_effects.values().any(|e| e.source_id == source_id) {
                continue;
            }
            let Some(source) = self.project.nodes.get(source_id) else { continue; };
            let b = source.bounds();
            let ref_x = (b.x0 + b.x1) * 0.5;
            let ref_y = (b.y0 + b.y1) * 0.5;
            let r = ((b.x1 - b.x0).abs().max((b.y1 - b.y0).abs()) * 1.5).max(10.0);
            let effect_id = uuid::Uuid::new_v4();
            let effect = CircularCloneEffect {
                id: effect_id,
                source_id,
                origin_x: ref_x,
                origin_y: ref_y,
                radius: r,
                copies: 6,
                angle_offset: 0.0,
                // Place base on the ring (not on the origin) so radius is usable immediately.
                base_x: ref_x + r,
                base_y: ref_y,
                hide_source: true,
                rotate_mode: self.ui_circular_rotate_mode,
            };
            after_doc.circular_effects.insert(effect_id, effect);
            created.push(source_id);
        }
        if created.is_empty() {
            self.status_message = "No new CircularClone effects".into();
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before: before_doc, after: after_doc },
        );
        self.status_message = format!("Enabled CircularClone for {} object(s). Use container to bake.", created.len());
    }

    pub fn remove_tiling_effect(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut removed = false;
        for oid in &objs {
            let keys: Vec<_> = after_doc.tiling_effects.iter().filter(|(_, e)| e.source_id == *oid).map(|(k, _)| *k).collect();
            for k in keys {
                after_doc.tiling_effects.swap_remove(&k);
                removed = true;
            }
        }
        if !removed { return; }
        self.history.push(&mut self.project, ProjectEdit::PatchDocument { before: before_doc, after: after_doc });
        self.status_message = "Removed Tiling effect(s)".into();
    }

    pub fn remove_circular_effect(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut removed = false;
        for oid in &objs {
            let keys: Vec<_> = after_doc.circular_effects.iter().filter(|(_, e)| e.source_id == *oid).map(|(k, _)| *k).collect();
            for k in keys {
                after_doc.circular_effects.swap_remove(&k);
                removed = true;
            }
        }
        if !removed { return; }
        self.history.push(&mut self.project, ProjectEdit::PatchDocument { before: before_doc, after: after_doc });
        self.status_message = "Removed CircularClone effect(s)".into();
    }

    /// First two selected nodes as (A, B) if both present.
    pub fn selection_boolean_pair(&self) -> Option<(NodeId, NodeId)> {
        if self.selection.len() < 2 {
            return None;
        }
        Some((self.selection[0], self.selection[1]))
    }

    /// All selected solid-face shapes eligible for boolean (order = selection order).
    pub fn selection_booleanable_shapes(&self) -> Vec<NodeId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| {
                self.project
                    .nodes
                    .get(*id)
                    .is_some_and(is_booleanable_shape)
            })
            .collect()
    }

    /// Classify pair for Path Magic: vector boolean vs image clip.
    /// When 3+ vector shapes are selected, still returns VectorBoolean for the first pair
    /// so the panel opens; multi-ops use [`Self::selection_booleanable_shapes`].
    pub fn selection_boolean_mode(
        &self,
    ) -> Option<BooleanPairMode> {
        let shapes = self.selection_booleanable_shapes();
        if shapes.len() >= 2 {
            return Some(BooleanPairMode::VectorBoolean {
                a: shapes[0],
                b: shapes[1],
            });
        }
        let (a, b) = self.selection_boolean_pair()?;
        let na = self.project.nodes.get(a)?;
        let nb = self.project.nodes.get(b)?;
        let a_shape = is_booleanable_shape(na);
        let b_shape = is_booleanable_shape(nb);
        let a_img = is_raster_image(na);
        let b_img = is_raster_image(nb);
        if a_shape && b_shape {
            return Some(BooleanPairMode::VectorBoolean { a, b });
        }
        if a_img && b_shape {
            return Some(BooleanPairMode::ImageClip {
                source: a,
                mask: b,
            });
        }
        if b_img && a_shape {
            return Some(BooleanPairMode::ImageClip {
                source: b,
                mask: a,
            });
        }
        None
    }

    pub fn selection_has_boolean_effect(&self) -> bool {
        self.selection.iter().any(|&id| {
            self.project.document.boolean_effects.values().any(|e| {
                e.a_id == id
                    || e.b_id == id
                    || e.result_node_id == Some(id)
            })
        })
    }

    pub fn selection_has_clip_mask(&self) -> bool {
        self.selection.iter().any(|&id| {
            self.project
                .document
                .clip_masks
                .values()
                .any(|cm| cm.source_id == id || cm.mask_id == id)
        })
    }

    fn find_boolean_effect_for_selection(&self) -> Option<uuid::Uuid> {
        let sel = &self.selection;
        self.project
            .document
            .boolean_effects
            .iter()
            .find(|(_, e)| {
                sel.contains(&e.a_id)
                    || sel.contains(&e.b_id)
                    || e.result_node_id.map(|r| sel.contains(&r)).unwrap_or(false)
            })
            .map(|(k, _)| *k)
    }

    /// Apply boolean: 2 shapes → live effect; 3+ shapes → one-shot fold (Union/Intersection only).
    pub fn apply_boolean_effect(&mut self) {
        let shapes = self.selection_booleanable_shapes();
        if shapes.len() < 2 {
            self.status_message =
                "Boolean needs two or more solid shapes (path/rect/circle/arc/polygon)".into();
            return;
        }
        if shapes.len() > 2 {
            if !self.ui_boolean_op.supports_multi() {
                self.status_message = format!(
                    "{} needs exactly 2 shapes; use Union or Intersection for {} shapes",
                    self.ui_boolean_op.label(),
                    shapes.len()
                );
                return;
            }
            self.apply_multi_boolean_fold(&shapes);
            return;
        }
        let a = shapes[0];
        let b = shapes[1];
        if self.project.document.boolean_effects.values().any(|e| {
            (e.a_id == a && e.b_id == b) || (e.a_id == b && e.b_id == a)
        }) {
            return;
        }
        let Some(na) = self.project.nodes.get(a).cloned() else { return };
        let Some(nb) = self.project.nodes.get(b).cloned() else { return };
        let Some(bez) = compute_boolean_bez(&na, &nb, self.ui_boolean_op, 0.75) else {
            self.status_message =
                "Boolean failed (could not convert shapes to polygons)".into();
            return;
        };
        let empty = bez.elements().is_empty();
        let mut result = if empty {
            // Placeholder so the effect exists; moves of A/B recompute when they overlap.
            Node::path_from_bez(
                {
                    let mut p = kurbo::BezPath::new();
                    let c = na.bounds().center();
                    p.move_to((c.x, c.y));
                    p.line_to((c.x + 1.0, c.y));
                    p.line_to((c.x + 1.0, c.y + 1.0));
                    p.close_path();
                    p
                },
                format!("{} {} {} (empty)", na.name, self.ui_boolean_op.label(), nb.name),
            )
        } else {
            Node::path_from_bez(
                bez,
                format!("{} {} {}", na.name, self.ui_boolean_op.label(), nb.name),
            )
        };
        result.style = na.style.clone();
        if empty {
            result.style.opacity = 0.0; // invisible until shapes overlap
        }

        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let result_id = after.nodes.insert(result);
        // Put result on active layer
        if let Some(layer) = after.document.layers.get_mut(after.document.active_layer_index) {
            if !layer.nodes.contains(&result_id) {
                layer.nodes.push(result_id);
            }
        }
        let effect = BooleanEffect {
            id: uuid::Uuid::new_v4(),
            a_id: a,
            b_id: b,
            op: self.ui_boolean_op,
            // Keep operands visible when result is empty so they can be moved into overlap.
            hide_operands: !empty,
            result_node_id: Some(result_id),
        };
        after.document.boolean_effects.insert(effect.id, effect);
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        // Select operands when empty (result is invisible); else select result.
        self.selection = if empty {
            vec![a, b]
        } else {
            vec![result_id]
        };
        self.status_message = if empty {
            format!(
                "Boolean {} applied (empty — move A/B so they overlap)",
                self.ui_boolean_op.label()
            )
        } else {
            format!("Boolean {} applied", self.ui_boolean_op.label())
        };
    }

    /// Fold Union/Intersection over N≥3 shapes into one baked result path (no live multi-link).
    fn apply_multi_boolean_fold(&mut self, shapes: &[NodeId]) {
        let op = self.ui_boolean_op;
        if shapes.len() < 3 || !op.supports_multi() {
            return;
        }
        let nodes: Vec<Node> = shapes
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .collect();
        if nodes.len() < 3 {
            return;
        }
        let mut acc = nodes[0].clone();
        // Work on a temporary path node carrying the accumulated bez.
        for other in nodes.iter().skip(1) {
            let Some(bez) = compute_boolean_bez(&acc, other, op, 0.75) else {
                self.status_message = format!(
                    "Boolean {} failed while combining {}",
                    op.label(),
                    other.name
                );
                return;
            };
            if bez.elements().is_empty() {
                self.status_message = format!(
                    "Boolean {} produced empty result (shapes may not overlap)",
                    op.label()
                );
                return;
            }
            let mut next = Node::path_from_bez(bez, acc.name.clone());
            next.style = acc.style.clone();
            acc = next;
        }
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        acc.name = format!("{} ({})", op.label(), names.join(" + "));
        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let result_id = after.nodes.insert(acc);
        if let Some(layer) = after.document.layers.get_mut(after.document.active_layer_index) {
            if !layer.nodes.contains(&result_id) {
                layer.nodes.push(result_id);
            }
        }
        // Hide original operands (same UX as pair live boolean with hide_operands).
        for id in shapes {
            if let Some(n) = after.nodes.get_mut(*id) {
                n.style.opacity = 0.0;
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection = vec![result_id];
        self.status_message = format!(
            "Boolean {} applied to {} shapes (baked result)",
            op.label(),
            shapes.len()
        );
    }

    pub fn reverse_boolean_operands(&mut self) {
        // Live effect reverse
        if let Some(eid) = self.find_boolean_effect_for_selection() {
            let before_doc = snapshot_document(&self.project.document);
            let mut after_doc = before_doc.clone();
            if let Some(e) = after_doc.boolean_effects.get_mut(&eid) {
                std::mem::swap(&mut e.a_id, &mut e.b_id);
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchDocument {
                    before: before_doc,
                    after: after_doc,
                },
            );
            self.refresh_boolean_effects_live();
            self.status_message = "Boolean A ↔ B reversed".into();
            return;
        }
        // Pre-apply: reverse selection order
        if self.selection.len() >= 2 {
            self.selection.swap(0, 1);
            self.status_message = "Operands A ↔ B reversed".into();
        }
    }

    pub fn set_boolean_op_live(&mut self, op: BooleanOpKind) {
        self.ui_boolean_op = op;
        if let Some(eid) = self.find_boolean_effect_for_selection() {
            let before_doc = snapshot_document(&self.project.document);
            let mut after_doc = before_doc.clone();
            if let Some(e) = after_doc.boolean_effects.get_mut(&eid) {
                e.op = op;
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchDocument {
                    before: before_doc,
                    after: after_doc,
                },
            );
            self.refresh_boolean_effects_live();
        }
    }

    /// Recompute result path geometry from current operands + op.
    /// Call after A/B move or op change — not needed mid-drag if A+B+result move together.
    pub fn refresh_boolean_effects_live(&mut self) {
        // Skip while dragging so we don't fight the drag snapshot / wipe the move.
        if !self.tools.select.drag_snapshot.is_empty() {
            return;
        }
        let effects: Vec<_> = self.project.document.boolean_effects.values().cloned().collect();
        for e in effects {
            let Some(na) = self.project.nodes.get(e.a_id).cloned() else { continue };
            let Some(nb) = self.project.nodes.get(e.b_id).cloned() else { continue };
            let Some(bez) = compute_boolean_bez(&na, &nb, e.op, 0.75) else { continue };
            let Some(rid) = e.result_node_id else { continue };
            if let Some(node) = self.project.nodes.get_mut(rid) {
                if let NodeKind::Path { path } = &mut node.kind {
                    if bez.elements().is_empty() {
                        // Keep a tiny handle so the result stays pickable/movable.
                        let c = na.bounds().center();
                        let mut p = kurbo::BezPath::new();
                        p.move_to((c.x, c.y));
                        p.line_to((c.x + 1.0, c.y));
                        p.line_to((c.x + 1.0, c.y + 1.0));
                        p.close_path();
                        *path = PathData::from_bez(&p);
                        node.style.opacity = 0.0;
                    } else {
                        *path = PathData::from_bez(&bez);
                        if node.style.opacity < 0.05 {
                            node.style.opacity = na.style.opacity.max(0.05);
                        }
                        // Once we have a real result, hide operands if that was intended.
                        if let Some(eff) = self
                            .project
                            .document
                            .boolean_effects
                            .values_mut()
                            .find(|eff| eff.result_node_id == Some(rid))
                        {
                            if !eff.hide_operands {
                                eff.hide_operands = true;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Bake: drop live effect, keep result path as normal object; unhide operands.
    pub fn bake_boolean_effect(&mut self) {
        let Some(eid) = self.find_boolean_effect_for_selection() else { return };
        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let Some(effect) = after.document.boolean_effects.swap_remove(&eid) else { return };
        // Result stays in nodes/layers; operands remain (visible again).
        let _ = effect;
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.status_message = "Boolean baked to path".into();
    }

    pub fn remove_boolean_effect(&mut self) {
        let Some(eid) = self.find_boolean_effect_for_selection() else { return };
        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let Some(effect) = after.document.boolean_effects.swap_remove(&eid) else { return };
        if let Some(rid) = effect.result_node_id {
            after.nodes.remove(rid);
            after.anim_timeline.nodes.remove(&rid);
            for layer in &mut after.document.layers {
                layer.nodes.retain(|id| *id != rid);
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.status_message = "Boolean effect removed".into();
    }

    /// Apply clip mask only for raster image + solid-face shape.
    pub fn apply_clip_mask(&mut self) {
        let Some(BooleanPairMode::ImageClip { source, mask }) = self.selection_boolean_mode() else {
            self.status_message =
                "Clip Mask needs a raster image + a solid shape (path/rect/circle/arc/polygon)"
                    .into();
            return;
        };
        if self
            .project
            .document
            .clip_masks
            .values()
            .any(|cm| cm.source_id == source && cm.mask_id == mask)
        {
            return;
        }
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let effect_id = uuid::Uuid::new_v4();
        let effect = ClipMaskEffect {
            id: effect_id,
            source_id: source,
            mask_id: mask,
            hide_mask: true,
        };
        after_doc.clip_masks.insert(effect.id, effect);
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        // Force texture rebuild next paint so live clip is visible/pickable immediately.
        self.clip_mask_signatures.remove(&effect_id);
        self.status_message = "Clip Mask applied (image → solid face)".into();
    }

    pub fn remove_clip_mask(&mut self) {
        let sel = self.selection.clone();
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let keys: Vec<uuid::Uuid> = after_doc
            .clip_masks
            .iter()
            .filter(|(_, cm)| sel.contains(&cm.source_id) || sel.contains(&cm.mask_id))
            .map(|(k, _)| *k)
            .collect();
        if keys.is_empty() {
            return;
        }
        for k in keys {
            after_doc.clip_masks.swap_remove(&k);
            self.image_textures.remove(&k);
            self.clip_mask_signatures.remove(&k);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        self.status_message = "Clip Mask removed".into();
    }

    /// Bake active clip mask to a raster image of **only the clip region** (mask solid face).
    /// Removes the live clip effect afterward; leaves source + mask nodes in place.
    pub fn bake_clip_mask_to_raster(&mut self) {
        let sel = self.selection.clone();
        let Some(cm) = self
            .project
            .document
            .clip_masks
            .values()
            .find(|cm| sel.contains(&cm.source_id) || sel.contains(&cm.mask_id))
            .cloned()
        else {
            self.status_message = "No clip mask on selection".into();
            return;
        };
        let Some(mask_node) = self.project.nodes.get(cm.mask_id).cloned() else {
            return;
        };
        let Some(source_node) = self.project.nodes.get(cm.source_id).cloned() else {
            return;
        };
        let NodeKind::Image {
            x: img_x,
            y: img_y,
            width: img_w,
            height: img_h,
            bytes,
            ..
        } = &source_node.kind
        else {
            self.status_message = "Clip bake needs a raster image source".into();
            return;
        };

        let mask_bounds = mask_node.bounds();
        let w = mask_bounds.width().max(1.0);
        let h = mask_bounds.height().max(1.0);
        let scale = 2.0f64;
        let pixel_w = (w * scale).round().max(1.0) as u32;
        let pixel_h = (h * scale).round().max(1.0) as u32;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let mime = if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
            "image/jpeg"
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
            "image/webp"
        } else {
            "image/png"
        };
        let mask_d = mask_node.bez_path().to_svg();
        let svg_data = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="{} {} {} {}" width="{}" height="{}">
              <defs>
                <clipPath id="clip" clipPathUnits="userSpaceOnUse">
                  <path d="{}" fill="black" stroke="none"/>
                </clipPath>
              </defs>
              <g clip-path="url(#clip)">
                <image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none"
                  href="data:{mime};base64,{b64}" xlink:href="data:{mime};base64,{b64}"/>
              </g>
            </svg>"#,
            mask_bounds.x0,
            mask_bounds.y0,
            w,
            h,
            pixel_w,
            pixel_h,
            mask_d,
            img_x,
            img_y,
            img_w,
            img_h,
        );

        let opt = crate::fonts::usvg_options();
        let Ok(tree) = usvg::Tree::from_str(&svg_data, &opt) else {
            self.status_message = "Clip bake failed (SVG parse)".into();
            return;
        };
        let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(pixel_w, pixel_h) else {
            self.status_message = "Clip bake failed (pixmap)".into();
            return;
        };
        let sx = pixel_w as f32 / w as f32;
        let sy = pixel_h as f32 / h as f32;
        // SVG already has width/height in pixels matching the pixmap — do NOT scale
        // again (from_scale made the result look zoomed vs the live clip).
        let _ = (sx, sy);
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        let rgba = pixmap.take();
        if !rgba.chunks(4).any(|px| px[3] > 8) {
            self.status_message =
                "Clip bake empty — image and mask may not overlap".into();
            return;
        }
        // Encode PNG
        let mut png_bytes = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            let enc = image::codecs::png::PngEncoder::new(&mut cursor);
            use image::ImageEncoder;
            if enc
                .write_image(
                    &rgba,
                    pixel_w,
                    pixel_h,
                    image::ExtendedColorType::Rgba8,
                )
                .is_err()
            {
                self.status_message = "Clip bake PNG encode failed".into();
                return;
            }
        }

        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let name = if source_node.name.trim().is_empty() {
            "Clipped raster".into()
        } else {
            format!("{} clipped", source_node.name)
        };
        // Place baked image at mask bounds in document units (matches clip region).
        let mut node = Node::image(mask_bounds.x0, mask_bounds.y0, w, h, png_bytes);
        node.name = name;
        let new_id = after.nodes.insert(node);
        if let Some(layer) = after.document.layers.get_mut(after.document.active_layer_index) {
            layer.nodes.push(new_id);
        }
        after.document.clip_masks.swap_remove(&cm.id);
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.image_textures.remove(&cm.id);
        self.clip_mask_signatures.remove(&cm.id);
        self.selection = vec![new_id];
        self.status_message = "Baked clip region to raster image".into();
    }

    pub fn swap_clip_mask_source(&mut self) {
        let sel = &self.selection;
        let effect_id = self
            .project
            .document
            .clip_masks
            .iter()
            .find(|(_, cm)| sel.contains(&cm.source_id) || sel.contains(&cm.mask_id))
            .map(|(k, _)| *k);
        let Some(eid) = effect_id else { return };
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        if let Some(cm) = after_doc.clip_masks.get_mut(&eid) {
            // Only swap if both remain valid roles after swap (image as source, shape as mask).
            let s = cm.source_id;
            let m = cm.mask_id;
            let ok = self.project.nodes.get(m).map(is_raster_image).unwrap_or(false)
                && self
                    .project
                    .nodes
                    .get(s)
                    .map(is_booleanable_shape)
                    .unwrap_or(false);
            if ok {
                std::mem::swap(&mut cm.source_id, &mut cm.mask_id);
            } else {
                self.status_message =
                    "Clip Mask swap requires image as source and shape as mask".into();
                return;
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        self.clip_mask_signatures.clear(); // force rebuild
        self.status_message = "Clip Mask swapped".into();
    }

    pub fn bake_tiling(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let mut child_ids = Vec::new();
        for &oid in &objs {
            if let Some(effect) = self.project.document.tiling_effects.values().find(|e| e.source_id == oid).cloned() {
                if let Some(source) = self.project.nodes.get(oid).cloned() {
                    let src_face: &dyn FaceRenderable = &source;
                    let b = source.bounds();
                    let w = b.x1 - b.x0;
                    let h = b.y1 - b.y0;
                    let first_left = b.x0 + effect.offset_x;
                    let first_top = b.y0 + effect.offset_y;
                    for ix in 0..effect.count_x {
                        for iy in 0..effect.count_y {
                            let left = first_left + ix as f64 * effect.gap_x;
                            let top = first_top + iy as f64 * effect.gap_y;
                            let cx = left + w / 2.0;
                            let cy = top + h / 2.0;
                            let rot = (ix as f64 * effect.row_rotation + iy as f64 * effect.col_rotation).to_radians();
                            let sc = 1.0 + (ix as f64 * effect.row_scale + iy as f64 * effect.col_scale);
                            let pl = PathPlacement { x: cx, y: cy, angle_rad: rot, scale: sc as f32, opacity_mul: 1.0 };
                            let mut node = node_at_placement(src_face, &pl);
                            node.name = format!("{} #t{}_{}", source.name, ix, iy);
                            let id = node.id;
                            self.history.push(&mut self.project, ProjectEdit::InsertNode { node });
                            child_ids.push(id);
                        }
                    }
                }
            }
        }
        if !child_ids.is_empty() {
            let group = Node::group(child_ids.clone(), "Tiled group".to_string());
            let gid = group.id;
            self.history.push(&mut self.project, ProjectEdit::InsertNode { node: group });
            self.selection = vec![gid];
            self.status_message = format!("Baked {} tiles", child_ids.len());
        }
    }

    /// Convert placement instance to a stable path node (unique id, geometric bez).
    fn bake_instance_to_path_node(source: &Node, pl: &PathPlacement, name: String) -> Node {
        let mut node = node_at_placement(source as &dyn FaceRenderable, pl);
        let bez = node.bez_path();
        let style = node.style.clone();
        let mut out = Node::path_from_bez(bez, name);
        out.style = style;
        out.id = uuid::Uuid::new_v4();
        out
    }

    /// Collect placed circular copies for one source (path nodes, unique ids).
    fn circular_bake_instances(
        source: &Node,
        effect: &CircularCloneEffect,
    ) -> Vec<Node> {
        let n = effect.copies.max(3);
        (0..n)
            .map(|i| {
                let pl = effect.path_placement(i);
                Self::bake_instance_to_path_node(
                    source,
                    &pl,
                    format!("{} #c{}", source.name, i + 1),
                )
            })
            .collect()
    }

    /// Bake CircularClone → group. Children live **only** under the group (not top-level layer
    /// entries), so selection bounds track the group and deleting the group removes all copies.
    pub fn bake_circular(&mut self) {
        let objs: Vec<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|&id| {
                self.project
                    .document
                    .circular_effects
                    .values()
                    .any(|e| e.source_id == id)
            })
            .collect();
        if objs.is_empty() {
            self.status_message = "Select a CircularClone object to bake".into();
            return;
        }

        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let mut all_child_ids: Vec<NodeId> = Vec::new();
        let mut group_ids: Vec<NodeId> = Vec::new();
        let mut total_copies = 0usize;

        for &oid in &objs {
            let Some(effect) = after
                .document
                .circular_effects
                .values()
                .find(|e| e.source_id == oid)
                .cloned()
            else {
                continue;
            };
            let Some(source) = after.nodes.get(oid).cloned() else {
                continue;
            };
            let instances = Self::circular_bake_instances(&source, &effect);
            if instances.is_empty() {
                continue;
            }
            total_copies += instances.len();
            let mut child_ids = Vec::with_capacity(instances.len());
            for node in instances {
                let id = node.id;
                // Store only — do **not** append to layer (group is the sole layer entry).
                after.nodes.insert(node);
                child_ids.push(id);
                all_child_ids.push(id);
            }
            let group = Node::group(
                child_ids,
                format!("{} circular", source.name),
            );
            let gid = group.id;
            after.nodes.insert(group);
            after.document.append_to_active_layer(gid);
            group_ids.push(gid);

            after.document.circular_effects.swap_remove(&effect.id);
            // Remove hidden source from store + layers + orphaned keyframes.
            after.remove_node_and_animation(oid);
        }

        if group_ids.is_empty() {
            self.status_message = "No CircularClone effect to bake".into();
            return;
        }

        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection = group_ids;
        self.status_message = format!(
            "Baked {total_copies} copies into {} group(s)",
            self.selection.len()
        );
    }

    /// Bake CircularClone → single path (each copy → path, boolean-union all).
    /// Multi-contour result is preserved via verb-faithful path storage.
    pub fn bake_circular_as_path(&mut self) {
        let objs: Vec<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|&id| {
                self.project
                    .document
                    .circular_effects
                    .values()
                    .any(|e| e.source_id == id)
            })
            .collect();
        if objs.is_empty() {
            self.status_message = "Select a CircularClone object to bake".into();
            return;
        }

        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let mut result_ids = Vec::new();

        for &oid in &objs {
            let Some(effect) = after
                .document
                .circular_effects
                .values()
                .find(|e| e.source_id == oid)
                .cloned()
            else {
                continue;
            };
            let Some(source) = after.nodes.get(oid).cloned() else {
                continue;
            };
            let instances = Self::circular_bake_instances(&source, &effect);
            if instances.is_empty() {
                continue;
            }

            // Fold union: path0 ∪ path1 ∪ … (shutter / overlapping chords stay clean multi-contour)
            let mut acc = instances[0].clone();
            for other in instances.iter().skip(1) {
                let Some(bez) =
                    compute_boolean_bez(&acc, other, BooleanOpKind::Union, 0.5)
                else {
                    self.status_message =
                        "Bake as path failed (boolean union could not convert shapes)".into();
                    return;
                };
                if bez.elements().is_empty() {
                    continue;
                }
                let style = acc.style.clone();
                acc = Node::path_from_bez(bez, format!("{} union", source.name));
                acc.style = style;
                acc.id = uuid::Uuid::new_v4();
            }
            acc.name = format!("{} circular path", source.name);
            let rid = acc.id;
            after.nodes.insert(acc);
            after.document.append_to_active_layer(rid);
            result_ids.push(rid);

            after.document.circular_effects.swap_remove(&effect.id);
            after.remove_node_and_animation(oid);
        }

        if result_ids.is_empty() {
            self.status_message = "No CircularClone effect to bake".into();
            return;
        }

        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection = result_ids;
        self.status_message = format!(
            "Baked circular clone(s) as {} path(s)",
            self.selection.len()
        );
    }

    /// Split CircularClone into many independent top-level path objects (no group).
    pub fn split_circular(&mut self) {
        let objs: Vec<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|&id| {
                self.project
                    .document
                    .circular_effects
                    .values()
                    .any(|e| e.source_id == id)
            })
            .collect();
        if objs.is_empty() {
            self.status_message = "Select a CircularClone object to split".into();
            return;
        }

        let before = snapshot_project(&self.project);
        let mut after = before.clone();
        let mut result_ids = Vec::new();
        let mut total = 0usize;

        for &oid in &objs {
            let Some(effect) = after
                .document
                .circular_effects
                .values()
                .find(|e| e.source_id == oid)
                .cloned()
            else {
                continue;
            };
            let Some(source) = after.nodes.get(oid).cloned() else {
                continue;
            };
            let instances = Self::circular_bake_instances(&source, &effect);
            for node in instances {
                let id = node.id;
                after.nodes.insert(node);
                after.document.append_to_active_layer(id);
                result_ids.push(id);
                total += 1;
            }
            after.document.circular_effects.swap_remove(&effect.id);
            after.remove_node_and_animation(oid);
        }

        if result_ids.is_empty() {
            self.status_message = "No CircularClone effect to split".into();
            return;
        }

        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection = result_ids;
        self.status_message = format!("Split into {total} separate objects");
    }

    pub fn close_open_paths_in_selection(&mut self) {
        let ids: Vec<_> = self
            .selection
            .iter()
            .filter(|id| {
                self.project.nodes.get(**id).is_some_and(|n| {
                    matches!(&n.kind, NodeKind::Path { path } if !path.is_closed())
                })
            })
            .copied()
            .collect();
        let count = ids.len();
        for id in ids {
            self.set_path_closed(id, true);
        }
        if count > 0 {
            self.status_message = format!("Closed {count} path(s)");
        }
    }

    pub fn open_closed_paths_in_selection(&mut self) {
        let ids: Vec<_> = self
            .selection
            .iter()
            .filter(|id| {
                self.project.nodes.get(**id).is_some_and(|n| {
                    matches!(&n.kind, NodeKind::Path { path } if path.is_closed())
                })
            })
            .copied()
            .collect();
        let count = ids.len();
        for id in ids {
            self.set_path_closed(id, false);
        }
        if count > 0 {
            self.status_message = format!("Opened {count} path(s)");
        }
    }

    pub fn begin_on_page_text_edit(&mut self, id: NodeId) {
        if self.on_page_text_edit.is_some() {
            self.finish_on_page_text_edit();
        }
        let Some(node) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let NodeKind::Text { style, .. } = &node.kind else {
            return;
        };
        self.ui_text_content = style.content.clone();
        self.ui_text_font_size = style.font_size;
        self.ui_text_width = style.width;
        self.ui_text_font_family = style.font_family.clone();
        self.ui_text_bold = style.bold;
        self.ui_text_italic = style.italic;
        self.on_page_text_before = Some(node);
        self.on_page_text_edit = Some(id);
        self.on_page_text_focus_pending = true;
        self.selection = vec![id];
        self.sync_inspector_from_selection();
        self.begin_text_focus_pan(id);
    }

    fn ease_text_pan(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    fn start_text_pan_anim(&mut self, to: egui::Vec2, duration: f32) {
        let from = self.viewport.pan;
        if (from - to).length_sq() < 0.5 {
            return;
        }
        self.text_pan_anim = Some(TextPanAnim {
            from,
            to,
            elapsed: 0.0,
            duration,
        });
    }

    fn begin_text_focus_pan(&mut self, id: NodeId) {
        let Some(canvas) = self.canvas_screen_rect else {
            return;
        };
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let NodeKind::Text { x, y, style } = &node.kind else {
            return;
        };
        if self.text_pan_restore.is_none() {
            self.text_pan_restore = Some(self.viewport.pan);
        }

        let editor_w = ((crate::document::text_bounds(*x, *y, style).width() as f32)
            * self.viewport.zoom)
            .max(220.0);
        let editor_h = (style.font_size * self.viewport.zoom * 5.0).max(160.0);
        let margin = 72.0;
        let desired = egui::pos2(
            canvas.center().x - editor_w * 0.5,
            canvas.center().y - editor_h * 0.45,
        );
        let min = canvas.min + egui::vec2(margin, margin);
        let max = canvas.max - egui::vec2(editor_w + margin, editor_h + margin);
        let target_screen = egui::pos2(
            desired.x.clamp(min.x, max.x.max(min.x)),
            desired.y.clamp(min.y, max.y.max(min.y)),
        );

        let current_screen = self.viewport.doc_to_screen((*x, *y), self.canvas_origin);
        let to = self.viewport.pan + (target_screen - current_screen);
        self.start_text_pan_anim(to, 0.28);
    }

    fn restore_text_focus_pan(&mut self) {
        if let Some(to) = self.text_pan_restore.take() {
            self.start_text_pan_anim(to, 0.32);
        }
    }

    fn update_text_pan_animation(&mut self, ctx: &Context) {
        let Some(mut anim) = self.text_pan_anim else {
            return;
        };
        let dt = ctx.input(|i| i.stable_dt).clamp(1.0 / 240.0, 1.0 / 30.0);
        anim.elapsed += dt;
        let t = Self::ease_text_pan(anim.elapsed / anim.duration.max(0.001));
        self.viewport.pan = anim.from + (anim.to - anim.from) * t;
        if anim.elapsed >= anim.duration {
            self.viewport.pan = anim.to;
            self.text_pan_anim = None;
        } else {
            self.text_pan_anim = Some(anim);
            ctx.request_repaint();
        }
    }

    pub(crate) fn patch_on_page_text_live(&mut self, id: NodeId) {
        let content = self.ui_text_content.clone();
        let font_size = self.ui_text_font_size;
        let width = self.ui_text_width;
        let bold = self.ui_text_bold;
        let italic = self.ui_text_italic;
        let family = self.ui_text_font_family.clone();
        let Some(node) = self.project.nodes.get_mut(id) else {
            return;
        };
        if let NodeKind::Text { style, .. } = &mut node.kind {
            style.content = content.clone();
            style.font_size = font_size;
            style.width = width;
            style.bold = bold;
            style.italic = italic;
            style.font_family = family;
            node.name = text_display_name(&content);
        }
    }

    pub fn finish_on_page_text_edit(&mut self) {
        let Some(id) = self.on_page_text_edit.take() else {
            self.on_page_text_newly_created = false;
            return;
        };
        self.restore_text_focus_pan();
        #[cfg(target_os = "android")]
        {
            if let Some(android_app) = crate::ANDROID_APP.get() {
                android_app.hide_soft_input(false);
            }
        }
        self.on_page_text_focus_pending = false;
        self.patch_on_page_text_live(id);

        let newly = self.on_page_text_newly_created;
        self.on_page_text_newly_created = false;

        let Some(after) = self.project.nodes.get(id).cloned() else {
            self.on_page_text_before = None;
            return;
        };
        let content_empty = if let NodeKind::Text { style, .. } = &after.kind {
            style.content.trim().is_empty()
        } else {
            true
        };

        if newly {
            // For brand-new text from the Text tool, do not keep empty nodes at all.
            // Discard with zero history footprint; only record Insert if it has content.
            self.on_page_text_before = None;
            if content_empty {
                self.project.nodes.remove(id);
                self.project.document.remove_from_layers(id);
                self.selection.retain(|&s| s != id);
                return;
            }
            // Commit: the node is live; to record without dup layer entry, re-insert via history.
            self.project.nodes.remove(id);
            self.project.document.remove_from_layers(id);
            self.history.push(
                &mut self.project,
                ProjectEdit::InsertNode { node: after },
            );
            self.selection = vec![id];
            self.sync_inspector_from_selection();
            return;
        }

        // Normal edit of a pre-existing text node: Patch history if changed.
        let Some(before) = self.on_page_text_before.take() else {
            return;
        };
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    /// Expand groups so deleting a group also deletes its children (store-only members too).
    fn expand_ids_for_delete(&self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = ids.to_vec();
        while let Some(id) = stack.pop() {
            if out.contains(&id) {
                continue;
            }
            out.push(id);
            if let Some(NodeKind::Group { children }) =
                self.project.nodes.get(id).map(|n| &n.kind)
            {
                for &c in children {
                    stack.push(c);
                }
            }
        }
        out
    }

    /// Drop broken ObjectFromApp links after document object deletes.
    fn prune_node_editor_object_links(&mut self) {
        let living: std::collections::HashSet<uuid::Uuid> =
            self.project.nodes.map.keys().copied().collect();
        for layer in &mut self.project.document.layers {
            if layer.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            if let Some(g) = layer.node_graph.as_mut() {
                g.prune_dead_object_links(&living);
            }
        }
    }

    /// Evaluate Real algebra for every Node Editor layer (frame / time / value / expr / param).
    fn eval_node_editor_graphs(&mut self) {
        let frame = self.anim_current_frame;
        let fps = self.anim_fps.max(1) as f32;
        for layer in &mut self.project.document.layers {
            if layer.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            layer.ensure_node_graph();
            if let Some(g) = layer.node_graph.as_mut() {
                g.eval_reals(frame, fps);
            }
        }
    }

    pub fn delete_nodes(&mut self, ids: &[NodeId]) {
        if ids.is_empty() {
            return;
        }
        let mut layer_deleted = false;
        for id in ids {
            if let Some(pos) = self.project.document.layers.iter().position(|l| l.id == *id) {
                self.delete_layer(pos);
                layer_deleted = true;
            }
        }
        if layer_deleted {
            return;
        }
        if !self.layer_editable() {
            return;
        }
        let expanded = self.expand_ids_for_delete(ids);
        let layer_index = self.project.document.active_layer_index;
        let layer_nodes_before = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut removed = Vec::new();
        let mut removed_anims = Vec::new();
        for id in &expanded {
            if let Some(node) = self.project.nodes.get(*id).cloned() {
                removed.push((*id, node));
            }
            if let Some(anim) = self.project.anim_timeline.nodes.get(id).cloned() {
                removed_anims.push((*id, anim));
            }
        }
        if removed.is_empty() {
            return;
        }
        // P7g: remember NE Output proxy fields cleared with these nodes (undo-safe).
        let gone: std::collections::HashSet<_> = removed.iter().map(|(id, _)| *id).collect();
        let ne_proxy_before: Vec<(usize, Option<uuid::Uuid>)> = self
            .project
            .document
            .layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let pid = l.ne_output_proxy?;
                if gone.contains(&pid) {
                    Some((i, Some(pid)))
                } else {
                    None
                }
            })
            .collect();
        self.history.push(
            &mut self.project,
            ProjectEdit::RemoveNodes {
                removed,
                removed_anims,
                layer_index,
                layer_nodes_before,
                ne_proxy_before,
            },
        );
        self.selection.retain(|id| !expanded.contains(id));
        self.prune_node_editor_object_links();
    }

    pub fn delete_on_page_text_node(&mut self, id: NodeId) {
        self.on_page_text_edit = None;
        self.restore_text_focus_pan();
        #[cfg(target_os = "android")]
        {
            if let Some(android_app) = crate::ANDROID_APP.get() {
                android_app.hide_soft_input(false);
            }
        }
        self.on_page_text_focus_pending = false;
        
        let newly = self.on_page_text_newly_created;
        self.on_page_text_newly_created = false;
        self.on_page_text_before = None;

        if newly {
            self.project.nodes.remove(id);
            self.project.document.remove_from_layers(id);
            self.selection.retain(|&s| s != id);
        } else {
            let layer_index = self.project.document.active_layer_index;
            let layer_nodes_before = self
                .project
                .document
                .active_layer()
                .map(|l| l.nodes.clone())
                .unwrap_or_default();
            if let Some(node) = self.project.nodes.get(id).cloned() {
                let removed_anims = self
                    .project
                    .anim_timeline
                    .nodes
                    .get(&id)
                    .cloned()
                    .map(|a| vec![(id, a)])
                    .unwrap_or_default();
                self.history.push(
                    &mut self.project,
                    ProjectEdit::RemoveNodes {
                        removed: vec![(id, node)],
                        removed_anims,
                        layer_index,
                        layer_nodes_before,
                        ne_proxy_before: Vec::new(),
                    },
                );
            }
            self.selection.retain(|&s| s != id);
        }
    }

    pub fn apply_fill_style_to_active(&mut self, fill: &crate::document::Fill) {
        if self.tools.active == ToolKind::Brush || (self.tools.active == ToolKind::Eyedropper && self.tools.last_active_tool == ToolKind::Brush) {
            match fill {
                crate::document::Fill::None => {}
                crate::document::Fill::Solid(paint) => {
                    self.tools.brush.fill_kind = crate::document::FillKind::Solid;
                    self.tools.brush.fill_stops = vec![
                        crate::document::GradientStop { pos: 0.0, color: *paint },
                        crate::document::GradientStop { pos: 1.0, color: *paint },
                    ];
                }
                crate::document::Fill::LinearGradient {
                    angle_deg,
                    line_x0,
                    line_y0,
                    line_x1,
                    line_y1,
                    stops,
                } => {
                    self.tools.brush.fill_kind = crate::document::FillKind::LinearGradient;
                    self.tools.brush.fill_stops = stops.clone();
                    self.tools.brush.gradient_angle = *angle_deg;
                    self.tools.brush.fill_line_x0 = *line_x0;
                    self.tools.brush.fill_line_y0 = *line_y0;
                    self.tools.brush.fill_line_x1 = *line_x1;
                    self.tools.brush.fill_line_y1 = *line_y1;
                }
                crate::document::Fill::RadialGradient {
                    center_x,
                    center_y,
                    stops,
                } => {
                    self.tools.brush.fill_kind = crate::document::FillKind::RadialGradient;
                    self.tools.brush.fill_stops = stops.clone();
                    self.tools.brush.radial_cx = *center_x;
                    self.tools.brush.radial_cy = *center_y;
                }
            }
            return;
        }

        match fill {
            crate::document::Fill::None => {}
            crate::document::Fill::Solid(paint) => {
                self.ui_fill_kind = crate::document::FillKind::Solid;
                self.ui_fill_stops = vec![
                    crate::document::GradientStop { pos: 0.0, color: *paint },
                    crate::document::GradientStop { pos: 1.0, color: *paint },
                ];
                self.fill_enabled = true;

                self.ui_stroke_kind = crate::document::FillKind::Solid;
                self.ui_stroke_stops = vec![
                    crate::document::GradientStop { pos: 0.0, color: *paint },
                    crate::document::GradientStop { pos: 1.0, color: *paint },
                ];
                self.stroke_enabled = true;
            }
            crate::document::Fill::LinearGradient {
                angle_deg,
                line_x0,
                line_y0,
                line_x1,
                line_y1,
                stops,
            } => {
                self.ui_fill_kind = crate::document::FillKind::LinearGradient;
                self.ui_fill_stops = stops.clone();
                self.ui_gradient_angle = *angle_deg;
                self.ui_fill_line_x0 = *line_x0;
                self.ui_fill_line_y0 = *line_y0;
                self.ui_fill_line_x1 = *line_x1;
                self.ui_fill_line_y1 = *line_y1;
                self.fill_enabled = true;

                self.ui_stroke_kind = crate::document::FillKind::LinearGradient;
                self.ui_stroke_stops = stops.clone();
                self.ui_stroke_angle = *angle_deg;
                self.ui_stroke_line_x0 = *line_x0;
                self.ui_stroke_line_y0 = *line_y0;
                self.ui_stroke_line_x1 = *line_x1;
                self.ui_stroke_line_y1 = *line_y1;
                self.stroke_enabled = true;
            }
            crate::document::Fill::RadialGradient {
                center_x,
                center_y,
                stops,
            } => {
                self.ui_fill_kind = crate::document::FillKind::RadialGradient;
                self.ui_fill_stops = stops.clone();
                self.ui_radial_cx = *center_x;
                self.ui_radial_cy = *center_y;
                self.fill_enabled = true;

                self.ui_stroke_kind = crate::document::FillKind::RadialGradient;
                self.ui_stroke_stops = stops.clone();
                self.ui_stroke_radial_cx = *center_x;
                self.ui_stroke_radial_cy = *center_y;
                self.stroke_enabled = true;
            }
        }
        self.apply_fill_to_selection();
        self.apply_stroke_to_selection();
    }

    /// Sample the pixel color from an Image node at a document position.
    fn sample_image_color(&self, node: &crate::document::Node, doc: (f64, f64)) -> Option<egui::Color32> {
        if let NodeKind::Image { x, y, width, height, .. } = node.kind {
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            let (u, v) = crate::document::image_doc_to_uv(
                x,
                y,
                width,
                height,
                node.transform.rotation_rad,
                doc.0,
                doc.1,
            )?;
            if let Some(color_image) = self.image_pixel_cache.get(&node.id) {
                let iw = color_image.size[0] as u32;
                let ih = color_image.size[1] as u32;
                if iw == 0 || ih == 0 {
                    return None;
                }
                let px = (u * iw as f64).floor() as i64;
                let py = (v * ih as f64).floor() as i64;
                if px >= 0 && py >= 0 && (px as u32) < iw && (py as u32) < ih {
                    let pixel = color_image.pixels[(py as usize) * (iw as usize) + (px as usize)];
                    if pixel.a() == 0 {
                        return None;
                    }
                    return Some(pixel);
                }
            }
        }
        None
    }

    pub fn color_at_doc_pos(&self, doc: (f64, f64)) -> egui::Color32 {
        let slop = 4.0 / self.viewport.zoom as f64;
        let (mut hit, bbox_only) = self.pick_node_at_with_bbox_fallback(doc, slop);
        if hit.is_none() {
            hit = bbox_only;
        }
        if let Some(id) = hit {
            if let Some(node) = self.project.nodes.get(id) {
                // For Image nodes, sample the actual pixel color
                if matches!(node.kind, NodeKind::Image { .. }) {
                    if let Some(color) = self.sample_image_color(node, doc) {
                        return color;
                    }
                    return egui::Color32::WHITE;
                }
                let fill_to_copy = match &node.style.fill {
                    crate::document::Fill::None => {
                        match &node.style.stroke.style {
                            crate::document::Fill::None => None,
                            other => Some(other),
                        }
                    }
                    other => Some(other),
                };
                if let Some(fill) = fill_to_copy {
                    return match fill {
                        crate::document::Fill::Solid(color) => color.to_egui(),
                        crate::document::Fill::LinearGradient { stops, .. }
                        | crate::document::Fill::RadialGradient { stops, .. } => {
                            stops.first().map(|s| s.color.to_egui()).unwrap_or(egui::Color32::WHITE)
                        }
                        crate::document::Fill::None => egui::Color32::WHITE,
                    };
                }
            }
        }
        egui::Color32::WHITE
    }

    pub fn tool_eyedropper_holding(
        &mut self,
        ctx: &egui::Context,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
    ) {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        if pressed {
            self.eyedropper_holding = true;
            self.eyedropper_releasing = false;
            self.eyedropper_t = 0.0;
            self.eyedropper_target_pos = Some(doc);
        }

        let is_released = released || (!down && self.eyedropper_holding);
        if is_released && self.eyedropper_holding {
            self.eyedropper_holding = false;
            self.eyedropper_releasing = true;
        }

        if self.eyedropper_holding {
            self.eyedropper_target_pos = Some(doc);
            if self.eyedropper_t < 1.0 {
                self.eyedropper_t = (self.eyedropper_t + dt / 0.25).min(1.0);
            }
            ctx.request_repaint();
        } else if self.eyedropper_releasing {
            self.eyedropper_t = (self.eyedropper_t - dt / 0.20).max(0.0);
            ctx.request_repaint();
            if self.eyedropper_t <= 0.0 {
                self.eyedropper_releasing = false;
                if let Some(target) = self.eyedropper_target_pos {
                    self.tool_eyedropper(target);
                } else {
                    self.tools.active = ToolKind::Select;
                }
            }
        }
    }

    pub fn tool_eyedropper(&mut self, doc: (f64, f64)) {
        let slop = 4.0 / self.viewport.zoom as f64;
        let (mut hit, bbox_only) = self.pick_node_at_with_bbox_fallback(doc, slop);
        if hit.is_none() {
            hit = bbox_only;
        }

        let mut picked_fill = None;
        let mut node_name = String::new();
        if let Some(id) = hit {
            if let Some(node) = self.project.nodes.get(id) {
                node_name = node.name.clone();
                // For Image nodes, sample pixel color directly
                if matches!(node.kind, NodeKind::Image { .. }) {
                    if let Some(color) = self.sample_image_color(node, doc) {
                        let paint = crate::document::Paint {
                            rgba: [
                                color.r() as f32 / 255.0,
                                color.g() as f32 / 255.0,
                                color.b() as f32 / 255.0,
                                color.a() as f32 / 255.0,
                            ],
                        };
                        picked_fill = Some(crate::document::Fill::Solid(paint));
                    }
                } else {
                    let fill_to_copy = match &node.style.fill {
                        crate::document::Fill::None => {
                            match &node.style.stroke.style {
                                crate::document::Fill::None => None,
                                other => Some(other),
                            }
                        }
                        other => Some(other),
                    };
                    if let Some(fill) = fill_to_copy {
                        picked_fill = Some(fill.clone());
                    }
                }
            }
        }
        
        if let Some(fill) = picked_fill {
            self.apply_fill_style_to_active(&fill);
            self.status_message = format!("Picked color from '{}'", node_name);
        }
        self.tools.active = self.tools.last_active_tool;
    }

    pub fn set_text_style(&mut self, id: NodeId, style: TextStyle, x: f64, y: f64) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Text {
            x: tx,
            y: ty,
            style: ts,
        } = &mut after.kind
        {
            *tx = x;
            *ty = y;
            after.name = text_display_name(&style.content);
            *ts = style;
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    /// Path node eligible for weight-flow sculpt (single Path selection).
    pub fn weight_flow_target_path(&self) -> Option<NodeId> {
        if self.selection.len() != 1 {
            return None;
        }
        let id = self.selection[0];
        let node = self.project.nodes.get(id)?;
        matches!(node.kind, NodeKind::Path { .. }).then_some(id)
    }

    /// True while the user is live-editing transform or path geometry (keyframing / skip apply).
    fn is_live_geometry_editing(&self) -> bool {
        self.tools.select.drag_mode.is_some()
            || self.tools.select.node_drag_active
            || self.tools.select.node_edit_target.is_some()
            || self.tools.select.mid_curve_drag.is_some()
            || self.tools.weight_flow.stroke.is_some()
    }

    /// When geom tracks already exist, keep them in sync with the live path (like pos/rot).
    pub fn sync_anim_geom_from_node(&mut self, id: NodeId) {
        let geom = self.get_node_geom_floats(id);
        if geom.is_empty() {
            return;
        }
        let frame = self.anim_current_frame;
        let Some(entry) = self.project.anim_timeline.nodes.get_mut(&id) else {
            return;
        };
        if entry.geom_tracks.is_empty()
            || entry.geom_tracks.iter().all(|t| t.keyframes.is_empty())
        {
            return;
        }
        while entry.geom_tracks.len() < geom.len() {
            entry.geom_tracks.push(KeyframeTrack::default());
        }
        for (i, &v) in geom.iter().enumerate() {
            if !entry.geom_tracks[i].keyframes.is_empty() {
                Self::write_anim_keyframe_at_edit(&mut entry.geom_tracks[i], frame, v);
            }
        }
    }

    /// Insert geom_* keyframes for the node at the current frame (REC mode / explicit capture).
    /// Seeds frame 0 from the previous baseline when recording a later frame for the first time.
    pub fn record_geom_keyframes_for_node(&mut self, id: NodeId) {
        let geom = self.get_node_geom_floats(id);
        if geom.is_empty() {
            return;
        }
        let frame = self.anim_current_frame;
        let baseline = self
            .anim_last_applied_states
            .get(&id)
            .map(|s| s.geom_floats.clone())
            .unwrap_or_else(|| geom.clone());

        let before_timeline = self.project.anim_timeline.clone();
        let entry = self.project.anim_timeline.nodes.entry(id).or_default();
        while entry.geom_tracks.len() < geom.len() {
            entry.geom_tracks.push(KeyframeTrack::default());
        }
        for i in 0..geom.len() {
            let base_v = if i < baseline.len() {
                baseline[i]
            } else {
                geom[i]
            };
            if entry.geom_tracks[i].keyframes.is_empty() && frame > 0 {
                entry.geom_tracks[i].insert(0, base_v);
            }
            entry.geom_tracks[i].insert(frame, geom[i]);
        }
        let after_timeline = self.project.anim_timeline.clone();
        if before_timeline != after_timeline {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchTimeline {
                    before: before_timeline,
                    after: after_timeline,
                },
            );
        }
        // Update baseline so the next scrub doesn't re-trigger spuriously.
        if let Some(node) = self.project.nodes.get(id) {
            self.anim_last_applied_states.insert(
                id,
                AnimAppliedState {
                    pos: node.get_pos(),
                    rotation: node.get_rotation(),
                    opacity: node.get_opacity(),
                    color: node.get_color(),
                    stroke_width: node.get_stroke_width(),
                    stroke_color: node.get_stroke_color(),
                    geom_floats: geom,
                    fill: node.style.fill.clone(),
                },
            );
        }
    }

    /// Returns true if the event was consumed (caller should not run Select/Node).
    fn tool_weight_flow(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        dt: f32,
    ) -> bool {
        use crate::path_physics::PathPhysicsSim;
        use crate::tools::{WeightFlowStroke, WeightFlowMode};
        use glam::Vec2;

        self.tools.weight_flow.cursor_doc = Some(doc);

        // Only Select / Node tools; otherwise leave weight flow toggle on but don't steal LMB.
        if !matches!(
            self.tools.active,
            crate::tools::ToolKind::Select | crate::tools::ToolKind::Node
        ) {
            if released || !down {
                self.tools.weight_flow.cancel_stroke();
            }
            return false;
        }

        let Some(id) = self.weight_flow_target_path() else {
            // No path selected: do not steal clicks — allow Select/marquee to pick objects.
            if self.tools.weight_flow.stroke.is_some() {
                self.cancel_weight_flow_stroke(true);
            }
            return false;
        };

        let brush = Vec2::new(doc.0 as f32, doc.1 as f32);
        let cfg = self.tools.weight_flow.config.clone();

        if pressed {
            let Some(node) = self.project.nodes.get(id).cloned() else {
                return true;
            };
            let NodeKind::Path { path } = &node.kind else {
                return true;
            };
            let anchors = path.anchor_positions();
            if anchors.len() < 2 {
                self.status_message = "Weight flow needs a path with at least 2 points".into();
                return true;
            }
            let closed = path.is_closed() && cfg.preserve_closed;
            let sim = PathPhysicsSim::from_anchors(
                &anchors,
                closed,
                cfg.point_mass,
                cfg.lock_endpoints && !path.is_closed(),
            );
            self.tools.weight_flow.stroke = Some(WeightFlowStroke {
                node_id: id,
                before: node,
                sim,
                brush_prev: Some(brush),
                brush_vel: Vec2::ZERO,
            });
            return true;
        }

        if down {
            let Some(stroke) = self.tools.weight_flow.stroke.as_mut() else {
                // No active stroke (e.g. press started outside canvas) — let Select handle it.
                return false;
            };
            if stroke.node_id != id {
                return false;
            }
            // Velocity from brush motion
            let vel = if let Some(prev) = stroke.brush_prev {
                (brush - prev) / dt.max(1e-4)
            } else {
                Vec2::ZERO
            };
            stroke.brush_vel = stroke.brush_vel.lerp(vel, 0.45);
            stroke.brush_prev = Some(brush);

            let mode = cfg.mode;
            let substeps = if matches!(mode, WeightFlowMode::Drag) {
                2
            } else {
                3
            };
            stroke
                .sim
                .step(brush, stroke.brush_vel, mode, &cfg, dt, substeps);

            let new_anchors = stroke.sim.to_anchors();
            if let Some(node) = self.project.nodes.get_mut(id) {
                if let NodeKind::Path { path } = &mut node.kind {
                    path.replace_anchors(&new_anchors);
                }
            }
            return true;
        }

        if released {
            if let Some(stroke) = self.tools.weight_flow.stroke.take() {
                let id = stroke.node_id;
                if let Some(after) = self.project.nodes.get(id).cloned() {
                    if after != stroke.before {
                        self.history.push(
                            &mut self.project,
                            ProjectEdit::PatchNode {
                                id,
                                before: stroke.before,
                                after,
                            },
                        );
                        self.status_message = "Weight flow stroke applied".into();
                    }
                }
                // Record path geom keyframes if REC is on (stroke already ended; capture once).
                if self.anim_keyframing_mode && !self.anim_is_playing {
                    self.record_geom_keyframes_for_node(id);
                } else {
                    self.sync_anim_geom_from_node(id);
                }
            }
            return true;
        }

        // Hovering with brush enabled + path selected: consume nothing extra unless stroke active
        self.tools.weight_flow.stroke.is_some()
    }

    fn tool_select(
        &mut self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
        shift: bool,
        ghost_pick: bool,
        pressed: bool,
        down: bool,
        released: bool,
        double_clicked: bool,
    ) {
        if double_clicked {
            let hit = self.pick_node_at_opts(doc, 4.0 / self.viewport.zoom as f64, ghost_pick);
            if let Some(id) = hit {
                self.tools.select.drag_mode = None;
                self.tools.select.marquee = None;
                self.tools.select.drag_snapshot.clear();
                if let Some(node) = self.project.nodes.get(id) {
                    if matches!(node.kind, NodeKind::Text { .. }) {
                        self.on_page_text_newly_created = false;
                        self.begin_on_page_text_edit(id);
                        return;
                    } else if !matches!(node.kind, NodeKind::Group { .. }) {
                        self.selection = vec![id];
                        self.tools.active = ToolKind::Node;
                        ui::promote_action_tab(self, ui::ActionTab::Geometry);
                        self.sync_inspector_from_selection();
                        return;
                    }
                }
            }
        }

        if pressed {
            // Resize handles for selected Video Layer
            if self.selection.len() == 1 {
                if let Some(id) = self.selection.first().copied() {
                    let mut layer_doc_rect = None;
                    let mut layer_found = None;
                    for layer in &self.project.document.layers {
                        if layer.id == id {
                            layer_found = Some(layer);
                            break;
                        }
                        if layer.kind == crate::document::LayerKind::AV {
                            let mut l_clips = layer.clone();
                            l_clips.ensure_av_clips();
                            if l_clips.av_clips.iter().any(|c| c.id == id) {
                                layer_found = Some(layer);
                                break;
                            }
                        }
                    }
                    if let Some(l) = layer_found {
                        if l.kind == crate::document::LayerKind::AV {
                            let mut dx = l.x as f64;
                            let mut dy = l.y as f64;
                            if let Some(track) = self.project.anim_timeline.nodes.get(&l.id) {
                                if let Some(x) = track.pos_x.interpolate(self.anim_current_frame) {
                                    dx = x;
                                }
                                if let Some(y) = track.pos_y.interpolate(self.anim_current_frame) {
                                    dy = y;
                                }
                            }
                            let t_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
                            let mut l_clips = l.clone();
                            l_clips.ensure_av_clips();
                            let primary_id = l
                                .video_clip_at_time(t_sec)
                                .map(|(cid, _, _, _, _)| cid)
                                .or_else(|| {
                                    l_clips
                                        .av_clips
                                        .iter()
                                        .find(|c| !c.is_audio_only())
                                        .map(|c| c.id)
                                })
                                .unwrap_or(l.id);

                            let aspect = self
                                .video_layers
                                .get(&primary_id)
                                .or_else(|| self.video_layers.get(&l.id))
                                .and_then(|s| s.texture.as_ref())
                                .map(|tex| {
                                    let tex_w = tex.size()[0] as f32;
                                    let tex_h = tex.size()[1] as f32;
                                    if tex_h > 0.0 {
                                        (tex_w / tex_h) as f64
                                    } else {
                                        1.0
                                    }
                                })
                                .or_else(|| {
                                    self.video_frame_cache.as_ref()
                                        .filter(|c| {
                                            c.layer_id == primary_id || c.layer_id == l.id
                                        })
                                        .map(|c| {
                                            let tex_w = c.texture.size()[0] as f32;
                                            let tex_h = c.texture.size()[1] as f32;
                                            if tex_h > 0.0 {
                                                (tex_w / tex_h) as f64
                                            } else {
                                                1.0
                                            }
                                        })
                                })
                                .unwrap_or(1.0);
                            let mut w = l.width as f64;
                            let mut h = l.height as f64;
                            if l.aspect_ratio_locked {
                                if w / h > aspect {
                                    w = h * aspect;
                                } else {
                                    h = w / aspect;
                                }
                            }
                            layer_doc_rect = Some(kurbo::Rect::new(dx, dy, dx + w, dy + h));
                        }
                    }
                    if let Some(r) = layer_doc_rect {
                        let tl = self.viewport.doc_to_screen((r.x0, r.y0), origin);
                        let br = self.viewport.doc_to_screen((r.x1, r.y1), origin);
                        let sr = egui::Rect::from_min_max(tl, br);
                        if let Some(handle) = render::hit_resize_handle(sr, screen, self.viewport.zoom) {
                            if self.tools.select.select_rotation_mode {
                                if matches!(handle, tools::ResizeHandle::Nw | tools::ResizeHandle::Ne | tools::ResizeHandle::Se | tools::ResizeHandle::Sw) {
                                    self.tools.select.drag_mode = Some(SelectDrag::Rotate);
                                    let cx = (r.x0 + r.x1) * 0.5;
                                    let cy = (r.y0 + r.y1) * 0.5;
                                    self.tools.select.rotate_center = Some((cx, cy));
                                    self.tools.select.rotate_start_angle = (doc.1 - cy).atan2(doc.0 - cx);
                                    let mut layer_pos = None;
                                    for (pos, l) in self.project.document.layers.iter().enumerate() {
                                        if l.id == id || (l.kind == crate::document::LayerKind::AV && {
                                            let mut lc = l.clone();
                                            lc.ensure_av_clips();
                                            lc.av_clips.iter().any(|c| c.id == id)
                                        }) {
                                            layer_pos = Some(pos);
                                            break;
                                        }
                                    }
                                    if let Some(pos) = layer_pos {
                                        self.tools.select.rotate_start_layer_rotation = self.project.document.layers[pos].rotation;
                                    }
                                    self.tools.select.last_doc = doc;
                                    self.sync_inspector_from_selection();
                                    return;
                                }
                            } else {
                                self.tools.select.drag_mode = Some(SelectDrag::Resize(handle));
                                self.tools.select.resize_anchor = r;
                                self.tools.select.last_doc = doc;
                                self.sync_inspector_from_selection();
                                return;
                            }
                        }
                    }
                }
            }

            // Resize / Rotate handles take priority over move (must run on pointer-down, not click-up).
            if self.selection.len() == 1 {
                if let Some(id) = self.selection.first().copied() {
                    if !self.node_has_tiling_or_circular(id) {
                        if let Some(node) = self.project.nodes.get(id) {
                            let sr = render::selection_screen_rect(
                                node,
                                &self.project.nodes,
                                &self.viewport,
                                origin,
                            );
                            if let Some(handle) =
                                render::hit_resize_handle(sr, screen, self.viewport.zoom)
                            {
                                if self.tools.select.select_rotation_mode {
                                    if matches!(handle, tools::ResizeHandle::Nw | tools::ResizeHandle::Ne | tools::ResizeHandle::Se | tools::ResizeHandle::Sw) {
                                        self.convert_rect_to_path(id);
                                        if let Some(node) = self.project.nodes.get(id) {
                                            self.tools.select.drag_mode = Some(SelectDrag::Rotate);
                                            let b = node.bounds_with_store(&self.project.nodes);
                                            let cx = (b.x0 + b.x1) * 0.5;
                                            let cy = (b.y0 + b.y1) * 0.5;
                                            self.tools.select.rotate_center = Some((cx, cy));
                                            self.tools.select.rotate_start_angle = (doc.1 - cy).atan2(doc.0 - cx);
                                            self.tools.select.drag_snapshot = vec![(id, node.clone())];
                                            self.tools.select.last_doc = doc;
                                            self.sync_inspector_from_selection();
                                            return;
                                        }
                                    }
                                } else {
                                    self.tools.select.drag_mode = Some(SelectDrag::Resize(handle));
                                    // Groups: use child-union bounds, not ZERO.
                                    self.tools.select.resize_anchor =
                                        node.bounds_with_store(&self.project.nodes);
                                    // Don't drag-snapshot a group shell alone — expand children.
                                    if let NodeKind::Group { children } = &node.kind {
                                        let mut snap = Vec::new();
                                        for &cid in children {
                                            if let Some(c) = self.project.nodes.get(cid) {
                                                snap.push((cid, c.clone()));
                                            }
                                        }
                                        self.tools.select.drag_snapshot = snap;
                                    } else {
                                        self.tools.select.drag_snapshot =
                                            vec![(id, node.clone())];
                                    }
                                    self.tools.select.last_doc = doc;
                                    self.sync_inspector_from_selection();
                                    return;
                                }
                            }
                        }
                    }
                }
            }

            // Gizmo for Tiling / CircularClone (edit the 3 points / angle)
            if self.selection.len() == 1 {
                if let Some(id) = self.selection.first().copied() {
                    let slop = 10.0 / (self.viewport.zoom as f64).max(0.1);
                    if let Some(pts) = self.get_tiling_gizmo_points(id) {
                        for (i, &(px, py)) in pts.iter().enumerate() {
                            if i == 0 { continue; } // Skip offset handle
                            if (px - doc.0).hypot(py - doc.1) < slop {
                                self.tools.select.effect_drag_doc_before =
                                    Some(self.project.document.clone());
                                self.tools.select.drag_mode = Some(SelectDrag::TilingGizmo(i));
                                self.tools.select.last_doc = doc;
                                return;
                            }
                        }
                    }
                    // Screen-space hit so yellow handles stay grabbable at any zoom.
                    if let Some(handle) = self.hit_circular_gizmo(id, screen, origin) {
                        self.tools.select.effect_drag_doc_before =
                            Some(self.project.document.clone());
                        self.tools.select.drag_mode = Some(SelectDrag::CircularGizmo(handle));
                        self.tools.select.last_doc = doc;
                        self.tools.select.drag_start_doc = Some(doc);
                        return;
                    }
                }
            }

            // Clicking a path edge selects both endpoints (switches to node edit).
            if let Some((id, from, to, _, _)) = self.hit_path_segment(screen, origin, doc) {
                self.tools.select.drag_mode = None;
                self.tools.select.marquee = None;
                self.tools.select.drag_snapshot.clear();
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                } else if !shift {
                    self.selection = vec![id];
                }
                self.tools.select.set_path_segment(id, from, to);
                self.tools.active = ToolKind::Node;
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                self.sync_inspector_from_selection();
                return;
            }

            // Hit check visible Video Layers first (only while playhead is inside a clip).
            // Shading is full-page / stack-order only — never pick or drag on canvas.
            let mut hit_layer_id = None;
            let mut hit_layer_rect = None;
            let t_sec = self.anim_current_frame as f32 / self.anim_fps as f32;
            for layer in self.project.document.layers.iter().rev() {
                if layer.visible
                    && !layer.locked
                    && layer.kind == crate::document::LayerKind::AV
                    && layer.has_canvas_video()
                    && layer.shows_video_at(t_sec)
                {
                    let mut dx = layer.x as f64;
                    let mut dy = layer.y as f64;
                    let mut rot = layer.rotation as f64;
                    if let Some(track) = self.project.anim_timeline.nodes.get(&layer.id) {
                        if let Some(x) = track.pos_x.interpolate(self.anim_current_frame) {
                            dx = x;
                        }
                        if let Some(y) = track.pos_y.interpolate(self.anim_current_frame) {
                            dy = y;
                        }
                        if let Some(r) = track.rotation.interpolate(self.anim_current_frame) {
                            rot = r;
                        }
                    }
                    let aspect = self
                        .video_layers
                        .get(&layer.id)
                        .and_then(|s| s.texture.as_ref())
                        .map(|tex| {
                            let tex_w = tex.size()[0] as f32;
                            let tex_h = tex.size()[1] as f32;
                            if tex_h > 0.0 {
                                (tex_w / tex_h) as f64
                            } else {
                                1.0
                            }
                        })
                        .or_else(|| {
                            self.video_frame_cache
                                .as_ref()
                                .filter(|c| c.layer_id == layer.id)
                                .map(|c| {
                                    let tex_w = c.texture.size()[0] as f32;
                                    let tex_h = c.texture.size()[1] as f32;
                                    if tex_h > 0.0 {
                                        (tex_w / tex_h) as f64
                                    } else {
                                        1.0
                                    }
                                })
                        })
                        .unwrap_or(1.0);
                    let mut w = layer.width as f64;
                    let mut h = layer.height as f64;
                    if layer.aspect_ratio_locked {
                        if w / h > aspect {
                            w = h * aspect;
                        } else {
                            h = w / aspect;
                        }
                    }
                    let cx = dx + w / 2.0;
                    let cy = dy + h / 2.0;
                    let px = doc.0 - cx;
                    let py = doc.1 - cy;
                    let rot_rad = (rot as f32).to_radians();
                    let cos = (-rot_rad).cos() as f64;
                    let sin = (-rot_rad).sin() as f64;
                    let local_x = px * cos - py * sin;
                    let local_y = px * sin + py * cos;
                    if local_x >= -w / 2.0
                        && local_x <= w / 2.0
                        && local_y >= -h / 2.0
                        && local_y <= h / 2.0
                    {
                        hit_layer_id = Some(layer.id);
                        hit_layer_rect = Some(kurbo::Rect::new(dx, dy, dx + w, dy + h));
                        break;
                    }
                }
            }
            if let Some(id) = hit_layer_id {
                self.tools.select.marquee = None;
                self.tools.select.clear_path_point_selection();
                if shift {
                    if self.selection.contains(&id) {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection.push(id);
                    }
                } else if !self.selection.contains(&id) {
                    self.selection = vec![id];
                    self.tools.select.select_rotation_mode = false;
                }
                if !self.selection.is_empty() {
                    self.tools.select.drag_mode = Some(SelectDrag::Move);
                    self.tools.select.drag_start_doc = Some(doc); // raw pointer (unsnapped)
                    self.tools.select.move_drag_engaged = false;
                    self.tools.select.resize_anchor = hit_layer_rect.unwrap();
                }
                self.tools.select.last_doc = doc;
                self.sync_inspector_from_selection();
                return;
            }

            let slop = 4.0 / self.viewport.zoom as f64;
            let hits = self.pick_all_nodes_at(doc, slop, ghost_pick);
            let hit = hits.first().copied();
            if let Some(edit_id) = self.on_page_text_edit {
                let keep_editing = hit == Some(edit_id);
                if !keep_editing {
                    self.finish_on_page_text_edit();
                }
            }

            // Sticky selection: with an active selection, ignore clicks on other objects
            // until Esc / empty space deselects (shift still allows multi-select).
            if self.selection_sticky
                && !self.selection.is_empty()
                && !shift
                && !ghost_pick
            {
                if let Some(id) = hit {
                    if !self.selection.contains(&id) {
                        // Clicked something else — keep current selection (allow drag of current).
                        // If pointer is on current selection, proceed to move.
                        // If not on selection at all, block switch.
                        let on_current = self.selection.iter().any(|&sid| {
                            self.project.nodes.get(sid).is_some_and(|n| {
                                self.hit_test_node_for_pick(sid, n, doc, slop)
                            })
                        });
                        if !on_current {
                            self.tools.select.last_doc = doc;
                            self.sync_inspector_from_selection();
                            return;
                        }
                    }
                }
            }

            // Multi-hit at same place: show object picker instead of guessing topmost.
            if !ghost_pick
                && !shift
                && hits.len() > 1
                && (self.selection.is_empty() || !self.selection_sticky)
            {
                // If any hit is already selected, keep it (sticky / re-click).
                if !hits.iter().any(|id| self.selection.contains(id)) {
                    self.hit_pick_menu = Some((screen, hits));
                    self.tools.select.drag_mode = None;
                    self.tools.select.marquee = None;
                    self.tools.select.last_doc = doc;
                    return;
                }
            }
            self.hit_pick_menu = None;

            if let Some(id) = hit {
                self.tools.select.marquee = None;
                self.tools.select.clear_path_point_selection();
                let already_selected = self.selection.contains(&id);
                self.tools.select.clicked_already_selected = already_selected;
                // Ghost pick (Ctrl+Shift): always select only that ghost for independent edit.
                if ghost_pick {
                    self.selection = vec![id];
                    self.tools.select.select_rotation_mode = false;
                    self.selection_sticky = true;
                } else if let Some((source, mask)) = self.clip_pair_for(id) {
                    // Clicking the visible clipped composite selects image + mask as a unit.
                    self.selection = vec![source, mask];
                    self.tools.select.select_rotation_mode = false;
                    self.selection_sticky = true;
                } else if shift {
                    if self.selection.contains(&id) {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection.push(id);
                    }
                    self.selection_sticky = !self.selection.is_empty();
                } else if !self.selection.contains(&id) {
                    self.selection = vec![id];
                    self.tools.select.select_rotation_mode = false;
                    self.selection_sticky = true;
                }
                if !self.selection.is_empty() {
                    self.tools.select.drag_mode = Some(SelectDrag::Move);
                    self.tools.select.drag_start_doc = Some(doc); // raw — threshold must not mix with grid snap
                    self.tools.select.move_drag_engaged = false;
                    let selection = self.selection.clone();
                    self.setup_bulk_drag_if_needed(&selection);
                    if self.tools.select.bulk_drag.is_none() {
                        let mut nodes_to_snapshot = Vec::new();
                        // Ghost edit: do not expand to sibling operands.
                        // Clip unit (source+mask both selected): snapshot both without re-expanding.
                        let drag_ids = if ghost_pick {
                            selection.clone()
                        } else if selection.len() == 2
                            && self
                                .clip_pair_for(selection[0])
                                .map(|(s, m)| {
                                    (selection[0] == s && selection[1] == m)
                                        || (selection[0] == m && selection[1] == s)
                                })
                                .unwrap_or(false)
                        {
                            selection.clone()
                        } else {
                            self.expand_drag_ids_for_path_effects(&selection)
                        };
                        for &sid in &drag_ids {
                            if let Some(node) = self.project.nodes.get(sid) {
                                if let NodeKind::Group { children } = &node.kind {
                                    // Parent group (world origin/rotation) + local children.
                                    nodes_to_snapshot.push((sid, node.clone()));
                                    for &cid in children {
                                        if let Some(child) = self.project.nodes.get(cid) {
                                            nodes_to_snapshot.push((cid, child.clone()));
                                        }
                                    }
                                } else {
                                    nodes_to_snapshot.push((sid, node.clone()));
                                }
                            }
                        }
                        self.tools.select.drag_snapshot = nodes_to_snapshot;
                        // Snapshot circular ring so Move can translate it rigidly with the object.
                        self.tools.select.circular_ring_drag_start.clear();
                        for &sid in &drag_ids {
                            if let Some(e) = self
                                .project
                                .document
                                .circular_effects
                                .values()
                                .find(|e| e.source_id == sid)
                            {
                                self.tools.select.circular_ring_drag_start.push((
                                    sid,
                                    e.base_x,
                                    e.base_y,
                                    e.origin_x,
                                    e.origin_y,
                                ));
                            }
                        }
                    }
                }
            } else {
                // Empty space: clear selection and sticky lock.
                self.tools.select.drag_mode = None;
                self.tools.select.clear_path_point_selection();
                self.hit_pick_menu = None;
                if !shift {
                    self.selection.clear();
                    self.tools.select.select_rotation_mode = false;
                    self.selection_sticky = false;
                }
                self.tools.select.marquee = Some(MarqueeSelect {
                    origin_doc: doc,
                    current_doc: doc,
                    shift,
                });
            }
            self.tools.select.last_doc = doc;
            self.sync_inspector_from_selection();
        } else if down {
            // Keep raw pointer for Move threshold (grid snap of `doc` used to cause instant jumps).
            let raw_doc = doc;
            // Grid-snap for resize/marquee; CircularGizmo uses raw + snap_gizmo_point.
            let doc = if matches!(
                self.tools.select.drag_mode,
                Some(SelectDrag::CircularGizmo(_)) | Some(SelectDrag::Move)
            ) {
                doc
            } else {
                self.viewport.snap(doc)
            };
            if let Some(marquee) = self.tools.select.marquee.as_mut() {
                marquee.current_doc = doc;
            } else if let Some(mode) = self.tools.select.drag_mode {
                match mode {
                    SelectDrag::Move => {
                        // Always measure click-vs-drag in raw pointer space.
                        let drag_start = self
                            .tools
                            .select
                            .drag_start_doc
                            .unwrap_or(self.tools.select.last_doc);
                        let total_dx = raw_doc.0 - drag_start.0;
                        let total_dy = raw_doc.1 - drag_start.1;
                        let screen_dist =
                            (total_dx.hypot(total_dy) * self.viewport.zoom as f64).abs();

                        if screen_dist > tools::SELECT_MOVE_THRESHOLD_PX {
                            self.tools.select.move_drag_engaged = true;
                            let selection_ids = self.selection.clone();
                            let (snapped_dx, snapped_dy) = self.apply_snapping((total_dx, total_dy), &selection_ids);

                            for &sid in &selection_ids {
                                let mut layer_pos = None;
                                for (pos, l) in self.project.document.layers.iter().enumerate() {
                                    // Shading is stack-order only — never translate on canvas.
                                    if l.kind == crate::document::LayerKind::Shading {
                                        continue;
                                    }
                                    if l.id == sid || (l.kind == crate::document::LayerKind::AV && {
                                        let mut lc = l.clone();
                                        lc.ensure_av_clips();
                                        lc.av_clips.iter().any(|c| c.id == sid)
                                    }) {
                                        layer_pos = Some(pos);
                                        break;
                                    }
                                }
                                if let Some(pos) = layer_pos {
                                    let layer = &mut self.project.document.layers[pos];
                                    layer.x = (self.tools.select.resize_anchor.x0 + snapped_dx) as f32;
                                    layer.y = (self.tools.select.resize_anchor.y0 + snapped_dy) as f32;
                                }
                            }

                            if self.tools.select.bulk_drag.is_some() {
                                self.apply_bulk_move_preview(snapped_dx, snapped_dy);
                            } else {
                                // Group children are local — only move the group origin.
                                let group_child_ids: std::collections::HashSet<NodeId> = self
                                    .tools
                                    .select
                                    .drag_snapshot
                                    .iter()
                                    .filter_map(|(id, n)| {
                                        if matches!(n.kind, NodeKind::Group { .. }) {
                                            None
                                        } else if self.selection.iter().any(|sid| {
                                            self.project.nodes.get(*sid).is_some_and(|g| {
                                                matches!(&g.kind, NodeKind::Group { children } if children.contains(id))
                                            })
                                        }) {
                                            Some(*id)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                for &(id, ref orig_node) in &self.tools.select.drag_snapshot {
                                    if group_child_ids.contains(&id) {
                                        continue;
                                    }
                                    if let Some(node) = self.project.nodes.get_mut(id) {
                                        *node = orig_node.clone();
                                        if matches!(node.kind, NodeKind::Group { .. }) {
                                            node.transform.translation[0] += snapped_dx;
                                            node.transform.translation[1] += snapped_dy;
                                        } else {
                                            node.translate(snapped_dx, snapped_dy);
                                        }
                                    }
                                }
                                // Circular ring rides with the source (rigid) so bbox size stays stable.
                                let ring_starts = self.tools.select.circular_ring_drag_start.clone();
                                for &(sid, bx, by, ox, oy) in &ring_starts {
                                    if let Some((_, e)) = self
                                        .project
                                        .document
                                        .circular_effects
                                        .iter_mut()
                                        .find(|(_, e)| e.source_id == sid)
                                    {
                                        e.base_x = bx + snapped_dx;
                                        e.base_y = by + snapped_dy;
                                        e.origin_x = ox + snapped_dx;
                                        e.origin_y = oy + snapped_dy;
                                        e.radius = (e.base_x - e.origin_x)
                                            .hypot(e.base_y - e.origin_y)
                                            .max(1.0);
                                    }
                                }
                                // Lively update attached flowchart connectors while dragging nodes
                                self.sync_flowchart_paths_if_active_layer();
                            }
                        }
                        self.tools.select.last_doc = raw_doc;
                    }
                    SelectDrag::Resize(handle) => {
                        if let Some(id) = self.selection.first().copied() {
                            // Magnetic + grid snap on the free handle (image, video, shapes).
                            let snapped_doc = self.snap_gizmo_point(doc, Some(id));
                            let new_bounds = tools::resize_bounds(
                                self.tools.select.resize_anchor,
                                handle,
                                snapped_doc,
                            );
                            let mut layer_pos = None;
                            for (pos, l) in self.project.document.layers.iter().enumerate() {
                                if l.kind == crate::document::LayerKind::Shading {
                                    continue;
                                }
                                if l.id == id || (l.kind == crate::document::LayerKind::AV && {
                                    let mut lc = l.clone();
                                    lc.ensure_av_clips();
                                    lc.av_clips.iter().any(|c| c.id == id)
                                }) {
                                    layer_pos = Some(pos);
                                    break;
                                }
                            }
                            if let Some(pos) = layer_pos {
                                let layer = &mut self.project.document.layers[pos];
                                layer.x = new_bounds.x0 as f32;
                                layer.y = new_bounds.y0 as f32;
                                layer.width = new_bounds.width().max(1.0) as f32;
                                layer.height = new_bounds.height().max(1.0) as f32;
                            } else if let Some(node) = self.project.nodes.get_mut(id) {
                                node.set_bounds(new_bounds);
                            }
                        }
                    }
                    SelectDrag::TilingGizmo(pt_idx) => {
                        let dx = doc.0 - self.tools.select.last_doc.0;
                        let dy = doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = doc;
                        if let Some(id) = self.selection.first().copied() {
                            if let Some((_, e)) = self.project.document.tiling_effects.iter_mut().find(|(_, e)| e.source_id == id) {
                                match pt_idx {
                                    0 => { e.offset_x += dx; e.offset_y += dy; }
                                    1 => { e.gap_x += dx; }
                                    2 => { e.gap_y += dy; }
                                    _ => {}
                                }
                            }
                        }
                    }
                    SelectDrag::CircularGizmo(pt_idx) => {
                        // `doc` is raw (unsnapped) pointer so handles track the mouse under Snap to Grid.
                        if let Some(id) = self.selection.first().copied() {
                            let snapped = self.snap_gizmo_point(doc, Some(id));
                            if let Some((_, e)) = self
                                .project
                                .document
                                .circular_effects
                                .iter_mut()
                                .find(|(_, e)| e.source_id == id)
                            {
                                match pt_idx {
                                    // Base (object on ring): absolute snapped position.
                                    0 => {
                                        e.base_x = snapped.0;
                                        e.base_y = snapped.1;
                                        e.radius = (e.base_x - e.origin_x)
                                            .hypot(e.base_y - e.origin_y)
                                            .max(1.0);
                                        e.angle_offset = 0.0;
                                    }
                                    // Origin (center): absolute snap, rigid move of base.
                                    1 => {
                                        let dx = snapped.0 - e.origin_x;
                                        let dy = snapped.1 - e.origin_y;
                                        e.origin_x = snapped.0;
                                        e.origin_y = snapped.1;
                                        e.base_x += dx;
                                        e.base_y += dy;
                                        e.radius = (e.base_x - e.origin_x)
                                            .hypot(e.base_y - e.origin_y)
                                            .max(1.0);
                                    }
                                    // Angle tip: snap pointer, then set angle_offset (fixed radius).
                                    2 => {
                                        let ox = e.origin_x;
                                        let oy = e.origin_y;
                                        let r = e.ring_radius();
                                        let base_ang = e.base_angle_rad();
                                        let pointer_ang =
                                            (snapped.1 - oy).atan2(snapped.0 - ox);
                                        let n = e.copies.max(3) as f64;
                                        let step = std::f64::consts::TAU / n;
                                        e.angle_offset =
                                            (pointer_ang - base_ang - step).to_degrees();
                                        e.base_x = ox + r * base_ang.cos();
                                        e.base_y = oy + r * base_ang.sin();
                                        e.radius = r;
                                    }
                                    _ => {}
                                }
                            }
                            self.sync_circular_ui_from_effect_id(id);
                        }
                        self.tools.select.last_doc = doc;
                    }
                    SelectDrag::Rotate => {
                        if let Some(id) = self.selection.first().copied() {
                            if let Some(center) = self.tools.select.rotate_center {
                                let dx = doc.0 - center.0;
                                let dy = doc.1 - center.1;
                                let current_angle = dy.atan2(dx);
                                let delta_angle = current_angle - self.tools.select.rotate_start_angle;
                                
                                if let Some(pos) = self.project.document.layers.iter().position(|l| l.id == id) {
                                    let layer = &mut self.project.document.layers[pos];
                                    let new_rot = self.tools.select.rotate_start_layer_rotation + delta_angle.to_degrees() as f32;
                                    layer.rotation = new_rot;
                                } else if matches!(
                                    self.project.nodes.get(id).map(|n| &n.kind),
                                    Some(NodeKind::Group { .. })
                                ) {
                                    // Group: rotate parent only (children stay local).
                                    let base = self
                                        .tools
                                        .select
                                        .drag_snapshot
                                        .iter()
                                        .find(|(sid, _)| *sid == id)
                                        .map(|(_, n)| n.transform.rotation_rad)
                                        .or_else(|| {
                                            self.project
                                                .nodes
                                                .get(id)
                                                .map(|n| n.transform.rotation_rad)
                                        })
                                        .unwrap_or(0.0);
                                    if let Some(n) = self.project.nodes.get_mut(id) {
                                        n.transform.rotation_rad = base + delta_angle;
                                    }
                                } else if !self.tools.select.drag_snapshot.is_empty() {
                                    // Multi-select / expanded children: rotate each about center.
                                    let cos = delta_angle.cos();
                                    let sin = delta_angle.sin();
                                    let (cx, cy) = center;
                                    for (sid, original) in self.tools.select.drag_snapshot.clone() {
                                        let mut node = original.clone();
                                        let (px, py) = original.get_pos();
                                        let ox = px - cx;
                                        let oy = py - cy;
                                        let nx = cx + ox * cos - oy * sin;
                                        let ny = cy + ox * sin + oy * cos;
                                        let (cpx, cpy) = node.get_pos();
                                        node.translate(nx - cpx, ny - cpy);
                                        node.set_rotation(original.get_rotation() + delta_angle);
                                        if let Some(n) = self.project.nodes.get_mut(sid) {
                                            *n = node;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if released {
            if let Some(m) = self.tools.select.marquee.take() {
                if tools::marquee_is_drag(m.origin_doc, m.current_doc) {
                    let rect = tools::marquee_rect(m.origin_doc, m.current_doc);
                    // Marquee never picks ghosts (boolean/clip hidden sources).
                    let hidden = self.hidden_canvas_sources();
                    let picked: Vec<NodeId> = if self.spatial_index.is_enabled() {
                        self.spatial_index.nodes_in_marquee(&self.project, &hidden, rect)
                    } else {
                        self.project
                            .document
                            .ordered_node_ids()
                            .into_iter()
                            .filter(|id| {
                                // Circular/tiling/on-path sources stay marquee-pickable.
                                if hidden.contains(id)
                                    && !crate::document::is_pickable_effect_source(
                                        &self.project.document,
                                        *id,
                                    )
                                {
                                    return false;
                                }
                                self.project.nodes.get(*id).is_some_and(|n| {
                                    if self.node_uses_extended_bounds(*id) {
                                        let eb = crate::document::get_effective_bounds(
                                            n,
                                            &self.project.document,
                                            &self.project.nodes,
                                        );
                                        let overlap = eb.intersect(rect);
                                        overlap.width() > 0.0 && overlap.height() > 0.0
                                    } else {
                                        tools::node_bounds_intersects_marquee(n, rect)
                                    }
                                })
                            })
                            .collect()
                    };
                    if m.shift {
                        for id in picked {
                            if !self.selection.contains(&id) {
                                self.selection.push(id);
                            }
                        }
                    } else {
                        self.selection = picked;
                    }
                } else if !m.shift {
                    self.selection.clear();
                    self.tools.select.select_rotation_mode = false;
                    self.selection_sticky = false;
                    self.hit_pick_menu = None;
                }
                self.sync_inspector_if_needed();
            } else if let Some(mode) = self.tools.select.drag_mode.take() {
                if matches!(mode, SelectDrag::TilingGizmo(_) | SelectDrag::CircularGizmo(_)) {
                    // Commit circular/tiling gizmo edits so undo works.
                    if let Some(before) = self.tools.select.effect_drag_doc_before.take() {
                        let after = self.project.document.clone();
                        let changed = before.circular_effects != after.circular_effects
                            || before.tiling_effects != after.tiling_effects;
                        if changed {
                            self.history.push(
                                &mut self.project,
                                ProjectEdit::PatchDocument { before, after },
                            );
                        }
                    }
                    self.tools.select.drag_snapshot.clear();
                } else {
                    if matches!(mode, SelectDrag::Move) {
                        let drag_start = self
                            .tools
                            .select
                            .drag_start_doc
                            .unwrap_or(self.tools.select.last_doc);
                        let total_dx = doc.0 - drag_start.0;
                        let total_dy = doc.1 - drag_start.1;
                        let screen_dist =
                            total_dx.hypot(total_dy) * self.viewport.zoom as f64;
                        let was_click = !self.tools.select.move_drag_engaged
                            || screen_dist < tools::SELECT_MOVE_THRESHOLD_PX;
                        if was_click {
                            // Pure click: restore pose, never commit a tiny move.
                            if self.tools.select.bulk_drag.is_some() {
                                self.revert_bulk_move_preview();
                                self.tools.select.bulk_drag = None;
                            } else {
                                for &(id, ref orig_node) in &self.tools.select.drag_snapshot {
                                    if let Some(node) = self.project.nodes.get_mut(id) {
                                        *node = orig_node.clone();
                                    }
                                }
                                // Restore circular ring snapshot if any.
                                let ring_starts =
                                    self.tools.select.circular_ring_drag_start.clone();
                                for &(sid, bx, by, ox, oy) in &ring_starts {
                                    if let Some((_, e)) = self
                                        .project
                                        .document
                                        .circular_effects
                                        .iter_mut()
                                        .find(|(_, e)| e.source_id == sid)
                                    {
                                        e.base_x = bx;
                                        e.base_y = by;
                                        e.origin_x = ox;
                                        e.origin_y = oy;
                                        e.radius = (e.base_x - e.origin_x)
                                            .hypot(e.base_y - e.origin_y)
                                            .max(1.0);
                                    }
                                }
                            }
                            self.sync_flowchart_paths_if_active_layer();
                            self.tools.select.drag_snapshot.clear();
                            // Second click on already-selected object → toggle rotate mode.
                            if self.selection.len() == 1
                                && self.tools.select.clicked_already_selected
                            {
                                self.tools.select.select_rotation_mode =
                                    !self.tools.select.select_rotation_mode;
                            }
                        } else if self.tools.select.bulk_drag.is_some() {
                            let selection_ids = self.selection.clone();
                            let (snapped_dx, snapped_dy) =
                                self.apply_snapping((total_dx, total_dy), &selection_ids);
                            self.revert_bulk_move_preview();
                            self.commit_bulk_drag(snapped_dx, snapped_dy);
                        } else {
                            self.commit_drag_edits();
                        }
                        self.tools.select.move_drag_engaged = false;
                        self.tools.select.drag_start_doc = None;
                    } else if self.tools.select.bulk_drag.is_none() {
                        self.commit_drag_edits();
                    }
                }
            }
        }
    }

    pub fn get_node_snap_points(&self, node: &Node) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        // Prefer world bounds (rotated text AABB, groups, …).
        let b = node.bounds_with_store(&self.project.nodes);
        let cx = (b.x0 + b.x1) * 0.5;
        let cy = (b.y0 + b.y1) * 0.5;
        // 9-box: center, 4 corners, 4 edge midpoints (all objects, including Image).
        pts.push((cx, cy));
        pts.push((b.x0, b.y0));
        pts.push((b.x1, b.y0));
        pts.push((b.x0, b.y1));
        pts.push((b.x1, b.y1));
        pts.push((cx, b.y0));
        pts.push((cx, b.y1));
        pts.push((b.x0, cy));
        pts.push((b.x1, cy));

        match &node.kind {
            NodeKind::Polygon { cx, cy, r, sides, rotation_rad } => {
                let verts = crate::document::regular_polygon_vertices(*cx, *cy, *r, *sides, *rotation_rad);
                pts.extend(verts.clone());
                let n = verts.len();
                if n >= 3 {
                    for i in 0..n {
                        let v1 = verts[i];
                        let v2 = verts[(i + 1) % n];
                        pts.push(((v1.0 + v2.0) * 0.5, (v1.1 + v2.1) * 0.5));
                    }
                }
            }
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                pts.push((*cx, *cy));
                // Cardinal extremes of the ellipse
                pts.push((*cx + *rx, *cy));
                pts.push((*cx - *rx, *cy));
                pts.push((*cx, *cy + *ry));
                pts.push((*cx, *cy - *ry));
            }
            NodeKind::Arc { cx, cy, radius, .. } => {
                pts.push((*cx, *cy));
                pts.push((*cx + *radius, *cy));
                pts.push((*cx - *radius, *cy));
                pts.push((*cx, *cy + *radius));
                pts.push((*cx, *cy - *radius));
            }
            NodeKind::Path { path } => {
                for p in &path.points {
                    pts.push((p[0], p[1]));
                }
            }
            NodeKind::Image { x, y, width, height, .. } => {
                let w = *width;
                let h = *height;
                // Explicit 9-box (matches transform handles)
                pts.push((*x, *y));
                pts.push((*x + w, *y));
                pts.push((*x, *y + h));
                pts.push((*x + w, *y + h));
                pts.push((*x + w * 0.5, *y));
                pts.push((*x + w * 0.5, *y + h));
                pts.push((*x, *y + h * 0.5));
                pts.push((*x + w, *y + h * 0.5));
                pts.push((*x + w * 0.5, *y + h * 0.5));
            }
            NodeKind::Text { x, y, style } => {
                let r = crate::document::text_bounds_rotated(
                    *x,
                    *y,
                    style,
                    node.transform.rotation_rad,
                );
                let tcx = (r.x0 + r.x1) * 0.5;
                let tcy = (r.y0 + r.y1) * 0.5;
                pts.push((tcx, tcy));
                pts.push((r.x0, r.y0));
                pts.push((r.x1, r.y0));
                pts.push((r.x0, r.y1));
                pts.push((r.x1, r.y1));
                pts.push((tcx, r.y0));
                pts.push((tcx, r.y1));
                pts.push((r.x0, tcy));
                pts.push((r.x1, tcy));
            }
            _ => {}
        }
        pts
    }

    pub fn get_canvas_snap_points(&self) -> Vec<(f64, f64)> {
        let w = self.project.document.width.max(1.0);
        let h = self.project.document.height.max(1.0);
        let mut pts = vec![
            (0.0, 0.0),
            (w, 0.0),
            (0.0, h),
            (w, h),
            (w * 0.5, 0.0),
            (w * 0.5, h),
            (0.0, h * 0.5),
            (w, h * 0.5),
            (w * 0.5, h * 0.5),
        ];
        // Quarter points along each edge (extra anchors for large pages)
        for t in [0.25, 0.75] {
            pts.push((w * t, 0.0));
            pts.push((w * t, h));
            pts.push((0.0, h * t));
            pts.push((w, h * t));
        }
        pts
    }

    pub fn try_equal_spacing_snap(
        &self,
        proposed: (f64, f64),
        target_pts: &[(f64, f64)],
        threshold: f64,
    ) -> Option<((f64, f64), Vec<SnapGuide>)> {
        let n = target_pts.len();
        for i in 0..n {
            let a = target_pts[i];
            for j in (i + 1)..n {
                let b = target_pts[j];
                
                // Horizontal spacing (aligned on Y)
                if (a.1 - b.1).abs() < 1.0 {
                    let d = (a.0 - b.0).abs();
                    if d > 5.0 {
                        let left_x = a.0.min(b.0);
                        let right_x = a.0.max(b.0);
                        
                        // Check right side
                        let target_right = right_x + d;
                        if (proposed.1 - a.1).abs() < threshold && (proposed.0 - target_right).abs() < threshold {
                            let mut guides = Vec::new();
                            guides.push(SnapGuide {
                                start: (left_x, a.1),
                                end: (right_x, a.1),
                                is_tangent: false,
                            });
                            guides.push(SnapGuide {
                                start: (right_x, a.1),
                                end: (target_right, a.1),
                                is_tangent: false,
                            });
                            return Some(((target_right, a.1), guides));
                        }
                        
                        // Check left side
                        let target_left = left_x - d;
                        if (proposed.1 - a.1).abs() < threshold && (proposed.0 - target_left).abs() < threshold {
                            let mut guides = Vec::new();
                            guides.push(SnapGuide {
                                start: (target_left, a.1),
                                end: (left_x, a.1),
                                is_tangent: false,
                            });
                            guides.push(SnapGuide {
                                start: (left_x, a.1),
                                end: (right_x, a.1),
                                is_tangent: false,
                            });
                            return Some(((target_left, a.1), guides));
                        }
                    }
                }
                
                // Vertical spacing (aligned on X)
                if (a.0 - b.0).abs() < 1.0 {
                    let d = (a.1 - b.1).abs();
                    if d > 5.0 {
                        let top_y = a.1.min(b.1);
                        let bottom_y = a.1.max(b.1);
                        
                        // Check bottom side
                        let target_bottom = bottom_y + d;
                        if (proposed.0 - a.0).abs() < threshold && (proposed.1 - target_bottom).abs() < threshold {
                            let mut guides = Vec::new();
                            guides.push(SnapGuide {
                                start: (a.0, top_y),
                                end: (a.0, bottom_y),
                                is_tangent: false,
                            });
                            guides.push(SnapGuide {
                                start: (a.0, bottom_y),
                                end: (a.0, target_bottom),
                                is_tangent: false,
                            });
                            return Some(((a.0, target_bottom), guides));
                        }
                        
                        // Check top side
                        let target_top = top_y - d;
                        if (proposed.0 - a.0).abs() < threshold && (proposed.1 - target_top).abs() < threshold {
                            let mut guides = Vec::new();
                            guides.push(SnapGuide {
                                start: (a.0, target_top),
                                end: (a.0, top_y),
                                is_tangent: false,
                            });
                            guides.push(SnapGuide {
                                start: (a.0, top_y),
                                end: (a.0, bottom_y),
                                is_tangent: false,
                            });
                            return Some(((a.0, target_top), guides));
                        }
                    }
                }
            }
        }
        None
    }

    pub fn snap_cursor(&mut self, doc: (f64, f64)) -> (f64, f64) {
        if !self.snap_magnet {
            if self.pixel_art_mode {
                let cell = self.pixel_cell_size as f64;
                return ((doc.0 / cell).round() * cell, (doc.1 / cell).round() * cell);
            }
            return doc;
        }
        let mut snapped = doc;
        self.live_snap_guides.clear();
        let threshold = (8.0 / self.viewport.zoom as f64).max(0.1);

        let mut target_pts = Vec::new();
        target_pts.extend(self.get_canvas_snap_points());
        for (_, node) in &self.project.nodes.map {
            target_pts.extend(self.get_node_snap_points(node));
        }

        // Try equal spacing snap first
        if let Some((eq_snapped, eq_guides)) = self.try_equal_spacing_snap(doc, &target_pts, threshold) {
            self.live_snap_guides = eq_guides;
            return eq_snapped;
        }

        let mut best_dx = threshold;
        let mut best_dy = threshold;
        let mut snap_pt_x = None;
        let mut snap_pt_y = None;

        for &tpt in &target_pts {
            let dx = tpt.0 - doc.0;
            let dy = tpt.1 - doc.1;
            if dx.abs() < best_dx.abs() {
                best_dx = dx;
                snap_pt_x = Some(tpt);
            }
            if dy.abs() < best_dy.abs() {
                best_dy = dy;
                snap_pt_y = Some(tpt);
            }
        }

        if let Some(tpt) = snap_pt_x {
            snapped.0 = tpt.0;
            self.live_snap_guides.push(SnapGuide {
                start: tpt,
                end: (tpt.0, snapped.1),
                is_tangent: false,
            });
        }
        if let Some(tpt) = snap_pt_y {
            snapped.1 = tpt.1;
            self.live_snap_guides.push(SnapGuide {
                start: tpt,
                end: (snapped.0, tpt.1),
                is_tangent: false,
            });
        }

        if self.pixel_art_mode {
            let cell = self.pixel_cell_size as f64;
            snapped.0 = (snapped.0 / cell).round() * cell;
            snapped.1 = (snapped.1 / cell).round() * cell;
        }

        snapped
    }

    pub fn apply_snapping(
        &mut self,
        proposed_translation: (f64, f64),
        selection: &[NodeId],
    ) -> (f64, f64) {
        self.live_snap_guides.clear();
        if !self.snap_magnet {
            return proposed_translation;
        }
        
        let mut original_pts = Vec::new();
        let mut dragged_circles = Vec::new();
        
        // Check if there is a selected Video layer
        let mut video_selection = Vec::new();
        for &id in selection {
            if let Some(l) = self.project.document.layers.iter().find(|l| l.id == id) {
                if l.kind == crate::document::LayerKind::AV {
                    video_selection.push(l.id);
                }
            }
        }

        if self.tools.select.drag_snapshot.is_empty() && video_selection.is_empty() {
            return proposed_translation;
        }

        let threshold = (8.0 / self.viewport.zoom as f64).max(0.1);
        let mut final_translation = proposed_translation;

        // 1. Identify target nodes/layers and their snap points
        let mut target_pts = Vec::new();
        let mut target_circles = Vec::new();
        target_pts.extend(self.get_canvas_snap_points());
        for (id, node) in &self.project.nodes.map {
            if selection.contains(id) {
                continue;
            }
            let pts = self.get_node_snap_points(node);
            target_pts.extend(pts);
            
            if node.is_circle() {
                if let NodeKind::Ellipse { cx, cy, rx, .. } = &node.kind {
                    target_circles.push(((*cx, *cy), *rx));
                }
            }
        }

        for layer in &self.project.document.layers {
            if selection.contains(&layer.id) {
                continue;
            }
            if layer.visible && layer.kind == crate::document::LayerKind::AV {
                let mut dx = layer.x as f64;
                let mut dy = layer.y as f64;
                if let Some(track) = self.project.anim_timeline.nodes.get(&layer.id) {
                    if let Some(x) = track.pos_x.interpolate(self.anim_current_frame) {
                        dx = x;
                    }
                    if let Some(y) = track.pos_y.interpolate(self.anim_current_frame) {
                        dy = y;
                    }
                }
                let aspect = self.video_layers.get(&layer.id)
                    .and_then(|s| s.texture.as_ref())
                    .map(|tex| {
                        let tex_w = tex.size()[0] as f32;
                        let tex_h = tex.size()[1] as f32;
                        if tex_h > 0.0 { (tex_w / tex_h) as f64 } else { 1.0 }
                    })
                    .or_else(|| {
                        self.video_frame_cache.as_ref()
                            .filter(|c| c.layer_id == layer.id)
                            .map(|c| {
                                let tex_w = c.texture.size()[0] as f32;
                                let tex_h = c.texture.size()[1] as f32;
                                if tex_h > 0.0 { (tex_w / tex_h) as f64 } else { 1.0 }
                            })
                    })
                    .unwrap_or(1.0);
                let mut w = layer.width as f64;
                let mut h = layer.height as f64;
                if layer.aspect_ratio_locked {
                    if w / h > aspect {
                        w = h * aspect;
                    } else {
                        h = w / aspect;
                    }
                }
                let cx = dx + w / 2.0;
                let cy = dy + h / 2.0;
                // Full 9-box for video/AV layers (corners, edge mids, center)
                target_pts.push((dx, dy));
                target_pts.push((dx + w, dy));
                target_pts.push((dx, dy + h));
                target_pts.push((dx + w, dy + h));
                target_pts.push((cx, dy));
                target_pts.push((cx, dy + h));
                target_pts.push((dx, cy));
                target_pts.push((dx + w, cy));
                target_pts.push((cx, cy));
            }
        }

        // 2. Identify candidate points in selection
        for (_, orig_node) in &self.tools.select.drag_snapshot {
            let pts = self.get_node_snap_points(orig_node);
            original_pts.extend(pts);
            
            if orig_node.is_circle() {
                if let NodeKind::Ellipse { cx, cy, rx, .. } = &orig_node.kind {
                    dragged_circles.push(((*cx, *cy), *rx));
                }
            }
        }

        if !video_selection.is_empty() {
            let r = self.tools.select.resize_anchor;
            let cx = r.center().x;
            let cy = r.center().y;
            original_pts.push((r.x0, r.y0));
            original_pts.push((r.x1, r.y0));
            original_pts.push((r.x0, r.y1));
            original_pts.push((r.x1, r.y1));
            original_pts.push((cx, r.y0));
            original_pts.push((cx, r.y1));
            original_pts.push((r.x0, cy));
            original_pts.push((r.x1, cy));
            original_pts.push((cx, cy));
        }

        // Try equal spacing snap first
        for &opt in &original_pts {
            let ppt = (opt.0 + proposed_translation.0, opt.1 + proposed_translation.1);
            if let Some((eq_snapped, eq_guides)) = self.try_equal_spacing_snap(ppt, &target_pts, threshold) {
                let snap_dx = eq_snapped.0 - ppt.0;
                let snap_dy = eq_snapped.1 - ppt.1;
                self.live_snap_guides = eq_guides;
                return (proposed_translation.0 + snap_dx, proposed_translation.1 + snap_dy);
            }
        }

        // 3. Try tangent snapping first if we have dragged circle(s)
        let mut tangent_snapped = false;
        let mut snap_offset = (0.0, 0.0);
        let mut best_dist_diff = threshold;
        let mut best_tangent_snap = None;

        for &(dc_orig, dr) in &dragged_circles {
            let dc_prop = (dc_orig.0 + proposed_translation.0, dc_orig.1 + proposed_translation.1);
            for &(tc, tr) in &target_circles {
                let dist = (dc_prop.0 - tc.0).hypot(dc_prop.1 - tc.1);
                let d_ideal = dr + tr;
                let diff = (dist - d_ideal).abs();
                if diff < best_dist_diff && dist > 0.01 {
                    best_dist_diff = diff;
                    let dir_x = (dc_prop.0 - tc.0) / dist;
                    let dir_y = (dc_prop.1 - tc.1) / dist;
                    let dc_snapped = (tc.0 + dir_x * d_ideal, tc.1 + dir_y * d_ideal);
                    best_tangent_snap = Some((tc, dc_snapped, dc_prop));
                }
            }
        }

        if let Some((tc, dc_snapped, dc_prop)) = best_tangent_snap {
            let snap_dx = dc_snapped.0 - dc_prop.0;
            let snap_dy = dc_snapped.1 - dc_prop.1;
            snap_offset = (snap_dx, snap_dy);
            tangent_snapped = true;
            self.live_snap_guides.push(SnapGuide {
                start: tc,
                end: dc_snapped,
                is_tangent: true,
            });
        }

        // 4. Try alignment snap
        let mut snap_x = if tangent_snapped { snap_offset.0 } else { 0.0 };
        let mut snap_y = if tangent_snapped { snap_offset.1 } else { 0.0 };

        if !tangent_snapped {
            let mut best_dx = threshold;
            let mut best_dy = threshold;
            let mut snap_pt_x = None;
            let mut snap_pt_y = None;

            for &opt in &original_pts {
                let ppt = (opt.0 + proposed_translation.0, opt.1 + proposed_translation.1);
                for &tpt in &target_pts {
                    let dx = tpt.0 - ppt.0;
                    let dy = tpt.1 - ppt.1;
                    
                    if dx.abs() < best_dx.abs() {
                        best_dx = dx;
                        snap_pt_x = Some((tpt, ppt));
                    }
                    if dy.abs() < best_dy.abs() {
                        best_dy = dy;
                        snap_pt_y = Some((tpt, ppt));
                    }
                }
            }

            if let Some((tpt, ppt)) = snap_pt_x {
                snap_x = best_dx;
                self.live_snap_guides.push(SnapGuide {
                    start: tpt,
                    end: (tpt.0, ppt.1),
                    is_tangent: false,
                });
            }
            if let Some((tpt, ppt)) = snap_pt_y {
                snap_y = best_dy;
                self.live_snap_guides.push(SnapGuide {
                    start: tpt,
                    end: (ppt.0, tpt.1),
                    is_tangent: false,
                });
            }
        }

        final_translation.0 += snap_x;
        final_translation.1 += snap_y;

        // 5. Try grid snap if enabled
        if self.viewport.snap_grid {
            let g = self.viewport.grid_step as f64;
            if g > 0.0 {
                let mut best_grid_dx = threshold;
                let mut best_grid_dy = threshold;
                let mut grid_snap_x = 0.0;
                let mut grid_snap_y = 0.0;
                let mut snapped_any_x = false;
                let mut snapped_any_y = false;

                for &opt in &original_pts {
                    let ppt = (opt.0 + final_translation.0, opt.1 + final_translation.1);
                    let grid_x = (ppt.0 / g).round() * g;
                    let grid_y = (ppt.1 / g).round() * g;
                    let dx = grid_x - ppt.0;
                    let dy = grid_y - ppt.1;
                    
                    if dx.abs() < best_grid_dx.abs() {
                        best_grid_dx = dx;
                        grid_snap_x = dx;
                        snapped_any_x = true;
                    }
                    if dy.abs() < best_grid_dy.abs() {
                        best_grid_dy = dy;
                        grid_snap_y = dy;
                        snapped_any_y = true;
                    }
                }
                
                if snapped_any_x {
                    final_translation.0 += grid_snap_x;
                }
                if snapped_any_y {
                    final_translation.1 += grid_snap_y;
                }
            }
        }

        // Correct guide lines end positions to match final snapped coordinate
        for guide in &mut self.live_snap_guides {
            if !guide.is_tangent {
                let end_ppt_orig = (guide.end.0 - proposed_translation.0, guide.end.1 - proposed_translation.1);
                guide.end = (end_ppt_orig.0 + final_translation.0, end_ppt_orig.1 + final_translation.1);
            }
        }

        final_translation
    }

    fn styled_shape_node(&self, mut node: Node) -> Node {
        node.style.stroke = self.build_ui_stroke();
        node.style.fill = self.build_ui_fill();
        node
    }

    fn tool_drag_shape(&mut self, doc: (f64, f64), down: bool, released: bool, ctrl: bool) {
        if self.tools.drag_shape.is_none() && down {
            let snapped_origin = self.snap_cursor(doc);
            self.tools.drag_shape = Some(DragNewShape {
                origin_doc: snapped_origin,
                current_doc: snapped_origin,
                kind: Some(self.tools.active),
            });
        } else if self.tools.drag_shape.is_some() {
            let mut snapped_current = self.snap_cursor(doc);
            // Line tool: Ctrl locks angle to 15° steps about the origin.
            if ctrl {
                if let Some(drag) = &self.tools.drag_shape {
                    if drag.kind == Some(ToolKind::Line) {
                        snapped_current =
                            tools::snap_angle_15deg(drag.origin_doc, snapped_current);
                    }
                }
            }
            if let Some(drag) = &mut self.tools.drag_shape {
                drag.current_doc = snapped_current;
            }
            if released {
                let drag = self.tools.drag_shape.take().unwrap();
                let kind = drag.kind;
                let origin = drag.origin_doc;
                let mut current = drag.current_doc;
                if ctrl && kind == Some(ToolKind::Line) {
                    current = tools::snap_angle_15deg(origin, current);
                }

                let Some(kind) = kind else {
                    return;
                };

                let is_flowchart = self.project.document.layers
                    .get(self.project.document.active_layer_index)
                    .map_or(false, |l| l.kind == crate::document::LayerKind::Flowchart);

                let node = match kind {
                    ToolKind::Rectangle => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        if w <= 2.0 || h <= 2.0 {
                            return;
                        }
                        if is_flowchart {
                            let corner_rx = (w.min(h) * 0.22).clamp(8.0, 48.0);
                            let mut n = Node::new(
                                crate::document::NodeKind::FlowchartNode {
                                    cx: x + w / 2.0,
                                    cy: y + h / 2.0,
                                    w,
                                    h,
                                    corner_rx,
                                    label: String::new(),
                                    label_font_size: 14.0,
                                    label_align: crate::document::TextAlign::Center,
                                    label_font_family: "Noto Sans".to_string(),
                                    label_bold: false,
                                    label_italic: false,
                                },
                                "Flowchart Node",
                            );
                            n.style.fill = self.build_ui_fill();
                            n.style.stroke = self.build_ui_stroke();
                            n
                        } else {
                            self.styled_shape_node(Node::rect(
                                x,
                                y,
                                w,
                                h,
                                self.build_ui_fill(),
                            ))
                        }
                    }
                    ToolKind::Circle => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        let mut n = Node::ellipse(cx, cy, r, r, self.build_ui_fill());
                        n.name = "Circle".into();
                        self.styled_shape_node(n)
                    }
                    ToolKind::Ellipse => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        if w <= 2.0 || h <= 2.0 {
                            return;
                        }
                        self.styled_shape_node(Node::ellipse(
                            x + w / 2.0,
                            y + h / 2.0,
                            w / 2.0,
                            h / 2.0,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Polygon => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        self.styled_shape_node(Node::polygon(
                            cx,
                            cy,
                            side / 2.0,
                            self.polygon_sides,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Line => {
                        let dx = current.0 - origin.0;
                        let dy = current.1 - origin.1;
                        if dx.hypot(dy) <= 2.0 {
                            return;
                        }
                        let mut stroke = self.build_ui_stroke();
                        if !self.stroke_enabled {
                            stroke.width = 1.0;
                            stroke.style = Fill::Solid(Paint::from_hex(0x1a1f2e, 1.0));
                        }
                        if is_flowchart {
                            let mut n = crate::document::flowchart::new_flowchart_path(vec![origin, current]);
                            // Snap endpoints to nearest flowchart nodes + anchors (using dist to the anchor itself),
                            // then route orthogonally. This makes "click near a node edge" attach reliably.
                            if let crate::document::NodeKind::FlowchartPath { path } = &mut n.kind {
                                let active_idx = self.project.document.active_layer_index;
                                if let Some(layer) = self.project.document.layers.get(active_idx) {
                                    let store = &self.project.nodes;
                                    // Use slop in doc units (forgiving when ending drag near a node port/edge)
                                    let anchor_slop = 80.0f64;
                                    let mut best_start_d = anchor_slop;
                                    let mut best_end_d = anchor_slop;

                                    for &nid in &layer.nodes {
                                        if let Some(nd) = store.get(nid) {
                                            if let Some(geom) = crate::document::flowchart::node_as_flowchart_geom(&nd.kind) {
                                                // For start
                                                let anc_s = crate::document::flowchart::snap_anchor_for_point(&geom, origin);
                                                let ap_s = geom.anchor_position(anc_s);
                                                let ds = (ap_s.0 - origin.0).hypot(ap_s.1 - origin.1);
                                                if ds < best_start_d {
                                                    path.start_node = Some(nid);
                                                    path.start_anchor = Some(anc_s);
                                                    if let Some(p0) = path.points.first_mut() {
                                                        *p0 = ap_s;
                                                    }
                                                    best_start_d = ds;
                                                }

                                                // For end
                                                let anc_e = crate::document::flowchart::snap_anchor_for_point(&geom, current);
                                                let ap_e = geom.anchor_position(anc_e);
                                                let de = (ap_e.0 - current.0).hypot(ap_e.1 - current.1);
                                                if de < best_end_d {
                                                    path.end_node = Some(nid);
                                                    path.end_anchor = Some(anc_e);
                                                    if let Some(p1) = path.points.last_mut() {
                                                        *p1 = ap_e;
                                                    }
                                                    best_end_d = de;
                                                }
                                            }
                                        }
                                    }
                                    let exclude: Vec<_> = [path.start_node, path.end_node].iter().filter_map(|x| *x).collect();
                                    let obstacles = crate::document::flowchart::flowchart_routing_obstacles(store, &layer.nodes, &exclude);
                                    crate::document::flowchart::sync_flowchart_path_endpoints(path, store, &obstacles);
                                }
                            }
                            n.style.stroke = stroke;
                            n.name = "Flowchart Connector".into();
                            n
                        } else {
                            Node::line(origin.0, origin.1, current.0, current.1, stroke)
                        }
                    }
                    ToolKind::Arc => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        // Default: 90 degree arc, no join (user edits angle/join in Geometry)
                        let start = -std::f64::consts::FRAC_PI_4;
                        let sweep = std::f64::consts::FRAC_PI_2;
                        self.styled_shape_node(Node::arc(
                            cx,
                            cy,
                            r,
                            start,
                            sweep,
                            crate::document::ArcJoin::NoJoin,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Plotter => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        if w <= 2.0 || h <= 2.0 {
                            return;
                        }
                        self.styled_shape_node(Node::plotter(x, y, w, h, self.build_ui_fill()))
                    }
                    _ => return,
                };
                self.insert_node(node);
                self.rebalance_active_flowchart_layer_if_any();
            }
        }
    }

    fn pen_push_anchor(&mut self, doc: (f64, f64), smooth: bool) {
        if self.tools.pen.extend_from_start {
            self.tools.pen.anchors.insert(0, doc);
            let mut smooth_anchors: Vec<usize> = self
                .tools
                .pen
                .smooth_anchors
                .iter()
                .map(|&i| i + 1)
                .collect();
            if smooth {
                smooth_anchors.push(0);
                smooth_anchors.sort_unstable();
                smooth_anchors.dedup();
            }
            let mut out = std::collections::HashMap::new();
            let mut inn = std::collections::HashMap::new();
            for (k, v) in &self.tools.pen.handle_out_offset {
                out.insert(k + 1, *v);
            }
            for (k, v) in &self.tools.pen.handle_in_offset {
                inn.insert(k + 1, *v);
            }
            self.tools.pen.smooth_anchors = smooth_anchors;
            self.tools.pen.handle_out_offset = out;
            self.tools.pen.handle_in_offset = inn;
            if smooth {
                self.tools.pen.curve_adjust = Some(0);
            }
        } else {
            let idx = self.tools.pen.anchors.len();
            self.tools.pen.anchors.push(doc);
            if smooth {
                if !self.tools.pen.smooth_anchors.contains(&idx) {
                    self.tools.pen.smooth_anchors.push(idx);
                    self.tools.pen.smooth_anchors.sort_unstable();
                    self.tools.pen.smooth_anchors.dedup();
                }
                self.tools.pen.curve_adjust = Some(idx);
            }
        }
    }

    fn tool_pen(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        ctrl: bool,
    ) {
        let endpoint_thresh = 8.0 / self.viewport.zoom as f64;

        if pressed {
            if let Some(first) = self.tools.pen.anchors.first() {
                if self.tools.pen.anchors.len() >= 2
                    && (first.0 - doc.0).hypot(first.1 - doc.1) < 2.0
                    && (self.tools.pen.continue_node.is_none() || self.tools.pen.was_closed)
                {
                    self.finish_pen_path(true);
                    return;
                }
            }

            if let Some(_) = self.tools.pen.continue_node {
                if let (Some(first), Some(last)) =
                    (self.tools.pen.anchors.first(), self.tools.pen.anchors.last())
                {
                    let near_start = (first.0 - doc.0).hypot(first.1 - doc.1) < endpoint_thresh;
                    let near_end = (last.0 - doc.0).hypot(last.1 - doc.1) < endpoint_thresh;
                    if near_start {
                        self.tools.pen.extend_from_start = true;
                        self.tools.pen.join_anchor = Some(0);
                        if !self.tools.pen.smooth_anchors.contains(&0) {
                            self.tools.pen.smooth_anchors.push(0);
                            self.tools.pen.smooth_anchors.sort_unstable();
                        }
                        return;
                    }
                    if near_end {
                        self.tools.pen.extend_from_start = false;
                        let end_idx = self.tools.pen.anchors.len().saturating_sub(1);
                        self.tools.pen.join_anchor = Some(end_idx);
                        if !self.tools.pen.smooth_anchors.contains(&end_idx) {
                            self.tools.pen.smooth_anchors.push(end_idx);
                            self.tools.pen.smooth_anchors.sort_unstable();
                        }
                        return;
                    }
                }
            }

            // Second point of a path: Ctrl locks angle to 15° about the first point
            // (only for 2-point paths — once a 3rd point is added, no angle lock).
            let mut place = doc;
            if self.tools.pen.anchors.len() == 1 && ctrl {
                place = tools::snap_angle_15deg(self.tools.pen.anchors[0], doc);
            }
            // Ctrl also enables smooth handles (existing); combine with angle lock above.
            self.pen_push_anchor(place, ctrl);
        }

        if down {
            if let Some(idx) = self.tools.pen.curve_adjust {
                if ctrl {
                    let Some(&(ax, ay)) = self.tools.pen.anchors.get(idx) else {
                        return;
                    };
                    let offset = [doc.0 - ax, doc.1 - ay];
                    self.tools.pen.handle_out_offset.insert(idx, offset);
                    self.tools.pen
                        .handle_in_offset
                        .insert(idx, [-offset[0], -offset[1]]);
                }
            }
        }

        if released {
            self.tools.pen.curve_adjust = None;
        }
    }

    fn handle_gradient_flow_input(
        &mut self,
        origin: Pos2,
        screen: Pos2,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
    ) -> bool {
        use crate::document::{linear_angle_from_line, translate_linear_line};

        if self.action_tab != ui::ActionTab::ColorStroke || self.selection.len() != 1 {
            self.gradient_flow_drag = None;
            return false;
        }
        let Some(id) = self.selection.first().copied() else {
            return false;
        };
        let Some(node) = self.project.nodes.get(id) else {
            return false;
        };
        let bounds = node.bounds();
        let slop = 12.0;

        let fill_active = self.ui_fill_edit_gradient_line
            && self.fill_enabled
            && matches!(
                self.ui_fill_kind,
                FillKind::LinearGradient | FillKind::RadialGradient
            );
        let stroke_active = self.ui_stroke_edit_gradient_line
            && self.stroke_enabled
            && matches!(
                self.ui_stroke_kind,
                FillKind::LinearGradient | FillKind::RadialGradient
            );
        if !fill_active && !stroke_active {
            self.gradient_flow_drag = None;
            return false;
        }

        if pressed {
            if fill_active {
                if let Some(handle) = render::pick_gradient_flow_handle(
                    &self.viewport,
                    origin,
                    bounds,
                    self.ui_fill_kind,
                    (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    ),
                    self.ui_radial_cx,
                    self.ui_radial_cy,
                    screen,
                    slop,
                ) {
                    let line = (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    );
                    self.gradient_flow_drag = Some(GradientFlowDrag {
                        target: GradientFlowTarget::Fill,
                        handle,
                        line_at_press: line,
                        doc_at_press: doc,
                    });
                }
            } else if stroke_active {
                if let Some(handle) = render::pick_gradient_flow_handle(
                    &self.viewport,
                    origin,
                    bounds,
                    self.ui_stroke_kind,
                    (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    ),
                    self.ui_stroke_radial_cx,
                    self.ui_stroke_radial_cy,
                    screen,
                    slop,
                ) {
                    let line = (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    );
                    self.gradient_flow_drag = Some(GradientFlowDrag {
                        target: GradientFlowTarget::Stroke,
                        handle,
                        line_at_press: line,
                        doc_at_press: doc,
                    });
                }
            }
        }

        if released {
            let was = self.gradient_flow_drag.is_some();
            self.gradient_flow_drag = None;
            return was;
        }

        let Some(drag) = self.gradient_flow_drag else {
            return false;
        };

        if !down {
            return false;
        }

        let w = (bounds.x1 - bounds.x0).max(1e-6);
        let h = (bounds.y1 - bounds.y0).max(1e-6);
        let (nx, ny) = render::linear_norm_from_bounds_drag(bounds, doc);

        match drag.target {
            GradientFlowTarget::Fill => match self.ui_fill_kind {
                FillKind::LinearGradient => {
                    let mut line = (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    );
                    match drag.handle {
                        crate::gradient_ui::GradientLineHandle::LinearEnd0 => {
                            line.0 = nx;
                            line.1 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearEnd1 => {
                            line.2 = nx;
                            line.3 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearMid => {
                            let dx = nx
                                - ((drag.doc_at_press.0 - bounds.x0) / w) as f32;
                            let dy = ny
                                - ((drag.doc_at_press.1 - bounds.y0) / h) as f32;
                            line = drag.line_at_press;
                            translate_linear_line(&mut line, dx, dy);
                        }
                        crate::gradient_ui::GradientLineHandle::RadialFocal => {}
                    }
                    self.ui_fill_line_x0 = line.0;
                    self.ui_fill_line_y0 = line.1;
                    self.ui_fill_line_x1 = line.2;
                    self.ui_fill_line_y1 = line.3;
                    self.ui_gradient_angle =
                        linear_angle_from_line(line.0, line.1, line.2, line.3);
                    self.apply_fill_to_selection();
                }
                FillKind::RadialGradient => {
                    if drag.handle == crate::gradient_ui::GradientLineHandle::RadialFocal {
                        let (cx, cy) = render::radial_from_bounds_drag(bounds, doc);
                        self.ui_radial_cx = cx;
                        self.ui_radial_cy = cy;
                        self.apply_fill_to_selection();
                    }
                }
                FillKind::Solid => {}
            },
            GradientFlowTarget::Stroke => match self.ui_stroke_kind {
                FillKind::LinearGradient => {
                    let mut line = (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    );
                    match drag.handle {
                        crate::gradient_ui::GradientLineHandle::LinearEnd0 => {
                            line.0 = nx;
                            line.1 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearEnd1 => {
                            line.2 = nx;
                            line.3 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearMid => {
                            let dx = nx
                                - ((drag.doc_at_press.0 - bounds.x0) / w) as f32;
                            let dy = ny
                                - ((drag.doc_at_press.1 - bounds.y0) / h) as f32;
                            line = drag.line_at_press;
                            translate_linear_line(&mut line, dx, dy);
                        }
                        crate::gradient_ui::GradientLineHandle::RadialFocal => {}
                    }
                    self.ui_stroke_line_x0 = line.0;
                    self.ui_stroke_line_y0 = line.1;
                    self.ui_stroke_line_x1 = line.2;
                    self.ui_stroke_line_y1 = line.3;
                    self.ui_stroke_angle =
                        linear_angle_from_line(line.0, line.1, line.2, line.3);
                    self.apply_stroke_to_selection();
                }
                FillKind::RadialGradient => {
                    if drag.handle == crate::gradient_ui::GradientLineHandle::RadialFocal {
                        let (cx, cy) = render::radial_from_bounds_drag(bounds, doc);
                        self.ui_stroke_radial_cx = cx;
                        self.ui_stroke_radial_cy = cy;
                        self.apply_stroke_to_selection();
                    }
                }
                FillKind::Solid => {}
            },
        }
        true
    }

    fn canvas_wheel_zoom(&mut self, ctx: &Context) {
        let Some(canvas_rect) = self.canvas_screen_rect else {
            return;
        };
        // Handle multi-touch zoom and pan first
        if let Some(multi_touch) = ctx.input(|i| i.multi_touch()) {
            if canvas_rect.contains(multi_touch.center_pos) {
                if (multi_touch.zoom_delta - 1.0).abs() > 1e-4 {
                    self.viewport.zoom_at(multi_touch.center_pos, self.canvas_origin, multi_touch.zoom_delta);
                }
                self.viewport.pan += multi_touch.translation_delta;
                return;
            }
        }
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let on_canvas = hover.is_some_and(|p| canvas_rect.contains(p));
        if !on_canvas {
            return;
        }
        // egui routes Ctrl+wheel into zoom_delta (not smooth_scroll_delta).
        let factor = ctx.input(|i| i.zoom_delta());
        if (factor - 1.0).abs() <= 1e-4 {
            return;
        }
        let pos = hover.unwrap_or(canvas_rect.center());
        self.viewport.zoom_at(pos, self.canvas_origin, factor);
    }

    fn tool_text(&mut self, doc: (f64, f64), pressed: bool) {
        if !pressed {
            return;
        }
        let style = TextStyle {
            content: String::new(),
            font_size: self.ui_text_font_size,
            font_family: self.ui_text_font_family.clone(),
            bold: self.ui_text_bold,
            italic: self.ui_text_italic,
            width: self.ui_text_width,
        };
        let mut node = self.styled_shape_node(Node::text(doc.0, doc.1, style));
        node.name = "Text".into();
        let id = node.id;
        // Add live for preview/typing but do NOT push history yet. Only commit on non-empty finish.
        let _ = self.project.nodes.insert(node.clone());
        self.project.document.append_to_active_layer(id);
        self.ui_text_content.clear();
        self.on_page_text_newly_created = true;
        self.begin_on_page_text_edit(id);
    }

    fn tool_brush(
        &mut self,
        doc: (f64, f64),
        time: f64,
        pressed: bool,
        down: bool,
        released: bool,
        pressure: Option<f32>,
        shift: bool,
        ctrl: bool,
    ) {
        let pixel = self.tools.brush.brush_type == crate::tools::BrushType::Pixel;
        // Shift = erase; Ctrl = straight line (Shift wins if both).
        let erase = pixel && shift;
        let line_mode = pixel && ctrl && !shift;

        if pressed {
            self.tools.brush.points.clear();
            self.tools.brush.pixel_line_anchor = None;
            self.tools.brush.pixel_erase_before.clear();
            if erase {
                // Snapshot nodes we may patch for one undo on release.
                self.pixel_erase_begin();
                self.pixel_erase_at(doc, time);
            } else if pixel {
                let (cx, cy, w, _h) = crate::tools::pixel_stamp_at(
                    doc,
                    self.viewport.step_x(),
                    self.viewport.step_y(),
                    self.tools.brush.pixel_cells,
                );
                self.tools.brush.points.push(([cx, cy], time, w as f32));
                if line_mode {
                    self.tools.brush.pixel_line_anchor = Some(doc);
                }
            } else {
                let size = self.tools.brush.size;
                let initial_w = if self.tools.brush.brush_type == crate::tools::BrushType::Pen {
                    let v = if let Some(p) = pressure { p as f64 } else { 1.0 };
                    let max_r = size as f64 / 2.0;
                    let y = (1.0 - v) * max_r;
                    let r = (max_r * max_r - y * y).max(0.0).sqrt();
                    (r * 2.0).max(1.0) as f32
                } else {
                    size
                };
                self.tools.brush.points.push(([doc.0, doc.1], time, initial_w));
            }
        } else if down {
            if erase {
                self.pixel_erase_at(doc, time);
            } else if pixel && line_mode {
                // Straight line preview: only stamps from anchor → cursor.
                let anchor = self
                    .tools
                    .brush
                    .pixel_line_anchor
                    .unwrap_or(doc);
                let gx = self.viewport.step_x();
                let gy = self.viewport.step_y();
                let cells = self.tools.brush.pixel_cells;
                let stamps =
                    crate::tools::pixel_stamps_along(anchor, doc, gx, gy, cells);
                self.tools.brush.points.clear();
                for (cx, cy, w, _h) in stamps {
                    self.tools.brush.points.push(([cx, cy], time, w as f32));
                }
            } else if pixel {
                // Freehand: fill every cell along the segment (no gaps when moving fast).
                let gx = self.viewport.step_x();
                let gy = self.viewport.step_y();
                let cells = self.tools.brush.pixel_cells;
                let prev_doc = self
                    .tools
                    .brush
                    .points
                    .last()
                    .map(|&(p, _, _)| (p[0], p[1]))
                    .unwrap_or(doc);
                let stamps =
                    crate::tools::pixel_stamps_along(prev_doc, doc, gx, gy, cells);
                for (cx, cy, w, _h) in stamps {
                    let dup = self.tools.brush.points.iter().any(|&(p, _, _)| {
                        (p[0] - cx).abs() < 1e-6 && (p[1] - cy).abs() < 1e-6
                    });
                    if !dup {
                        self.tools.brush.points.push(([cx, cy], time, w as f32));
                    }
                }
            } else if let Some(&(prev_pos, prev_time, prev_w)) = self.tools.brush.points.last() {
                let size = self.tools.brush.size;
                let heavy = self.tools.brush.heavy;
                let smoothness = self.tools.brush.smoothness;

                // Stabilizer pull radius — keep it from starving samples on fast curves.
                // Old: heavy*60 forced gaps of 13+ px → polygonal spirals.
                let r = (heavy * 18.0) as f64;
                let raw_dist =
                    ((doc.0 - prev_pos[0]).powi(2) + (doc.1 - prev_pos[1]).powi(2)).sqrt();
                // Always accept a sample when the cursor moved meaningfully (or always while down
                // for dense freehand). Stabilizer still softens position, but we no longer
                // drop entire frames when moving fast.
                let min_step = (size as f64 * 0.08).max(0.75);
                if raw_dist > min_step {
                    let stabilized_pos = if r > 0.0001 && raw_dist > r {
                        let pull_ratio = r / raw_dist;
                        [
                            doc.0 - (doc.0 - prev_pos[0]) * pull_ratio,
                            doc.1 - (doc.1 - prev_pos[1]) * pull_ratio,
                        ]
                    } else {
                        [doc.0, doc.1]
                    };

                    let dist = ((stabilized_pos[0] - prev_pos[0]).powi(2)
                        + (stabilized_pos[1] - prev_pos[1]).powi(2))
                    .sqrt();
                    let dt = time - prev_time;
                    let speed = if dt > 0.0001 { dist / dt } else { 0.0 };

                    let target_w = if self.tools.brush.brush_type == crate::tools::BrushType::Pen {
                        let max_r = size as f64 / 2.0;
                        let v = if let Some(p) = pressure {
                            p as f64
                        } else {
                            let speed_factor = (speed / 1200.0).clamp(0.0, 1.0);
                            1.0 - speed_factor
                        };
                        let v = v.clamp(0.0, 1.0);
                        let y = (1.0 - v) * max_r;
                        let rr = (max_r * max_r - y * y).max(0.0).sqrt();
                        (rr * 2.0).max(1.0) as f32
                    } else {
                        let base_min = (size * 0.3).max(1.0);
                        let base_max = (size * 2.0).max(4.0);
                        let min_w = base_min + (base_max - base_min) * heavy;
                        let max_w = base_max;
                        let factor = (speed / 1200.0).min(1.0) as f32;
                        max_w - (max_w - min_w) * factor
                    };

                    let pos_smooth = (smoothness.min(0.85) as f64) * 0.85; // less lag than before
                    let smoothed_pos = [
                        prev_pos[0] * pos_smooth + stabilized_pos[0] * (1.0 - pos_smooth),
                        prev_pos[1] * pos_smooth + stabilized_pos[1] * (1.0 - pos_smooth),
                    ];

                    let prev_effective_w = if prev_w < 0.01 { target_w } else { prev_w };
                    let max_change = (dist * 0.3).max(0.5);
                    let delta = target_w - prev_effective_w;
                    let target_w_limited =
                        prev_effective_w + delta.clamp(-max_change as f32, max_change as f32);

                    let alpha_w = (0.3 - 0.28 * smoothness).clamp(0.01, 1.0);
                    let new_w = prev_effective_w * (1.0 - alpha_w) + target_w_limited * alpha_w;

                    // Mid-stroke: one sample only (keeps preview cheap). Dense Catmull
                    // resampling happens on release via densify_brush_centerline.
                    self.tools.brush.points.push((smoothed_pos, time, new_w));
                    // If the frame jumped a long way, add a few mid-points (cap 4) so
                    // release densify has anchors — without bloating live preview.
                    if dist > (size as f64 * 0.5).max(4.0) {
                        let n = ((dist / ((size as f64 * 0.35).max(3.0))).ceil() as usize)
                            .clamp(2, 4);
                        // Replace the single push with subdivided path (rewrite last).
                        self.tools.brush.points.pop();
                        for i in 1..=n {
                            let t = i as f64 / n as f64;
                            let p = [
                                prev_pos[0] + (smoothed_pos[0] - prev_pos[0]) * t,
                                prev_pos[1] + (smoothed_pos[1] - prev_pos[1]) * t,
                            ];
                            let w = prev_effective_w + (new_w - prev_effective_w) * t as f32;
                            let ti = prev_time + (time - prev_time) * t;
                            self.tools.brush.points.push((p, ti, w));
                        }
                    }
                }
            }
        }

        if released {
            if !self.tools.brush.pixel_erase_before.is_empty() {
                self.pixel_erase_commit();
                self.tools.brush.points.clear();
                self.tools.brush.pixel_line_anchor = None;
                return;
            }

            let mut pts = self.tools.brush.points.clone();
            // Catmull-resample freehand centerline so outline isn't a raw frame polyline.
            if matches!(
                self.tools.brush.brush_type,
                crate::tools::BrushType::Standard | crate::tools::BrushType::Calligraphy
            ) && pts.len() >= 3
            {
                pts = densify_brush_centerline(&pts, (self.tools.brush.size as f64 * 0.15).max(1.0));
            }
            if !pts.is_empty()
                && (self.tools.brush.brush_type == crate::tools::BrushType::Pixel
                    || pts.len() >= 2)
            {
                if self.tools.brush.brush_type != crate::tools::BrushType::Calligraphy
                    && self.tools.brush.brush_type != crate::tools::BrushType::Pixel
                {
                    if let Some(last) = pts.last_mut() {
                        last.2 = 0.0;
                    }
                }

                let mut node = if self.tools.brush.brush_type == crate::tools::BrushType::Pixel {
                    let gx = self.viewport.step_x().max(0.5);
                    let gy = self.viewport.step_y().max(0.5);
                    let aspect = gy / gx;
                    let bez = pixel_stamps_to_path(&pts, aspect);
                    let name = if self.tools.brush.pixel_line_anchor.is_some() {
                        "Pixel Line"
                    } else {
                        "Pixel Brush"
                    };
                    Node::path_from_bez(bez, name)
                } else if self.tools.brush.brush_type == crate::tools::BrushType::Pen {
                    let pen_pts = if pts.len() >= 3 {
                        densify_brush_centerline(&pts, (self.tools.brush.size as f64 * 0.12).max(0.8))
                    } else {
                        pts.clone()
                    };
                    Node::new(
                        NodeKind::BrushStroke {
                            points: pen_pts.iter().map(|(p, _, w)| (*p, *w)).collect(),
                        },
                        "Pen Stroke",
                    )
                } else {
                    let bez = generate_brush_outline(
                        &pts,
                        self.tools.brush.smoothness,
                        self.tools.brush.brush_type,
                    );
                    Node::path_from_bez(bez, "Brush")
                };

                node.style.fill = self.build_brush_fill();
                node.style.stroke = Stroke {
                    style: Fill::none(),
                    width: 0.0,
                    line_join: crate::document::LineJoin::Miter,
                    line_cap: crate::document::LineCap::Butt,
                    paint_order: crate::document::StrokePaintOrder::BehindFill,
                    start_marker: crate::document::PathMarker::default(),
                    mid_marker: crate::document::PathMarker::default(),
                    end_marker: crate::document::PathMarker::default(),
                };
                self.insert_node(node);
            }
            self.tools.brush.points.clear();
            self.tools.brush.pixel_line_anchor = None;
        }
    }

    /// Begin a Shift-erase stroke: remember current Path nodes for undo.
    fn pixel_erase_begin(&mut self) {
        self.tools.brush.pixel_erase_before.clear();
        let ids: Vec<NodeId> = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        for id in ids {
            if let Some(n) = self.project.nodes.get(id) {
                if matches!(n.kind, NodeKind::Path { .. }) {
                    self.tools
                        .brush
                        .pixel_erase_before
                        .push((id, n.clone()));
                }
            }
        }
    }

    /// Erase grid stamps under `doc` from path geometry on the active layer.
    fn pixel_erase_at(&mut self, doc: (f64, f64), time: f64) {
        let gx = self.viewport.step_x();
        let gy = self.viewport.step_y();
        let cells = self.tools.brush.pixel_cells;
        let stamps = if let Some(&(p, _, _)) = self.tools.brush.points.last() {
            crate::tools::pixel_stamps_along((p[0], p[1]), doc, gx, gy, cells)
        } else {
            let (cx, cy, w, h) = crate::tools::pixel_stamp_at(doc, gx, gy, cells);
            vec![(cx, cy, w, h)]
        };
        // Track for red erase preview.
        for (cx, cy, w, _h) in &stamps {
            let dup = self.tools.brush.points.iter().any(|&(p, _, _)| {
                (p[0] - cx).abs() < 1e-6 && (p[1] - cy).abs() < 1e-6
            });
            if !dup {
                self.tools
                    .brush
                    .points
                    .push(([*cx, *cy], time, *w as f32));
            }
        }

        let erase_rects: Vec<(f64, f64, f64, f64)> = stamps;
        let ids: Vec<NodeId> = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut empty_ids = Vec::new();
        for id in ids {
            let Some(node) = self.project.nodes.get_mut(id) else {
                continue;
            };
            let NodeKind::Path { path } = &mut node.kind else {
                continue;
            };
            let new_bez = strip_pixel_rects_from_bez(&path.to_bez(), &erase_rects);
            let empty = !new_bez.elements().iter().any(|e| {
                matches!(
                    e,
                    kurbo::PathEl::LineTo(_)
                        | kurbo::PathEl::CurveTo(_, _, _)
                        | kurbo::PathEl::QuadTo(_, _)
                )
            });
            if empty {
                empty_ids.push(id);
            } else {
                *path = crate::document::PathData::from_bez(&new_bez);
            }
        }
        for id in empty_ids {
            let _ = self.project.nodes.remove(id);
            if let Some(layer) = self.project.document.active_layer_mut() {
                layer.nodes.retain(|&n| n != id);
            }
        }
    }

    fn pixel_erase_commit(&mut self) {
        let before = std::mem::take(&mut self.tools.brush.pixel_erase_before);
        if before.is_empty() {
            return;
        }
        let mut patches: Vec<(NodeId, Node, Node)> = Vec::new();
        let mut removed: Vec<(NodeId, Node)> = Vec::new();
        for (id, before_node) in before {
            match self.project.nodes.get(id) {
                Some(after) if after != &before_node => {
                    patches.push((id, before_node, after.clone()));
                }
                None => removed.push((id, before_node)),
                _ => {}
            }
        }
        if patches.is_empty() && removed.is_empty() {
            return;
        }

        // Record undo: restore `before` then let history re-apply current after.
        for (id, before_n, _) in &patches {
            if let Some(n) = self.project.nodes.get_mut(*id) {
                *n = before_n.clone();
            }
        }
        // Put deleted nodes back so RemoveNodes can remove them formally.
        let layer_index = self.project.document.active_layer_index;
        let layer_nodes_before: Vec<NodeId> = {
            // layer order as it was at start of erase is in our before snapshots + survivors.
            // Use current + removed ids for a workable restore.
            let mut order = self
                .project
                .document
                .layers
                .get(layer_index)
                .map(|l| l.nodes.clone())
                .unwrap_or_default();
            for (id, node) in &removed {
                self.project.nodes.insert(node.clone());
                if !order.contains(id) {
                    order.push(*id);
                }
            }
            if let Some(layer) = self.project.document.layers.get_mut(layer_index) {
                layer.nodes = order.clone();
            }
            order
        };

        if !patches.is_empty() {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNodes {
                    patches: patches.clone(),
                },
            );
        }
        if !removed.is_empty() {
            self.history.push(
                &mut self.project,
                ProjectEdit::RemoveNodes {
                    removed,
                    removed_anims: Vec::new(),
                    layer_index,
                    layer_nodes_before,
                    ne_proxy_before: Vec::new(),
                },
            );
        }
        self.status_message = "Erased pixels".into();
    }

    fn hit_path_segment(
        &self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
    ) -> Option<(NodeId, usize, usize, f64, f64)> {
        let threshold_doc = 6.0 / self.viewport.zoom as f64; // tighter to avoid selecting when mouse shifted a bit left or far
        let ids: Vec<NodeId> = if self.selection.is_empty() {
            self.project.document.ordered_node_ids()
        } else {
            self.selection.clone()
        };
        let mut best: Option<(NodeId, usize, usize, f64, f64, f32)> = None;
        for id in ids {
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            let NodeKind::Path { path } = &node.kind else {
                continue;
            };
            let Some((from, to, px, py)) =
                path.hit_segment(doc.0, doc.1, threshold_doc)
            else {
                continue;
            };
            let hit_screen = self.viewport.doc_to_screen((px, py), origin);
            let d = screen.distance(hit_screen);
            if best.as_ref().map_or(true, |(_, _, _, _, _, bd)| d < *bd) {
                best = Some((id, from, to, px, py, d));
            }
        }
        let screen_thresh = 6.0;
        best.filter(|(.., d)| *d <= screen_thresh)
            .map(|(id, from, to, px, py, _)| (id, from, to, px, py))
    }

    fn hit_node_edit(
        &self,
        screen: Pos2,
        origin: Pos2,
    ) -> Option<(NodeId, PathEditTarget)> {
        let anchor_threshold = 7.0; // tighter selection to prevent picking left/nearby objects when mouse shifted
        let handle_threshold = 9.0;
        let mut best: Option<(NodeId, PathEditTarget, f32)> = None;
        let ids: Vec<NodeId> = if self.selection.is_empty() {
            self.project.document.ordered_node_ids()
        } else {
            self.selection.clone()
        };
        for id in ids {
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            for (target, p) in node.path_edit_targets() {
                let threshold = match target {
                    PathEditTarget::Anchor(_) => anchor_threshold,
                    PathEditTarget::HandleOut(_) | PathEditTarget::HandleIn(_) => {
                        handle_threshold
                    }
                    PathEditTarget::MidCtrl1(_) | PathEditTarget::MidCtrl2(_) => 15.0, // easier to hit the yellow tangent points; zoom-robust screen px
                };
                let ps = self.viewport.doc_to_screen(p, origin);
                let d = screen.distance(ps);
                if d < threshold {
                    let prefer = matches!(
                        target,
                        PathEditTarget::HandleOut(_) | PathEditTarget::HandleIn(_) | PathEditTarget::MidCtrl1(_) | PathEditTarget::MidCtrl2(_)
                    );
                    let replace = best.as_ref().map_or(true, |(_, bt, bd)| {
                        if prefer && !matches!(bt, PathEditTarget::Anchor(_)) {
                            d < *bd
                        } else if prefer {
                            true
                        } else {
                            d < *bd
                        }
                    });
                    if replace {
                        best = Some((id, target, d));
                    }
                }
            }
        }
        best.map(|(id, target, _)| (id, target))
    }

    fn tool_node(
        &mut self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
        shift: bool,
        ctrl: bool,
        pressed: bool,
        down: bool,
        released: bool,
        _released_anywhere: bool,
        double_clicked: bool,
    ) {
        if released {
            if let Some(m) = self.tools.select.marquee.take() {
                if tools::marquee_is_drag(m.origin_doc, m.current_doc) {
                    let rect = tools::marquee_rect(m.origin_doc, m.current_doc);
                    let mut newly_selected = Vec::new();
                    for id in &self.selection {
                        if let Some(node) = self.project.nodes.get(*id) {
                            if let NodeKind::Path { path } = &node.kind {
                                let anchors = path.anchor_positions();
                                for (idx, &pos) in anchors.iter().enumerate() {
                                    let pt = kurbo::Point::new(pos.0, pos.1);
                                    if rect.contains(pt) {
                                        newly_selected.push((*id, idx));
                                    }
                                }
                            }
                        }
                    }
                    if m.shift {
                        for item in newly_selected {
                            if !self.tools.select.selected_path_points.contains(&item) {
                                self.tools.select.selected_path_points.push(item);
                            }
                        }
                    } else {
                        self.tools.select.selected_path_points = newly_selected;
                    }
                } else if !m.shift {
                    self.tools.select.clear_path_point_selection();
                }
                self.sync_inspector_from_selection();
                return;
            }

            if !self.tools.select.drag_snapshot.is_empty() {
                self.commit_drag_edits();
                self.tools.select.node_drag_origin = None;
                self.tools.select.node_drag_active = false;
                return;
            }
        }

        if double_clicked {
            self.tools.select.drag_snapshot.clear();
            self.tools.select.node_edit_target = None;
            self.tools.select.node_drag_origin = None;
            self.tools.select.node_drag_active = false;
            if let Some((id, from, to, px, py)) = self.hit_path_segment(screen, origin, doc) {
                let Some(before) = self.project.nodes.get(id).cloned() else {
                    return;
                };
                let mut after = before.clone();
                if let NodeKind::Path { path } = &mut after.kind {
                    let anchor_count = path.anchor_positions().len();
                    let new_idx = if to > from { to } else { anchor_count };
                    path.insert_anchor_on_segment(from, to, px, py);
                    self.history.push(
                        &mut self.project,
                        ProjectEdit::PatchNode { id, before, after },
                    );
                    self.tools.select.set_path_segment(id, from, new_idx);
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                    self.status_message = "Added point on path".into();
                }
                return;
            }
            if let Some((id, PathEditTarget::Anchor(pi))) = self.hit_node_edit(screen, origin) {
                if self.project.nodes.get(id).is_some_and(|n| matches!(n.kind, NodeKind::Path { .. })) {
                    self.set_path_anchor_smooth(id, pi, {
                        self.project
                            .nodes
                            .get(id)
                            .and_then(|n| match &n.kind {
                                NodeKind::Path { path } => {
                                    Some(!path.is_anchor_smooth(pi))
                                }
                                _ => None,
                            })
                            .unwrap_or(true)
                    });
                    self.tools.select.set_single_path_point(id, pi);
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                }
            }
            return;
        }

        if pressed {
            if let Some((id, target)) = self.hit_node_edit(screen, origin) {
                let pi = target.anchor_index();
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                    self.sync_inspector_from_selection();
                }
                if self
                    .project
                    .nodes
                    .get(id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                {
                    if matches!(target, PathEditTarget::Anchor(_)) {
                        let is_already_selected = self.tools.select.selected_path_points.iter().any(|&(sid, idx)| sid == id && idx == pi);
                        if !is_already_selected || ctrl || shift {
                            self.tools.select.toggle_path_point(id, pi, ctrl || shift);
                        }
                    } else {
                        self.tools.select.set_single_path_point(id, pi);
                    }
                } else {
                    self.tools.select.clear_path_point_selection();
                }
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                let Some(node) = self.project.nodes.get(id) else {
                    return;
                };
                self.tools.select.drag_snapshot = vec![(id, node.clone())];
                self.tools.select.drag_mode = Some(SelectDrag::Move);
                self.tools.select.node_edit_target = Some(target);
                self.tools.select.node_drag_origin = Some(doc);
                self.tools.select.last_doc = doc;
                self.tools.select.node_drag_active = false;
                return;
            }

            if let Some((id, from, to, _, _)) = self.hit_path_segment(screen, origin, doc) {
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                    self.sync_inspector_from_selection();
                }
                self.tools.select.set_path_segment(id, from, to);
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                return;
            }

            let slop = 16.0 / self.viewport.zoom as f64;
            let (mut hit, bbox_only) = self.pick_node_at_with_bbox_fallback(doc, slop);
            if hit.is_none() {
                hit = bbox_only;
            }
            if let Some(id) = hit {
                if shift {
                    if self.selection.contains(&id) {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection.push(id);
                    }
                } else if self.selection.len() == 1 && self.selection[0] == id {
                    self.tools.select.clear_path_point_selection();
                    self.tools.select.selected_path_segment = None;
                } else {
                    self.selection = vec![id];
                    self.tools.select.clear_path_point_selection();
                    self.tools.select.selected_path_segment = None;
                }
                self.sync_inspector_from_selection();
            } else {
                if !shift {
                    self.tools.select.clear_path_point_selection();
                }
                self.tools.select.marquee = Some(MarqueeSelect {
                    origin_doc: doc,
                    current_doc: doc,
                    shift,
                });
            }
            self.tools.select.last_doc = doc;
            return;
        }

        if down {
            if let Some(marquee) = self.tools.select.marquee.as_mut() {
                marquee.current_doc = doc;
            } else if let (Some(drag_first), Some(target)) = (
                self.tools.select.drag_snapshot.first().cloned(),
                self.tools.select.node_edit_target,
            ) {
                let (id, _) = drag_first;
                let threshold = 3.0 / self.viewport.zoom as f64;
                if let Some(origin) = self.tools.select.node_drag_origin {
                    if !self.tools.select.node_drag_active {
                        let moved = (doc.0 - origin.0).hypot(doc.1 - origin.1);
                        if moved < threshold {
                            return;
                        }
                        self.tools.select.node_drag_active = true;
                    }
                }
                if self.tools.select.node_drag_active {
                    let mut use_doc = doc;
                    // Apply grid snap for bezier handle / anchor interaction.
                    // Skip snap for yellow fillet points (MidCtrl) to allow precise radius adjust, esp. when zoomed in.
                    let is_yellow = matches!(target, PathEditTarget::MidCtrl1(_) | PathEditTarget::MidCtrl2(_));
                    if self.viewport.snap_grid && !is_yellow {
                        let g = self.viewport.grid_step as f64;
                        if g > 0.0 {
                            use_doc = (
                                (doc.0 / g).round() * g,
                                (doc.1 / g).round() * g,
                            );
                        }
                    }
                    // Extend magnetic (object/canvas) snap to edit mode (esp. path anchors/handles)
                    if self.snap_magnet && !is_yellow {
                        if matches!(target, PathEditTarget::Anchor(_) | PathEditTarget::HandleIn(_) | PathEditTarget::HandleOut(_)) {
                            use_doc = self.snap_cursor(use_doc);
                        }
                    }
                    // Ctrl: 15° angle lock when editing a 2-point path (line), relative to the other end.
                    if ctrl && matches!(target, PathEditTarget::Anchor(_)) && !is_yellow {
                        if let Some(node) = self.project.nodes.get(id) {
                            if let NodeKind::Path { path } = &node.kind {
                                let anchors = path.anchor_positions();
                                if anchors.len() == 2 {
                                    let pi = target.anchor_index();
                                    if let Some(&other) = anchors.get(1 - pi.min(1)) {
                                        use_doc = tools::snap_angle_15deg(other, use_doc);
                                    }
                                }
                            } else if let NodeKind::FlowchartPath { path } = &node.kind {
                                if path.points.len() == 2 {
                                    let pi = target.anchor_index();
                                    if let Some(&other) = path.points.get(1 - pi.min(1)) {
                                        use_doc = tools::snap_angle_15deg(other, use_doc);
                                    }
                                }
                            }
                        }
                    }
                    let indices = self.tools.select.points_on_path(id);
                    if matches!(target, PathEditTarget::Anchor(_)) && indices.len() > 1 {
                        let dx = use_doc.0 - self.tools.select.last_doc.0;
                        let dy = use_doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = use_doc;
                        if let Some(node) = self.project.nodes.get_mut(id) {
                            if let NodeKind::Path { path } = &mut node.kind {
                                path.move_anchors_by(&indices, dx, dy);
                            }
                        }
                    } else if let Some(node) = self.project.nodes.get_mut(id) {
                        node.apply_path_edit_target(target, use_doc.0, use_doc.1);
                        self.tools.select.last_doc = use_doc;
                    }

                    let is_flowchart_path = self.project.nodes.get(id).map_or(false, |n| matches!(n.kind, NodeKind::FlowchartPath { .. }));
                    if is_flowchart_path {
                        let active_idx = self.project.document.active_layer_index;
                        if let Some(layer) = self.project.document.layers.get(active_idx) {
                            let mut snap_start_node = None;
                            let mut snap_start_anchor = None;
                            let mut snap_start_pt = None;
                            
                            let mut snap_end_node = None;
                            let mut snap_end_anchor = None;
                            let mut snap_end_pt = None;
                            
                            if let Some(node) = self.project.nodes.get(id) {
                                if let NodeKind::FlowchartPath { path } = &node.kind {
                                    if let PathEditTarget::Anchor(idx) = target {
                                        let store = &self.project.nodes;
                                        let anchor_slop = 24.0f64;
                                        
                                        if idx == 0 {
                                            let mut best_start_d = anchor_slop;
                                            for &nid in &layer.nodes {
                                                if nid == id { continue; }
                                                if let Some(nd) = store.get(nid) {
                                                    if let Some(geom) = crate::document::flowchart::node_as_flowchart_geom(&nd.kind) {
                                                        let anc_s = crate::document::flowchart::snap_anchor_for_point(&geom, path.points[0]);
                                                        let ap_s = geom.anchor_position(anc_s);
                                                        let ds = (ap_s.0 - path.points[0].0).hypot(ap_s.1 - path.points[0].1);
                                                        if ds < best_start_d {
                                                            snap_start_node = Some(nid);
                                                            snap_start_anchor = Some(anc_s);
                                                            snap_start_pt = Some(ap_s);
                                                            best_start_d = ds;
                                                        }
                                                    }
                                                }
                                            }
                                        } else if idx == path.points.len() - 1 {
                                            let mut best_end_d = anchor_slop;
                                            for &nid in &layer.nodes {
                                                if nid == id { continue; }
                                                if let Some(nd) = store.get(nid) {
                                                    if let Some(geom) = crate::document::flowchart::node_as_flowchart_geom(&nd.kind) {
                                                        let anc_e = crate::document::flowchart::snap_anchor_for_point(&geom, path.points[idx]);
                                                        let ap_e = geom.anchor_position(anc_e);
                                                        let de = (ap_e.0 - path.points[idx].0).hypot(ap_e.1 - path.points[idx].1);
                                                        if de < best_end_d {
                                                            snap_end_node = Some(nid);
                                                            snap_end_anchor = Some(anc_e);
                                                            snap_end_pt = Some(ap_e);
                                                            best_end_d = de;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            
                            if let Some(node) = self.project.nodes.get_mut(id) {
                                if let NodeKind::FlowchartPath { path } = &mut node.kind {
                                    if let PathEditTarget::Anchor(idx) = target {
                                        if idx == 0 {
                                            path.start_node = snap_start_node;
                                            path.start_anchor = snap_start_anchor;
                                            if let Some(pt) = snap_start_pt {
                                                path.points[0] = pt;
                                            }
                                        } else if idx == path.points.len() - 1 {
                                            path.end_node = snap_end_node;
                                            path.end_anchor = snap_end_anchor;
                                            if let Some(pt) = snap_end_pt {
                                                path.points[idx] = pt;
                                            }
                                        }
                                    }
                                }
                            }
                            self.sync_flowchart_paths_if_active_layer();
                        }
                    }
                }
            }
        }
    }

    fn mcp_kind_label(kind: &crate::document::NodeKind) -> &'static str {
        match kind {
            crate::document::NodeKind::Rect { .. } => "rect",
            crate::document::NodeKind::Ellipse { .. } => "ellipse",
            crate::document::NodeKind::Polygon { .. } => "polygon",
            crate::document::NodeKind::Path { .. } => "path",
            crate::document::NodeKind::FlowchartPath { .. } => "flowchart_path",
            crate::document::NodeKind::FlowchartNode { .. } => "flowchart_node",
            crate::document::NodeKind::Text { .. } => "text",
            crate::document::NodeKind::Group { .. } => "group",
            crate::document::NodeKind::Image { .. } => "image",
            crate::document::NodeKind::Plotter { .. } => "plotter",
            crate::document::NodeKind::Arc { .. } => "arc",
            crate::document::NodeKind::BrushStroke { .. } => "brush",
        }
    }


    fn mcp_paint_hex(node: &crate::document::Node) -> Option<String> {
        use crate::document::Fill;
        if let Fill::Solid(p) = node.style.fill {
            let r = (p.rgba[0] * 255.0).round() as u32;
            let g = (p.rgba[1] * 255.0).round() as u32;
            let b = (p.rgba[2] * 255.0).round() as u32;
            return Some(format!("#{:02X}{:02X}{:02X}", r, g, b));
        }
        None
    }

    fn mcp_truncate_str(s: &str, max_chars: usize) -> String {
        // Byte pre-cap avoids scanning multi-megabyte text just to truncate a preview.
        let s = if s.len() > max_chars.saturating_mul(4).max(256) {
            match s.char_indices().nth(max_chars) {
                Some((i, _)) => &s[..i],
                None => s,
            }
        } else {
            s
        };
        let mut it = s.chars();
        let head: String = it.by_ref().take(max_chars.saturating_sub(1)).collect();
        if it.next().is_some() {
            format!("{head}…")
        } else {
            head
        }
    }

    fn mcp_list_all_objects_json(&self) -> Result<String, String> {
        let mut items = Vec::new();
        for (layer_idx, layer) in self.project.document.layers.iter().enumerate() {
            if !layer.visible || !layer.is_renderer || layer.kind != crate::document::LayerKind::Image {
                continue;
            }
            let layer_editable = !layer.locked;
            for id in &layer.nodes {
                let Some(node) = self.project.nodes.get(*id) else { continue };
                let b = node.bounds();
                items.push(serde_json::json!({
                    "id": id.to_string(),
                    "layer_index": layer_idx,
                    "layer_name": Self::mcp_truncate_str(&layer.name, 64),
                    "name": Self::mcp_truncate_str(&node.name, 64),
                    "kind": Self::mcp_kind_label(&node.kind),
                    "editable": layer_editable,
                    "bounds": { "x": b.x0, "y": b.y0, "w": b.width(), "h": b.height() },
                    "transform": {
                        "translate_x": node.transform.translation[0],
                        "translate_y": node.transform.translation[1],
                        "scale_x": node.transform.scale[0],
                        "scale_y": node.transform.scale[1],
                        "rotation_deg": node.transform.rotation_rad.to_degrees(),
                    },
                    "opacity": node.style.opacity,
                    "fill_color": Self::mcp_paint_hex(node),
                    "stroke_width": node.style.stroke.width,
                }));
            }
        }
        serde_json::to_string_pretty(&items).map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "android"))]
    fn mcp_capture_canvas_raster(
        &mut self,
        resolution_percent: f32,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
        save_path: Option<String>,
    ) -> Result<crate::mcp::McpHostResponse, String> {
        use base64::Engine;
        let view = io::resolve_capture_view(&self.project, x, y, w, h);
        let pct = resolution_percent.clamp(1.0, 100.0);
        let (pw, ph, mut rgba) = io::rasterize_document_view(
            &self.project,
            view,
            pct,
            self.anim_current_frame,
            &std::collections::HashMap::new(),
        )
        .ok_or("Rasterize failed (empty region or SVG error)")?;
        // CPU raster only knows named presets (galaxy/starfield/blackhole). Custom WGSL
        // was left as page color (white). GPU-composite procedural shading when possible.
        let mut shading_gpu = false;
        if let Some(rs) = self.wgpu_render.as_ref() {
            // Wall-clock so time-based wave isn't frozen at frame 0.
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f32())
                .unwrap_or(0.0);
            shading_gpu = crate::shading::composite_shading_layers_into_rgba(
                rs,
                &self.project,
                &mut rgba,
                pw,
                ph,
                t,
            );
        }
        if let Some(path) = save_path {
            let p = std::path::PathBuf::from(path);
            io::write_image_file(&p, io::ExportImageFormat::Png, pw, ph, &rgba)
                .map_err(|e| e.to_string())?;
        }
        let png = image::RgbaImage::from_raw(pw, ph, rgba.clone())
            .ok_or("Invalid RGBA buffer")?;
        let mut buf = Vec::new();
        png.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        self.mcp_preview.rgba = rgba;
        self.mcp_preview.width = pw;
        self.mcp_preview.height = ph;
        self.mcp_preview.bounds = [view.x0, view.y0, view.width(), view.height()];
        self.mcp_preview.resolution_percent = pct;
        self.mcp_preview.texture = None;
        self.mcp_preview.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let meta = serde_json::json!({
            "pixel_width": pw,
            "pixel_height": ph,
            "resolution_percent": pct,
            "shading_gpu": shading_gpu,
            "bounds": {
                "x": view.x0,
                "y": view.y0,
                "w": view.width(),
                "h": view.height(),
            },
            "document_size": {
                "w": self.project.document.width,
                "h": self.project.document.height,
            },
            "objects_remain_editable": true,
            "hint": "Use list_all_objects + get_object/update_object/set_object_* to edit vectors after preview. shading_gpu=true means custom WGSL was GPU-composited."
        });
        Ok(crate::mcp::McpHostResponse::RasterPreview {
            meta_json: serde_json::to_string_pretty(&meta).unwrap_or_default(),
            png_base64: b64,
        })
    }

    fn mcp_list_objects_json(&self) -> Result<String, String> {
        let layer = self
            .project
            .document
            .active_layer()
            .ok_or("No active layer")?;
        let layer_editable = !layer.locked && layer.visible;
        let mut items = Vec::new();
        for id in &layer.nodes {
            let Some(node) = self.project.nodes.get(*id) else {
                continue;
            };
            let b = node.bounds();
            let mut item = serde_json::json!({
                "id": id.to_string(),
                "name": Self::mcp_truncate_str(&node.name, 64),
                "kind": Self::mcp_kind_label(&node.kind),
                "editable": layer_editable,
                "fill_color": Self::mcp_paint_hex(node),
                "bounds": {
                    "x": b.x0,
                    "y": b.y0,
                    "w": b.width(),
                    "h": b.height(),
                }
            });
            // Never dump multi-megabyte text content into list responses.
            if let crate::document::NodeKind::Text { style, .. } = &node.kind {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("text_bytes".into(), serde_json::json!(style.content.len()));
                    obj.insert(
                        "text_preview".into(),
                        serde_json::json!(Self::mcp_truncate_str(&style.content, 48)),
                    );
                }
            }
            items.push(item);
        }
        serde_json::to_string_pretty(&items).map_err(|e| e.to_string())
    }

    fn mcp_get_object_json(&self, id_str: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let node = self
            .project
            .nodes
            .get(id)
            .ok_or_else(|| format!("Object not found: {id_str}"))?
            .clone();
        let mut value = serde_json::to_value(&node).map_err(|e| e.to_string())?;
        if let Some(obj) = value.as_object_mut() {
            if let Some(kind) = obj.get_mut("kind") {
                if let Some(img) = kind.get_mut("Image") {
                    if let Some(bytes) = img.get_mut("bytes") {
                        if let Some(n) = bytes.as_array().map(|a| a.len()) {
                            *bytes = serde_json::json!(format!("<{n} bytes omitted>"));
                        }
                    }
                }
            }
        }
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
    }


    fn mcp_ensure_editable(&self) -> Result<(), String> {
        if !self.layer_editable() {
            return Err("Active layer is locked or hidden".into());
        }
        Ok(())
    }

    fn mcp_finish_node(&mut self, mut node: crate::document::Node, style: &crate::mcp::drawing::McpShapeStyle) {
        if let Some(n) = style.name.clone() {
            node.name = n;
        }
        // Always apply stroke settings so users can remove stroke by passing stroke_width:0 (and optionally stroke_alpha:0)
        // even without specifying a stroke_color. Text and rects will get clean solid fill only when requested.
        node.style.stroke = crate::mcp::drawing::stroke_from_style(style);
        let id = node.id;
        self.insert_node(node);
        let _ = id;
    }

    /// Resolve Node Editor layer index from optional layer_id / layer_index.
    fn mcp_resolve_ne_layer_idx(&self, args: &serde_json::Value) -> Result<usize, String> {
        if let Some(id_str) = args.get("layer_id").and_then(|v| v.as_str()) {
            let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
            return self
                .project
                .document
                .layers
                .iter()
                .position(|l| l.id == id)
                .ok_or_else(|| format!("Layer {id_str} not found"));
        }
        if let Some(i) = args.get("layer_index").and_then(|v| v.as_u64()) {
            let i = i as usize;
            if i >= self.project.document.layers.len() {
                return Err(format!("layer_index {i} out of range"));
            }
            return Ok(i);
        }
        let active = self.project.document.active_layer_index;
        if self
            .project
            .document
            .layers
            .get(active)
            .is_some_and(|l| l.kind == crate::document::LayerKind::NodeEditor)
        {
            return Ok(active);
        }
        self.project
            .document
            .layers
            .iter()
            .position(|l| l.kind == crate::document::LayerKind::NodeEditor)
            .ok_or_else(|| {
                "No Node Editor layer — use add_node_editor_layer first".into()
            })
    }

    fn mcp_with_node_graph_mut<R>(
        &mut self,
        args: &serde_json::Value,
        f: impl FnOnce(&mut crate::document::NodeGraph, usize, uuid::Uuid) -> Result<R, String>,
    ) -> Result<R, String> {
        let idx = self.mcp_resolve_ne_layer_idx(args)?;
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let layer = after
            .layers
            .get_mut(idx)
            .ok_or("Layer missing")?;
        if layer.kind != crate::document::LayerKind::NodeEditor {
            return Err("Layer is not a Node Editor layer".into());
        }
        layer.ensure_node_graph();
        let layer_id = layer.id;
        let g = layer
            .node_graph
            .as_mut()
            .ok_or("Node graph missing")?;
        let result = f(g, idx, layer_id)?;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        Ok(result)
    }

    fn mcp_node_editor_tool(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        use crate::mcp::node_editor as ne;
        match name {
            "list_graph_node_kinds" => Ok(ne::list_kinds_json()),
            "add_node_editor_layer" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Node Editor");
                self.add_node_editor_layer(name);
                let idx = self.project.document.active_layer_index;
                let id = self
                    .project
                    .document
                    .layers
                    .get(idx)
                    .map(|l| l.id.to_string())
                    .unwrap_or_default();
                Ok(format!(
                    "Added Node Editor layer \"{name}\" id={id} index={idx}"
                ))
            }
            "open_node_editor" => {
                let idx = self.mcp_resolve_ne_layer_idx(args)?;
                let layer = self
                    .project
                    .document
                    .layers
                    .get(idx)
                    .ok_or("Layer missing")?;
                if layer.kind != crate::document::LayerKind::NodeEditor {
                    return Err("Not a Node Editor layer".into());
                }
                let lid = layer.id;
                self.project.document.active_layer_index = idx;
                self.selection = vec![lid];
                self.node_editor_ui.open(lid);
                Ok(format!("Opened Node Editor for layer {lid}"))
            }
            "list_graph_nodes" => {
                let idx = self.mcp_resolve_ne_layer_idx(args)?;
                let layer = self
                    .project
                    .document
                    .layers
                    .get(idx)
                    .ok_or("Layer missing")?;
                let g = layer
                    .node_graph
                    .as_ref()
                    .ok_or("No node graph on layer")?;
                let nodes: Vec<_> = g
                    .nodes
                    .values()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.to_string(),
                            "name": n.name,
                            "kind": ne::kind_label(&n.kind),
                            "title": n.kind.default_title(),
                            "x": n.x,
                            "y": n.y,
                            "ports": ne::ports_json(&n.kind),
                            "fields": ne::node_fields_json(&n.kind),
                            "error": n.error,
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&serde_json::json!({
                    "layer_id": layer.id.to_string(),
                    "layer_index": idx,
                    "layer_name": layer.name,
                    "output_node_id": g.output_node_id.map(|id| id.to_string()),
                    "node_count": nodes.len(),
                    "nodes": nodes,
                }))
                .map_err(|e| e.to_string())
            }
            "list_graph_links" => {
                let idx = self.mcp_resolve_ne_layer_idx(args)?;
                let layer = self
                    .project
                    .document
                    .layers
                    .get(idx)
                    .ok_or("Layer missing")?;
                let g = layer
                    .node_graph
                    .as_ref()
                    .ok_or("No node graph on layer")?;
                let links: Vec<_> = g
                    .links
                    .iter()
                    .map(|l| {
                        serde_json::json!({
                            "id": l.id.to_string(),
                            "from_node": l.from_node.to_string(),
                            "from_port": l.from_port,
                            "to_node": l.to_node.to_string(),
                            "to_port": l.to_port,
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&serde_json::json!({
                    "layer_id": layer.id.to_string(),
                    "link_count": links.len(),
                    "links": links,
                }))
                .map_err(|e| e.to_string())
            }
            "get_graph_output" => {
                let idx = self.mcp_resolve_ne_layer_idx(args)?;
                let layer = self
                    .project
                    .document
                    .layers
                    .get(idx)
                    .ok_or("Layer missing")?;
                let g = layer
                    .node_graph
                    .as_ref()
                    .ok_or("No node graph on layer")?;
                let eval = g.resolve_output_image();
                let image = match &eval.image {
                    crate::document::GraphImageSource::Empty => {
                        serde_json::json!({ "type": "empty" })
                    }
                    crate::document::GraphImageSource::FilePath(p) => {
                        serde_json::json!({ "type": "file", "path": p })
                    }
                    crate::document::GraphImageSource::AppObjects(ids) => {
                        serde_json::json!({
                            "type": "app_objects",
                            "ids": ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                        })
                    }
                };
                let snd = g.resolve_output_sound();
                let sound = match &snd.sound {
                    crate::document::GraphSoundSource::Empty => {
                        serde_json::json!({ "type": "empty" })
                    }
                    crate::document::GraphSoundSource::FilePath(p) => {
                        serde_json::json!({ "type": "file", "path": p })
                    }
                };
                serde_json::to_string_pretty(&serde_json::json!({
                    "layer_id": layer.id.to_string(),
                    "image": image,
                    "sound": sound,
                    "sound_volume": snd.volume,
                    "sound_media_time_sec": snd.media_time_sec,
                    "sound_playback_rate": snd.playback_rate,
                    "sound_eq_bass": snd.eq_bass,
                    "sound_eq_mid": snd.eq_mid,
                    "sound_eq_treble": snd.eq_treble,
                    "blur_px": eval.blur_px,
                    "brightness": eval.brightness,
                    "contrast": eval.contrast,
                    "saturation": eval.saturation,
                    "hue_shift": eval.hue_shift,
                    "geo_off_x": eval.geo_off_x,
                    "geo_off_y": eval.geo_off_y,
                    "geo_rot_deg": eval.geo_rot_deg,
                    "geo_scale_w": eval.geo_scale_w,
                    "geo_scale_h": eval.geo_scale_h,
                    "effects_on_path": eval.effects_on_path,
                    "root_error": g.root_error,
                }))
                .map_err(|e| e.to_string())
            }
            "add_graph_node" => {
                let kind_str = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or("kind required")?;
                let mut kind = ne::kind_from_args(kind_str, args)?;
                let (kind2, param) = ne::attach_param(kind, args)?;
                kind = kind2;
                let x = args.get("x").and_then(|v| v.as_f64()).map(|v| v as f32);
                let y = args.get("y").and_then(|v| v.as_f64()).map(|v| v as f32);
                let name_opt = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let id = self.mcp_with_node_graph_mut(args, |g, _idx, _lid| {
                    let nx = x.unwrap_or_else(|| {
                        40.0 + (g.nodes.len() as f32 % 5.0) * 180.0
                    });
                    let ny = y.unwrap_or_else(|| {
                        40.0 + (g.nodes.len() as f32 / 5.0).floor() * 100.0
                    });
                    if let Some(p) = param {
                        g.parameters.push(p);
                    }
                    let nid = g.add_node(kind, nx, ny);
                    if let Some(n) = name_opt {
                        if let Some(node) = g.nodes.get_mut(&nid) {
                            node.name = n;
                        }
                    }
                    Ok(nid)
                })?;
                Ok(format!(
                    "Added graph node {id} kind={kind_str}"
                ))
            }
            "edit_graph_node" => {
                let node_id = args
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .ok_or("node_id required")?;
                let nid = uuid::Uuid::parse_str(node_id).map_err(|e| e.to_string())?;
                self.mcp_with_node_graph_mut(args, |g, _, _| {
                    let node = g
                        .nodes
                        .get_mut(&nid)
                        .ok_or_else(|| format!("Graph node {node_id} not found"))?;
                    if let Some(x) = args.get("x").and_then(|v| v.as_f64()) {
                        node.x = x as f32;
                    }
                    if let Some(y) = args.get("y").and_then(|v| v.as_f64()) {
                        node.y = y as f32;
                    }
                    if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                        node.name = n.to_string();
                    }
                    match &mut node.kind {
                        crate::document::GraphNodeKind::Value { value } => {
                            if let Some(v) = args.get("value").and_then(|x| x.as_f64()) {
                                *value = v;
                            }
                        }
                        crate::document::GraphNodeKind::ExprX { expr }
                        | crate::document::GraphNodeKind::ExprXy { expr }
                        | crate::document::GraphNodeKind::ExprXyz { expr } => {
                            if let Some(e) = args.get("expr").and_then(|x| x.as_str()) {
                                *expr = e.to_string();
                            }
                        }
                        crate::document::GraphNodeKind::ObjectImage { path }
                        | crate::document::GraphNodeKind::ObjectVideo { path }
                        | crate::document::GraphNodeKind::ObjectAudio { path }
                        | crate::document::GraphNodeKind::ObjectSeptic { path }
                        | crate::document::GraphNodeKind::ObjectMouse { path } => {
                            if let Some(p) = args.get("path").and_then(|x| x.as_str()) {
                                *path = p.to_string();
                            }
                        }
                        crate::document::GraphNodeKind::ObjectFromApp { node_ids } => {
                            if args.get("app_object_ids").is_some() {
                                *node_ids = ne::parse_uuid_list(args.get("app_object_ids"));
                            }
                        }
                        crate::document::GraphNodeKind::MouseEncoder {
                            time_threshold,
                            gain,
                        } => {
                            if let Some(v) = args
                                .get("time_threshold")
                                .or_else(|| args.get("threshold"))
                                .and_then(|x| x.as_f64())
                            {
                                *time_threshold = v.clamp(0.001, 5.0);
                            }
                            if let Some(v) = args.get("gain").and_then(|x| x.as_f64()) {
                                *gain = v.clamp(0.0, 64.0);
                            }
                        }
                        crate::document::GraphNodeKind::Visualizer { gain } => {
                            if let Some(v) = args.get("gain").and_then(|x| x.as_f64()) {
                                *gain = v.max(0.0);
                            }
                        }
                        _ => {}
                    }
                    // Sync param real default value when editing ParamReal via value + parameters list
                    if let crate::document::GraphNodeKind::ParamReal { param_id } = node.kind {
                        if let Some(v) = args.get("value").and_then(|x| x.as_f64()) {
                            if let Some(p) = g.parameters.iter_mut().find(|p| p.id == param_id) {
                                p.v0 = v;
                            }
                        }
                    }
                    Ok(())
                })?;
                Ok(format!("Updated graph node {node_id}"))
            }
            "remove_graph_node" => {
                let node_id = args
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .ok_or("node_id required")?;
                let nid = uuid::Uuid::parse_str(node_id).map_err(|e| e.to_string())?;
                self.mcp_with_node_graph_mut(args, |g, _, _| {
                    if !g.nodes.contains_key(&nid) {
                        return Err(format!("Graph node {node_id} not found"));
                    }
                    g.remove_node(nid);
                    Ok(())
                })?;
                Ok(format!("Removed graph node {node_id}"))
            }
            "connect_graph_nodes" => {
                let from = args
                    .get("from_node")
                    .and_then(|v| v.as_str())
                    .ok_or("from_node required")?;
                let to = args
                    .get("to_node")
                    .and_then(|v| v.as_str())
                    .ok_or("to_node required")?;
                let from_id = uuid::Uuid::parse_str(from).map_err(|e| e.to_string())?;
                let to_id = uuid::Uuid::parse_str(to).map_err(|e| e.to_string())?;
                let from_port_raw = args
                    .get("from_port")
                    .and_then(|v| v.as_str())
                    .unwrap_or("out");
                let to_port_raw = args
                    .get("to_port")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image");
                let (from_port, to_port) = self.mcp_with_node_graph_mut(args, |g, _, _| {
                    let from_port = ne::resolve_port_id(g, from_id, from_port_raw, true)?;
                    let to_port = ne::resolve_port_id(g, to_id, to_port_raw, false)?;
                    g.try_add_link(from_id, &from_port, to_id, &to_port)?;
                    Ok((from_port, to_port))
                })?;
                Ok(format!(
                    "Connected {from}:{from_port} → {to}:{to_port}"
                ))
            }
            "disconnect_graph_link" => {
                let link_id = args
                    .get("link_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok());
                let to_node = args
                    .get("to_node")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok());
                let to_port = args.get("to_port").and_then(|v| v.as_str());
                if link_id.is_none() && to_node.is_none() {
                    return Err("Provide link_id or to_node (+ optional to_port)".into());
                }
                let removed = self.mcp_with_node_graph_mut(args, |g, _, _| {
                    let before = g.links.len();
                    if let Some(lid) = link_id {
                        g.links.retain(|l| l.id != lid);
                    } else if let Some(tn) = to_node {
                        if let Some(tp) = to_port {
                            g.links
                                .retain(|l| !(l.to_node == tn && l.to_port == tp));
                        } else {
                            g.links.retain(|l| l.to_node != tn);
                        }
                    }
                    Ok(before.saturating_sub(g.links.len()))
                })?;
                Ok(format!("Removed {removed} connector(s)"))
            }
            _ => Err(format!("unknown node editor tool: {name}")),
        }
    }

    /// MCP brush: pixel grid stamps, pen stroke, or soft brush outline (mirrors in-app brush).
    fn mcp_brush(
        &mut self,
        args: &serde_json::Value,
        style: &crate::mcp::drawing::McpShapeStyle,
    ) -> Result<String, String> {
        use crate::document::{Fill, Node, NodeKind, Paint, Stroke};
        use crate::mcp::drawing::{fill_from_style, parse_color_value, stroke_from_style};
        use std::collections::BTreeMap;

        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("pixel")
            .to_ascii_lowercase();
        let erase = args
            .get("erase")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let cells = args
            .get("cells")
            .and_then(|v| v.as_u64())
            .map(|c| c as u32)
            .unwrap_or(self.tools.brush.pixel_cells.max(1))
            .max(1);
        let size = args
            .get("size")
            .and_then(|v| v.as_f64())
            .map(|s| s as f32)
            .unwrap_or(self.tools.brush.size.max(1.0))
            .max(0.5);
        let smoothness = args
            .get("smoothness")
            .and_then(|v| v.as_f64())
            .map(|s| s as f32)
            .unwrap_or(self.tools.brush.smoothness)
            .clamp(0.0, 1.0);

        let gx = args
            .get("step_x")
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| self.viewport.step_x())
            .max(0.5);
        let gy = args
            .get("step_y")
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| self.viewport.step_y())
            .max(0.5);

        let default_fill = fill_from_style(style);
        let default_rgb = style.fill_rgb.unwrap_or(0x5b8def);
        let default_alpha = style.fill_alpha;

        // (cx, cy, w, h, rgb, alpha)
        let mut colored_stamps: Vec<(f64, f64, f64, f64, u32, f32)> = Vec::new();
        // Soft/pen polyline in document space
        let mut freehand: Vec<(f64, f64)> = Vec::new();

        // --- pattern (2D pixel art) ---
        if let Some(rows) = args.get("pattern").and_then(|v| v.as_array()) {
            let ox = args
                .get("origin_x")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let oy = args
                .get("origin_y")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let (base_i, base_j) = crate::tools::pixel_cell_index((ox, oy), gx, gy);
            for (row_i, row) in rows.iter().enumerate() {
                let Some(cols) = row.as_array() else {
                    return Err("pattern rows must be arrays".into());
                };
                for (col_i, cell) in cols.iter().enumerate() {
                    let color = mcp_brush_cell_color(cell, default_rgb, default_alpha);
                    let Some((rgb, a)) = color else {
                        continue;
                    };
                    let doc_x = (base_i + col_i as i64) as f64 * gx + gx * 0.5;
                    let doc_y = (base_j + row_i as i64) as f64 * gy + gy * 0.5;
                    let (cx, cy, w, h) =
                        crate::tools::pixel_stamp_at((doc_x, doc_y), gx, gy, cells);
                    colored_stamps.push((cx, cy, w, h, rgb, a));
                }
            }
        }

        // --- stamps array ---
        if let Some(arr) = args.get("stamps").and_then(|v| v.as_array()) {
            for s in arr {
                let rgb = s
                    .get("fill_color")
                    .and_then(parse_color_value)
                    .unwrap_or(default_rgb);
                let a = s
                    .get("fill_alpha")
                    .and_then(|v| v.as_f64())
                    .map(|x| x.clamp(0.0, 1.0) as f32)
                    .unwrap_or(default_alpha);
                let doc = if let (Some(x), Some(y)) = (
                    s.get("x").and_then(|v| v.as_f64()),
                    s.get("y").and_then(|v| v.as_f64()),
                ) {
                    (x, y)
                } else {
                    let i = s
                        .get("i")
                        .or_else(|| s.get("col"))
                        .and_then(|v| v.as_i64())
                        .ok_or("stamp needs x,y or i/j (col/row)")?;
                    let j = s
                        .get("j")
                        .or_else(|| s.get("row"))
                        .and_then(|v| v.as_i64())
                        .ok_or("stamp needs x,y or i/j (col/row)")?;
                    (i as f64 * gx + gx * 0.5, j as f64 * gy + gy * 0.5)
                };
                let (cx, cy, w, h) = crate::tools::pixel_stamp_at(doc, gx, gy, cells);
                colored_stamps.push((cx, cy, w, h, rgb, a));
            }
        }

        // --- points polyline ---
        if let Some(arr) = args.get("points").and_then(|v| v.as_array()) {
            for p in arr {
                let (x, y) = mcp_brush_xy(p).ok_or("points entries need x,y or [x,y]")?;
                freehand.push((x, y));
            }
        }

        // --- line x0,y0 → x1,y1 ---
        let has_line = args.get("x0").is_some() || args.get("x1").is_some();
        if has_line {
            let x0 = args.get("x0").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y0 = args.get("y0").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let x1 = args.get("x1").and_then(|v| v.as_f64()).unwrap_or(x0);
            let y1 = args.get("y1").and_then(|v| v.as_f64()).unwrap_or(y0);
            freehand.push((x0, y0));
            freehand.push((x1, y1));
        }

        // --- single x,y ---
        if freehand.is_empty()
            && colored_stamps.is_empty()
            && (args.get("x").is_some() || args.get("y").is_some())
        {
            let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
            freehand.push((x, y));
        }

        if freehand.is_empty() && colored_stamps.is_empty() {
            return Err(
                "brush needs points, stamps, pattern, x/y, or x0,y0,x1,y1".into(),
            );
        }

        // Expand freehand into pixel stamps when mode=pixel and no pattern/stamps colors yet
        // (or always for freehand in pixel mode).
        let is_pixel = mode == "pixel" || mode == "pixel_brush" || mode.is_empty();
        if is_pixel && !freehand.is_empty() {
            // Dense stamps along each segment
            let mut path_pts = freehand.clone();
            if path_pts.len() == 1 {
                path_pts.push(path_pts[0]);
            }
            for w in path_pts.windows(2) {
                let stamps =
                    crate::tools::pixel_stamps_along(w[0], w[1], gx, gy, cells);
                for (cx, cy, w, h) in stamps {
                    colored_stamps.push((cx, cy, w, h, default_rgb, default_alpha));
                }
            }
        }

        // --- erase ---
        if erase {
            if !is_pixel {
                return Err("erase only supported for mode=pixel".into());
            }
            let prev_cells = self.tools.brush.pixel_cells;
            self.tools.brush.pixel_cells = cells;
            // Temporarily override viewport steps via direct erase using strip API
            self.pixel_erase_begin();
            self.tools.brush.points.clear();
            let mut erased_cells = 0usize;
            let mut seen = std::collections::HashSet::new();
            // Batch all unique erase rects then strip once per node for speed.
            let mut erase_rects: Vec<(f64, f64, f64, f64)> = Vec::new();
            for &(cx, cy, w, h, _, _) in &colored_stamps {
                let key = (
                    (cx * 1000.0).round() as i64,
                    (cy * 1000.0).round() as i64,
                );
                if !seen.insert(key) {
                    continue;
                }
                erase_rects.push((cx, cy, w, h));
                erased_cells += 1;
            }
            let ids: Vec<crate::document::NodeId> = self
                .project
                .document
                .active_layer()
                .map(|l| l.nodes.clone())
                .unwrap_or_default();
            let mut empty_ids = Vec::new();
            for id in ids {
                let Some(node) = self.project.nodes.get_mut(id) else {
                    continue;
                };
                let NodeKind::Path { path } = &mut node.kind else {
                    continue;
                };
                let new_bez = strip_pixel_rects_from_bez(&path.to_bez(), &erase_rects);
                let empty = !new_bez.elements().iter().any(|e| {
                    matches!(
                        e,
                        kurbo::PathEl::LineTo(_)
                            | kurbo::PathEl::CurveTo(_, _, _)
                            | kurbo::PathEl::QuadTo(_, _)
                    )
                });
                if empty {
                    empty_ids.push(id);
                } else {
                    *path = crate::document::PathData::from_bez(&new_bez);
                }
            }
            for id in empty_ids {
                let _ = self.project.nodes.remove(id);
                if let Some(layer) = self.project.document.active_layer_mut() {
                    layer.nodes.retain(|&n| n != id);
                }
            }
            self.pixel_erase_commit();
            self.tools.brush.pixel_cells = prev_cells;
            self.tools.brush.points.clear();
            return Ok(format!(
                "Erased pixel stamps under {erased_cells} cells"
            ));
        }

        // --- soft / pen modes ---
        if !is_pixel {
            if freehand.len() < 2 {
                return Err("pen/brush mode needs at least 2 points (or a line)".into());
            }
            let brush_type = match mode.as_str() {
                "pen" => crate::tools::BrushType::Pen,
                "calligraphy" | "calli" => crate::tools::BrushType::Calligraphy,
                _ => crate::tools::BrushType::Standard,
            };
            let mut pts: Vec<([f64; 2], f64, f32)> = Vec::with_capacity(freehand.len());
            for (i, &(x, y)) in freehand.iter().enumerate() {
                pts.push(([x, y], i as f64 * 0.016, size));
            }
            if brush_type != crate::tools::BrushType::Calligraphy {
                if let Some(last) = pts.last_mut() {
                    last.2 = 0.0;
                }
            }
            let mut node = if brush_type == crate::tools::BrushType::Pen {
                Node::new(
                    NodeKind::BrushStroke {
                        points: pts.iter().map(|(p, _, w)| (*p, *w)).collect(),
                    },
                    "Pen Stroke",
                )
            } else {
                let bez = generate_brush_outline(&pts, smoothness, brush_type);
                Node::path_from_bez(
                    bez,
                    if brush_type == crate::tools::BrushType::Calligraphy {
                        "Calligraphy"
                    } else {
                        "Brush"
                    },
                )
            };
            node.style.fill = default_fill;
            node.style.stroke = Stroke {
                style: Fill::none(),
                width: 0.0,
                line_join: crate::document::LineJoin::Miter,
                line_cap: crate::document::LineCap::Butt,
                paint_order: crate::document::StrokePaintOrder::BehindFill,
                start_marker: crate::document::PathMarker::default(),
                mid_marker: crate::document::PathMarker::default(),
                end_marker: crate::document::PathMarker::default(),
            };
            if style.stroke_rgb.is_some() && style.stroke_width > 0.0 {
                node.style.stroke = stroke_from_style(style);
            }
            if let Some(n) = style.name.clone() {
                node.name = n;
            }
            let id = node.id;
            self.insert_node(node);
            return Ok(format!("Created {mode} stroke {id} ({} pts)", freehand.len()));
        }

        // --- pixel paint: group by color → path of rects ---
        if colored_stamps.is_empty() {
            return Err("no stamps to paint".into());
        }

        let mut by_color: BTreeMap<(u32, u32), Vec<([f64; 2], f64, f32)>> = BTreeMap::new();
        let aspect = gy / gx;
        let mut seen = std::collections::HashSet::new();
        for &(cx, cy, w, _h, rgb, a) in &colored_stamps {
            let key = (
                (cx * 1000.0).round() as i64,
                (cy * 1000.0).round() as i64,
                (w * 100.0).round() as i64,
            );
            if !seen.insert(key) {
                continue;
            }
            // Pack alpha into discrete key (0..1000)
            let a_key = (a.clamp(0.0, 1.0) * 1000.0).round() as u32;
            by_color
                .entry((rgb, a_key))
                .or_default()
                .push(([cx, cy], 0.0, w as f32));
        }

        let mut ids = Vec::new();
        let multi = by_color.len() > 1;
        for ((rgb, a_key), pts) in by_color {
            let a = a_key as f32 / 1000.0;
            let bez = pixel_stamps_to_path(&pts, aspect);
            let name = style
                .name
                .clone()
                .unwrap_or_else(|| {
                    if multi {
                        format!("Pixel Brush #{rgb:06x}")
                    } else {
                        "Pixel Brush".into()
                    }
                });
            let mut node = Node::path_from_bez(bez, &name);
            node.style.fill = Fill::Solid(Paint::from_hex(rgb, a));
            node.style.stroke = Stroke {
                style: Fill::none(),
                width: 0.0,
                line_join: crate::document::LineJoin::Miter,
                line_cap: crate::document::LineCap::Butt,
                paint_order: crate::document::StrokePaintOrder::BehindFill,
                start_marker: crate::document::PathMarker::default(),
                mid_marker: crate::document::PathMarker::default(),
                end_marker: crate::document::PathMarker::default(),
            };
            ids.push(node.id);
            self.insert_node(node);
        }

        Ok(format!(
            "Created pixel brush ({} stamps → {} object(s): {})",
            seen.len(),
            ids.len(),
            ids.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    fn mcp_drawing_tool(&mut self, name: &str, args: serde_json::Value) -> Result<String, String> {
        use crate::document::{ArcJoin, Fill, Node, NodeKind, TextStyle};
        use crate::mcp::drawing::{fill_from_style, parse_arc_join, style_from_args, stroke_from_style};
        if crate::mcp::node_editor::is_node_editor_tool(name) {
            return self.mcp_node_editor_tool(name, &args);
        }
        if !matches!(name, "add_layer" | "add_shading_layer" | "set_shading_wgsl") {
            self.mcp_ensure_editable()?;
        }
        let style = style_from_args(&args);
        match name {
            "create_rectangle" => {
                let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let w = args.get("w").and_then(|v| v.as_f64()).unwrap_or(100.0);
                let h = args.get("h").and_then(|v| v.as_f64()).unwrap_or(80.0);
                let rx = args.get("rx").and_then(|v| v.as_f64()).unwrap_or(0.0).max(0.0);
                let mut node = Node::rect(x, y, w.max(1.0), h.max(1.0), fill_from_style(&style));
                if let NodeKind::Rect { rx: ref mut r, .. } = node.kind {
                    *r = rx;
                }
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created rectangle {id}"))
            }
            "create_image" => {
                use crate::mcp::drawing::{image_pixel_size, load_image_bytes_from_args};
                let bytes = load_image_bytes_from_args(&args)?;
                let (pw, ph) = image_pixel_size(&bytes)?;
                let scale = args
                    .get("scale")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0)
                    .max(0.01);
                let default_w = pw as f64 * scale;
                let default_h = ph as f64 * scale;
                let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let w = args
                    .get("width")
                    .or_else(|| args.get("w"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default_w)
                    .max(1.0);
                let h = args
                    .get("height")
                    .or_else(|| args.get("h"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default_h)
                    .max(1.0);
                let mut node = Node::image(x, y, w, h, bytes);
                if let Some(n) = style.name.clone() {
                    node.name = n;
                } else if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                    node.name = n.to_string();
                }
                let id = node.id;
                self.insert_node(node);
                Ok(format!(
                    "Created image {id} ({pw}x{ph}px → display {w:.0}x{h:.0})"
                ))
            }
            "create_rectangles" => {
                let rects = args.get("rects").and_then(|v| v.as_array()).ok_or("rects array required")?;
                let mut batch: Vec<crate::document::Node> = Vec::with_capacity(rects.len());
                for r in rects {
                    let x = r.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let y = r.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let w = r.get("w").and_then(|v| v.as_f64()).unwrap_or(4.0).max(1.0);
                    let h = r.get("h").and_then(|v| v.as_f64()).unwrap_or(4.0).max(1.0);
                    let local_style = crate::mcp::drawing::style_from_args(r);
                    let mut node = Node::rect(x, y, w, h, fill_from_style(&local_style));
                    if local_style.stroke_rgb.is_some() {
                        node.style.stroke = stroke_from_style(&local_style);
                    }
                    batch.push(node);
                }
                let n = batch.len();
                if n > 0 {
                    // Queue for incremental processing to avoid blocking main thread long enough
                    // to trigger epaint RwLock deadlock (10s timeout) or starve audio/collab.
                    // Large pixel-art batches (e.g. blackhole/galaxy) are spread over frames.
                    self.pending_mcp_bulk_rects.push(batch);
                }
                Ok(format!("Created {} rectangles (queued for smooth creation)", n))
            }
            "brush" | "brush_stroke" | "paint_brush" => self.mcp_brush(&args, &style),
            "create_circle" => {
                let cx = args.get("cx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cy = args.get("cy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let r = args.get("r").and_then(|v| v.as_f64()).unwrap_or(50.0).max(0.5);
                let mut node = Node::ellipse(cx, cy, r, r, fill_from_style(&style));
                node.name = "Circle".into();
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created circle {id}"))
            }
            "create_ellipse" => {
                let cx = args.get("cx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cy = args.get("cy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let rx = args.get("rx").and_then(|v| v.as_f64()).unwrap_or(60.0).max(0.5);
                let ry = args.get("ry").and_then(|v| v.as_f64()).unwrap_or(40.0).max(0.5);
                let node = Node::ellipse(cx, cy, rx, ry, fill_from_style(&style));
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created ellipse {id}"))
            }
            "create_line" => {
                let x0 = args.get("x0").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y0 = args.get("y0").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let x1 = args.get("x1").and_then(|v| v.as_f64()).unwrap_or(100.0);
                let y1 = args.get("y1").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let mut node = Node::line(x0, y0, x1, y1, stroke_from_style(&style));
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created line {id}"))
            }
            "create_polygon" => {
                let cx = args.get("cx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cy = args.get("cy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let r = args.get("r").and_then(|v| v.as_f64()).unwrap_or(50.0).max(0.5);
                let sides = args.get("sides").and_then(|v| v.as_u64()).unwrap_or(6) as u32;
                let rot = args
                    .get("rotation_deg")
                    .and_then(|v| v.as_f64())
                    .map(|d| d.to_radians())
                    .unwrap_or(0.0);
                let mut node = Node::polygon(cx, cy, r, sides.max(3), fill_from_style(&style));
                if let NodeKind::Polygon { rotation_rad, .. } = &mut node.kind {
                    *rotation_rad = rot;
                }
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created polygon {id}"))
            }
            "create_arc" => {
                let cx = args.get("cx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cy = args.get("cy").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let radius = args.get("radius").and_then(|v| v.as_f64()).unwrap_or(50.0).max(0.5);
                let start = args
                    .get("start_angle_deg")
                    .and_then(|v| v.as_f64())
                    .map(|d| d.to_radians())
                    .unwrap_or(0.0);
                let sweep = args
                    .get("sweep_angle_deg")
                    .and_then(|v| v.as_f64())
                    .map(|d| d.to_radians())
                    .unwrap_or(90.0_f64.to_radians());
                let join = parse_arc_join(args.get("join").unwrap_or(&serde_json::Value::Null));
                let node = Node::arc(cx, cy, radius, start, sweep, join, fill_from_style(&style));
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created arc {id}"))
            }
            "create_text" => {
                let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let content = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Text")
                    .to_string();
                let font_size = args
                    .get("font_size")
                    .and_then(|v| v.as_f64())
                    .map(|s| s.max(1.0) as f32)
                    .unwrap_or(24.0);
                let box_w = args
                    .get("width")
                    .or_else(|| args.get("max_width"))
                    .and_then(|v| v.as_f64())
                    .map(|w| w.max(0.0) as f32)
                    .unwrap_or(0.0);
                let mut text_style = TextStyle {
                    content,
                    font_size,
                    width: box_w,
                    ..TextStyle::default()
                };
                let fill = fill_from_style(&style);
                if let Fill::Solid(p) = fill {
                    let _ = p;
                }
                let mut node = Node::text(x, y, text_style);
                node.style.fill = fill_from_style(&style);
                // Text should default to clean solid fill (no stroke) unless stroke params are explicitly given.
                // This prevents "bold" look from default stroke width.
                let has_stroke_param = args.get("stroke_width").is_some()
                    || args.get("stroke_color").is_some()
                    || args.get("stroke").is_some()
                    || args.get("stroke_alpha").is_some();
                let mut effective_style = style;
                if !has_stroke_param {
                    effective_style.stroke_width = 0.0;
                }
                let id = node.id;
                self.mcp_finish_node(node, &effective_style);
                Ok(format!("Created text {id}"))
            }
            "set_object_style" => {
                // Support both single "id" and "ids": [...] for convenience
                if let Some(arr) = args.get("ids").and_then(|v| v.as_array()) {
                    self.mcp_set_objects_style_from_args(arr, &args)
                } else {
                    let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                    self.mcp_patch_node(id_str, &args)
                }
            }
            "set_objects_style" => {
                let arr = args.get("ids").and_then(|v| v.as_array()).ok_or("ids array required")?;
                self.mcp_set_objects_style_from_args(arr, &args)
            }
            "set_object_transform" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let patch = args.clone();
                self.mcp_patch_node(id_str, &patch)
            }
            "set_object_geometry" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let geom = args.get("geometry").cloned().unwrap_or(serde_json::json!({}));
                self.mcp_patch_node(id_str, &geom)
            }

            // === Animation tools ===
            "set_keyframe" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let property = args.get("property").and_then(|v| v.as_str()).ok_or("property required")?.to_string();
                let frame = args.get("frame").and_then(|v| v.as_u64()).ok_or("frame required")? as usize;
                let value = args.get("value").and_then(|v| v.as_f64()).ok_or("value required")?;
                let interp_str = args.get("interpolation").and_then(|v| v.as_str()).unwrap_or("linear");
                let mode = match interp_str.to_lowercase().as_str() {
                    "bezier" | "cubic" => crate::document::InterpolationMode::Bezier,
                    _ => crate::document::InterpolationMode::Linear,
                };
                self.mcp_set_keyframe(id_str, &property, frame, value, mode)
            }
            "remove_keyframe" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let property = args.get("property").and_then(|v| v.as_str()).ok_or("property required")?.to_string();
                let frame = args.get("frame").and_then(|v| v.as_u64()).ok_or("frame required")? as usize;
                self.mcp_remove_keyframe(id_str, &property, frame)
            }
            "get_keyframes" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let property = args.get("property").and_then(|v| v.as_str()).map(|s| s.to_string());
                self.mcp_get_keyframes(id_str, property.as_deref())
            }
            "set_keyframe_interpolation" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let property = args.get("property").and_then(|v| v.as_str()).ok_or("property required")?.to_string();
                let frame = args.get("frame").and_then(|v| v.as_u64()).ok_or("frame required")? as usize;
                let interp_str = args.get("interpolation").and_then(|v| v.as_str());
                let handle_left = args.get("handle_left").and_then(|v| v.as_array()).map(|a| {
                    (a.get(0).and_then(|x| x.as_f64()).unwrap_or(-5.0),
                     a.get(1).and_then(|y| y.as_f64()).unwrap_or(0.0))
                });
                let handle_right = args.get("handle_right").and_then(|v| v.as_array()).map(|a| {
                    (a.get(0).and_then(|x| x.as_f64()).unwrap_or(5.0),
                     a.get(1).and_then(|y| y.as_f64()).unwrap_or(0.0))
                });
                let handle_mode_str = args.get("handle_mode").and_then(|v| v.as_str());
                self.mcp_set_keyframe_interpolation(id_str, &property, frame, interp_str, handle_left, handle_right, handle_mode_str)
            }
            "set_current_anim_frame" => {
                let frame = args.get("frame").and_then(|v| v.as_u64()).ok_or("frame required")? as usize;
                self.anim_current_frame = frame;
                self.apply_animation_for_frame(frame);
                Ok(format!("Current frame set to {}", frame))
            }
            "get_current_anim_frame" => {
                Ok(format!("{}", self.anim_current_frame))
            }
            "set_keyframes" => {
                let kfs = args.get("keyframes").and_then(|v| v.as_array()).ok_or("keyframes array required")?;
                self.mcp_set_keyframes(kfs)
            }
            "clear_animation_track" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let property = args.get("property").and_then(|v| v.as_str()).ok_or("property required")?.to_string();
                self.mcp_clear_animation_track(id_str, &property)
            }
            "add_stack_animation" => self.mcp_add_stack_animation(&args),
            "edit_stack_animation" => self.mcp_edit_stack_animation(&args),
            "remove_stack_animation" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let stack_id = args
                    .get("stack_id")
                    .and_then(|v| v.as_str())
                    .ok_or("stack_id required")?;
                self.mcp_remove_stack_animation(id_str, stack_id)
            }
            "list_stack_animations" => {
                let id_filter = args.get("id").and_then(|v| v.as_str());
                self.mcp_list_stack_animations(id_filter)
            }

            "create_path" => {
                let svg_d = args
                    .get("svg_d")
                    .and_then(|v| v.as_str())
                    .ok_or("svg_d required")?;
                let bez = crate::mcp::path_parse::bez_from_svg_d(svg_d)?;
                let mut node = crate::document::Node::path_from_bez(bez, "Path");
                node.style.fill = fill_from_style(&style);
                if style.stroke_rgb.is_some() {
                    node.style.stroke = stroke_from_style(&style);
                }
                let id = node.id;
                self.mcp_finish_node(node, &style);
                Ok(format!("Created path {id}"))
            }
            "add_layer" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Layer");
                self.add_layer(name);
                Ok(format!("Added layer \"{name}\""))
            }
            "add_shading_layer" => {
                let wgsl = args
                    .get("wgsl")
                    .and_then(|v| v.as_str())
                    .ok_or("add_shading_layer requires \"wgsl\" (WGSL fragment source, compiled at runtime)")?;
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Shading");
                let pass_name = args
                    .get("pass_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Shader");
                let uniforms = args.get("uniforms").and_then(|v| v.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                        .collect::<Vec<f32>>()
                });
                // Default: edit the active / targeted shading layer in place (no stack spam).
                // Pass `"new": true` to always create a fresh layer.
                let force_new = args.get("new").and_then(|v| v.as_bool()).unwrap_or(false);
                let has_target = args.get("layer_id").is_some() || args.get("layer_index").is_some();
                let active_is_shading = self
                    .project
                    .document
                    .layers
                    .get(self.project.document.active_layer_index)
                    .is_some_and(|l| l.kind == crate::document::LayerKind::Shading);
                let any_shading = self
                    .project
                    .document
                    .layers
                    .iter()
                    .any(|l| l.kind == crate::document::LayerKind::Shading);
                if !force_new && (has_target || active_is_shading || any_shading) {
                    let layer_index = if let Some(i) =
                        args.get("layer_index").and_then(|v| v.as_u64())
                    {
                        i as usize
                    } else if let Some(id) = args.get("layer_id").and_then(|v| v.as_str()) {
                        let uid = uuid::Uuid::parse_str(id).map_err(|_| "bad layer_id")?;
                        self.project
                            .document
                            .layers
                            .iter()
                            .position(|l| l.id == uid)
                            .ok_or("layer_id not found")?
                    } else if active_is_shading {
                        self.project.document.active_layer_index
                    } else {
                        self.project
                            .document
                            .layers
                            .iter()
                            .position(|l| l.kind == crate::document::LayerKind::Shading)
                            .ok_or("no shading layer")?
                    };
                    self.set_shading_wgsl(layer_index, wgsl, Some(pass_name), uniforms)?;
                    // Optionally rename the layer when editing in place.
                    if let Some(l) = self.project.document.layers.get_mut(layer_index) {
                        if name != "Shading" {
                            l.name = name.to_string();
                        }
                    }
                    Ok(format!(
                        "Updated shading layer index {layer_index} (in-place) pass \"{pass_name}\" — GPU compile OK"
                    ))
                } else {
                    self.add_shading_layer_with_wgsl(name, pass_name, wgsl, uniforms)?;
                    Ok(format!(
                        "Added shading layer \"{name}\" with runtime WGSL pass \"{pass_name}\" — GPU compile OK"
                    ))
                }
            }
            "set_shading_wgsl" => {
                let wgsl = args
                    .get("wgsl")
                    .and_then(|v| v.as_str())
                    .ok_or("set_shading_wgsl requires \"wgsl\"")?;
                let layer_index = if let Some(i) = args.get("layer_index").and_then(|v| v.as_u64()) {
                    i as usize
                } else if let Some(id) = args.get("layer_id").and_then(|v| v.as_str()) {
                    let uid = uuid::Uuid::parse_str(id).map_err(|_| "bad layer_id")?;
                    self.project
                        .document
                        .layers
                        .iter()
                        .position(|l| l.id == uid)
                        .ok_or("layer_id not found")?
                } else {
                    // Prefer active shading layer, else first shading.
                    self.project
                        .document
                        .layers
                        .get(self.project.document.active_layer_index)
                        .filter(|l| l.kind == crate::document::LayerKind::Shading)
                        .map(|_| self.project.document.active_layer_index)
                        .or_else(|| {
                            self.project
                                .document
                                .layers
                                .iter()
                                .position(|l| l.kind == crate::document::LayerKind::Shading)
                        })
                        .ok_or("no shading layer (pass layer_index or layer_id)")?
                };
                let pass_name = args.get("pass_name").and_then(|v| v.as_str());
                let uniforms = args.get("uniforms").and_then(|v| v.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                        .collect::<Vec<f32>>()
                });
                self.set_shading_wgsl(layer_index, wgsl, pass_name, uniforms)?;
                Ok(format!(
                    "Updated shading WGSL on layer index {layer_index} — GPU compile OK"
                ))
            }
            "list_animatable_properties" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                self.mcp_list_animatable_properties(id_str)
            }
            "list_animation_tracks" => {
                let id_filter = args.get("id").and_then(|v| v.as_str());
                self.mcp_list_animation_tracks(id_filter)
            }
            "play_animation" => {
                let playing = args
                    .get("playing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.anim_is_playing = playing;
                if playing {
                    let now = std::time::Instant::now();
                    self.anim_playback_wall = Some(now);
                    self.anim_play_origin = Some((now, self.anim_current_frame));
                    self.anim_time_accumulator = 0.0;
                    self.apply_animation_for_frame(self.anim_current_frame);
                } else {
                    self.anim_playback_wall = None;
                    self.anim_play_origin = None;
                }
                Ok(if playing {
                    format!("Playing from frame {}", self.anim_current_frame)
                } else {
                    format!("Paused at frame {}", self.anim_current_frame)
                })
            }
            "get_object_properties" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                self.mcp_get_object_properties(id_str)
            }
            "set_selection" => {
                self.mcp_set_selection(&args)
            }
            "duplicate_object" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let ox = args.get("offset_x").and_then(|v| v.as_f64()).unwrap_or(20.0);
                let oy = args.get("offset_y").and_then(|v| v.as_f64()).unwrap_or(20.0);
                self.mcp_duplicate_object(id_str, ox, oy)
            }
            "reorder_object" => {
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .ok_or("action required")?;
                self.mcp_reorder_object(id_str, action)
            }
            "list_layers" => self.mcp_list_layers(),
            "set_active_layer" => {
                if let Some(idx) = args.get("index").and_then(|v| v.as_u64()) {
                    let i = idx as usize;
                    if i >= self.project.document.layers.len() {
                        return Err(format!("layer index {i} out of range"));
                    }
                    self.project.document.active_layer_index = i;
                    return Ok(format!("Active layer index {i}"));
                }
                let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("index or id required")?;
                let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
                let pos = self
                    .project
                    .document
                    .layers
                    .iter()
                    .position(|l| l.id == id)
                    .ok_or_else(|| format!("layer not found: {id_str}"))?;
                self.project.document.active_layer_index = pos;
                Ok(format!("Active layer {id_str} (index {pos})"))
            }
            _ => Err(format!("Unknown drawing tool: {name}")),
        }
    }

    fn mcp_patch_nodes(&mut self, patches: Vec<(uuid::Uuid, crate::document::Node, crate::document::Node)>) -> Result<String, String> {
        if patches.is_empty() {
            return Ok("No changes".into());
        }
        let count = patches.len();
        let real: Vec<_> = patches.into_iter().filter(|(_, b, a)| b != a).collect();
        if !real.is_empty() {
            self.history.push(&mut self.project, crate::history::ProjectEdit::PatchNodes { patches: real });
        }
        Ok(format!("Updated style on {} object(s)", count))
    }

    fn mcp_set_objects_style_from_args(&mut self, id_values: &[serde_json::Value], args: &serde_json::Value) -> Result<String, String> {
        use crate::mcp::drawing::apply_style_patch;
        let mut patches = Vec::new();
        for idv in id_values {
            if let Some(id_str) = idv.as_str() {
                if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                    if let Some(before) = self.project.nodes.get(id).cloned() {
                        let mut after = before.clone();
                        if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                            after.name = name.to_string();
                        }
                        let _ = apply_style_patch(&mut after.style, args);
                        // basic transform? not for style tool
                        if before != after {
                            patches.push((id, before, after));
                        }
                    }
                }
            }
        }
        self.mcp_patch_nodes(patches)
    }

    // === Animation / selection / layer MCP helpers ===
    fn mcp_resolve_start_const(
        starts: Option<&serde_json::Value>,
        track: &str,
        fallback: f64,
    ) -> f64 {
        let Some(s) = starts else {
            return fallback;
        };
        if let Some(v) = s.get(track).and_then(|v| v.as_f64()) {
            return v;
        }
        match track {
            "pos_x" => s.get("x").and_then(|v| v.as_f64()).unwrap_or(fallback),
            "pos_y" => s.get("y").and_then(|v| v.as_f64()).unwrap_or(fallback),
            "color_r" => s.get("r").and_then(|v| v.as_f64()).unwrap_or(fallback),
            "color_g" => s.get("g").and_then(|v| v.as_f64()).unwrap_or(fallback),
            "color_b" => s.get("b").and_then(|v| v.as_f64()).unwrap_or(fallback),
            "color_a" => s.get("a").and_then(|v| v.as_f64()).unwrap_or(fallback),
            _ => s
                .get("x")
                .and_then(|v| v.as_f64())
                .or_else(|| s.get("s").and_then(|v| v.as_f64()))
                .unwrap_or(fallback),
        }
    }

    fn mcp_add_stack_animation(&mut self, args: &serde_json::Value) -> Result<String, String> {
        let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        if self.project.nodes.get(id).is_none() {
            return Err(format!("Object not found: {id_str}"));
        }
        let start_frame = args
            .get("start_frame")
            .and_then(|v| v.as_u64())
            .ok_or("start_frame required")? as usize;
        let duration_frames = args
            .get("duration_frames")
            .and_then(|v| v.as_u64())
            .ok_or("duration_frames required")? as usize;
        let duration_frames = duration_frames.max(1);
        let tracks: Vec<String> = args
            .get("tracks")
            .and_then(|v| v.as_array())
            .ok_or("tracks array required")?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if tracks.is_empty() {
            return Err("tracks must be non-empty".into());
        }
        let exprs: Vec<String> = args
            .get("exprs")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let starts = args.get("starts");
        let live_geom = self.get_node_geom_floats(id);
        let before = self.project.anim_timeline.clone();
        let entry = self.project.anim_timeline.nodes.entry(id).or_default();
        // Ensure geom track slots exist if needed
        for t in &tracks {
            entry.ensure_track(t);
        }
        let end = start_frame.saturating_add(duration_frames);
        let mut channels = Vec::new();
        for (i, track) in tracks.iter().enumerate() {
            let _ = i;
            let geom_def = if let Some(gidx) = track
                .strip_prefix("geom_")
                .and_then(|s| s.parse::<usize>().ok())
            {
                live_geom.get(gidx).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            // Prefer exact key at start_frame; else live geom — not distant interpolate.
            let def = entry
                .get_track(track)
                .and_then(|tr| {
                    tr.keyframes
                        .iter()
                        .find(|k| k.frame == start_frame)
                        .map(|k| k.value)
                })
                .unwrap_or(geom_def);
            let start_value = Self::mcp_resolve_start_const(starts, track, def);
            let expr = exprs.get(i).cloned().unwrap_or_default();
            if !expr.trim().is_empty() {
                if let Err(e) = crate::document::eval_expr(&expr, 0.5, 0.0) {
                    return Err(format!("invalid expr for {track}: {}", e.0));
                }
            }
            channels.push(crate::document::StackAnimChannel {
                track: track.clone(),
                expr,
                start_value,
                last_error: None,
            });
        }
        let labels_ref: Vec<&str> = tracks.iter().map(|s| s.as_str()).collect();
        entry.clear_keyframes_under_stack(&labels_ref, start_frame, end);
        for ch in &channels {
            if let Some(tr) = entry.get_track_mut(&ch.track) {
                tr.insert(start_frame, ch.start_value);
            }
        }
        let stack_id = uuid::Uuid::new_v4();
        entry.stack_functions.push(crate::document::StackAnimationFunction {
            id: stack_id,
            start_frame,
            duration_frames,
            channels,
        });
        entry.ensure_stack_start_keyframes();
        entry.ensure_stack_end_keyframes();
        let after = self.project.anim_timeline.clone();
        self.history.push(
            &mut self.project,
            crate::history::ProjectEdit::PatchTimeline { before, after },
        );
        self.apply_animation_for_frame(self.anim_current_frame);
        Ok(format!(
            "Added stack animation {stack_id} on {id_str} frames {start_frame}..{end}"
        ))
    }

    fn mcp_edit_stack_animation(&mut self, args: &serde_json::Value) -> Result<String, String> {
        let id_str = args.get("id").and_then(|v| v.as_str()).ok_or("id required")?;
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let stack_id = args
            .get("stack_id")
            .and_then(|v| v.as_str())
            .ok_or("stack_id required")?;
        let stack_id = uuid::Uuid::parse_str(stack_id).map_err(|e| e.to_string())?;
        let before = self.project.anim_timeline.clone();
        let entry = self
            .project
            .anim_timeline
            .nodes
            .get_mut(&id)
            .ok_or_else(|| format!("No animation for {id_str}"))?;
        let sf = entry
            .stack_functions
            .iter_mut()
            .find(|s| s.id == stack_id)
            .ok_or_else(|| format!("stack_id not found: {stack_id}"))?;
        let old_start = sf.start_frame;
        let old_end = sf.end_frame();
        if let Some(sf_frame) = args.get("start_frame").and_then(|v| v.as_u64()) {
            sf.start_frame = sf_frame as usize;
        }
        if let Some(d) = args.get("duration_frames").and_then(|v| v.as_u64()) {
            sf.duration_frames = (d as usize).max(1);
        }
        if let Some(arr) = args.get("exprs").and_then(|v| v.as_array()) {
            for (i, ch) in sf.channels.iter_mut().enumerate() {
                if let Some(e) = arr.get(i).and_then(|v| v.as_str()) {
                    ch.expr = e.to_string();
                    if ch.expr.trim().is_empty() {
                        ch.last_error = None;
                    } else if let Err(err) = crate::document::eval_expr(&ch.expr, 0.5, 0.0) {
                        return Err(format!("invalid expr for {}: {}", ch.track, err.0));
                    } else {
                        ch.last_error = None;
                    }
                }
            }
        }
        let starts = args.get("starts");
        if starts.is_some() {
            for ch in sf.channels.iter_mut() {
                ch.start_value =
                    Self::mcp_resolve_start_const(starts, &ch.track, ch.start_value);
            }
        }
        let labels: Vec<String> = sf.channels.iter().map(|c| c.track.clone()).collect();
        let start_f = sf.start_frame;
        let end_f = sf.end_frame();
        let start_vals: Vec<(String, f64)> = sf
            .channels
            .iter()
            .map(|c| (c.track.clone(), c.start_value))
            .collect();
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let lo = old_start.min(start_f);
        let hi = old_end.max(end_f);
        entry.clear_keyframes_under_stack(&refs, start_f, end_f);
        for label in &labels {
            if let Some(tr) = entry.get_track_mut(label) {
                tr.keyframes
                    .retain(|kf| kf.frame == start_f || kf.frame < lo || kf.frame > hi);
            }
        }
        for (tr, v) in start_vals {
            if let Some(track) = entry.get_track_mut(&tr) {
                track.insert(start_f, v);
            }
        }
        entry.ensure_stack_start_keyframes();
        entry.ensure_stack_end_keyframes();
        let after = self.project.anim_timeline.clone();
        self.history.push(
            &mut self.project,
            crate::history::ProjectEdit::PatchTimeline { before, after },
        );
        self.apply_animation_for_frame(self.anim_current_frame);
        Ok(format!("Updated stack animation {stack_id}"))
    }

    fn mcp_remove_stack_animation(
        &mut self,
        id_str: &str,
        stack_id_str: &str,
    ) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let stack_id = uuid::Uuid::parse_str(stack_id_str).map_err(|e| e.to_string())?;
        let before = self.project.anim_timeline.clone();
        let entry = self
            .project
            .anim_timeline
            .nodes
            .get_mut(&id)
            .ok_or_else(|| format!("No animation for {id_str}"))?;
        if !entry.remove_stack_function_with_keyframes(stack_id) {
            return Err(format!("stack_id not found: {stack_id_str}"));
        }
        let after = self.project.anim_timeline.clone();
        self.history.push(
            &mut self.project,
            crate::history::ProjectEdit::PatchTimeline { before, after },
        );
        self.apply_animation_for_frame(self.anim_current_frame);
        Ok(format!("Removed stack animation {stack_id_str}"))
    }

    fn mcp_list_stack_animations(&self, id_filter: Option<&str>) -> Result<String, String> {
        let filter = id_filter
            .map(|s| uuid::Uuid::parse_str(s).map_err(|e| e.to_string()))
            .transpose()?;
        let mut out = Vec::new();
        for (nid, anim) in &self.project.anim_timeline.nodes {
            if filter.is_some_and(|f| f != *nid) {
                continue;
            }
            for sf in &anim.stack_functions {
                let channels: Vec<_> = sf
                    .channels
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "track": c.track,
                            "expr": c.expr,
                            "start_value": c.start_value,
                            "error": c.last_error,
                        })
                    })
                    .collect();
                out.push(serde_json::json!({
                    "object_id": nid.to_string(),
                    "stack_id": sf.id.to_string(),
                    "start_frame": sf.start_frame,
                    "duration_frames": sf.duration_frames,
                    "end_frame": sf.end_frame(),
                    "channels": channels,
                    "f_t": format!(
                        "f(t) = ({})",
                        sf.channels
                            .iter()
                            .map(|c| if c.expr.trim().is_empty() {
                                format!("{:.4}", c.start_value)
                            } else {
                                c.expr.clone()
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }));
            }
        }
        Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
    }

    fn mcp_list_animatable_properties(&self, id_str: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let node = self
            .project
            .nodes
            .get(id)
            .ok_or_else(|| format!("Object not found: {id_str}"))?;
        let mut props = vec![
            "pos_x".into(),
            "pos_y".into(),
            "rotation".into(),
            "opacity".into(),
            "color_r".into(),
            "color_g".into(),
            "color_b".into(),
            "color_a".into(),
            "stroke_width".into(),
            "stroke_r".into(),
            "stroke_g".into(),
            "stroke_b".into(),
            "stroke_a".into(),
        ];
        let n_geom = node.get_geom_floats().len();
        for i in 0..n_geom {
            props.push(format!("geom_{i}"));
        }
        let labels: Vec<_> = (0..n_geom)
            .map(|i| {
                serde_json::json!({
                    "property": format!("geom_{i}"),
                    "label": self.get_node_geom_track_name(id, i),
                })
            })
            .collect();
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "id": id_str,
            "kind": self.mcp_node_kind_name(node),
            "properties": props,
            "geom_labels": labels,
            "workflow": "set_keyframe(id, property, frame, value, interpolation?) → scrub with set_current_anim_frame / play_animation → remove_keyframe / clear_animation_track",
        }))
        .unwrap_or_default())
    }

    fn mcp_list_animation_tracks(&self, id_filter: Option<&str>) -> Result<String, String> {
        let filter = id_filter
            .map(|s| uuid::Uuid::parse_str(s).map_err(|e| e.to_string()))
            .transpose()?;
        let mut tracks = Vec::new();
        for (nid, anim) in &self.project.anim_timeline.nodes {
            if filter.is_some_and(|f| f != *nid) {
                continue;
            }
            let name = self
                .project
                .nodes
                .get(*nid)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| nid.to_string());
            let push_track = |prop: &str, track: &crate::document::KeyframeTrack, tracks: &mut Vec<serde_json::Value>| {
                if track.keyframes.is_empty() {
                    return;
                }
                let frames: Vec<usize> = track.keyframes.iter().map(|k| k.frame).collect();
                tracks.push(serde_json::json!({
                    "id": nid.to_string(),
                    "name": name,
                    "property": prop,
                    "keyframe_count": track.keyframes.len(),
                    "frame_min": frames.iter().copied().min(),
                    "frame_max": frames.iter().copied().max(),
                    "frames": frames,
                }));
            };
            for prop in [
                "pos_x",
                "pos_y",
                "rotation",
                "opacity",
                "color_r",
                "color_g",
                "color_b",
                "color_a",
                "stroke_width",
                "stroke_r",
                "stroke_g",
                "stroke_b",
                "stroke_a",
            ] {
                if let Some(t) = anim.get_track(prop) {
                    push_track(prop, t, &mut tracks);
                }
            }
            for (i, t) in anim.geom_tracks.iter().enumerate() {
                push_track(&format!("geom_{i}"), t, &mut tracks);
            }
            for (lbl, t) in &anim.param_tracks {
                push_track(lbl, t, &mut tracks);
            }
        }
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "current_frame": self.anim_current_frame,
            "playing": self.anim_is_playing,
            "content_max_frame": self.get_content_max_animation_frame(),
            "tracks": tracks,
        }))
        .unwrap_or_default())
    }

    fn mcp_get_object_properties(&self, id_str: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let node = self
            .project
            .nodes
            .get(id)
            .ok_or_else(|| format!("Object not found: {id_str}"))?;
        let b = node.bounds();
        let fill = match &node.style.fill {
            crate::document::Fill::Solid(p) => serde_json::json!({
                "kind": "solid",
                "rgba": p.rgba,
                "hex": format!("#{:02X}{:02X}{:02X}",
                    (p.rgba[0]*255.0) as u8, (p.rgba[1]*255.0) as u8, (p.rgba[2]*255.0) as u8),
            }),
            crate::document::Fill::None => serde_json::json!({ "kind": "none" }),
            other => serde_json::json!({ "kind": format!("{:?}", other).split_whitespace().next().unwrap_or("other") }),
        };
        let stroke = serde_json::json!({
            "width": node.style.stroke.width,
            "paint_order": node.style.stroke.paint_order.label(),
            "line_join": format!("{:?}", node.style.stroke.line_join),
            "line_cap": format!("{:?}", node.style.stroke.line_cap),
        });
        let geom: Vec<serde_json::Value> = node
            .get_geom_floats()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                serde_json::json!({
                    "index": i,
                    "property": format!("geom_{i}"),
                    "label": self.get_node_geom_track_name(id, i),
                    "value": v,
                })
            })
            .collect();
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "id": id_str,
            "name": node.name,
            "kind": self.mcp_node_kind_name(node),
            "bounds": { "x0": b.x0, "y0": b.y0, "x1": b.x1, "y1": b.y1, "w": b.width(), "h": b.height() },
            "transform": {
                "translate": node.transform.translation,
                "scale": node.transform.scale,
                "rotation_deg": node.transform.rotation_rad.to_degrees(),
            },
            "style": {
                "opacity": node.style.opacity,
                "blend_mode": node.style.blend_mode.label(),
                "fill": fill,
                "stroke": stroke,
            },
            "geometry": geom,
            "animatable": [
                "pos_x","pos_y","rotation","opacity","color_r","color_g","color_b","color_a",
                "stroke_width","stroke_r","stroke_g","stroke_b","stroke_a"
            ],
        }))
        .unwrap_or_default())
    }

    fn mcp_node_kind_name(&self, node: &crate::document::Node) -> &'static str {
        match &node.kind {
            NodeKind::Rect { .. } => "rect",
            NodeKind::Ellipse { .. } => "ellipse",
            NodeKind::Polygon { .. } => "polygon",
            NodeKind::Path { .. } => "path",
            NodeKind::Text { .. } => "text",
            NodeKind::Image { .. } => "image",
            NodeKind::Plotter { .. } => "plotter",
            NodeKind::Arc { .. } => "arc",
            NodeKind::Group { .. } => "group",
            NodeKind::BrushStroke { .. } => "brush",
            NodeKind::FlowchartNode { .. } => "flowchart_node",
            NodeKind::FlowchartPath { .. } => "flowchart_path",
        }
    }

    fn mcp_set_selection(&mut self, args: &serde_json::Value) -> Result<String, String> {
        let mut ids = Vec::new();
        if let Some(arr) = args.get("ids").and_then(|v| v.as_array()) {
            for v in arr {
                let s = v.as_str().ok_or("ids must be strings")?;
                ids.push(uuid::Uuid::parse_str(s).map_err(|e| e.to_string())?);
            }
        } else if let Some(s) = args.get("id").and_then(|v| v.as_str()) {
            ids.push(uuid::Uuid::parse_str(s).map_err(|e| e.to_string())?);
        }
        for id in &ids {
            if self.project.nodes.get(*id).is_none() {
                return Err(format!("Object not found: {id}"));
            }
        }
        self.selection = ids.clone();
        Ok(format!("Selection set to {} object(s)", ids.len()))
    }

    fn mcp_duplicate_object(&mut self, id_str: &str, ox: f64, oy: f64) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let src = self
            .project
            .nodes
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Object not found: {id_str}"))?;
        // Prefer source layer so duplicate lands with siblings.
        if let Some(layer_idx) = self
            .project
            .document
            .layers
            .iter()
            .position(|l| l.nodes.contains(&id))
        {
            self.project.document.active_layer_index = layer_idx;
        }
        let mut dup = src.duplicate();
        dup.translate(ox, oy);
        let new_id = dup.id;
        self.history
            .push(&mut self.project, crate::history::ProjectEdit::InsertNode { node: dup });
        self.selection = vec![new_id];
        Ok(format!("Duplicated {id_str} → {new_id}"))
    }

    fn mcp_reorder_object(&mut self, id_str: &str, action: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        if self.project.nodes.get(id).is_none() {
            return Err(format!("Object not found: {id_str}"));
        }
        self.selection = vec![id];
        let delta = match action.to_ascii_lowercase().as_str() {
            "raise" | "up" | "forward" => 1,
            "lower" | "down" | "backward" => -1,
            "bring_to_front" | "front" | "to_front" => {
                // Raise many times within layer.
                for _ in 0..64 {
                    self.nudge_z_order(1);
                }
                return Ok(format!("Brought {id_str} toward front"));
            }
            "send_to_back" | "back" | "to_back" => {
                for _ in 0..64 {
                    self.nudge_z_order(-1);
                }
                return Ok(format!("Sent {id_str} toward back"));
            }
            _ => {
                return Err(
                    "action must be raise|lower|bring_to_front|send_to_back".into(),
                )
            }
        };
        self.nudge_z_order(delta);
        Ok(format!("Reordered {id_str} ({action})"))
    }

    fn mcp_list_layers(&self) -> Result<String, String> {
        let layers: Vec<_> = self
            .project
            .document
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                serde_json::json!({
                    "index": i,
                    "id": l.id.to_string(),
                    "name": l.name,
                    "kind": format!("{:?}", l.kind),
                    "visible": l.visible,
                    "locked": l.locked,
                    "active": i == self.project.document.active_layer_index,
                    "node_count": l.nodes.len(),
                    "av_clips": l.av_clips.len(),
                    "shading_passes": l.shading_passes.len(),
                })
            })
            .collect();
        Ok(serde_json::to_string_pretty(&serde_json::json!({ "layers": layers }))
            .unwrap_or_default())
    }

    fn mcp_set_keyframe(&mut self, id_str: &str, property: &str, frame: usize, value: f64, mode: crate::document::InterpolationMode) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let before = self.project.anim_timeline.clone();
        let entry = self.project.anim_timeline.nodes.entry(id).or_default();
        if let Some(track) = entry.get_track_mut(property) {
            if track.keyframes.is_empty() && frame != 0 {
                // seed frame 0 if first keyframe not at 0
                if let Some(node) = self.project.nodes.get(id) {
                    let def = match property {
                        "pos_x" => node.get_pos().0,
                        "pos_y" => node.get_pos().1,
                        "rotation" => node.get_rotation(),
                        "opacity" => node.get_opacity() as f64,
                        "color_r" => node.get_color()[0] as f64,
                        "color_g" => node.get_color()[1] as f64,
                        "color_b" => node.get_color()[2] as f64,
                        "color_a" => node.get_color()[3] as f64,
                        _ => 0.0,
                    };
                    track.insert(0, def);
                }
            }
            track.insert(frame, value);
            // set interpolation on the keyframe we just touched
            if let Some(kf) = track.keyframes.iter_mut().find(|k| k.frame == frame) {
                kf.interpolation = mode;
            }
            self.apply_animation_for_frame(self.anim_current_frame);
            let after = self.project.anim_timeline.clone();
            self.history.push(&mut self.project, crate::history::ProjectEdit::PatchTimeline { before, after });
            Ok(format!("Set keyframe {}@{} = {}", property, frame, value))
        } else {
            Err(format!("Unknown animation property '{}'", property))
        }
    }

    fn mcp_remove_keyframe(&mut self, id_str: &str, property: &str, frame: usize) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        self.delete_keyframe(id, property, frame);
        Ok(format!("Removed keyframe {}@{}", property, frame))
    }

    fn mcp_get_keyframes(&mut self, id_str: &str, property: Option<&str>) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let anim = match self.project.anim_timeline.nodes.get(&id) {
            Some(a) => a,
            None => return Ok("[]".to_string()),
        };
        let mut out = serde_json::Map::new();
        let props = if let Some(p) = property {
            vec![p.to_string()]
        } else {
            vec![
                "pos_x".into(),
                "pos_y".into(),
                "rotation".into(),
                "opacity".into(),
                "color_r".into(),
                "color_g".into(),
                "color_b".into(),
                "color_a".into(),
                "stroke_width".into(),
                "stroke_r".into(),
                "stroke_g".into(),
                "stroke_b".into(),
                "stroke_a".into(),
            ]
        };
        for prop in &props {
            if let Some(track) = anim.get_track(prop) {
                let kfs: Vec<_> = track.keyframes.iter().map(|kf| {
                    serde_json::json!({
                        "frame": kf.frame,
                        "value": kf.value,
                        "interpolation": match kf.interpolation {
                            crate::document::InterpolationMode::Linear => "linear",
                            crate::document::InterpolationMode::Bezier => "bezier",
                        }
                    })
                }).collect();
                out.insert(prop.clone(), serde_json::json!(kfs));
            }
            // also handle geom_ if requested
            if prop.starts_with("geom_") {
                if let Some(track) = anim.get_track(prop) {
                    let kfs: Vec<_> = track.keyframes.iter().map(|kf| serde_json::json!({"frame": kf.frame, "value": kf.value})).collect();
                    out.insert(prop.clone(), serde_json::json!(kfs));
                }
            }
        }
        // include geom tracks if no specific property or all
        if property.is_none() || property.unwrap_or("").starts_with("geom") {
            for (i, track) in anim.geom_tracks.iter().enumerate() {
                if !track.keyframes.is_empty() {
                    let name = format!("geom_{}", i);
                    let kfs: Vec<_> = track.keyframes.iter().map(|kf| serde_json::json!({"frame": kf.frame, "value": kf.value})).collect();
                    out.insert(name, serde_json::json!(kfs));
                }
            }
        }
        Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
    }

    fn mcp_set_keyframe_interpolation(&mut self, id_str: &str, property: &str, frame: usize, interp: Option<&str>, handle_left: Option<(f64,f64)>, handle_right: Option<(f64,f64)>, handle_mode: Option<&str>) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let before = self.project.anim_timeline.clone();
        let changed = if let Some(anim) = self.project.anim_timeline.nodes.get_mut(&id) {
            if let Some(track) = anim.get_track_mut(property) {
                if let Some(kf) = track.keyframes.iter_mut().find(|k| k.frame == frame) {
                    if let Some(i) = interp {
                        kf.interpolation = match i.to_lowercase().as_str() {
                            "bezier" => crate::document::InterpolationMode::Bezier,
                            _ => crate::document::InterpolationMode::Linear,
                        };
                    }
                    if let Some(hl) = handle_left {
                        kf.handle_left = hl;
                    }
                    if let Some(hr) = handle_right {
                        kf.handle_right = hr;
                    }
                    if let Some(m) = handle_mode {
                        kf.handle_mode = match m.to_lowercase().as_str() {
                            "left" | "leftonly" => crate::document::BezierHandleMode::LeftOnly,
                            "right" | "rightonly" => crate::document::BezierHandleMode::RightOnly,
                            "asymmetric" => crate::document::BezierHandleMode::Asymmetric,
                            "equal" | "equallength" => crate::document::BezierHandleMode::EqualLength,
                            "sym" | "symmetric" => crate::document::BezierHandleMode::Symmetric,
                            _ => crate::document::BezierHandleMode::Both,
                        };
                    }
                    true
                } else { false }
            } else { false }
        } else { false };
        if changed {
            let after = self.project.anim_timeline.clone();
            self.history.push(&mut self.project, crate::history::ProjectEdit::PatchTimeline { before, after });
            self.apply_animation_for_frame(self.anim_current_frame);
            Ok(format!("Updated interpolation for {}@{}", property, frame))
        } else {
            Err("Keyframe not found or no change".into())
        }
    }

    fn mcp_clear_animation_track(&mut self, id_str: &str, property: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let before = self.project.anim_timeline.clone();
        let mut changed = false;
        if let Some(anim) = self.project.anim_timeline.nodes.get_mut(&id) {
            if let Some(track) = anim.get_track_mut(property) {
                if !track.keyframes.is_empty() {
                    track.keyframes.clear();
                    changed = true;
                }
            }
        }
        if changed {
            let after = self.project.anim_timeline.clone();
            self.history.push(&mut self.project, crate::history::ProjectEdit::PatchTimeline { before, after });
            self.apply_animation_for_frame(self.anim_current_frame);
            Ok(format!("Cleared track {}", property))
        } else {
            Ok("No keyframes to clear".into())
        }
    }

    fn mcp_set_keyframes(&mut self, kfs: &[serde_json::Value]) -> Result<String, String> {
        if kfs.is_empty() {
            return Ok("No keyframes provided".into());
        }
        let before = self.project.anim_timeline.clone();
        let mut count = 0usize;
        for kf in kfs {
            let id_str = kf.get("id").and_then(|v| v.as_str()).ok_or("id required in keyframe")?;
            let property = kf.get("property").and_then(|v| v.as_str()).ok_or("property required")?;
            let frame = kf.get("frame").and_then(|v| v.as_u64()).ok_or("frame required")? as usize;
            let value = kf.get("value").and_then(|v| v.as_f64()).ok_or("value required")?;
            let interp_str = kf.get("interpolation").and_then(|v| v.as_str()).unwrap_or("linear");
            let mode = match interp_str.to_lowercase().as_str() {
                "bezier" | "cubic" => crate::document::InterpolationMode::Bezier,
                _ => crate::document::InterpolationMode::Linear,
            };
            let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
            let entry = self.project.anim_timeline.nodes.entry(id).or_default();
            if let Some(track) = entry.get_track_mut(property) {
                if track.keyframes.is_empty() && frame != 0 {
                    // seed initial if needed (simplified)
                    track.insert(0, value);
                }
                track.insert(frame, value);
                if let Some(k) = track.keyframes.iter_mut().find(|k| k.frame == frame) {
                    k.interpolation = mode;
                }
                count += 1;
            }
        }
        self.apply_animation_for_frame(self.anim_current_frame);
        let after = self.project.anim_timeline.clone();
        if before != after {
            self.history.push(&mut self.project, crate::history::ProjectEdit::PatchTimeline { before, after });
        }
        Ok(format!("Set {} keyframes (batched)", count))
    }

    fn mcp_patch_node(&mut self, id_str: &str, patch: &serde_json::Value) -> Result<String, String> {
        use crate::document::NodeKind;
        use crate::mcp::drawing::apply_style_patch;
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        let before = self
            .project
            .nodes
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Object not found: {id_str}"))?;
        let mut after = before.clone();
        if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
            after.name = name.to_string();
        }
        apply_style_patch(&mut after.style, patch)?;
        if let Some(tx) = patch.get("translate_x").and_then(|v| v.as_f64()) {
            after.transform.translation[0] = tx;
        }
        if let Some(ty) = patch.get("translate_y").and_then(|v| v.as_f64()) {
            after.transform.translation[1] = ty;
        }
        if let Some(sx) = patch.get("scale_x").and_then(|v| v.as_f64()) {
            after.transform.scale[0] = sx;
        }
        if let Some(sy) = patch.get("scale_y").and_then(|v| v.as_f64()) {
            after.transform.scale[1] = sy;
        }
        if let Some(deg) = patch.get("rotation_deg").and_then(|v| v.as_f64()) {
            // Must bake geometry (set_rotation), not only store transform metadata.
            after.set_rotation(deg.to_radians());
        }
        Self::mcp_apply_geometry_patch(&mut after, patch)?;
        if before != after {
            self.history.push(
                &mut self.project,
                crate::history::ProjectEdit::PatchNode { id, before, after },
            );
        }
        Ok(format!("Updated object {id_str}"))
    }

    fn mcp_apply_geometry_patch(node: &mut crate::document::Node, patch: &serde_json::Value) -> Result<(), String> {
        use crate::document::{ArcJoin, NodeKind, PathData};
        use crate::mcp::drawing::parse_arc_join;
        match &mut node.kind {
            NodeKind::Rect { x, y, w, h, rx } => {
                if let Some(v) = patch.get("x").and_then(|v| v.as_f64()) {
                    *x = v;
                }
                if let Some(v) = patch.get("y").and_then(|v| v.as_f64()) {
                    *y = v;
                }
                if let Some(v) = patch.get("w").and_then(|v| v.as_f64()) {
                    *w = v.max(1.0);
                }
                if let Some(v) = patch.get("h").and_then(|v| v.as_f64()) {
                    *h = v.max(1.0);
                }
                if let Some(v) = patch.get("rx").and_then(|v| v.as_f64()) {
                    *rx = v.max(0.0);
                }
            }
            NodeKind::Ellipse { cx, cy, rx, ry } => {
                if let Some(v) = patch.get("cx").and_then(|v| v.as_f64()) {
                    *cx = v;
                }
                if let Some(v) = patch.get("cy").and_then(|v| v.as_f64()) {
                    *cy = v;
                }
                if let Some(v) = patch.get("rx").and_then(|v| v.as_f64()) {
                    *rx = v.max(0.5);
                }
                if let Some(v) = patch.get("ry").and_then(|v| v.as_f64()) {
                    *ry = v.max(0.5);
                }
                if let Some(v) = patch.get("r").and_then(|v| v.as_f64()) {
                    *rx = v.max(0.5);
                    *ry = v.max(0.5);
                }
            }
            NodeKind::Polygon {
                cx,
                cy,
                r,
                sides,
                rotation_rad,
            } => {
                if let Some(v) = patch.get("cx").and_then(|v| v.as_f64()) {
                    *cx = v;
                }
                if let Some(v) = patch.get("cy").and_then(|v| v.as_f64()) {
                    *cy = v;
                }
                if let Some(v) = patch.get("r").and_then(|v| v.as_f64()) {
                    *r = v.max(0.5);
                }
                if let Some(v) = patch.get("sides").and_then(|v| v.as_u64()) {
                    *sides = (v as u32).max(3);
                }
                if let Some(v) = patch.get("rotation_deg").and_then(|v| v.as_f64()) {
                    *rotation_rad = v.to_radians();
                }
            }
            NodeKind::Path { path } => {
                if let Some(d) = patch.get("svg_d").and_then(|v| v.as_str()) {
                    if let Ok(bez) = crate::mcp::path_parse::bez_from_svg_d(d) {
                        *path = crate::document::PathData::from_bez(&bez);
                    }
                }
                let (x0, y0, x1, y1) = (
                    patch.get("x0").and_then(|v| v.as_f64()),
                    patch.get("y0").and_then(|v| v.as_f64()),
                    patch.get("x1").and_then(|v| v.as_f64()),
                    patch.get("y1").and_then(|v| v.as_f64()),
                );
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (x0, y0, x1, y1) {
                    *path = PathData {
                        verbs: vec![0, 1],
                        points: vec![[x0, y0], [x1, y1]],
                        closed: false,
                        smooth_anchors: Vec::new(),
                        handle_out_offset: std::collections::HashMap::new(),
                        handle_in_offset: std::collections::HashMap::new(),
                        handle_modes: std::collections::HashMap::new(),
                        corner_fillets: std::collections::HashMap::new(),
                    };
                }
            }
            NodeKind::Arc {
                cx,
                cy,
                radius,
                start_angle_rad,
                sweep_angle_rad,
                join,
            } => {
                if let Some(v) = patch.get("cx").and_then(|v| v.as_f64()) {
                    *cx = v;
                }
                if let Some(v) = patch.get("cy").and_then(|v| v.as_f64()) {
                    *cy = v;
                }
                if let Some(v) = patch
                    .get("radius")
                    .or_else(|| patch.get("r"))
                    .and_then(|v| v.as_f64())
                {
                    *radius = v.max(0.5);
                }
                if let Some(v) = patch.get("start_angle_deg").and_then(|v| v.as_f64()) {
                    *start_angle_rad = v.to_radians();
                }
                if let Some(v) = patch.get("sweep_angle_deg").and_then(|v| v.as_f64()) {
                    *sweep_angle_rad = v.to_radians();
                }
                if patch.get("join").is_some() {
                    *join = parse_arc_join(patch.get("join").unwrap());
                }
            }
            NodeKind::Text { x, y, style } => {
                if let Some(v) = patch.get("x").and_then(|v| v.as_f64()) {
                    *x = v;
                }
                if let Some(v) = patch.get("y").and_then(|v| v.as_f64()) {
                    *y = v;
                }
                if let Some(t) = patch.get("text").and_then(|v| v.as_str()) {
                    style.content = t.to_string();
                }
                if let Some(v) = patch.get("font_size").and_then(|v| v.as_f64()) {
                    style.font_size = v.max(1.0) as f32;
                }
                if let Some(v) = patch
                    .get("width")
                    .or_else(|| patch.get("max_width"))
                    .and_then(|v| v.as_f64())
                {
                    style.width = v.max(0.0) as f32;
                }
            }
            NodeKind::Image {
                x,
                y,
                width,
                height,
                ..
            } => {
                // NE Output Object + image nodes: x,y,w,h (also accept width/height aliases).
                if let Some(v) = patch.get("x").and_then(|v| v.as_f64()) {
                    *x = v;
                }
                if let Some(v) = patch.get("y").and_then(|v| v.as_f64()) {
                    *y = v;
                }
                if let Some(v) = patch
                    .get("w")
                    .or_else(|| patch.get("width"))
                    .and_then(|v| v.as_f64())
                {
                    *width = v.max(1.0);
                }
                if let Some(v) = patch
                    .get("h")
                    .or_else(|| patch.get("height"))
                    .and_then(|v| v.as_f64())
                {
                    *height = v.max(1.0);
                }
            }
            _ => {}
        }
        Ok(())
    }


    fn mcp_update_object(&mut self, id_str: &str, patch: serde_json::Value) -> Result<(), String> {
        self.mcp_patch_node(id_str, &patch).map(|_| ())
    }

    fn mcp_delete_object(&mut self, id_str: &str) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(id_str).map_err(|e| e.to_string())?;
        if self.project.nodes.get(id).is_none() {
            return Err(format!("Object not found: {id_str}"));
        }
        self.selection = vec![id];
        self.delete_selection();
        Ok(())
    }

    fn process_pending_mcp_bulk_rects(&mut self) {
        #[cfg(target_os = "android")]
        { return; }
        #[cfg(not(target_os = "android"))]
        {
            const MAX_PER_FRAME: usize = 64;
            let mut chunk: Vec<Node> = Vec::new();
            while chunk.len() < MAX_PER_FRAME {
                if let Some(mut batch) = self.pending_mcp_bulk_rects.pop() {
                    let take = MAX_PER_FRAME - chunk.len();
                    if batch.len() <= take {
                        chunk.append(&mut batch);
                    } else {
                        let rest = batch.split_off(take);
                        chunk.append(&mut batch);
                        self.pending_mcp_bulk_rects.push(rest);
                    }
                } else {
                    break;
                }
            }
            if !chunk.is_empty() {
                Self::apply_nodes_live(&mut self.project, &chunk);
                self.mcp_bulk_staging.extend(chunk);
            }
            if self.pending_mcp_bulk_rects.is_empty() && !self.mcp_bulk_staging.is_empty() {
                let last_id = self.mcp_bulk_staging.last().map(|n| n.id);
                let nodes = std::mem::take(&mut self.mcp_bulk_staging);
                self.history.push_applied(
                    &mut self.project,
                    ProjectEdit::InsertNodesApplied { nodes },
                );
                if let Some(id) = last_id {
                    self.selection = vec![id];
                    self.sync_inspector_from_selection();
                }
            }
        }
    }

    pub(crate) fn poll_mcp_bridge(&mut self) {
        #[cfg(target_os = "android")]
        {
            return;
        }
        #[cfg(not(target_os = "android"))]
        {
            // Drain any completed heavy captures from bg threads (preview side effects)
            while let Ok(up) = self.mcp_preview_update_rx.try_recv() {
                self.mcp_preview.rgba = up.rgba;
                self.mcp_preview.width = up.width;
                self.mcp_preview.height = up.height;
                self.mcp_preview.bounds = up.bounds;
                self.mcp_preview.resolution_percent = up.resolution_percent;
                self.mcp_preview.texture = None;
                self.mcp_preview.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
            }

            let pending = match self.mcp_bridge.as_mut() {
                Some(bridge) => bridge.drain_pending(),
                None => return,
            };
            for (req, reply_tx) in pending {
                // Heavy rasterize off main thread to avoid blocking egui paint lock (prevents 10s RwLock deadlock panic)
                if let crate::mcp::McpHostRequest::CaptureCanvasRaster {
                    resolution_percent,
                    x,
                    y,
                    w,
                    h,
                    save_path,
                } = req
                {
                    let proj = self.project.clone();
                    let anim_frame = self.anim_current_frame;
                    let preview_tx = self.mcp_preview_update_tx.clone();
                    std::thread::spawn(move || {
                        use base64::Engine;
                        let view = crate::io::resolve_capture_view(&proj, x, y, w, h);
                        let pct = resolution_percent.clamp(1.0, 100.0);
                        if let Some((pw, ph, rgba)) = crate::io::rasterize_document_view(
                            &proj,
                            view,
                            pct,
                            anim_frame,
                            &std::collections::HashMap::new(),
                        ) {
                            if let Some(p) = save_path {
                                let path = std::path::PathBuf::from(p);
                                let _ = crate::io::write_image_file(
                                    &path,
                                    crate::io::ExportImageFormat::Png,
                                    pw,
                                    ph,
                                    &rgba,
                                );
                            }
                            // update preview (non blocking send)
                            let _ = preview_tx.send(McpPreviewUpdate {
                                rgba: rgba.clone(),
                                width: pw,
                                height: ph,
                                bounds: [view.x0, view.y0, view.width(), view.height()],
                                resolution_percent: pct,
                            });
                            // compute response and send from this thread
                            if let Some(png) = image::RgbaImage::from_raw(pw, ph, rgba.clone()) {
                                let mut buf = Vec::new();
                                if png
                                    .write_to(
                                        &mut std::io::Cursor::new(&mut buf),
                                        image::ImageFormat::Png,
                                    )
                                    .is_ok()
                                {
                                    let b64 =
                                        base64::engine::general_purpose::STANDARD.encode(&buf);
                                    let meta = serde_json::json!({
                                        "pixel_width": pw,
                                        "pixel_height": ph,
                                        "resolution_percent": pct,
                                        "bounds": { "x": view.x0, "y": view.y0, "w": view.width(), "h": view.height() },
                                        "document_size": { "w": proj.document.width, "h": proj.document.height },
                                        "objects_remain_editable": true,
                                    });
                                    let resp = crate::mcp::McpHostResponse::RasterPreview {
                                        meta_json: serde_json::to_string_pretty(&meta)
                                            .unwrap_or_default(),
                                        png_base64: b64,
                                    };
                                    let _ = reply_tx.send(resp);
                                    return;
                                }
                            }
                        }
                        let _ = reply_tx.send(crate::mcp::McpHostResponse::Err {
                            message: "Capture raster failed".into(),
                        });
                    });
                    continue;
                }

                let resp = self.handle_mcp_request(req);
                let _ = reply_tx.send(resp);
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    fn handle_mcp_request(
        &mut self,
        req: crate::mcp::McpHostRequest,
    ) -> crate::mcp::McpHostResponse {
        match req {
            crate::mcp::McpHostRequest::Snapshot => crate::mcp::McpHostResponse::Snapshot(
                crate::mcp::McpAppSnapshot {
                    title: self.project.document.title.clone(),
                    project_path: self
                        .project_save_path
                        .as_ref()
                        .map(|p| p.display().to_string()),
                    status_message: self.status_message.clone(),
                    collab_text: self.collab.chat_log_plain(),
                    anim_frame: self.anim_current_frame,
                    anim_playing: self.anim_is_playing,
                    ui_fps: self.ui_fps,
                },
            ),
            crate::mcp::McpHostRequest::SaveProject { path } => {
                if let Some(p) = path.as_deref().map(std::path::Path::new) {
                    match self.save_project_to_path(p) {
                        Ok(()) => crate::mcp::McpHostResponse::Ok {
                            message: format!("Saved {}", p.display()),
                        },
                        Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                    }
                } else if let Some(p) = self.project_save_path.clone() {
                    match self.save_project_to_path(&p) {
                        Ok(()) => crate::mcp::McpHostResponse::Ok {
                            message: format!("Saved {}", p.display()),
                        },
                        Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                    }
                } else {
                    self.pending_save_project = true;
                    crate::mcp::McpHostResponse::Ok {
                        message: "No path set — opened Save dialog on UI thread".into(),
                    }
                }
            }
            crate::mcp::McpHostRequest::SetTitle(title) => {
                self.set_document_title(title);
                crate::mcp::McpHostResponse::Ok {
                    message: "Title updated".into(),
                }
            }
            crate::mcp::McpHostRequest::GetCollabText => {
                crate::mcp::McpHostResponse::Text(self.collab.chat_log_plain())
            }
            crate::mcp::McpHostRequest::SetCollabText(text) => {
                self.collab.send_chat(text);
                crate::mcp::McpHostResponse::Ok {
                    message: "Chat message sent".into(),
                }
            }
            crate::mcp::McpHostRequest::ProjectJson => {
                match serde_json::to_string_pretty(&self.project) {
                    Ok(j) => crate::mcp::McpHostResponse::Text(j),
                    Err(e) => crate::mcp::McpHostResponse::Err {
                        message: e.to_string(),
                    },
                }
            }
            crate::mcp::McpHostRequest::CaptureCanvasRaster {
                resolution_percent,
                x,
                y,
                w,
                h,
                save_path,
            } => match self.mcp_capture_canvas_raster(
                resolution_percent,
                x,
                y,
                w,
                h,
                save_path,
            ) {
                Ok(resp) => resp,
                Err(e) => crate::mcp::McpHostResponse::Err { message: e },
            },
            crate::mcp::McpHostRequest::ListAllObjects => {
                match self.mcp_list_all_objects_json() {
                    Ok(j) => crate::mcp::McpHostResponse::Text(j),
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::ListObjects => {
                match self.mcp_list_objects_json() {
                    Ok(j) => crate::mcp::McpHostResponse::Text(j),
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::GetObject { id } => {
                match self.mcp_get_object_json(&id) {
                    Ok(j) => crate::mcp::McpHostResponse::Text(j),
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::DrawingTool { name, args } => {
                match self.mcp_drawing_tool(&name, args) {
                    Ok(msg) => crate::mcp::McpHostResponse::Ok { message: msg },
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::UpdateObject { id, patch } => {
                match self.mcp_update_object(&id, patch) {
                    Ok(()) => crate::mcp::McpHostResponse::Ok {
                        message: format!("Updated {id}"),
                    },
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::DeleteObject { id } => {
                match self.mcp_delete_object(&id) {
                    Ok(()) => crate::mcp::McpHostResponse::Ok {
                        message: format!("Deleted {id}"),
                    },
                    Err(e) => crate::mcp::McpHostResponse::Err { message: e },
                }
            }
            crate::mcp::McpHostRequest::UiHealth => {
                // Count by kind for diagnosis — never materialize full text content.
                let mut kind_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                let mut max_text_chars: usize = 0;
                let mut total_text_chars: usize = 0;
                let mut max_name_chars: usize = 0;
                for node in self.project.nodes.map.values() {
                    let k = match &node.kind {
                        NodeKind::Rect { .. } => "rect",
                        NodeKind::Ellipse { .. } => "ellipse",
                        NodeKind::Text { style, .. } => {
                            // len() is O(1); avoid chars().count() on multi-MB strings every health poll.
                            let n = style.content.len();
                            max_text_chars = max_text_chars.max(n);
                            total_text_chars = total_text_chars.saturating_add(n);
                            "text"
                        }
                        NodeKind::Path { .. } => "path",
                        NodeKind::FlowchartPath { .. } => "flowchart_path",
                        NodeKind::FlowchartNode { .. } => "flowchart_node",
                        NodeKind::Polygon { .. } => "polygon",
                        NodeKind::Image { .. } => "image",
                        NodeKind::Plotter { .. } => "plotter",
                        NodeKind::Group { .. } => "group",
                        NodeKind::Arc { .. } => "arc",
                        NodeKind::BrushStroke { .. } => "brush",
                    }
                    .to_string();
                    *kind_counts.entry(k).or_default() += 1;
                    max_name_chars = max_name_chars.max(node.name.len());
                }
                let path_count = *kind_counts.get("path").unwrap_or(&0);
                let text_count = *kind_counts.get("text").unwrap_or(&0);
                let rect_count = *kind_counts.get("rect").unwrap_or(&0);
                let object_count = self.project.nodes.map.len();
                let fps = self.ui_fps;
                let long_text = max_text_chars > 8_192;
                let cpu_stress = fps < 25.0 && (object_count > 150 || long_text);
                let path_heavy = path_count > 80;
                let mut hints: Vec<String> = Vec::new();
                if cpu_stress {
                    hints.push(format!(
                        "Low UI FPS ({fps:.1}) with {object_count} objects — CPU-bound canvas paint."
                    ));
                }
                if long_text {
                    hints.push(format!(
                        "Very long text object (~{max_text_chars} bytes). Prefer shorter labels or split; \
                         list_objects truncates names/previews to avoid JSON bloat."
                    ));
                }
                if path_heavy {
                    hints.push(format!(
                        "{path_count} path nodes (each MCP create_line is a separate path). \
                         Prefer one create_path with M/C cubic beziers per curve instead of hundreds of segments."
                    ));
                }
                if object_count > 200 && !self.enable_layer_raster_cache {
                    hints.push(
                        "Dense rect-only layers: enable View → Layer raster cache; keep off for text-heavy docs."
                            .into(),
                    );
                } else if object_count > 200
                    && self.enable_layer_raster_cache
                    && self.layer_raster_cache.is_empty()
                {
                    hints.push(
                        "Layer raster cache on but inactive (text on layer, drag, or <150 nodes per layer)."
                            .into(),
                    );
                }
                if cpu_stress {
                    hints.push(
                        "Run optimized binary: cargo build --release (fat LTO, codegen-units=1 in Cargo.toml)."
                            .into(),
                    );
                }
                let suggestion = if hints.is_empty() {
                    String::new()
                } else {
                    hints.join(" ")
                };
                let health = serde_json::json!({
                    "fps": fps,
                    "cpu_stress": cpu_stress,
                    "object_count": object_count,
                    "kind_counts": kind_counts,
                    "path_count": path_count,
                    "text_count": text_count,
                    "rect_count": rect_count,
                    "max_text_bytes": max_text_chars,
                    "total_text_bytes": total_text_chars,
                    "max_name_bytes": max_name_chars,
                    "current_anim_frame": self.anim_current_frame,
                    "anim_playing": self.anim_is_playing,
                    "num_animated_nodes": self.project.anim_timeline.nodes.len(),
                    "layer_raster_cache_enabled": self.enable_layer_raster_cache,
                    "layer_raster_cache_count": self.layer_raster_cache.len(),
                    "layer_raster_cache_pending": self.layer_cache_pending.len(),
                    "spatial_index_enabled": self.spatial_index.is_enabled(),
                    "history_revision": self.history.revision(),
                    "suggestion_for_low_fps": suggestion,
                });
                crate::mcp::McpHostResponse::Text(serde_json::to_string_pretty(&health).unwrap_or_default())
            }
        }
    }
}

impl eframe::App for VadadeeBerryApp {
    fn on_exit(&mut self) {
        // Stop capture / audio first so we don't delete WAVs still held open.
        #[cfg(not(target_os = "android"))]
        {
            self.screen_captures.clear();
        }
        self.audio_players.clear();
        self.audio_player_media_path.clear();
        if let Ok(mut m) = self.audio_extract_status.lock() {
            m.clear();
        }
        if let Ok(mut m) = self.audio_pcm_cache.lock() {
            m.clear();
        }
        #[cfg(not(target_os = "android"))]
        purge_vadadee_disk_caches(CachePurgeOpts::on_exit());
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.tick_flood_fill_anim(ctx);
        let painting = self.tools.raster.painting
            || (self.tools.active == ToolKind::Brush && !self.tools.brush.points.is_empty())
            || self.flood_fill_anim.is_some();
        // While freehand is active, skip non-essential background work so every frame
        // can sample the pointer (smooth strokes need high sample rate, not perfect caches).
        if !painting {
            self.update_clip_mask_textures(ctx);
            // Keep boolean result paths in sync when operands move (cheap if no effects).
            if !self.project.document.boolean_effects.is_empty() {
                self.refresh_boolean_effects_live();
            }
            self.update_layer_raster_cache(ctx);
        }
        if self.cached_draw_order_revision != self.history.revision() {
            self.rebuild_spatial_index();
        }
        // Graph editor transition animation tick
        let dt = ctx.input(|i| i.stable_dt);
        let target_t = if self.anim_graph_editor_track.is_some() && self.anim_graph_editor_target_track.is_none() {
            1.0
        } else {
            0.0
        };

        if (self.anim_graph_editor_t - target_t).abs() > 0.001 {
            let speed = 6.0;
            if self.anim_graph_editor_t < target_t {
                self.anim_graph_editor_t = (self.anim_graph_editor_t + dt * speed).min(target_t);
            } else {
                self.anim_graph_editor_t = (self.anim_graph_editor_t - dt * speed).max(target_t);
            }
            ctx.request_repaint();
        } else {
            self.anim_graph_editor_t = target_t;
            if target_t == 0.0 && self.anim_graph_editor_target_track.is_some() {
                self.anim_graph_editor_track = self.anim_graph_editor_target_track.take();
                ctx.request_repaint();
            }
        }

        let piano_target = if self.piano_roll_clip.is_some() { 1.0 } else { 0.0 };
        if (self.piano_roll_t - piano_target).abs() > 0.001 {
            let speed = 6.0;
            if self.piano_roll_t < piano_target {
                self.piano_roll_t = (self.piano_roll_t + dt * speed).min(piano_target);
            } else {
                self.piano_roll_t = (self.piano_roll_t - dt * speed).max(piano_target);
            }
            ctx.request_repaint();
        } else {
            self.piano_roll_t = piano_target;
        }

        // --- UI FPS tracking for health check ---
        let dt = ctx.input(|i| i.stable_dt);
        if dt > 0.0001 {
            let instant = 1.0 / dt;
            self.ui_fps = self.ui_fps * 0.85 + instant * 0.15;
        }

        // --- ANIMATION TIMELINE PLAYBACK & RECORDING ---
        // Wall-clock advance so play continues when the window is unfocused / another workspace.
        let mut frame_changed = false;
        let window_focused = ctx.input(|i| i.focused);
        // NE graph audio free-runs in rodio at real time — playhead must match wall clock
        // or audio drifts ahead of the timeline (or restarts late after prepare).
        let ne_audio_playing = self.anim_is_playing
            && self.project.document.layers.iter().any(|l| {
                l.visible
                    && l.kind == crate::document::LayerKind::NodeEditor
                    && l.node_graph
                        .as_ref()
                        .and_then(|g| g.resolve_output_sound().path().map(|p| !p.is_empty()))
                        .unwrap_or(false)
            });
        if self.anim_is_playing {
            let now = std::time::Instant::now();
            if self.anim_play_origin.is_none() {
                self.anim_play_origin = Some((now, self.anim_current_frame));
            }
            let (origin_t, origin_f) = self.anim_play_origin.unwrap();
            self.anim_playback_wall = Some(now);

            let fps = (self.anim_fps as f32).max(1.0);
            let max_frame = self.get_content_max_animation_frame();
            let span = max_frame.saturating_add(1).max(1);
            let elapsed = now.duration_since(origin_t).as_secs_f32().clamp(0.0, 3600.0);
            let ideal = (origin_f.saturating_add((elapsed * fps).floor() as usize)) % span;
            let cur = self.anim_current_frame % span;

            if ne_audio_playing {
                // Absolute wall-clock: timeline tracks audio real-time (may skip visual frames).
                // Do NOT rebase origin each tick — that slowed the playhead while audio ran free.
                if ideal != cur {
                    self.anim_current_frame = ideal;
                    frame_changed = true;
                }
            } else {
                // No graph audio: cap steps so a slow UI never teleports 4→34 in one paint.
                let behind = if ideal >= cur {
                    ideal - cur
                } else {
                    (span - cur) + ideal
                };
                if behind > 0 {
                    let steps = behind.min(2);
                    self.anim_current_frame = (cur + steps) % span;
                    frame_changed = true;
                    self.anim_play_origin = Some((now, self.anim_current_frame));
                }
            }

            if window_focused {
                ctx.request_repaint();
            } else if ne_audio_playing {
                // Keep audio+timeline alive off-workspace (~30 Hz) so resume isn't cold.
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(1000));
            }
        } else {
            self.anim_playback_wall = None;
            self.anim_play_origin = None;
        }

        let frame_scrubbed = self.anim_current_frame != self.anim_last_seen_frame;
        // Never re-apply animation mid-drag — it would fight live position / path-point edits.
        let dragging = self.is_live_geometry_editing();
        if (frame_scrubbed || frame_changed) && !dragging {
            self.apply_animation_for_frame(self.anim_current_frame);
            self.anim_last_seen_frame = self.anim_current_frame;
            self.anim_last_applied_states.clear();
            for id in &self.selection {
                if let Some(node) = self.project.nodes.get(*id) {
                    let gf = self.get_node_geom_floats(*id);
                    self.anim_last_applied_states.insert(*id, AnimAppliedState {
                        pos: node.get_pos(),
                        rotation: node.get_rotation(),
                        opacity: node.get_opacity(),
                        color: node.get_color(),
                        stroke_width: node.get_stroke_width(),
                        stroke_color: node.get_stroke_color(),
                        geom_floats: gf,
                        fill: node.style.fill.clone(),
                    });
                }
            }
        }

        // Node Editor algebra (Value / Frame / Time / Expr / ParamReal) every frame.
        self.eval_node_editor_graphs();

        // Record keyframes while keyframing: whole-object drag, path node drag, weight-flow sculpt.
        if self.anim_keyframing_mode && !self.anim_is_playing && self.is_live_geometry_editing() {
            // Ensure reference state is populated
            for id in &self.selection {
                if let Some(node) = self.project.nodes.get(*id) {
                    let gf = self.get_node_geom_floats(*id);
                    self.anim_last_applied_states.entry(*id).or_insert_with(|| AnimAppliedState {
                        pos: node.get_pos(),
                        rotation: node.get_rotation(),
                        opacity: node.get_opacity(),
                        color: node.get_color(),
                        stroke_width: node.get_stroke_width(),
                        stroke_color: node.get_stroke_color(),
                        geom_floats: gf,
                        fill: node.style.fill.clone(),
                    });
                } else if let Some(layer) = self.project.document.layers.iter().find(|l| l.id == *id && l.kind == crate::document::LayerKind::AV) {
                    self.anim_last_applied_states.entry(*id).or_insert_with(|| AnimAppliedState {
                        pos: (layer.x as f64, layer.y as f64),
                        rotation: layer.rotation as f64,
                        opacity: 1.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                        stroke_width: 0.0,
                        stroke_color: [0.0, 0.0, 0.0, 0.0],
                        geom_floats: vec![],
                        fill: Fill::default(),
                    });
                }
            }

            // Detect user modifications
            let mut keyframes_updated = false;
            for id in &self.selection {
                if let Some(node) = self.project.nodes.get(*id) {
                    let pos = node.get_pos();
                    let rot = node.get_rotation();
                    let op = node.get_opacity();
                    let color = node.get_color();
                    let stroke_w = node.get_stroke_width();
                    let stroke_col = node.get_stroke_color();
                    let geom = self.get_node_geom_floats(*id);
                    
                    let last_state = self.anim_last_applied_states.get(id);
                    if let Some(last) = last_state {
                        let mut changed_pos = false;
                        let mut changed_rot = false;
                        let mut changed_op = false;
                        let mut changed_col = false;
                        let mut changed_stroke_w = false;
                        let mut changed_stroke_col = false;
                        let mut changed_geom = false;
                        
                        let mut temp_node = node.clone();
                        temp_node.set_rotation(last.rotation);
                        let unrot_pos = temp_node.get_pos();
                        
                        let dx = unrot_pos.0 - last.pos.0;
                        let dy = unrot_pos.1 - last.pos.1;
                        if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
                            changed_pos = true;
                            temp_node.translate(-dx, -dy);
                        }
                        
                        if (rot - last.rotation).abs() > 1e-9 {
                            changed_rot = true;
                        }
                        
                        if (op - last.opacity).abs() > 1e-6 {
                            changed_op = true;
                        }
                        
                        for i in 0..4 {
                            if (color[i] - last.color[i]).abs() > 1e-6 {
                                changed_col = true;
                            }
                        }
                        if (stroke_w - last.stroke_width).abs() > 1e-6 {
                            changed_stroke_w = true;
                        }
                        for i in 0..4 {
                            if (stroke_col[i] - last.stroke_color[i]).abs() > 1e-6 {
                                changed_stroke_col = true;
                            }
                        }
                        
                        // Compare live geom (path anchors / handles) directly — do not use
                        // temp_node after un-translate (that shifts all path points and hides edits).
                        let mut geom_really_changed = false;
                        if geom.len() == last.geom_floats.len() {
                            for i in 0..geom.len() {
                                if (geom[i] - last.geom_floats[i]).abs() > 1e-6 {
                                    geom_really_changed = true;
                                    break;
                                }
                            }
                        } else if !geom.is_empty() {
                            geom_really_changed = true;
                        }
                        if geom_really_changed {
                            changed_geom = true;
                        }
                        
                        if changed_pos
                            || changed_rot
                            || changed_op
                            || changed_col
                            || changed_stroke_w
                            || changed_stroke_col
                            || changed_geom
                        {
                            let before_timeline = self.project.anim_timeline.clone();
                            let entry = self.project.anim_timeline.nodes.entry(*id).or_default();
                            
                            if changed_pos {
                                // Seed a baseline only when recording a later frame (not frame 0),
                                // so the first edit at the beginning is the single keyframe.
                                if entry.pos_x.keyframes.is_empty() && self.anim_current_frame > 0 {
                                    entry.pos_x.insert(0, last.pos.0);
                                }
                                if entry.pos_y.keyframes.is_empty() && self.anim_current_frame > 0 {
                                    entry.pos_y.insert(0, last.pos.1);
                                }
                                entry.pos_x.insert(self.anim_current_frame, pos.0);
                                entry.pos_y.insert(self.anim_current_frame, pos.1);
                                entry.sync_stack_starts_from_keyframes();
                                entry.ensure_stack_start_keyframes();
                                entry.ensure_stack_end_keyframes();
                            }
                            if changed_rot {
                                if entry.rotation.keyframes.is_empty() && self.anim_current_frame > 0 {
                                    entry.rotation.insert(0, last.rotation);
                                }
                                entry.rotation.insert(self.anim_current_frame, rot);
                            }
                            if changed_op {
                                if entry.opacity.keyframes.is_empty() && self.anim_current_frame > 0 {
                                    entry.opacity.insert(0, last.opacity as f64);
                                }
                                entry.opacity.insert(self.anim_current_frame, op as f64);
                            }
                            if changed_col {
                                if entry.color_r.keyframes.is_empty() && self.anim_current_frame > 0 {
                                    entry.color_r.insert(0, last.color[0] as f64);
                                    entry.color_g.insert(0, last.color[1] as f64);
                                    entry.color_b.insert(0, last.color[2] as f64);
                                    entry.color_a.insert(0, last.color[3] as f64);
                                }
                                entry.color_r.insert(self.anim_current_frame, color[0] as f64);
                                entry.color_g.insert(self.anim_current_frame, color[1] as f64);
                                entry.color_b.insert(self.anim_current_frame, color[2] as f64);
                                entry.color_a.insert(self.anim_current_frame, color[3] as f64);
                            }
                            if changed_stroke_w {
                                if entry.stroke_width.keyframes.is_empty()
                                    && self.anim_current_frame > 0
                                {
                                    entry
                                        .stroke_width
                                        .insert(0, last.stroke_width as f64);
                                }
                                entry
                                    .stroke_width
                                    .insert(self.anim_current_frame, stroke_w as f64);
                            }
                            if changed_stroke_col {
                                if entry.stroke_r.keyframes.is_empty() && self.anim_current_frame > 0
                                {
                                    entry.stroke_r.insert(0, last.stroke_color[0] as f64);
                                    entry.stroke_g.insert(0, last.stroke_color[1] as f64);
                                    entry.stroke_b.insert(0, last.stroke_color[2] as f64);
                                    entry.stroke_a.insert(0, last.stroke_color[3] as f64);
                                }
                                entry
                                    .stroke_r
                                    .insert(self.anim_current_frame, stroke_col[0] as f64);
                                entry
                                    .stroke_g
                                    .insert(self.anim_current_frame, stroke_col[1] as f64);
                                entry
                                    .stroke_b
                                    .insert(self.anim_current_frame, stroke_col[2] as f64);
                                entry
                                    .stroke_a
                                    .insert(self.anim_current_frame, stroke_col[3] as f64);
                            }
                            if changed_geom {
                                while entry.geom_tracks.len() < geom.len() {
                                    entry.geom_tracks.push(KeyframeTrack::default());
                                }
                                for i in 0..geom.len() {
                                    let baseline = if i < last.geom_floats.len() { last.geom_floats[i] } else { geom[i] };
                                    if entry.geom_tracks[i].keyframes.is_empty() && self.anim_current_frame > 0 {
                                        entry.geom_tracks[i].insert(0, baseline);
                                    }
                                    entry.geom_tracks[i].insert(self.anim_current_frame, geom[i]);
                                }
                            }

                            let after_timeline = self.project.anim_timeline.clone();
                            self.history.push(
                                &mut self.project,
                                ProjectEdit::PatchTimeline { before: before_timeline, after: after_timeline },
                            );
                            
                            keyframes_updated = true;
                        }
                    }
                } else if let Some(layer) = self.project.document.layers.iter().find(|l| l.id == *id && l.kind == crate::document::LayerKind::AV) {
                    let pos = (layer.x as f64, layer.y as f64);
                    let rot = layer.rotation as f64;
                    let last_state = self.anim_last_applied_states.get(id);
                    if let Some(last) = last_state {
                        let mut changed_pos = false;
                        let mut changed_rot = false;
                        
                        let dx = pos.0 - last.pos.0;
                        let dy = pos.1 - last.pos.1;
                        if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
                            changed_pos = true;
                        }
                        if (rot - last.rotation).abs() > 1e-9 {
                            changed_rot = true;
                        }
                        
                        if changed_pos || changed_rot {
                            let before_timeline = self.project.anim_timeline.clone();
                            let entry = self.project.anim_timeline.nodes.entry(*id).or_default();
                            if changed_pos {
                                if entry.pos_x.keyframes.is_empty() {
                                    entry.pos_x.insert(0, last.pos.0);
                                }
                                if entry.pos_y.keyframes.is_empty() {
                                    entry.pos_y.insert(0, last.pos.1);
                                }
                                entry.pos_x.insert(self.anim_current_frame, pos.0);
                                entry.pos_y.insert(self.anim_current_frame, pos.1);
                                entry.sync_stack_starts_from_keyframes();
                                entry.ensure_stack_start_keyframes();
                                entry.ensure_stack_end_keyframes();
                            }
                            if changed_rot {
                                if entry.rotation.keyframes.is_empty() {
                                    entry.rotation.insert(0, last.rotation);
                                }
                                entry.rotation.insert(self.anim_current_frame, rot);
                            }
                            let after_timeline = self.project.anim_timeline.clone();
                            self.history.push(
                                &mut self.project,
                                ProjectEdit::PatchTimeline {
                                    before: before_timeline,
                                    after: after_timeline,
                                },
                            );
                            keyframes_updated = true;
                        }
                    }
                }
            }
            if keyframes_updated {
                self.anim_last_applied_states.clear();
                for id in &self.selection {
                    if let Some(node) = self.project.nodes.get(*id) {
                        let gf = self.get_node_geom_floats(*id);
                        self.anim_last_applied_states.insert(*id, AnimAppliedState {
                            pos: node.get_pos(),
                            rotation: node.get_rotation(),
                            opacity: node.get_opacity(),
                            color: node.get_color(),
                            stroke_width: node.get_stroke_width(),
                            stroke_color: node.get_stroke_color(),
                            geom_floats: gf,
                            fill: node.style.fill.clone(),
                        });
                    } else if let Some(layer) = self.project.document.layers.iter().find(|l| l.id == *id && l.kind == crate::document::LayerKind::AV) {
                        self.anim_last_applied_states.insert(*id, AnimAppliedState {
                            pos: (layer.x as f64, layer.y as f64),
                            rotation: layer.rotation as f64,
                            opacity: 1.0,
                            color: [1.0, 1.0, 1.0, 1.0],
                            stroke_width: 0.0,
                            stroke_color: [0.0, 0.0, 0.0, 0.0],
                            geom_floats: vec![],
                            fill: Fill::default(),
                        });
                    }
                }
            }
        }

        // Manage Animation action tab availability dynamically
        let has_anim_tab = self.action_tab_order.contains(&ui::ActionTab::Animation);
        if self.anim_show_timeline_window {
            if !has_anim_tab {
                self.action_tab_order.push(ui::ActionTab::Animation);
            }
        } else {
            if has_anim_tab {
                self.action_tab_order.retain(|t| *t != ui::ActionTab::Animation);
                if self.action_tab == ui::ActionTab::Animation {
                    self.action_tab = ui::ActionTab::Layer; // Fallback
                }
            }
        }

        #[cfg(target_os = "android")]
        {
            if let Some(id) = self.on_page_text_edit {
                if let Some(android_app) = crate::ANDROID_APP.get() {
                    let state = android_app.text_input_state();
                    if state.text != self.last_android_text {
                        self.ui_text_content = state.text.clone();
                        self.last_android_text = state.text.clone();
                        self.patch_on_page_text_live(id);
                        ctx.request_repaint();
                    } else if self.ui_text_content != self.last_android_text {
                        let text = self.ui_text_content.clone();
                        self.last_android_text = text.clone();
                        let len = text.chars().count();
                        let new_state = winit::platform::android::activity::input::TextInputState {
                            text: text.clone(),
                            selection: winit::platform::android::activity::input::TextSpan { start: len, end: len },
                            compose_region: None,
                        };
                        android_app.set_text_input_state(new_state);
                    }
                }
            }
        }

        self.process_file_dialogs();
        if self.paste_progress.is_some() {
            self.advance_paste_operation(ctx);
        }
        let paste_from_events = self.handle_object_clipboard_shortcuts(ctx);
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        self.handle_paste_hotkey_fallback(ctx, paste_from_events);
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        self.handle_text_paste_fallback(ctx);
        if self.ui_anim.needs_repaint() || self.paste_progress.is_some() {
            ctx.request_repaint();
        }
        self.update_text_pan_animation(ctx);
        self.poll_video_export(ctx);
        self.keyboard_shortcuts(ctx);
        self.canvas_wheel_zoom(ctx);
        if self.sync_audio_playback() {
            ctx.request_repaint();
        }
        #[cfg(not(target_os = "android"))]
        {
            // Screen record mouse track is global (X11 / evdev) — not the app window.
            // Keep UI warm while recording so stop / status stay responsive.
            if !self.screen_captures.is_empty() {
                crate::screen_capture::clear_app_pointer();
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        self.update_window_focus_status(ctx);
        self.tick_live_collaboration_poll(ctx);
        self.poll_mcp_bridge();
        self.process_pending_mcp_bulk_rects();
        self.sync_window_title(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        ui::chrome(self, ui);
    }
}

/// Drop closed rectangular subpaths whose center falls inside any erase stamp.
fn strip_pixel_rects_from_bez(
    bez: &kurbo::BezPath,
    erase: &[(f64, f64, f64, f64)],
) -> kurbo::BezPath {
    use kurbo::{BezPath, PathEl, Point};
    if erase.is_empty() {
        return bez.clone();
    }

    let should_drop = |start: Option<Point>, pts: &[Point], closed: bool| -> bool {
        if !closed {
            return false;
        }
        let mut all = Vec::new();
        if let Some(s) = start {
            all.push(s);
        }
        all.extend_from_slice(pts);
        if all.len() < 4 {
            return false;
        }
        let min_x = all.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = all.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = all.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = all.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let w = max_x - min_x;
        let h = max_y - min_y;
        if w <= 1e-6 || h <= 1e-6 {
            return false;
        }
        let is_rect = all.iter().all(|p| {
            let on_v = (p.x - min_x).abs() < 1e-3 || (p.x - max_x).abs() < 1e-3;
            let on_h = (p.y - min_y).abs() < 1e-3 || (p.y - max_y).abs() < 1e-3;
            on_v && on_h
        });
        if !is_rect {
            return false;
        }
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        erase.iter().any(|&(ex, ey, ew, eh)| {
            (cx - ex).abs() <= (w + ew) * 0.5 + 1e-3
                && (cy - ey).abs() <= (h + eh) * 0.5 + 1e-3
        })
    };

    let mut out = BezPath::new();
    let mut sub_start: Option<Point> = None;
    let mut sub_pts: Vec<Point> = Vec::new();

    let emit = |start: Option<Point>, pts: &[Point], closed: bool, out: &mut BezPath| {
        if start.is_none() && pts.is_empty() {
            return;
        }
        if should_drop(start, pts, closed) {
            return;
        }
        if let Some(s) = start {
            out.move_to(s);
            for p in pts {
                out.line_to(*p);
            }
            if closed {
                out.close_path();
            }
        }
    };

    for el in bez.elements() {
        match el {
            PathEl::MoveTo(p) => {
                emit(sub_start, &sub_pts, false, &mut out);
                sub_start = Some(*p);
                sub_pts.clear();
            }
            PathEl::LineTo(p) => sub_pts.push(*p),
            PathEl::QuadTo(_, p) => sub_pts.push(*p),
            PathEl::CurveTo(_, _, p) => sub_pts.push(*p),
            PathEl::ClosePath => {
                emit(sub_start, &sub_pts, true, &mut out);
                sub_start = None;
                sub_pts.clear();
            }
        }
    }
    emit(sub_start, &sub_pts, false, &mut out);
    out
}

/// Parse brush point: `{x,y}` or `[x,y]`.
fn mcp_brush_xy(v: &serde_json::Value) -> Option<(f64, f64)> {
    if let Some(arr) = v.as_array() {
        if arr.len() >= 2 {
            return Some((arr[0].as_f64()?, arr[1].as_f64()?));
        }
        return None;
    }
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    Some((x, y))
}

/// Pattern cell → Some(rgb, alpha) or None if empty.
fn mcp_brush_cell_color(
    cell: &serde_json::Value,
    default_rgb: u32,
    default_alpha: f32,
) -> Option<(u32, f32)> {
    use crate::mcp::drawing::parse_color_value;
    if cell.is_null() {
        return None;
    }
    if let Some(b) = cell.as_bool() {
        return if b {
            Some((default_rgb, default_alpha))
        } else {
            None
        };
    }
    if let Some(n) = cell.as_i64() {
        if n == 0 {
            return None;
        }
        if n == 1 {
            return Some((default_rgb, default_alpha));
        }
        return Some(((n as u32) & 0xFFFFFF, default_alpha));
    }
    if let Some(n) = cell.as_u64() {
        if n == 0 {
            return None;
        }
        if n == 1 {
            return Some((default_rgb, default_alpha));
        }
        return Some((n as u32 & 0xFFFFFF, default_alpha));
    }
    if let Some(s) = cell.as_str() {
        let t = s.trim();
        if t.is_empty() || t == "0" || t.eq_ignore_ascii_case("null") || t == "-" {
            return None;
        }
        if t == "1" || t.eq_ignore_ascii_case("true") {
            return Some((default_rgb, default_alpha));
        }
        return parse_color_value(cell).map(|rgb| (rgb, default_alpha));
    }
    parse_color_value(cell).map(|rgb| (rgb, default_alpha))
}

/// Bake pixel-brush stamps into a path of axis-aligned rects (pixel look).
/// `points` are (center, time, width_x); height = width_x * aspect (gy/gx).
fn pixel_stamps_to_path(points: &[([f64; 2], f64, f32)], aspect: f64) -> kurbo::BezPath {
    use kurbo::{BezPath, Rect};
    let mut path = BezPath::new();
    let aspect = aspect.max(1e-6);
    // Dedup identical cells (re-entered stamps).
    let mut seen = std::collections::HashSet::new();
    for &(pos, _, w) in points {
        let w = w as f64;
        if w <= 0.0 {
            continue;
        }
        let h = w * aspect;
        let key = (
            (pos[0] * 1000.0).round() as i64,
            (pos[1] * 1000.0).round() as i64,
            (w * 100.0).round() as i64,
        );
        if !seen.insert(key) {
            continue;
        }
        let r = Rect::new(
            pos[0] - w * 0.5,
            pos[1] - h * 0.5,
            pos[0] + w * 0.5,
            pos[1] + h * 0.5,
        );
        path.move_to(r.origin());
        path.line_to((r.x1, r.y0));
        path.line_to((r.x1, r.y1));
        path.line_to((r.x0, r.y1));
        path.close_path();
    }
    path
}

/// Resample brush centerline with Catmull-Rom so sparse frame samples become a smooth path.
fn densify_brush_centerline(
    points: &[([f64; 2], f64, f32)],
    spacing: f64,
) -> Vec<([f64; 2], f64, f32)> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let spacing = spacing.max(0.5);
    let n = points.len();
    let mut out = Vec::with_capacity(n * 4);
    out.push(points[0]);

    let pos = |i: isize| -> [f64; 2] {
        let i = i.clamp(0, (n - 1) as isize) as usize;
        points[i].0
    };
    let cat = |p0: f64, p1: f64, p2: f64, p3: f64, t: f64| -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        0.5 * ((2.0 * p1)
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
    };

    for seg in 0..n.saturating_sub(1) {
        let p0 = pos(seg as isize - 1);
        let p1 = pos(seg as isize);
        let p2 = pos(seg as isize + 1);
        let p3 = pos(seg as isize + 2);
        let chord = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        if chord < 1e-6 {
            continue;
        }
        let steps = ((chord / spacing).ceil() as usize).clamp(2, 64);
        let w0 = points[seg].2;
        let w1 = points[seg + 1].2;
        let t0 = points[seg].1;
        let t1 = points[seg + 1].1;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let x = cat(p0[0], p1[0], p2[0], p3[0], t);
            let y = cat(p0[1], p1[1], p2[1], p3[1], t);
            let w = w0 + (w1 - w0) * t as f32;
            let tm = t0 + (t1 - t0) * t;
            // Skip near-duplicates of last out point.
            if let Some(last) = out.last() {
                let d = (x - last.0[0]).hypot(y - last.0[1]);
                if d < spacing * 0.35 && i < steps {
                    continue;
                }
            }
            out.push(([x, y], tm, w));
        }
    }
    if out.len() < 2 {
        points.to_vec()
    } else {
        out
    }
}

fn generate_brush_outline(
    points: &[([f64; 2], f64, f32)],
    smoothness: f32,
    brush_type: crate::tools::BrushType,
) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    if points.len() < 2 {
        return path;
    }
    let n = points.len();
    let mut left_pts = Vec::with_capacity(n);
    let mut right_pts = Vec::with_capacity(n);

    for i in 0..n {
        let (pos, _, w) = points[i];
        let half_w = (w / 2.0) as f64;

        let normal = if brush_type == crate::tools::BrushType::Calligraphy {
            [0.7071067811865476, 0.7071067811865476]
        } else if i == 0 {
            let next_pos = points[1].0;
            let dx = next_pos[0] - pos[0];
            let dy = next_pos[1] - pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else if i == n - 1 {
            let prev_pos = points[n - 2].0;
            let dx = pos[0] - prev_pos[0];
            let dy = pos[1] - prev_pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else {
            let prev_pos = points[i - 1].0;
            let next_pos = points[i + 1].0;
            let dx1 = pos[0] - prev_pos[0];
            let dy1 = pos[1] - prev_pos[1];
            let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();

            let dx2 = next_pos[0] - pos[0];
            let dy2 = next_pos[1] - pos[1];
            let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();

            let nx1 = if len1 > 0.0001 { -dy1 / len1 } else { 0.0 };
            let ny1 = if len1 > 0.0001 { dx1 / len1 } else { 1.0 };

            let nx2 = if len2 > 0.0001 { -dy2 / len2 } else { 0.0 };
            let ny2 = if len2 > 0.0001 { dx2 / len2 } else { 1.0 };

            let nx = (nx1 + nx2) / 2.0;
            let ny = (ny1 + ny2) / 2.0;
            let nlen = (nx * nx + ny * ny).sqrt();
            if nlen > 0.0001 {
                [nx / nlen, ny / nlen]
            } else {
                [0.0, 1.0]
            }
        };

        left_pts.push([pos[0] + normal[0] * half_w, pos[1] + normal[1] * half_w]);
        right_pts.push([pos[0] - normal[0] * half_w, pos[1] - normal[1] * half_w]);
    }

    let mut right_pts_rev = right_pts.clone();
    right_pts_rev.reverse();

    crate::render::append_smoothed_points(&mut path, &left_pts, smoothness, true);

    if brush_type == crate::tools::BrushType::Pen && n > 0 {
        let end_idx = n - 1;
        let c = points[end_idx].0;
        let r = (points[end_idx].2 as f64) / 2.0;
        if r > 0.1 {
            let dx = left_pts[end_idx][0] - c[0];
            let dy = left_pts[end_idx][1] - c[1];
            let start_angle = dy.atan2(dx);
            let sweep = std::f64::consts::PI;
            let arc = kurbo::Arc::new((c[0], c[1]), (r, r), start_angle, sweep, 0.0);
            for el in arc.to_path(0.1).elements().iter().skip(1) {
                path.push(*el);
            }
        }
    }

    crate::render::append_smoothed_points(&mut path, &right_pts_rev, smoothness, false);

    if brush_type == crate::tools::BrushType::Pen && n > 0 {
        let c = points[0].0;
        let r = (points[0].2 as f64) / 2.0;
        if r > 0.1 {
            let dx = right_pts[0][0] - c[0];
            let dy = right_pts[0][1] - c[1];
            let start_angle = dy.atan2(dx);
            let sweep = std::f64::consts::PI;
            let arc = kurbo::Arc::new((c[0], c[1]), (r, r), start_angle, sweep, 0.0);
            for el in arc.to_path(0.1).elements().iter().skip(1) {
                path.push(*el);
            }
        }
    }

    path.close_path();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bezier_interpolation() {
        let mut track = KeyframeTrack::default();
        track.insert(0, 10.0);
        track.insert(100, 110.0);

        track.keyframes[0].interpolation = InterpolationMode::Bezier;
        track.keyframes[0].handle_right = (50.0, 50.0);

        assert!((track.interpolate(0).unwrap() - 10.0).abs() < 1e-5);
        assert!((track.interpolate(100).unwrap() - 110.0).abs() < 1e-5);

        let mid_val = track.interpolate(50).unwrap();
        assert!((mid_val - 60.0).abs() < 1e-4);
    }

    #[test]
    fn test_pure_motion_geometry_equivalence() {
        use crate::document::{NodeKind, PathData};
        let path = PathData::from_anchor_data(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            &[],
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            true,
        );
        
        let mut node = Node {
            id: uuid::Uuid::new_v4(),
            name: "Test Path".to_string(),
            kind: NodeKind::Path { path },
            style: crate::document::NodeStyle::default(),
            transform: crate::document::Transform2D::default(),
            path_effect_links: Vec::new(),
        };

        let last_pos = node.get_pos();
        let last_rotation = node.get_rotation();
        let last_geom_floats = node.get_geom_floats();

        node.translate(20.0, 30.0);
        node.set_rotation(0.785);

        let mut temp_node = node.clone();
        temp_node.set_rotation(last_rotation);
        let unrot_pos = temp_node.get_pos();
        let dx_un = last_pos.0 - unrot_pos.0;
        let dy_un = last_pos.1 - unrot_pos.1;
        temp_node.translate(dx_un, dy_un);

        let temp_geom = temp_node.get_geom_floats();
        
        assert_eq!(temp_geom.len(), last_geom_floats.len());
        for i in 0..temp_geom.len() {
            assert!((temp_geom[i] - last_geom_floats[i]).abs() < 1e-6, "Index {} differs: {} vs {}", i, temp_geom[i], last_geom_floats[i]);
        }
    }

    impl VadadeeBerryApp {
        pub fn new_for_test() -> Self {
            let fonts = FontRegistry::new();
            let default_font = fonts.default_family();
            Self {
                live_snap_guides: Vec::new(),
                snap_magnet: true,
            pixel_art_mode: false,
            pixel_cell_size: 1.0,
                anim_current_frame: 0,
                anim_is_playing: false,
                anim_playback_wall: None,
                anim_play_origin: None,
                anim_keyframing_mode: false,
                anim_show_timeline_window: false,
                show_video_editor_window: None,
                show_shader_editor_window: None,
                piano_roll_clip: None,
                piano_roll_t: 0.0,
                piano_tool: crate::av_ui::PianoTool::default(),
                piano_zoom: 1.0,
                piano_scroll_offset: 0.0,
                piano_pitch_scroll: 36.0,
                av_timeline_drag: None,
                node_editor_ui: crate::node_editor_ui::NodeEditorUiState::default(),
                ui_shading_pass_sel: 0,
                anim_time_accumulator: 0.0,
                anim_last_seen_frame: 0,
                anim_last_applied_states: std::collections::HashMap::new(),
                anim_timeline_scroll: 0.0,
                anim_timeline_follow: true,
                anim_edit_mode: false,
                anim_dragged_keyframe: None,
                anim_selected_keyframe: None,
                anim_graph_editor_track: None,
                anim_graph_editor_target_track: None,
                anim_graph_editor_t: 0.0,
                anim_graph_editor_dragged_kf: None,
                anim_graph_editor_dragged_handle: None,
                anim_graph_kf_drag_start: None,
                anim_graph_selected_segment: None,
                anim_graph_region_select: None,
                anim_graph_selected_stack: None,
                anim_graph_stack_drag: None,
                anim_stack_formula_dialog: None,
                anim_stack_formula_draft: String::new(),
                plotter_formula_dialog: None,
                plotter_formula_draft: String::new(),
                plotter_inline_expr: None,
                plotter_expr_edit_before: None,
                object_rename_dialog: None,
                anim_graph_scroll: 0.0,
                anim_graph_visible_frames: 100.0,
                anim_timeline_visible_frames: 100.0,
                anim_graph_view_val_min: 0.0,
                anim_graph_view_val_max: 1.0,
                anim_fps: 60,
                ui_fps: 60.0,
                enable_layer_raster_cache: false,
            gpu_shading: true,
            wgpu_render: None,
                video_frame_cache: None,
                video_layers: std::collections::HashMap::new(),
                clip_mask_signatures: std::collections::HashMap::new(),
                layer_raster_cache: std::collections::HashMap::new(),
                layer_cache_pending: std::collections::HashSet::new(),
                layer_cache_result_tx: {
                    let (tx, _rx) = std::sync::mpsc::channel();
                    tx
                },
                layer_cache_result_rx: {
                    let (_tx, rx) = std::sync::mpsc::channel();
                    rx
                },
                audio_device: rodio::DeviceSinkBuilder::open_default_sink().ok(),
                audio_players: std::collections::HashMap::new(),
                audio_player_buffer_offset: std::collections::HashMap::new(),
                audio_player_last_file_pos: std::collections::HashMap::new(),
                audio_player_playback_rate: std::collections::HashMap::new(),
                audio_player_media_path: std::collections::HashMap::new(),
                audio_layers_skip: std::collections::HashSet::new(),
                audio_extract_status: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                audio_pcm_cache: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                audio_prepare_rx: std::collections::HashMap::new(),
            #[cfg(not(target_os = "android"))]
            screen_captures: std::collections::HashMap::new(),

                project: Document::new_default_project(),
                viewport: Viewport::default(),
                tools: ToolState {
                    active: ToolKind::Select,
                    ..Default::default()
                },
                selection: vec![],
                hit_pick_menu: None,
                selection_sticky: false,
                history: History::default(),
                ui_fill_stops: default_gradient_stops(),
                ui_fill_stop_sel: 0,
                ui_fill_edit_gradient_line: false,
                ui_fill_kind: FillKind::Solid,
                ui_gradient_angle: 90.0,
                ui_fill_line_x0: 0.0,
                ui_fill_line_y0: 0.5,
                ui_fill_line_x1: 1.0,
                ui_fill_line_y1: 0.5,
                ui_radial_cx: 0.5,
                ui_radial_cy: 0.5,
                polygon_sides: 6,
                ui_stroke_stops: vec![
                    GradientStop::new(0.0, Paint::from_hex(0x1a1f2e, 1.0)),
                    GradientStop::new(1.0, Paint::from_hex(0x1a1f2e, 1.0)),
                ],
                ui_stroke_stop_sel: 0,
                ui_stroke_edit_gradient_line: false,
                ui_stroke_line_join: crate::document::LineJoin::Miter,
                ui_stroke_line_cap: crate::document::LineCap::Butt,
                ui_stroke_paint_order: crate::document::StrokePaintOrder::BehindFill,
                ui_stroke_kind: FillKind::Solid,
                ui_stroke_angle: 0.0,
                ui_marker_start: crate::document::PathMarker::default(),
                ui_marker_mid: crate::document::PathMarker::default(),
                ui_marker_end: crate::document::PathMarker::default(),
                ui_marker_use_common_size: false,
                ui_marker_common_size: 10.0,
                ui_stroke_line_x0: 0.0,
                ui_stroke_line_y0: 0.5,
                ui_stroke_line_x1: 1.0,
                ui_stroke_line_y1: 0.5,
                ui_stroke_radial_cx: 0.5,
                ui_stroke_radial_cy: 0.5,
                ui_stroke_width: 2.0,
                ui_text_content: "Text".into(),
                ui_text_font_size: 24.0,
                ui_text_width: 0.0,
                ui_text_font_family: default_font,
                fonts,
                ui_text_bold: false,
                ui_text_italic: false,
                fill_enabled: true,
                stroke_enabled: true,
                status_message: "Idle".into(),
                clipboard: Vec::new(),
                action_tab_scroll_home: false,
                on_page_text_edit: None,
                on_page_text_focus_pending: false,
                on_page_text_before: None,
                on_page_text_newly_created: false,
                image_textures: std::collections::HashMap::new(),
                image_pixel_cache: std::collections::HashMap::new(),
                flood_fill_anim: None,
                graph_path_textures: std::collections::HashMap::new(),
                graph_gpu_fx: std::collections::HashMap::new(),
                graph_base_rgba: std::collections::HashMap::new(),
                graph_preview_rgba: std::collections::HashMap::new(),
                graph_color_rgba: std::collections::HashMap::new(),
                cursor_doc: None,
                action_bar_open: true,
                action_bar_width: 300.0,
                action_tab: ui::ActionTab::default(),
                action_tab_order: ui::ActionTab::all_tabs(),
                ui_on_path_mode: OnPathMode::GapDuplicate,
                ui_on_path_gap: 48.0,
                ui_on_path_count: 5,
                ui_on_path_cyclic: true,
                ui_on_path_rotate: true,
                ui_on_path_loft_scale: 1.0,
                ui_on_path_loft_opacity: 0.75,
                ui_on_path_container_h: 280.0,
                timeline_container_h: 56.0,
                timeline_container_w: 0.0,
                video_editor_container_h: 130.0,
                video_editor_container_w: 0.0,
                ui_tiling_rows: 3,
                ui_tiling_cols: 3,
                ui_tiling_offset_x: 0.0,
                ui_tiling_offset_y: 0.0,
                ui_tiling_row_rot: 0.0,
                ui_tiling_col_rot: 0.0,
                ui_tiling_row_scale: 0.0,
                ui_tiling_col_scale: 0.0,
                ui_tiling_gap_x: 48.0,
                ui_tiling_gap_y: 48.0,
                ui_circular_copies: 6,
                ui_boolean_op: BooleanOpKind::Union,
                ui_circular_angle_offset: 0.0,
                ui_circular_origin_x: 0.0,
                ui_circular_origin_y: 0.0,
                ui_circular_rotate_mode: CircularRotateMode::ReferenceOrigin,
                ui_anim: {
                    let mut anim = UiAnimation::new();
                    anim.seed_status_board("Idle", 80.0, 56.0);
                    anim
                },
                gradient_editor_focus: crate::gradient_ui::GradientEditorFocus::None,
                gradient_flow_drag: None,
                canvas_screen_rect: None,
                canvas_origin: Pos2::ZERO,
                pending_open_svg: false,
                pending_open_project: false,
            cached_project: None,
            cached_project_label: None,
                pending_save_project: false,
                pending_export_svg: false,
                pending_export_image: false,
                export_image_format: io::ExportImageFormat::Png,
                export_image_selection_only: false,
                eyedropper_holding: false,
                eyedropper_releasing: false,
                eyedropper_t: 0.0,
                eyedropper_target_pos: None,
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
                paste_hotkey_was_down: false,
                paste_progress: None,
                toolbar_expanded: false,
                toolbar_outer_rect: None,
                toolbar_drag_active: false,
                text_editor_rect: None,
                text_pan_restore: None,
                text_pan_anim: None,
                last_android_text: String::new(),
                path_overlay_rect: None,
                video_export: VideoExportState::default(),
                project_save_path: None,
                left_dock: crate::left_dock::LeftDockState::default(),
                collab: crate::collab::CollabSession::new(),
                collab_last_cursor_sent: None,

            collab_canvas_sync_accum: 0.0,
            collab_last_ui_sync: (ui::ActionTab::default(), 0),
            collab_last_wire_hash: 0,
            collab_asset_cache: std::collections::HashMap::new(),
            cursor_bubble_edit: false,
            cursor_bubble_focus_pending: false,
            cursor_bubble_text: String::new(),
                #[cfg(not(target_os = "android"))]
                mcp_bridge: None,
                #[cfg(not(target_os = "android"))]
                mcp_preview: McpPreviewState::default(),
                #[cfg(not(target_os = "android"))]
                mcp_preview_update_tx: {
                    let (tx, _rx) = std::sync::mpsc::channel();
                    tx
                },
                #[cfg(not(target_os = "android"))]
                mcp_preview_update_rx: {
                    let (_tx, rx) = std::sync::mpsc::channel();
                    rx
                },
                #[cfg(not(target_os = "android"))]
                pending_mcp_bulk_rects: Vec::new(),
                #[cfg(not(target_os = "android"))]
                mcp_bulk_staging: Vec::new(),
                spatial_index: crate::spatial_index::SpatialIndex::default(),
                cached_draw_order: Vec::new(),
                cached_draw_order_revision: u64::MAX,
                audio_output_warned: false,
                canvas_focused: false,
                window_was_focused: true,
            }
        }
    }

    #[test]
    fn test_gradient_color_animation() {
        let mut app = VadadeeBerryApp::new_for_test();
        let node_id = uuid::Uuid::new_v4();
        
        let initial_stops = vec![
            GradientStop::new(0.0, Paint { rgba: [1.0, 0.0, 0.0, 1.0] }), // Red
            GradientStop::new(1.0, Paint { rgba: [0.0, 0.0, 1.0, 1.0] }), // Blue
        ];
        
        let fill = Fill::LinearGradient {
            angle_deg: 90.0,
            line_x0: 0.0,
            line_y0: 0.0,
            line_x1: 1.0,
            line_y1: 1.0,
            stops: initial_stops,
        };
        
        let mut node = Node::rect(0.0, 0.0, 100.0, 100.0, fill);
        node.id = node_id;
        app.project.nodes.insert(node);
        
        // Add color animation tracks
        let mut anim = NodeAnimation::default();
        anim.color_r.insert(0, 1.0);
        anim.color_r.insert(10, 0.5);
        anim.color_g.insert(0, 1.0);
        anim.color_g.insert(10, 0.5);
        anim.color_b.insert(0, 1.0);
        anim.color_b.insert(10, 0.5);
        anim.color_a.insert(0, 1.0);
        anim.color_a.insert(10, 1.0);
        app.project.anim_timeline.nodes.insert(node_id, anim);
        
        // Run apply_animation_for_frame at frame 10
        app.apply_animation_for_frame(10);
        
        // Verify the node's fill
        let updated_node = app.project.nodes.get(node_id).unwrap();
        match &updated_node.style.fill {
            Fill::LinearGradient { stops, .. } => {
                // Animated color target is [0.5, 0.5, 0.5, 1.0]
                // Red stop: [1.0, 0.0, 0.0, 1.0] * [0.5, 0.5, 0.5, 1.0] = [0.5, 0.0, 0.0, 1.0]
                assert!((stops[0].color.rgba[0] - 0.5).abs() < 1e-5);
                assert!((stops[0].color.rgba[1] - 0.0).abs() < 1e-5);
                assert!((stops[0].color.rgba[2] - 0.0).abs() < 1e-5);
                
                // Blue stop: [0.0, 0.0, 1.0, 1.0] * [0.5, 0.5, 0.5, 1.0] = [0.0, 0.0, 0.5, 1.0]
                assert!((stops[1].color.rgba[0] - 0.0).abs() < 1e-5);
                assert!((stops[1].color.rgba[1] - 0.0).abs() < 1e-5);
                assert!((stops[1].color.rgba[2] - 0.5).abs() < 1e-5);
            }
            _ => panic!("Expected fill to be LinearGradient"),
        }
    }
}

fn is_video_container_ext(path: &str) -> bool {
    crate::document::AvClip::path_is_video_container(path)
}

impl VadadeeBerryApp {
    /// True while background video→WAV extract is running for this media path.
    fn ne_audio_extract_busy(&self, path: &str) -> bool {
        self.audio_extract_status
            .lock()
            .ok()
            .and_then(|m| m.get(path).map(|s| s.is_extracting()))
            .unwrap_or(false)
    }

    /// Kick extract for every NE video sound path as soon as the graph references it.
    /// Rate-limited: at most once every 500ms (not every UI frame).
    fn warm_ne_video_audio_extract(&mut self) {
        use std::time::{Duration, Instant};
        static LAST: std::sync::OnceLock<std::sync::Mutex<Option<Instant>>> =
            std::sync::OnceLock::new();
        let slot = LAST.get_or_init(|| std::sync::Mutex::new(None));
        {
            let mut last = slot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = *last {
                if t.elapsed() < Duration::from_millis(500) {
                    return;
                }
            }
            *last = Some(Instant::now());
        }

        let mut paths: Vec<String> = Vec::new();
        for layer in &self.project.document.layers {
            // Warm extract for hidden NE layers too — visibility toggle must not force
            // a cold re-extract (and cache wipe) before sound can resume.
            if layer.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            let Some(g) = layer.node_graph.as_ref() else {
                continue;
            };
            if let Some(p) = g.resolve_output_sound().path() {
                paths.push(p.to_string());
            }
            for n in g.nodes.values() {
                match &n.kind {
                    crate::document::GraphNodeKind::ObjectVideo { path }
                        if !path.trim().is_empty() =>
                    {
                        paths.push(path.clone());
                    }
                    crate::document::GraphNodeKind::VideoPlayer => {
                        if let Some(src) = g.input_source_node(n.id, "video") {
                            if let Some(sn) = g.nodes.get(&src) {
                                if let crate::document::GraphNodeKind::ObjectVideo { path } =
                                    &sn.kind
                                {
                                    if !path.trim().is_empty() {
                                        paths.push(path.clone());
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        paths.sort();
        paths.dedup();
        for p in paths {
            if crate::document::AvClip::path_is_video_container(&p)
                && std::path::Path::new(&p).is_file()
            {
                // resolve is cheap when Ready/cached; only spawns once per cache key.
                let _ = resolve_audio_path_for_rodio(
                    &p,
                    &self.audio_extract_status,
                    &self.audio_pcm_cache,
                );
            }
        }
    }
}

/// Status bar powerline is single-row — collapse newlines / long panics into one line.
fn status_one_line(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 120;
    if joined.chars().count() > MAX {
        let mut out: String = joined.chars().take(MAX.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        joined
    }
}

fn cached_wav_path_for_video(video_path: &str) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    // Prefer XDG cache (always writable). Sidecar next to video is a fallback.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    video_path.hash(&mut hasher);
    dirs_next_audio_cache_dir().join(format!("vadadee_audio_{:016x}.wav", hasher.finish()))
}

/// Legacy / co-located extract: `movie.mp4` → `movie.vadadee.wav` next to the file.
fn sidecar_wav_path_for_video(video_path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(video_path);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    p.with_file_name(format!("{stem}.vadadee.wav"))
}

/// First playable extracted WAV for a video (XDG cache, then sidecar).
fn find_playable_extracted_wav(video_path: &str) -> Option<std::path::PathBuf> {
    let cached = cached_wav_path_for_video(video_path);
    if crate::audio_extract::wav_is_playable(&cached) {
        return Some(cached);
    }
    let side = sidecar_wav_path_for_video(video_path);
    if crate::audio_extract::wav_is_playable(&side) {
        return Some(side);
    }
    None
}

/// Start background WAV extract (idempotent). Demuxes each video **at most once**
/// per process via `ensure_extracted_wav` OnceLock.
fn spawn_video_audio_extract(
    video_path: &str,
    status_map: &std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, AudioExtractStatus>>>,
    _pcm_cache: &crate::audio_extract::AudioPcmCache,
) {
    if !is_video_container_ext(video_path) {
        return;
    }

    // Already good on disk (cache or sidecar) → Ready, zero work.
    if let Some(existing) = find_playable_extracted_wav(video_path) {
        if let Ok(mut map) = status_map.lock() {
            map.insert(
                video_path.to_string(),
                AudioExtractStatus::Ready(existing),
            );
        }
        return;
    }

    let out_wav = cached_wav_path_for_video(video_path);

    {
        let mut map = status_map.lock().unwrap();
        match map.get(video_path) {
            Some(AudioExtractStatus::Failed) => return,
            Some(AudioExtractStatus::Extracting { .. }) => return,
            Some(AudioExtractStatus::Ready(p)) if crate::audio_extract::wav_is_playable(p) => {
                return;
            }
            _ => {}
        }
        map.insert(
            video_path.to_string(),
            AudioExtractStatus::Extracting { progress: 0.0 },
        );
    }

    let path_clone = video_path.to_string();
    let map_for_report = status_map.clone();
    let map_for_done = status_map.clone();
    let out_wav_clone = out_wav.clone();

    std::thread::Builder::new()
        .name("vadadee-audio-extract".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(dirs_next_audio_cache_dir());
            let path_key = path_clone.clone();
            let report: crate::audio_extract::ExtractProgress = std::sync::Arc::new(move |p| {
                if let Ok(mut m) = map_for_report.lock() {
                    m.insert(
                        path_key.clone(),
                        AudioExtractStatus::Extracting {
                            progress: p.clamp(0.0, 1.0),
                        },
                    );
                }
            });

            // OnceLock inside ensure_extracted_wav: concurrent threads wait, demux once.
            let result = crate::audio_extract::ensure_extracted_wav(
                std::path::Path::new(&path_clone),
                &out_wav_clone,
                report,
            );

            let mut m = map_for_done.lock().unwrap();
            match result {
                Ok(out_path) => {
                    m.insert(path_clone, AudioExtractStatus::Ready(out_path));
                }
                Err(e) => {
                    log::warn!("audio extract failed for {path_clone}: {e}");
                    m.insert(path_clone, AudioExtractStatus::Failed);
                }
            }
        })
        .ok();
}

fn dirs_next_audio_cache_dir() -> std::path::PathBuf {
    // Prefer XDG cache, then home, then temp.
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("vadadee-berry").join("audio");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".cache")
            .join("vadadee-berry")
            .join("audio");
    }
    std::env::temp_dir().join("vadadee-berry-audio")
}

fn dirs_vadadee_cache_root() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("vadadee-berry");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".cache")
            .join("vadadee-berry");
    }
    std::env::temp_dir().join("vadadee-berry-cache")
}

/// Default screen-record folder under XDG cache (not user Videos).
fn dirs_next_screen_cache_dir() -> std::path::PathBuf {
    dirs_vadadee_cache_root().join("screen")
}

/// Remove regenerable extract WAVs + temp open-cache left behind after crash/exit.
/// Safe: never touches user project files or Videos/… recordings.
pub fn purge_vadadee_disk_caches(opts: CachePurgeOpts) {
    let mut removed = 0u32;
    let mut bytes: u64 = 0;

    // 1) Audio extract WAVs under ~/.cache/vadadee-berry/audio
    let audio_dir = dirs_next_audio_cache_dir();
    removed += purge_dir_files(
        &audio_dir,
        &["wav", "WAV"],
        opts.max_age,
        opts.remove_all_in_audio,
        &mut bytes,
    );

    // 2) Orphan screen cache only when age policy says so (default takes are often intentional).
    if opts.purge_screen_cache {
        let screen_dir = dirs_next_screen_cache_dir();
        removed += purge_dir_files(
            &screen_dir,
            &["sepscrr", "mp4", "wav", "m4a"],
            opts.max_age,
            false,
            &mut bytes,
        );
    }

    // 3) Temp open-project cache
    let open_cache = std::env::temp_dir().join(".vadadee-berry-open-cache.vadadee-berry.json");
    if open_cache.is_file() {
        if let Ok(meta) = open_cache.metadata() {
            bytes += meta.len();
        }
        if std::fs::remove_file(&open_cache).is_ok() {
            removed += 1;
        }
    }

    // 4) AV temp dir leftovers
    let av_tmp = std::env::temp_dir().join("vadadee-berry-av");
    removed += purge_dir_files(&av_tmp, &[], opts.max_age, opts.remove_all_tmp, &mut bytes);

    if removed > 0 {
        log::info!(
            "[cache] purged {removed} file(s), ~{:.1} MB from disk cache",
            bytes as f64 / (1024.0 * 1024.0)
        );
    }
}

#[derive(Clone, Copy)]
pub struct CachePurgeOpts {
    /// Delete files older than this (None = ignore age, only used with remove_all_*).
    pub max_age: Option<std::time::Duration>,
    /// Wipe entire audio extract cache (regenerable on next play).
    pub remove_all_in_audio: bool,
    /// Wipe vadadee-berry-av temp dir contents.
    pub remove_all_tmp: bool,
    /// Also age-prune ~/.cache/vadadee-berry/screen
    pub purge_screen_cache: bool,
}

impl CachePurgeOpts {
    /// Startup: drop stale extracts from previous sessions; keep recent ones for quick resume.
    fn on_startup() -> Self {
        Self {
            max_age: Some(std::time::Duration::from_secs(60 * 60 * 24)), // 24h
            remove_all_in_audio: false,
            remove_all_tmp: false,
            purge_screen_cache: true,
        }
    }

    /// Exit: clear regenerable audio extracts + temp open cache so .cache doesn't grow forever.
    fn on_exit() -> Self {
        Self {
            max_age: None,
            remove_all_in_audio: true,
            remove_all_tmp: true,
            purge_screen_cache: false, // user may still want last take under cache/
        }
    }
}

fn purge_dir_files(
    dir: &std::path::Path,
    exts: &[&str],
    max_age: Option<std::time::Duration>,
    remove_all: bool,
    bytes_out: &mut u64,
) -> u32 {
    if !dir.is_dir() {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    let mut n = 0u32;
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        if !exts.is_empty() {
            let ok = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        if !remove_all {
            let Some(age) = max_age else {
                continue;
            };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .map(|d| d > age)
                .unwrap_or(false);
            if !old {
                continue;
            }
        }
        *bytes_out += meta.len();
        if std::fs::remove_file(&path).is_ok() {
            n += 1;
        }
    }
    n
}

/// Rodio cannot stream most video containers; use extracted stereo WAV.
fn resolve_audio_path_for_rodio(
    video_path: &str,
    status_map: &std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, AudioExtractStatus>>>,
    pcm_cache: &crate::audio_extract::AudioPcmCache,
) -> Option<std::path::PathBuf> {
    use crate::document::AvClip;
    // Still images / non-audio paths must never hit symphonia (EOF spam every frame).
    if AvClip::path_is_still_image(video_path) || video_path.is_empty() {
        return None;
    }
    if !is_video_container_ext(video_path) {
        // Pure audio files only (mp3/wav/…). Skip unknown extensions.
        if !AvClip::path_is_audio_only(video_path) {
            return None;
        }
        let pb = std::path::PathBuf::from(video_path);
        if !pb.is_file() {
            return None;
        }
        if pb.metadata().map(|m| m.len()).unwrap_or(0) < 2048 {
            return None;
        }
        crate::audio_extract::spawn_preload_pcm(
            pcm_cache.clone(),
            video_path.to_string(),
            pb.clone(),
        );
        return Some(pb);
    }

    // Ready in status map → use immediately if still playable.
    if let Ok(map) = status_map.lock() {
        if let Some(AudioExtractStatus::Ready(pb)) = map.get(video_path) {
            if crate::audio_extract::wav_is_playable(pb) {
                return Some(pb.clone());
            }
        }
    }

    // XDG cache or co-located `*.vadadee.wav` (older extracts wrote next to the video).
    if let Some(existing) = find_playable_extracted_wav(video_path) {
        if let Ok(mut map) = status_map.lock() {
            map.insert(
                video_path.to_string(),
                AudioExtractStatus::Ready(existing.clone()),
            );
        }
        return Some(existing);
    }

    // Never return the raw video path for rodio.
    spawn_video_audio_extract(video_path, status_map, pcm_cache);

    let map = status_map.lock().unwrap();
    match map.get(video_path) {
        Some(AudioExtractStatus::Ready(pb)) if crate::audio_extract::wav_is_playable(pb) => {
            Some(pb.clone())
        }
        _ => None,
    }
}

fn apply_color_controls(img: &mut image::RgbaImage, hue: f32, sat: f32, bright: f32, contrast: f32) {
    for pixel in img.pixels_mut() {
        let [r, g, b, _a] = pixel.0;
        
        let mut rf = r as f32 / 255.0;
        let mut gf = g as f32 / 255.0;
        let mut bf = b as f32 / 255.0;
        
        // 1. Contrast
        if contrast != 1.0 {
            rf = (rf - 0.5) * contrast + 0.5;
            gf = (gf - 0.5) * contrast + 0.5;
            bf = (bf - 0.5) * contrast + 0.5;
        }
        
        // 2. Brightness
        if bright != 1.0 {
            rf *= bright;
            gf *= bright;
            bf *= bright;
        }
        
        // 3. Saturation (luminance-based grayscale interpolation)
        if sat != 1.0 {
            let lum = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            rf = lum + (rf - lum) * sat;
            gf = lum + (gf - lum) * sat;
            bf = lum + (bf - lum) * sat;
        }
        
        pixel.0[0] = (rf * 255.0).clamp(0.0, 255.0) as u8;
        pixel.0[1] = (gf * 255.0).clamp(0.0, 255.0) as u8;
        pixel.0[2] = (bf * 255.0).clamp(0.0, 255.0) as u8;
    }
    
    // 4. Hue rotation
    if hue != 0.0 {
        let mut dyn_img = image::DynamicImage::ImageRgba8(img.clone());
        dyn_img = dyn_img.huerotate(hue as i32);
        *img = dyn_img.to_rgba8();
    }
}

fn adjust_frame_color(bytes: &[u8], hue: f32, sat: f32, bright: f32, contrast: f32) -> Option<Vec<u8>> {
    if let Ok(dyn_img) = image::load_from_memory(bytes) {
        let mut rgba = dyn_img.to_rgba8();
        apply_color_controls(&mut rgba, hue, sat, bright, contrast);
        let mut out_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out_bytes);
        if image::write_buffer_with_format(
            &mut cursor,
            &rgba,
            rgba.width(),
            rgba.height(),
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        ).is_ok() {
            return Some(out_bytes);
        }
    }
    None
}

fn paint_rotated_image(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: egui::Rect,
    rotation_rad: f32,
    opacity: f32,
) {
    paint_rotated_image_mirrored(painter, texture_id, rect, rotation_rad, opacity, false, false);
}

fn paint_rotated_image_mirrored(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: egui::Rect,
    rotation_rad: f32,
    opacity: f32,
    flip_h: bool,
    flip_v: bool,
) {
    paint_rotated_image_mirrored_tint(
        painter, texture_id, rect, rotation_rad, opacity, 1.0, flip_h, flip_v,
    );
}

/// Like [`paint_rotated_image_mirrored`] with optional RGB multiply (brightness).
fn paint_rotated_image_mirrored_tint(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: egui::Rect,
    rotation_rad: f32,
    opacity: f32,
    rgb_mul: f32,
    flip_h: bool,
    flip_v: bool,
) {
    paint_rotated_image_mirrored_tint_uv(
        painter,
        texture_id,
        rect,
        rotation_rad,
        opacity,
        rgb_mul,
        flip_h,
        flip_v,
        (0.0, 0.0, 1.0, 1.0),
    );
}

/// Paint with UV sub-rect (for free Zoom effect — no CPU rebake).
fn paint_rotated_image_mirrored_tint_uv(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: egui::Rect,
    rotation_rad: f32,
    opacity: f32,
    rgb_mul: f32,
    flip_h: bool,
    flip_v: bool,
    uv: (f32, f32, f32, f32),
) {
    let mut mesh = egui::Mesh::with_texture(texture_id);
    // Vertex tint ≈ brightness: RGB *= m (m>1 clamps to white in egui).
    let m = rgb_mul.clamp(0.0, 8.0);
    let a = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let v = ((m.min(1.0)) * 255.0).round() as u8;
    let color = egui::Color32::from_rgba_unmultiplied(v, v, v, a);

    let mut points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];

    if rotation_rad != 0.0 {
        let center = rect.center();
        let cos = rotation_rad.cos();
        let sin = rotation_rad.sin();
        for pt in &mut points {
            let d = *pt - center;
            let rx = d.x * cos - d.y * sin;
            let ry = d.x * sin + d.y * cos;
            *pt = center + egui::vec2(rx, ry);
        }
    }

    let (mut u0, mut v0, mut u1, mut v1) = uv;
    if flip_h {
        std::mem::swap(&mut u0, &mut u1);
    }
    if flip_v {
        std::mem::swap(&mut v0, &mut v1);
    }

    mesh.vertices.push(egui::epaint::Vertex {
        pos: points[0],
        uv: egui::pos2(u0, v0),
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: points[1],
        uv: egui::pos2(u1, v0),
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: points[2],
        uv: egui::pos2(u1, v1),
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: points[3],
        uv: egui::pos2(u0, v1),
        color,
    });

    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);

    painter.add(mesh);
}



