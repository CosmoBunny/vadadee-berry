mod node;
mod path_effects;
mod style;

pub use node::*;
pub use path_effects::*;
pub use style::*;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub nodes: Vec<NodeId>,
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
            layers: vec![Layer {
                id: layer_id,
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                nodes: vec![],
            }],
            defs: IndexMap::new(),
            path_effects: IndexMap::new(),
        };
        ProjectFile::new(document, NodeStore::default())
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
        let layer = Layer {
            id: Uuid::new_v4(),
            name: name.into(),
            visible: true,
            locked: false,
            nodes: vec![],
        };
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
}

impl ProjectFile {
    pub fn new(document: Document, nodes: NodeStore) -> Self {
        Self { document, nodes }
    }
}