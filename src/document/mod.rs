mod node;
mod path_effects;
mod style;
mod animation;
mod av_clip;
mod music;
mod shading;
pub mod flowchart;
mod node_graph;
pub mod septic;

pub use av_clip::*;
pub use node::*;
pub use path_effects::*;
pub use style::*;
pub use animation::*;
pub use music::*;
pub use shading::*;
pub use node_graph::*;
pub use septic::*;
pub mod expr;
pub use expr::{eval_expr, eval_expr_vars, ExprError, ExprVars};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A4 at 96 DPI (pixels), portrait.
pub const A4_WIDTH_PX: f64 = 794.0;
pub const A4_HEIGHT_PX: f64 = 1123.0;
/// CSS / Inkscape style: 96 px per inch.
pub const PX_PER_MM: f64 = 96.0 / 25.4;

pub fn px_to_mm(px: f64) -> f64 {
    px / PX_PER_MM
}

pub fn mm_to_px(mm: f64) -> f64 {
    mm * PX_PER_MM
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PageUnit {
    #[default]
    Px,
    Mm,
}

/// Inkscape-like document: page size + layer stack of drawable nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub active_layer_index: usize,
    #[serde(default)]
    pub defs: IndexMap<String, String>,
    /// Shared path effects (object-on-path, etc.) keyed by effect id.
    #[serde(default)]
    pub path_effects: IndexMap<Uuid, ObjectOnPathEffect>,
    /// Tiling effects (separate from ObjectOnPath), keyed by effect id.
    #[serde(default)]
    pub tiling_effects: IndexMap<Uuid, TilingEffect>,
    /// CircularClone effects (separate from ObjectOnPath), keyed by effect id.
    #[serde(default)]
    pub circular_effects: IndexMap<Uuid, CircularCloneEffect>,
    /// Clip Mask effects: raster (image) source clipped to solid-face mask shape.
    #[serde(default)]
    pub clip_masks: IndexMap<Uuid, ClipMaskEffect>,
    /// Boolean ops (union / intersection / difference) between solid-face shapes.
    #[serde(default)]
    pub boolean_effects: IndexMap<Uuid, BooleanEffect>,
    #[serde(default = "default_page_color")]
    pub page_color: [f32; 4],
    /// Display unit for page size fields in the UI (stored width/height are always px).
    #[serde(default)]
    pub page_unit: PageUnit,
}

fn default_page_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerKind {
    Image,
    /// Unified Audio/Video (media) layer. A single AV layer can provide video frames and/or audio.
    AV,
    /// WGSL post-process passes composited over the canvas.
    Shading,
    /// Dynamic flowchart with nodes and orthogonal paths.
    Flowchart,
    /// Node-based processing graph (typed ports, Output Object as continuous video sink).
    NodeEditor,
    /// OS screen/window capture → `.sepscrr` (video + mouse; no keyboard in v1).
    ScreenRecord,
}

/// Role of an AV layer in the media queue (video / audio / DAW tracks stay separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AvRole {
    /// Timeline video clips (raster frames on canvas).
    #[default]
    Video,
    /// Timeline audio-only media clips.
    Audio,
    /// Piano-roll / DAW music clips (no wav required).
    Daw,
}

impl Default for LayerKind {
    fn default() -> Self {
        Self::Image
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub nodes: Vec<NodeId>,
    
    #[serde(default)]
    pub kind: LayerKind,
    #[serde(default)]
    pub video_path: String,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_is_renderer")]
    pub is_renderer: bool,
    
    #[serde(default = "default_zero")]
    pub x: f32,
    #[serde(default = "default_zero")]
    pub y: f32,
    #[serde(default = "default_zero")]
    pub rotation: f32,
    #[serde(default = "default_width")]
    pub width: f32,
    #[serde(default = "default_height")]
    pub height: f32,
    #[serde(default = "default_true")]
    pub aspect_ratio_locked: bool,
    #[serde(default = "default_zero")]
    pub hue: f32,
    #[serde(default = "default_one")]
    pub saturation: f32,
    #[serde(default = "default_one")]
    pub brightness: f32,
    #[serde(default = "default_one")]
    pub contrast: f32,
    #[serde(default = "default_zero")]
    pub eq_bass: f32,
    #[serde(default = "default_zero")]
    pub eq_mid: f32,
    #[serde(default = "default_zero")]
    pub eq_treble: f32,
    #[serde(default = "default_zero")]
    pub video_start_offset: f32,
    #[serde(default = "default_max_duration")]
    pub video_play_length: f32,
    #[serde(default = "default_zero")]
    pub video_timeline_start: f32,
    /// Probed file duration (seconds); caps timeline/export when set.
    #[serde(default)]
    pub media_source_duration: Option<f32>,
    /// Media clips on the AV timeline (video and/or audio objects inside one layer).
    #[serde(default)]
    pub av_clips: Vec<AvClip>,
    /// DAW-style music clips on the AV timeline.
    #[serde(default)]
    pub music_clips: Vec<MusicClip>,
    /// When `kind == AV`, which media queue this layer belongs to.
    #[serde(default)]
    pub av_role: AvRole,
    /// WGSL shading passes when `kind == Shading`.
    #[serde(default)]
    pub shading_passes: Vec<ShadingPass>,
    /// Node graph when `kind == NodeEditor` (one graph per layer).
    #[serde(default)]
    pub node_graph: Option<NodeGraph>,
    /// P6b: selectable canvas proxy for Output Object (`NodeKind::Image` with live FX texture).
    #[serde(default)]
    pub ne_output_proxy: Option<Uuid>,
    /// ScreenRecord: last `.sepscrr` session file (for Septic Player / open).
    #[serde(default)]
    pub septic_path: String,
    /// ScreenRecord: folder where new takes are written (pick folder, not file).
    #[serde(default)]
    pub capture_dir: String,
    /// ScreenRecord: include system cursor in captured pixels.
    #[serde(default = "default_true")]
    pub capture_cursor: bool,
    /// ScreenRecord: capture system/default audio (Pulse/PipeWire monitor).
    #[serde(default = "default_true")]
    pub capture_audio: bool,
    /// ScreenRecord: target container FPS (portal still limited by shot rate; pads to this).
    #[serde(default = "default_capture_fps")]
    pub capture_fps: u32,
    /// ScreenRecord: video bitrate in kbps (`0` = auto from resolution × fps).
    #[serde(default = "default_capture_bitrate_kbps")]
    pub capture_bitrate_kbps: u32,
    /// ScreenRecord: true while a capture session is active (runtime; not required on disk).
    #[serde(default)]
    pub screen_recording: bool,
}

impl Document {
    /// P7d/e: if `node_id` is a NE Output proxy Image, return owning layer index.
    pub fn ne_output_proxy_layer_index(&self, node_id: Uuid) -> Option<usize> {
        self.layers.iter().position(|l| {
            l.kind == LayerKind::NodeEditor && l.ne_output_proxy == Some(node_id)
        })
    }
}

fn default_max_duration() -> f32 {
    3600.0
}

fn default_zero() -> f32 {
    0.0
}

fn default_one() -> f32 {
    1.0
}

fn default_width() -> f32 {
    A4_WIDTH_PX as f32
}

fn default_height() -> f32 {
    A4_HEIGHT_PX as f32
}

fn default_true() -> bool {
    true
}

fn default_capture_fps() -> u32 {
    60
}

/// `0` keeps auto bitrate (scale with encode size × fps).
fn default_capture_bitrate_kbps() -> u32 {
    0
}

impl Layer {
    pub fn new_image(id: Uuid, name: String, visible: bool, locked: bool, nodes: Vec<NodeId>) -> Self {
        Self {
            id,
            name,
            visible,
            locked,
            nodes,
            kind: LayerKind::Image,
            video_path: String::new(),
            volume: 1.0,
            is_renderer: true,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: AvRole::Video,
            shading_passes: Vec::new(),
            node_graph: None,
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    pub fn new_av_layer(id: Uuid, name: String, media_path: String) -> Self {
        let role = if !media_path.is_empty() && AvClip::path_is_audio_only(&media_path) {
            AvRole::Audio
        } else {
            AvRole::Video
        };
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::AV,
            video_path: media_path,
            volume: 1.0,
            is_renderer: true,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: role,
            shading_passes: Vec::new(),
            node_graph: None,
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    pub fn new_empty_av_layer(id: Uuid, name: String) -> Self {
        Self::new_av_layer(id, name, String::new())
    }

    pub fn new_empty_av_layer_with_role(id: Uuid, name: String, role: AvRole) -> Self {
        let mut layer = Self::new_empty_av_layer(id, name);
        layer.av_role = role;
        layer
    }

    pub fn new_shading_layer(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::Shading,
            video_path: String::new(),
            volume: 1.0,
            is_renderer: true,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            // Display size follows the document page; shaders are not free-transform objects.
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: AvRole::Video,
            // Empty by default — callers attach the desired pass (preset / custom / MCP).
            // UI fills vignette only if still empty when the layer is inspected.
            shading_passes: Vec::new(),
            node_graph: None,
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    pub fn new_flowchart_layer(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::Flowchart,
            video_path: String::new(),
            volume: 1.0,
            is_renderer: true,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: AvRole::Video,
            shading_passes: Vec::new(),
            node_graph: None,
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    pub fn new_node_editor_layer(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::NodeEditor,
            video_path: String::new(),
            volume: 1.0,
            is_renderer: true,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: AvRole::Video,
            shading_passes: Vec::new(),
            node_graph: Some(NodeGraph::new_empty()),
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    pub fn new_screen_record_layer(id: Uuid, name: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::ScreenRecord,
            video_path: String::new(),
            volume: 1.0,
            is_renderer: false,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: A4_WIDTH_PX as f32,
            height: A4_HEIGHT_PX as f32,
            aspect_ratio_locked: true,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: 0.0,
            media_source_duration: None,
            av_clips: Vec::new(),
            music_clips: Vec::new(),
            av_role: AvRole::Video,
            shading_passes: Vec::new(),
            node_graph: None,
            ne_output_proxy: None,
            septic_path: String::new(),
            capture_dir: String::new(),
            capture_cursor: true,
            capture_audio: true,
            capture_fps: 60,
            capture_bitrate_kbps: 0,
            screen_recording: false,
        }
    }

    /// Ensure node_graph exists for NodeEditor layers.
    pub fn ensure_node_graph(&mut self) {
        if self.kind == LayerKind::NodeEditor && self.node_graph.is_none() {
            self.node_graph = Some(NodeGraph::new_empty());
        }
    }

    /// P6b/P7g: ensure a selectable canvas Image proxy for the Output Object.
    /// Creates once (empty bytes; live FX texture is painted from the graph).
    /// Clears stale proxy ids; reuses an existing empty "Output Object" Image if present.
    pub fn ensure_ne_output_proxy(&mut self, nodes: &mut NodeStore) -> Option<Uuid> {
        if self.kind != LayerKind::NodeEditor {
            return None;
        }
        if let Some(id) = self.ne_output_proxy {
            if nodes.get(id).is_some() {
                if !self.nodes.contains(&id) {
                    self.nodes.push(id);
                }
                return Some(id);
            }
            // Stale pointer (deleted without undo restore) — clear and recreate.
            self.ne_output_proxy = None;
        }
        // Prefer an existing empty Output Object Image already on this layer (load/undo).
        for &nid in &self.nodes {
            if let Some(n) = nodes.get(nid) {
                if n.name == "Output Object" {
                    if let NodeKind::Image { bytes, .. } = &n.kind {
                        if bytes.is_empty() {
                            self.ne_output_proxy = Some(nid);
                            return Some(nid);
                        }
                    }
                }
            }
        }
        let mut node = Node::image(
            self.x as f64,
            self.y as f64,
            self.width.max(1.0) as f64,
            self.height.max(1.0) as f64,
            Vec::new(),
        );
        node.name = "Output Object".into();
        let id = node.id;
        nodes.insert(node);
        self.nodes.push(id);
        self.ne_output_proxy = Some(id);
        Some(id)
    }

    /// Canvas/export placement for Output Object FilePath paint (proxy node + graph geo).
    /// Returns `(dx, dy, w, h, rot_rad)` in document space. Rotation is **radians**.
    pub fn ne_output_paint_geom(
        &self,
        nodes: &NodeStore,
        eval: &GraphOutputEval,
    ) -> (f64, f64, f64, f64, f64) {
        let (base_x, base_y, base_w, base_h, base_rot_rad) =
            if let Some(pid) = self.ne_output_proxy {
                if let Some(n) = nodes.get(pid) {
                    if let NodeKind::Image {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } = &n.kind
                    {
                        (*x, *y, *width, *height, n.get_rotation())
                    } else {
                        (
                            self.x as f64,
                            self.y as f64,
                            self.width as f64,
                            self.height as f64,
                            (self.rotation as f64).to_radians(),
                        )
                    }
                } else {
                    (
                        self.x as f64,
                        self.y as f64,
                        self.width as f64,
                        self.height as f64,
                        (self.rotation as f64).to_radians(),
                    )
                }
            } else {
                (
                    self.x as f64,
                    self.y as f64,
                    self.width as f64,
                    self.height as f64,
                    (self.rotation as f64).to_radians(),
                )
            };
        let dx = base_x + eval.geo_off_x;
        let dy = base_y + eval.geo_off_y;
        let w = (base_w * eval.geo_scale_w).max(1.0);
        let h = (base_h * eval.geo_scale_h).max(1.0);
        let rot = base_rot_rad + eval.geo_rot_deg.to_radians();
        (dx, dy, w, h, rot)
    }

    /// One-shot fit: only when the Output proxy is still the default page/A4 box.
    /// Never overrides user resize — free transform stays free.
    pub fn fit_ne_output_proxy_to_image(
        &mut self,
        nodes: &mut NodeStore,
        img_w: u32,
        img_h: u32,
        page_w: f64,
        page_h: f64,
    ) {
        if img_w == 0 || img_h == 0 {
            return;
        }
        let Some(pid) = self.ne_output_proxy else {
            return;
        };
        let Some(node) = nodes.get_mut(pid) else {
            return;
        };
        let NodeKind::Image {
            x,
            y,
            width,
            height,
            ..
        } = &mut node.kind
        else {
            return;
        };
        let def_w = self.width as f64;
        let def_h = self.height as f64;
        let near_default = (*width - def_w).abs() < 2.0 && (*height - def_h).abs() < 2.0;
        let near_a4 = (*width - A4_WIDTH_PX).abs() < 2.0 && (*height - A4_HEIGHT_PX).abs() < 2.0;
        if !near_default && !near_a4 {
            return;
        }
        let max_w = page_w.max(1.0);
        let max_h = page_h.max(1.0);
        let (tw, th) = (img_w as f64, img_h as f64);
        let (w, h) = if (max_w / tw) <= (max_h / th) {
            let w = max_w;
            let h = th * (max_w / tw);
            (w.min(max_w).max(1.0), h.min(max_h).max(1.0))
        } else {
            let h = max_h;
            let w = tw * (max_h / th);
            (w.min(max_w).max(1.0), h.min(max_h).max(1.0))
        };
        *width = w;
        *height = h;
        *x = ((max_w - w) * 0.5).max(0.0);
        *y = ((max_h - h) * 0.5).max(0.0);
    }

    /// Migrate legacy single-clip fields into `av_clips` (idempotent).
    pub fn ensure_av_clips(&mut self) {
        if self.kind != LayerKind::AV || !self.av_clips.is_empty() {
            return;
        }
        if self.video_path.is_empty()
            && self.video_timeline_start == 0.0
            && self.video_play_length >= 3599.0
        {
            return;
        }
        self.av_clips.push(AvClip::from_legacy(
            self.id,
            self.name.clone(),
            self.video_path.clone(),
            self.video_start_offset,
            self.video_play_length,
            self.video_timeline_start,
            self.media_source_duration,
        ));
    }

    pub fn has_canvas_video(&self) -> bool {
        if self.kind != LayerKind::AV || matches!(self.av_role, AvRole::Daw | AvRole::Audio) {
            return false;
        }
        if !self.av_clips.is_empty() {
            return self
                .av_clips
                .iter()
                .any(|c| !c.is_audio_only() && !c.media_path.is_empty());
        }
        !self.video_path.is_empty() && AvClip::path_is_visual_media(&self.video_path)
    }

    /// Whether a video/image frame should be painted on the canvas at timeline time `t` (seconds).
    pub fn shows_video_at(&self, t: f32) -> bool {
        if self.kind != LayerKind::AV || !self.visible {
            return false;
        }
        if matches!(self.av_role, AvRole::Daw | AvRole::Audio) {
            return false;
        }
        if !self.av_clips.is_empty() {
            return self.av_clips.iter().any(|c| {
                !c.is_audio_only() && !c.media_path.is_empty() && c.contains_timeline_sec(t)
            });
        }
        if self.video_path.is_empty() || !AvClip::path_is_visual_media(&self.video_path) {
            return false;
        }
        let start = self.video_timeline_start;
        let end = start + self.timeline_play_secs();
        t >= start && t < end
    }

    /// Primary video clip active at `t`, with legacy single-clip fallback fields.
    /// Returns (clip_id, path, start_offset, play_length_secs, timeline_start).
    pub fn video_clip_at_time(&self, t: f32) -> Option<(Uuid, &str, f32, f32, f32)> {
        if !self.shows_video_at(t) {
            return None;
        }
        if !self.av_clips.is_empty() {
            return self
                .av_clips
                .iter()
                .find(|c| {
                    !c.is_audio_only() && !c.media_path.is_empty() && c.contains_timeline_sec(t)
                })
                .map(|c| {
                    (
                        c.id,
                        c.media_path.as_str(),
                        c.video_start_offset,
                        c.timeline_play_secs(),
                        c.video_timeline_start,
                    )
                });
        }
        Some((
            self.id,
            self.video_path.as_str(),
            self.video_start_offset,
            self.timeline_play_secs(),
            self.video_timeline_start,
        ))
    }

    /// Copy a specific clip into legacy layer fields (details UI).
    pub fn sync_legacy_from_clip_id(&mut self, clip_id: Uuid) {
        if let Some(clip) = self.av_clips.iter().find(|c| c.id == clip_id).cloned() {
            self.video_path = clip.media_path;
            self.video_start_offset = clip.video_start_offset;
            self.video_play_length = clip.video_play_length;
            self.video_timeline_start = clip.video_timeline_start;
            self.media_source_duration = clip.media_source_duration;
        }
    }

    /// Copy the first visual clip into legacy layer fields for details UI.
    pub fn sync_legacy_from_primary_clip(&mut self) {
        let pick = self
            .av_clips
            .first()
            .map(|c| c.id)
            .or_else(|| self.av_clips.first().map(|c| c.id));
        if let Some(id) = pick {
            self.sync_legacy_from_clip_id(id);
        }
    }

    /// Sync legacy fields from the video clip covering timeline time `t`.
    pub fn sync_legacy_from_clip_at(&mut self, t: f32) {
        if let Some(id) = self
            .av_clips
            .iter()
            .find(|c| !c.is_audio_only() && c.contains_timeline_sec(t))
            .map(|c| c.id)
            .or_else(|| self.av_clips.first().map(|c| c.id))
        {
            self.sync_legacy_from_clip_id(id);
        }
    }

    /// Push Active Track Details trim/duration onto **one** clip (never all clips).
    pub fn sync_clip_from_legacy(&mut self, clip_id: Uuid) {
        if self.kind != LayerKind::AV {
            return;
        }
        self.ensure_av_clips();
        if let Some(clip) = self.av_clips.iter_mut().find(|c| c.id == clip_id) {
            if clip.media_path.is_empty() && !self.video_path.is_empty() {
                clip.media_path = self.video_path.clone();
            }
            clip.video_start_offset = self.video_start_offset.max(0.0);
            clip.video_play_length = self.video_play_length.max(0.1);
            if self.media_source_duration.is_some() {
                clip.media_source_duration = self.media_source_duration;
            }
        }
    }

    /// Push Active Track Details onto the first clip only (legacy name kept).
    pub fn sync_primary_clip_from_legacy(&mut self) {
        let id = self.av_clips.first().map(|c| c.id);
        if let Some(id) = id {
            self.sync_clip_from_legacy(id);
        }
    }

    /// Ensure clips exist; push Active Track Details onto the **first** clip only.
    /// Other queue items keep their own trim/duration (never bulk-overwritten).
    pub fn prepare_av_for_export(&mut self) {
        if self.kind != LayerKind::AV {
            return;
        }
        self.ensure_av_clips();
        if !self.av_clips.is_empty() {
            self.sync_primary_clip_from_legacy();
        }
    }

    // Back-compat shims (now both create unified AV layer)
    pub fn new_video(id: Uuid, name: String, video_path: String) -> Self {
        Self::new_av_layer(id, name, video_path)
    }
    pub fn new_audio(id: Uuid, name: String, audio_path: String) -> Self {
        Self::new_av_layer(id, name, audio_path)
    }

    /// Seconds of source media used on the timeline (play length capped by probe + trim).
    pub fn timeline_play_secs(&self) -> f32 {
        let source_cap = self
            .media_source_duration
            .unwrap_or(self.video_play_length)
            .max(0.0);
        let remaining = (source_cap - self.video_start_offset.max(0.0)).max(0.0);
        if self.video_play_length >= 3599.0 {
            return remaining;
        }
        self.video_play_length.min(remaining).max(0.0)
    }

    /// End time of this clip on the project timeline (seconds).
    pub fn timeline_end_secs(&self) -> f32 {
        self.video_timeline_start + self.timeline_play_secs()
    }
}

fn default_volume() -> f32 {
    1.0
}

fn default_is_renderer() -> bool {
    true
}


impl Document {
    pub fn new_default_project() -> ProjectFile {
        Self::new_empty_project()
    }

    pub fn new_empty_project() -> ProjectFile {
        let layer_id = Uuid::new_v4();
        let document = Self {
            title: "Untitled".into(),
            width: A4_WIDTH_PX,
            height: A4_HEIGHT_PX,
            active_layer_index: 0,
            layers: vec![Layer::new_image(layer_id, "Layer 1".into(), true, false, vec![])],
            defs: IndexMap::new(),
            path_effects: IndexMap::new(),
            tiling_effects: IndexMap::new(),
            circular_effects: IndexMap::new(),
            clip_masks: IndexMap::new(),
            boolean_effects: IndexMap::new(),
            page_color: default_page_color(),
            page_unit: PageUnit::Px,
        };
        ProjectFile::new(document, NodeStore::default())
    }

    pub fn page_color_egui(&self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(
            (self.page_color[0] * 255.0) as u8,
            (self.page_color[1] * 255.0) as u8,
            (self.page_color[2] * 255.0) as u8,
            (self.page_color[3] * 255.0) as u8,
        )
    }

    pub fn page_color_svg(&self) -> String {
        let r = (self.page_color[0] * 255.0) as u8;
        let g = (self.page_color[1] * 255.0) as u8;
        let b = (self.page_color[2] * 255.0) as u8;
        let a = self.page_color[3];
        format!(r#"fill="rgb({},{},{})" fill-opacity="{:.2}""#, r, g, b, a)
    }


    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active_layer_index)
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.layers.get(self.active_layer_index)
    }

    pub fn append_to_active_layer(&mut self, id: NodeId) {
        if let Some(layer) = self.layers.get_mut(self.active_layer_index) {
            layer.nodes.push(id);
        }
    }

    pub fn remove_from_layers(&mut self, id: NodeId) {
        for layer in &mut self.layers {
            layer.nodes.retain(|n| *n != id);
        }
    }

    pub fn ordered_node_ids(&self) -> Vec<NodeId> {
        self.layers
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| l.nodes.iter().copied())
            .collect()
    }

    pub fn add_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_image(Uuid::new_v4(), name.into(), true, false, vec![]);
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_av_layer(&mut self, name: impl Into<String>, media_path: String) -> usize {
        let name_str = name.into();
        let clean_name = std::path::Path::new(&name_str)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&name_str)
            .to_string();
        let mut layer = Layer::new_av_layer(Uuid::new_v4(), clean_name.clone(), media_path.clone());
        if !media_path.is_empty() {
            let mut clip = AvClip::new_from_media(clean_name, media_path, 0.0);
            clip.id = layer.id;
            layer.av_clips.push(clip);
        }
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_empty_av_layer(&mut self, name: impl Into<String>) -> usize {
        self.add_empty_av_layer_with_role(name, AvRole::Video)
    }

    pub fn add_empty_av_layer_with_role(
        &mut self,
        name: impl Into<String>,
        role: AvRole,
    ) -> usize {
        let layer = Layer::new_empty_av_layer_with_role(Uuid::new_v4(), name.into(), role);
        self.layers.push(layer);
        self.layers.len() - 1
    }

    /// First layer with the given AV role, if any.
    pub fn find_av_role_layer(&self, role: AvRole) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| l.kind == LayerKind::AV && l.av_role == role)
    }

    /// Prefer active layer when it matches role; else first match; else create.
    pub fn ensure_av_role_layer(&mut self, role: AvRole, default_name: &str) -> usize {
        if let Some(l) = self.active_layer() {
            if l.kind == LayerKind::AV && l.av_role == role {
                return self.active_layer_index;
            }
        }
        if let Some(idx) = self.find_av_role_layer(role) {
            return idx;
        }
        let idx = self.add_empty_av_layer_with_role(default_name, role);
        self.active_layer_index = idx;
        idx
    }

    /// Insert shading at the **bottom** of the stack by default. It always fills the
    /// document page; stack order is changeable via Raise/Lower (not canvas drag).
    pub fn add_shading_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_shading_layer(Uuid::new_v4(), name.into());
        self.layers.insert(0, layer);
        // Existing layer indices shift up; active points at the new shading layer.
        self.active_layer_index = 0;
        0
    }

    pub fn add_flowchart_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_flowchart_layer(Uuid::new_v4(), name.into());
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_screen_record_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_screen_record_layer(Uuid::new_v4(), name.into());
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_node_editor_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_node_editor_layer(Uuid::new_v4(), name.into());
        self.layers.push(layer);
        self.layers.len() - 1
    }

    // Back-compat (now both create AV layers)
    pub fn add_video_layer(&mut self, name: impl Into<String>, video_path: String) -> usize {
        self.add_av_layer(name, video_path)
    }
    pub fn add_audio_layer(&mut self, name: impl Into<String>, audio_path: String) -> usize {
        self.add_av_layer(name, audio_path)
    }


    pub fn move_node_in_active_layer(&mut self, id: NodeId, delta: isize) -> bool {
        let Some(layer) = self.layers.get_mut(self.active_layer_index) else {
            return false;
        };
        let Some(pos) = layer.nodes.iter().position(|n| *n == id) else {
            return false;
        };
        let new_pos = (pos as isize + delta).clamp(0, layer.nodes.len() as isize - 1) as usize;
        if new_pos == pos {
            return false;
        }
        let node = layer.nodes.remove(pos);
        layer.nodes.insert(new_pos, node);
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeStore {
    pub map: IndexMap<NodeId, Node>,
}

impl NodeStore {
    pub fn insert(&mut self, node: Node) -> NodeId {
        let id = node.id;
        self.map.insert(id, node);
        id
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.map.get(&id)
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.map.get_mut(&id)
    }

    pub fn remove(&mut self, id: NodeId) -> Option<Node> {
        self.map.swap_remove(&id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub document: Document,
    pub nodes: NodeStore,
    #[serde(default)]
    pub anim_timeline: AnimationTimeline,
}

impl ProjectFile {
    pub fn new(document: Document, nodes: NodeStore) -> Self {
        Self {
            document,
            nodes,
            anim_timeline: AnimationTimeline::default(),
        }
    }

    /// True if `id` is a living node or a document layer (AV layers can hold tracks).
    pub fn owns_animation_id(&self, id: NodeId) -> bool {
        self.nodes.get(id).is_some() || self.document.layers.iter().any(|l| l.id == id)
    }

    /// Drop timeline entries whose node/layer no longer exists (orphans after delete/bake).
    pub fn prune_orphan_animation_tracks(&mut self) -> usize {
        let live: std::collections::HashSet<NodeId> = self
            .nodes
            .map
            .keys()
            .copied()
            .chain(self.document.layers.iter().map(|l| l.id))
            .collect();
        let before = self.anim_timeline.nodes.len();
        self.anim_timeline.nodes.retain(|id, _| live.contains(id));
        before.saturating_sub(self.anim_timeline.nodes.len())
    }

    /// Remove a node from the store/layers and its animation tracks.
    pub fn remove_node_and_animation(&mut self, id: NodeId) {
        self.nodes.remove(id);
        self.document.remove_from_layers(id);
        self.anim_timeline.nodes.remove(&id);
    }
}

#[cfg(test)]
mod p7_proxy_tests {
    use super::*;

    fn ne_project() -> ProjectFile {
        let mut pf = Document::new_empty_project();
        let idx = pf.document.add_node_editor_layer("NE");
        pf.document.active_layer_index = idx;
        pf
    }

    #[test]
    fn ensure_ne_output_proxy_creates_and_reuses() {
        let mut pf = ne_project();
        let i = pf.document.active_layer_index;
        let id1 = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .expect("proxy");
        let id2 = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .expect("proxy again");
        assert_eq!(id1, id2);
        assert_eq!(pf.document.layers[i].ne_output_proxy, Some(id1));
        assert!(pf.nodes.get(id1).is_some());
        assert!(pf.document.layers[i].nodes.contains(&id1));
        let n = pf.nodes.get(id1).unwrap();
        assert_eq!(n.name, "Output Object");
        assert!(matches!(n.kind, NodeKind::Image { ref bytes, .. } if bytes.is_empty()));
    }

    #[test]
    fn ensure_ne_output_proxy_rebinds_stale_id() {
        let mut pf = ne_project();
        let i = pf.document.active_layer_index;
        let id1 = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .unwrap();
        // Simulate orphaned pointer: remove node but leave field set.
        pf.nodes.remove(id1);
        pf.document.layers[i].nodes.retain(|n| *n != id1);
        pf.document.layers[i].ne_output_proxy = Some(id1);
        let id2 = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .unwrap();
        assert_ne!(id1, id2);
        assert!(pf.nodes.get(id2).is_some());
        assert_eq!(pf.document.layers[i].ne_output_proxy, Some(id2));
    }

    #[test]
    fn ensure_ne_output_proxy_reuses_existing_named_image() {
        let mut pf = ne_project();
        let i = pf.document.active_layer_index;
        let mut node = Node::image(10.0, 20.0, 100.0, 80.0, Vec::new());
        node.name = "Output Object".into();
        let nid = node.id;
        pf.nodes.insert(node);
        pf.document.layers[i].nodes.push(nid);
        // Field unset — should adopt existing node.
        assert!(pf.document.layers[i].ne_output_proxy.is_none());
        let got = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .unwrap();
        assert_eq!(got, nid);
        assert_eq!(pf.document.layers[i].ne_output_proxy, Some(nid));
    }

    #[test]
    fn ne_output_proxy_layer_index_and_paint_geom() {
        let mut pf = ne_project();
        let i = pf.document.active_layer_index;
        let id = pf.document.layers[i]
            .ensure_ne_output_proxy(&mut pf.nodes)
            .unwrap();
        if let Some(n) = pf.nodes.get_mut(id) {
            if let NodeKind::Image {
                x, y, width, height, ..
            } = &mut n.kind
            {
                *x = 50.0;
                *y = 60.0;
                *width = 200.0;
                *height = 100.0;
            }
            n.set_rotation(std::f64::consts::FRAC_PI_2);
        }
        assert_eq!(
            pf.document.ne_output_proxy_layer_index(id),
            Some(i)
        );
        assert!(pf.document.ne_output_proxy_layer_index(Uuid::new_v4()).is_none());

        let eval = GraphOutputEval {
            geo_off_x: 5.0,
            geo_off_y: 7.0,
            geo_scale_w: 1.0,
            geo_scale_h: 1.0,
            geo_rot_deg: 0.0,
            ..Default::default()
        };
        let (dx, dy, w, h, rot) = pf.document.layers[i].ne_output_paint_geom(&pf.nodes, &eval);
        assert!((dx - 55.0).abs() < 1e-9);
        assert!((dy - 67.0).abs() < 1e-9);
        assert!((w - 200.0).abs() < 1e-9);
        assert!((h - 100.0).abs() < 1e-9);
        assert!((rot - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn ensure_ne_output_proxy_noop_on_image_layer() {
        let mut pf = Document::new_empty_project();
        let id = pf.document.layers[0].ensure_ne_output_proxy(&mut pf.nodes);
        assert!(id.is_none());
    }
}