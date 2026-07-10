mod node;
mod path_effects;
mod style;
mod animation;
mod av_clip;
mod music;
mod shading;
pub mod flowchart;

pub use av_clip::*;
pub use node::*;
pub use path_effects::*;
pub use style::*;
pub use animation::*;
pub use music::*;
pub use shading::*;
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
    /// DAW-style music clips on the AV timeline (same layer as media).
    #[serde(default)]
    pub music_clips: Vec<MusicClip>,
    /// WGSL shading passes when `kind == Shading`.
    #[serde(default)]
    pub shading_passes: Vec<ShadingPass>,
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
            shading_passes: Vec::new(),
        }
    }

    pub fn new_av_layer(id: Uuid, name: String, media_path: String) -> Self {
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
            shading_passes: Vec::new(),
        }
    }

    pub fn new_empty_av_layer(id: Uuid, name: String) -> Self {
        Self::new_av_layer(id, name, String::new())
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
            // Empty by default — callers attach the desired pass (preset / custom / MCP).
            // UI fills vignette only if still empty when the layer is inspected.
            shading_passes: Vec::new(),
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
            shading_passes: Vec::new(),
        }
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
        if self.kind != LayerKind::AV {
            return false;
        }
        if !self.av_clips.is_empty() {
            return self.av_clips.iter().any(|c| !c.is_audio_only());
        }
        !self.video_path.is_empty() && !AvClip::from_legacy(
            self.id,
            String::new(),
            self.video_path.clone(),
            0.0,
            0.0,
            0.0,
            None,
        )
        .is_audio_only()
    }

    /// Copy primary clip timing into legacy layer fields (UI / preview).
    pub fn sync_legacy_from_primary_clip(&mut self) {
        if let Some(clip) = self.av_clips.first() {
            self.video_path = clip.media_path.clone();
            self.video_start_offset = clip.video_start_offset;
            self.video_play_length = clip.video_play_length;
            self.video_timeline_start = clip.video_timeline_start;
            self.media_source_duration = clip.media_source_duration;
        }
    }

    /// Push Active Track Details trim/duration onto the primary clip.
    /// Timeline placement stays clip-authoritative (strip drag); only in-point and
    /// play length are written from the layer fields used by the details bar.
    pub fn sync_primary_clip_from_legacy(&mut self) {
        if self.kind != LayerKind::AV {
            return;
        }
        self.ensure_av_clips();
        if let Some(clip) = self.av_clips.first_mut() {
            if clip.media_path.is_empty() && !self.video_path.is_empty() {
                clip.media_path = self.video_path.clone();
            }
            clip.video_start_offset = self.video_start_offset.max(0.0);
            clip.video_play_length = self.video_play_length.max(0.1);
            if self.media_source_duration.is_some() {
                clip.media_source_duration = self.media_source_duration;
            } else if clip.media_source_duration.is_none() {
                clip.media_source_duration = self.media_source_duration;
            }
        }
    }

    /// Ensure clips exist and primary clip carries layer Trim Start / Play Duration.
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
        let mut layer = Layer::new_av_layer(Uuid::new_v4(), name.into(), media_path.clone());
        if !media_path.is_empty() {
            let mut clip = AvClip::new_from_media(layer.name.clone(), media_path, 0.0);
            clip.id = layer.id;
            layer.av_clips.push(clip);
        }
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_empty_av_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_empty_av_layer(Uuid::new_v4(), name.into());
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_shading_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_shading_layer(Uuid::new_v4(), name.into());
        self.layers.push(layer);
        self.layers.len() - 1
    }

    pub fn add_flowchart_layer(&mut self, name: impl Into<String>) -> usize {
        let layer = Layer::new_flowchart_layer(Uuid::new_v4(), name.into());
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