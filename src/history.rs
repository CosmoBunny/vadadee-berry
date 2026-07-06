use undo::{Edit, Record};

use crate::document::{Document, Node, NodeId, ProjectFile, AnimationTimeline};

pub const DEFAULT_UNDO_LIMIT: usize = 30;

#[derive(Debug, Clone)]
pub enum ProjectEdit {
    InsertNode { node: Node },
    InsertNodes { nodes: Vec<Node> },
    /// Nodes already live in the project; history only records undo/redo.
    InsertNodesApplied { nodes: Vec<Node> },
    PatchNodes { patches: Vec<(NodeId, Node, Node)> },
    RemoveNodes {
        removed: Vec<(NodeId, Node)>,
        layer_index: usize,
        layer_nodes_before: Vec<NodeId>,
    },
    PatchNode {
        id: NodeId,
        before: Node,
        after: Node,
    },
    PatchDocument {
        before: Document,
        after: Document,
    },
    ReorderNodes {
        layer_index: usize,
        before: Vec<NodeId>,
        after: Vec<NodeId>,
    },
    SetDocument {
        before: ProjectFile,
        after: ProjectFile,
    },
    PatchTimeline {
        before: AnimationTimeline,
        after: AnimationTimeline,
    },
}

impl Edit for ProjectEdit {
    type Target = ProjectFile;
    type Output = ();

    fn edit(&mut self, target: &mut Self::Target) -> Self::Output {
        apply_forward(self, target);
    }

    fn undo(&mut self, target: &mut Self::Target) -> Self::Output {
        apply_inverse(self, target);
    }
}

pub struct History {
    record: Record<ProjectEdit>,
    /// Bumped on every edit/undo/redo — used to invalidate canvas raster caches.
    revision: u64,
}

impl Default for History {
    fn default() -> Self {
        Self {
            record: Record::builder().limit(DEFAULT_UNDO_LIMIT).build(),
            revision: 0,
        }
    }
}

impl History {
    /// Create a history with a custom undo/redo limit.
    /// Limit of 0 is invalid (panics internally from the undo crate).
    pub fn with_limit(limit: usize) -> Self {
        Self {
            record: Record::builder().limit(limit).build(),
            revision: 0,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl History {
    pub fn clear(&mut self) {
        self.record = Record::builder().limit(DEFAULT_UNDO_LIMIT).build();
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn push(&mut self, project: &mut ProjectFile, cmd: ProjectEdit) {
        self.record.edit(project, cmd);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Record an edit whose forward apply is already reflected in `project`.
    pub fn push_applied(&mut self, project: &mut ProjectFile, cmd: ProjectEdit) {
        self.record.edit(project, cmd);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn undo(&mut self, project: &mut ProjectFile) -> bool {
        if self.record.undo(project).is_some() {
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, project: &mut ProjectFile) -> bool {
        if self.record.redo(project).is_some() {
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        self.record.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.record.can_redo()
    }

    /// Current undo/redo limit (number of undo steps kept).
    pub fn limit(&self) -> usize {
        self.record.limit()
    }
}

fn apply_forward(cmd: &ProjectEdit, project: &mut ProjectFile) {
    match cmd {
        ProjectEdit::InsertNode { node } => {
            let id = project.nodes.insert(node.clone());
            project.document.append_to_active_layer(id);
        }
        ProjectEdit::InsertNodes { nodes } => {
            for node in nodes {
                project.nodes.insert(node.clone());
                project.document.append_to_active_layer(node.id);
            }
        }
        ProjectEdit::InsertNodesApplied { .. } => {}
        ProjectEdit::RemoveNodes {
            removed,
            layer_index,
            layer_nodes_before,
        } => {
            let gone: std::collections::HashSet<_> = removed.iter().map(|(id, _)| *id).collect();
            for (id, _) in removed {
                project.nodes.remove(*id);
            }
            if let Some(layer) = project.document.layers.get_mut(*layer_index) {
                layer.nodes = layer_nodes_before
                    .iter()
                    .filter(|id| !gone.contains(id))
                    .copied()
                    .collect();
            }
        }
        ProjectEdit::PatchNode { id, after, .. } => {
            if let Some(n) = project.nodes.get_mut(*id) {
                *n = after.clone();
            }
        }
        ProjectEdit::PatchNodes { patches } => {
            for (id, _, after) in patches {
                if let Some(n) = project.nodes.get_mut(*id) {
                    *n = after.clone();
                }
            }
        }
        ProjectEdit::PatchDocument { after, .. } => {
            project.document = after.clone();
        }
        ProjectEdit::ReorderNodes {
            layer_index,
            after,
            ..
        } => {
            if let Some(layer) = project.document.layers.get_mut(*layer_index) {
                layer.nodes = after.clone();
            }
        }
        ProjectEdit::SetDocument { after, .. } => {
            *project = after.clone();
        }
        ProjectEdit::PatchTimeline { after, .. } => {
            project.anim_timeline = after.clone();
        }
    }
}

fn apply_inverse(cmd: &ProjectEdit, project: &mut ProjectFile) {
    match cmd {
        ProjectEdit::InsertNode { node } => {
            let id = node.id;
            project.nodes.remove(id);
            project.document.remove_from_layers(id);
        }
        ProjectEdit::InsertNodes { nodes } => {
            for node in nodes {
                project.nodes.remove(node.id);
                project.document.remove_from_layers(node.id);
            }
        }
        ProjectEdit::InsertNodesApplied { nodes } => {
            for node in nodes {
                project.nodes.remove(node.id);
                project.document.remove_from_layers(node.id);
            }
        }
        ProjectEdit::RemoveNodes {
            removed,
            layer_index,
            layer_nodes_before,
        } => {
            if let Some(layer) = project.document.layers.get_mut(*layer_index) {
                layer.nodes = layer_nodes_before.clone();
            }
            for (id, node) in removed {
                project.nodes.map.insert(*id, node.clone());
            }
        }
        ProjectEdit::PatchNode { id, before, .. } => {
            if let Some(n) = project.nodes.get_mut(*id) {
                *n = before.clone();
            }
        }
        ProjectEdit::PatchNodes { patches } => {
            for (id, before, _) in patches {
                if let Some(n) = project.nodes.get_mut(*id) {
                    *n = before.clone();
                }
            }
        }
        ProjectEdit::PatchDocument { before, .. } => {
            project.document = before.clone();
        }
        ProjectEdit::ReorderNodes {
            layer_index,
            before,
            ..
        } => {
            if let Some(layer) = project.document.layers.get_mut(*layer_index) {
                layer.nodes = before.clone();
            }
        }
        ProjectEdit::SetDocument { before, .. } => {
            *project = before.clone();
        }
        ProjectEdit::PatchTimeline { before, .. } => {
            project.anim_timeline = before.clone();
        }
    }
}

pub fn snapshot_project(project: &ProjectFile) -> ProjectFile {
    project.clone()
}

pub fn snapshot_document(doc: &Document) -> Document {
    doc.clone()
}
