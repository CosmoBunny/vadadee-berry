mod node;
mod path_effects;
mod style;
mod animation;

pub use node::*;
pub use path_effects::*;
pub use style::*;
pub use animation::*;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A4 at 96 DPI (pixels), portrait.
pub const A4_WIDTH_PX: f64 = 794.0;
pub const A4_HEIGHT_PX: f64 = 1123.0;

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
    #[serde(default = "default_page_color")]
    pub page_color: [f32; 4],
}

fn default_page_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerKind {
    Image,
    Video,
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
        }
    }

    pub fn new_video(id: Uuid, name: String, video_path: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            nodes: vec![],
            kind: LayerKind::Video,
            video_path,
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
        }
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
            page_color: default_page_color(),
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

    pub fn add_video_layer(&mut self, name: impl Into<String>, video_path: String) -> usize {
        let layer = Layer::new_video(Uuid::new_v4(), name.into(), video_path);
        self.layers.push(layer);
        self.layers.len() - 1
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
}