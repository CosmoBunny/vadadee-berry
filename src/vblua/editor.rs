//! Scriptable editor automation (Phase 16): transactions + canvas node ops.
//!
//! Lua shape:
//! ```lua
//! vblua.editor.transaction(function()
//!   vblua.graph.create("Blur")
//!   error("boom")              -- aborts the WHOLE batch, nothing applies
//! end)
//! local copy = vblua.editor.duplicate_node(id)
//! vblua.editor.rename_node(id, "Hero")
//! vblua.editor.delete_node(id)
//! ```
//!
//! Rules:
//! - Normal runs keep partial work on error (traya convention). Inside
//!   `transaction()`, ANY error discards every queued command — all or
//!   nothing. One batch still equals one undo entry per store.
//! - Canvas node ops (rename/duplicate/delete) validate against a node
//!   snapshot at call time and preallocate ids like graph creates.
//! - Deletions capture the removed node + animation + layer context at apply
//!   time so the host records an exact `RemoveNodes` undo entry.
//! - App-owned state (selection, clipboard, playback, export) is NOT exposed:
//!   it lives in event-loop state, not the project. Documented, not faked.

use std::collections::HashMap;

use crate::document::{Document, Node, NodeId, NodeStore};

/// Read-only canvas node row.
#[derive(Debug, Clone)]
pub struct NodeRow {
    pub name: String,
    pub layer_id: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub rotation_deg: f64,
    pub opacity: f64,
}

/// Short kind label for script filters (`object:type()` style checks).
pub fn canvas_kind_label(kind: &crate::document::NodeKind) -> &'static str {
    use crate::document::NodeKind::*;
    match kind {
        Rect { .. } => "rect",
        Ellipse { .. } => "ellipse",
        Polygon { .. } => "polygon",
        Path { .. } => "path",
        Text { .. } => "text",
        Group { .. } => "group",
        Image { .. } => "image",
        Plotter { .. } => "plotter",
        Arc { .. } => "arc",
        BrushStroke { .. } => "brush",
        FlowchartNode { .. } => "flowchart",
        FlowchartPath { .. } => "flowchart_path",
    }
}

fn node_row(node: &Node, layer_id: &str) -> NodeRow {
    NodeRow {
        name: node.name.clone(),
        layer_id: layer_id.to_string(),
        kind: canvas_kind_label(&node.kind).to_string(),
        x: node.transform.translation[0],
        y: node.transform.translation[1],
        rotation_deg: node.transform.rotation_rad.to_degrees(),
        opacity: node.style.opacity as f64,
    }
}

/// Canvas node inventory (id -> row) for call-time validation.
#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot {
    pub nodes: HashMap<String, NodeRow>,
}

impl EditorSnapshot {
    pub fn capture(doc: &Document, store: &NodeStore) -> Self {
        let mut nodes = HashMap::new();
        for layer in &doc.layers {
            let layer_id = layer.id.to_string();
            for id in &layer.nodes {
                let row = store.map.get(id).map(|n| node_row(n, &layer_id));
                nodes.insert(
                    id.to_string(),
                    row.unwrap_or(NodeRow {
                        name: String::new(),
                        layer_id: layer_id.clone(),
                        kind: "missing".into(),
                        x: 0.0,
                        y: 0.0,
                        rotation_deg: 0.0,
                        opacity: 1.0,
                    }),
                );
            }
        }
        // Orphan store nodes (not on any layer) stay addressable.
        for (id, node) in &store.map {
            nodes
                .entry(id.to_string())
                .or_insert_with(|| node_row(node, ""));
        }
        Self { nodes }
    }
}

/// One staged canvas-node mutation.
#[derive(Debug, Clone)]
pub enum NodeCommand {
    RenameNode { node_id: NodeId, name: String },
    DuplicateNode { src: NodeId, new_id: NodeId },
    DeleteNode { node_id: NodeId },
    MoveNode { node_id: NodeId, x: f64, y: f64 },
    SetOpacity { node_id: NodeId, opacity: f32 },
}

/// What a node-command batch removed (for exact `RemoveNodes` undo).
#[derive(Debug, Clone)]
pub struct RemovedNode {
    pub node: Node,
    pub anim: Option<crate::document::NodeAnimation>,
    pub layer_index: usize,
    pub layer_nodes_before: Vec<NodeId>,
}

impl NodeCommand {
    /// Apply one command to the project. Returns the created node (if any).
    pub fn apply_to_project(
        &self,
        doc: &mut Document,
        nodes: &mut NodeStore,
        timeline: &mut crate::document::AnimationTimeline,
    ) -> NodeApplyOutcome {
        match self {
            NodeCommand::RenameNode { node_id, name } => {
                if let Some(n) = nodes.map.get_mut(node_id) {
                    n.name = name.clone();
                    return NodeApplyOutcome::Changed;
                }
                NodeApplyOutcome::Noop
            }
            NodeCommand::DuplicateNode { src, new_id } => {
                let Some(src_node) = nodes.map.get(src).cloned() else {
                    return NodeApplyOutcome::Noop;
                };
                let layer_idx = doc
                    .layers
                    .iter()
                    .position(|l| l.nodes.contains(src))
                    .unwrap_or(doc.active_layer_index);
                let mut node = src_node;
                node.id = *new_id;
                node.name = format!("{} copy", node.name.chars().take(100).collect::<String>());
                nodes.insert(node.clone());
                if let Some(layer) = doc.layers.get_mut(layer_idx) {
                    layer.nodes.push(*new_id);
                }
                NodeApplyOutcome::Created(node)
            }
            NodeCommand::DeleteNode { node_id } => {
                let Some(node) = nodes.map.shift_remove(node_id) else {
                    return NodeApplyOutcome::Noop;
                };
                let anim = timeline.nodes.remove(node_id);
                let mut layer_index = 0;
                let mut before = Vec::new();
                for (i, layer) in doc.layers.iter_mut().enumerate() {
                    if layer.nodes.contains(node_id) {
                        layer_index = i;
                        before = layer.nodes.clone();
                        layer.nodes.retain(|id| id != node_id);
                    }
                }
                NodeApplyOutcome::Removed(RemovedNode {
                    node,
                    anim,
                    layer_index,
                    layer_nodes_before: before,
                })
            }
            NodeCommand::MoveNode { node_id, x, y } => {
                let Some(n) = nodes.map.get_mut(node_id) else {
                    return NodeApplyOutcome::Noop;
                };
                n.transform.translation = [
                    x.clamp(-16384.0, 16384.0),
                    y.clamp(-16384.0, 16384.0),
                ];
                NodeApplyOutcome::Changed
            }
            NodeCommand::SetOpacity { node_id, opacity } => {
                let Some(n) = nodes.map.get_mut(node_id) else {
                    return NodeApplyOutcome::Noop;
                };
                n.style.opacity = opacity.clamp(0.0, 1.0);
                NodeApplyOutcome::Changed
            }
        }
    }
}

/// Outcome of one node command (drives precise undo entries).
#[derive(Debug)]
pub enum NodeApplyOutcome {
    Noop,
    Changed,
    Created(Node),
    Removed(RemovedNode),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AnimationTimeline, Fill};

    fn project_one_node() -> (Document, NodeStore, AnimationTimeline, NodeId) {
        let doc = Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![crate::document::Layer::new_image(
                uuid::Uuid::new_v4(),
                "L".into(),
                true,
                false,
                vec![],
            )],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        };
        let mut nodes = NodeStore::default();
        let node = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
        let id = node.id;
        nodes.insert(node);
        let mut doc = doc;
        doc.layers[0].nodes.push(id);
        (doc, nodes, AnimationTimeline::default(), id)
    }

    #[test]
    fn rename_duplicate_delete_round_trip() {
        let (mut doc, mut nodes, mut tl, id) = project_one_node();
        assert!(matches!(
            NodeCommand::RenameNode {
                node_id: id,
                name: "Hero".into()
            }
            .apply_to_project(&mut doc, &mut nodes, &mut tl),
            NodeApplyOutcome::Changed
        ));
        assert_eq!(nodes.map.get(&id).unwrap().name, "Hero");
        let copy = uuid::Uuid::new_v4();
        match (NodeCommand::DuplicateNode { src: id, new_id: copy })
            .apply_to_project(&mut doc, &mut nodes, &mut tl)
        {
            NodeApplyOutcome::Created(n) => assert_eq!(n.id, copy),
            _ => panic!("expected Created"),
        }
        assert!(doc.layers[0].nodes.contains(&copy));
        match (NodeCommand::DeleteNode { node_id: copy })
            .apply_to_project(&mut doc, &mut nodes, &mut tl)
        {
            NodeApplyOutcome::Removed(r) => {
                assert_eq!(r.node.id, copy);
                assert_eq!(r.layer_nodes_before.len(), 2);
            }
            _ => panic!("expected Removed"),
        }
        assert!(!nodes.map.contains_key(&copy));
    }

    #[test]
    fn move_and_opacity_apply() {
        let (mut doc, mut nodes, mut timeline, id) = project_one_node();
        let cmd = NodeCommand::MoveNode { node_id: id, x: 30.0, y: -5.0 };
        match cmd.apply_to_project(&mut doc, &mut nodes, &mut timeline) {
            NodeApplyOutcome::Changed => {}
            o => panic!("expected Changed, got {o:?}"),
        }
        let n = nodes.map.get(&id).unwrap();
        assert_eq!(n.transform.translation, [30.0, -5.0]);
        let cmd = NodeCommand::SetOpacity { node_id: id, opacity: 2.0 };
        match cmd.apply_to_project(&mut doc, &mut nodes, &mut timeline) {
            NodeApplyOutcome::Changed => {}
            o => panic!("expected Changed, got {o:?}"),
        }
        assert_eq!(nodes.map.get(&id).unwrap().style.opacity, 1.0);
        // Unknown id is a Noop, never a panic.
        let cmd = NodeCommand::MoveNode { node_id: uuid::Uuid::new_v4(), x: 1.0, y: 1.0 };
        assert!(matches!(
            cmd.apply_to_project(&mut doc, &mut nodes, &mut timeline),
            NodeApplyOutcome::Noop
        ));
    }

    #[test]
    fn snapshot_rows_carry_kind_and_geometry() {
        let (doc, nodes, _, id) = project_one_node();
        let snap = EditorSnapshot::capture(&doc, &nodes);
        let row = snap.nodes.get(&id.to_string()).unwrap();
        assert_eq!(row.kind, "rect");
        assert_eq!(row.layer_id, doc.layers[0].id.to_string());
    }
}
