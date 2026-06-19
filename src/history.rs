use undo::{Edit, Record};

use crate::document::{Document, Node, NodeId, ProjectFile};

#[derive(Debug, Clone)]
pub enum ProjectEdit {
    InsertNode { node: Node },
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
}

impl Default for History {
    fn default() -> Self {
        Self {
            record: Record::new(),
        }
    }
}

impl History {
    pub fn push(&mut self, project: &mut ProjectFile, cmd: ProjectEdit) {
        self.record.edit(project, cmd);
    }

    pub fn undo(&mut self, project: &mut ProjectFile) -> bool {
        self.record.undo(project).is_some()
    }

    pub fn redo(&mut self, project: &mut ProjectFile) -> bool {
        self.record.redo(project).is_some()
    }

    pub fn can_undo(&self) -> bool {
        self.record.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.record.can_redo()
    }
}

fn apply_forward(cmd: &ProjectEdit, project: &mut ProjectFile) {
    match cmd {
        ProjectEdit::InsertNode { node } => {
            let id = project.nodes.insert(node.clone());
            project.document.append_to_active_layer(id);
        }
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
    }
}

fn apply_inverse(cmd: &ProjectEdit, project: &mut ProjectFile) {
    match cmd {
        ProjectEdit::InsertNode { node } => {
            let id = node.id;
            project.nodes.remove(id);
            project.document.remove_from_layers(id);
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
    }
}

pub fn snapshot_project(project: &ProjectFile) -> ProjectFile {
    project.clone()
}

pub fn snapshot_document(doc: &Document) -> Document {
    doc.clone()
}