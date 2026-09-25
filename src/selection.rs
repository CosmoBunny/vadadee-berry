//! Selection queries with narrow dependencies (P2 refactor).
//!
//! Per architecture review: behavior must move out of `VadadeeBerryApp`
//! into subsystem units that receive only what they need. These were the
//! first movers because they touch exactly two things — the project and
//! the selection — and are called from app.rs, ui.rs and tools.
//!
//! Long-term home of `SelectionController`; mutation commands
//! (delete/reorder/transform selection) stay on the
//! [`crate::commands::EditorCommand`] path.

use crate::document::NodeKind;
use crate::document::{LayerKind, Node, NodeId, ProjectFile};
use crate::document::{has_effect_for_objects, path_effect_by_form_node};

/// Union of geometric bounds of the selected nodes.
pub fn selection_bounds(project: &ProjectFile, selection: &[NodeId]) -> Option<kurbo::Rect> {
    if selection.is_empty() {
        return None;
    }
    let mut union_rect: Option<kurbo::Rect> = None;
    for id in selection {
        if let Some(node) = project.nodes.get(*id) {
            let bounds = node.bounds_with_store(&project.nodes);
            if let Some(ref mut u) = union_rect {
                *u = u.union(bounds);
            } else {
                union_rect = Some(bounds);
            }
        }
    }
    union_rect
}

/// Selection bounds expanded by half stroke width so raster export/clipboard
/// does not clip strokes (geometry bounds alone omit stroke extent).
pub fn selection_bounds_for_raster(
    project: &ProjectFile,
    selection: &[NodeId],
) -> Option<kurbo::Rect> {
    let mut r = selection_bounds(project, selection)?;
    let mut pad = 0.5_f64; // subpixel AA fringe
    for id in selection {
        let Some(node) = project.nodes.get(*id) else {
            continue;
        };
        if node.style.stroke.style.is_visible() && node.style.stroke.width > 0.0 {
            pad = pad.max(node.style.stroke.width as f64 * 0.5 + 0.5);
        }
    }
    r = r.inflate(pad, pad);
    // Degenerate thin selection: ensure non-zero export viewBox.
    if r.width() < 1e-3 {
        r = kurbo::Rect::new(r.x0 - 0.5, r.y0, r.x1 + 0.5, r.y1);
    }
    if r.height() < 1e-3 {
        r = kurbo::Rect::new(r.x0, r.y0 - 0.5, r.x1, r.y1 + 0.5);
    }
    Some(r)
}

/// Kind of the layer when exactly one layer is selected.
pub fn selected_layer_kind(project: &ProjectFile, selection: &[NodeId]) -> Option<LayerKind> {
    if selection.len() != 1 {
        return None;
    }
    let id = selection[0];
    project
        .document
        .layers
        .iter()
        .find(|l| l.id == id)
        .map(|l| l.kind)
}

/// True when the selection is exactly one image node.
pub fn selection_is_single_image(project: &ProjectFile, selection: &[NodeId]) -> bool {
    if selection.len() != 1 {
        return false;
    }
    project
        .nodes
        .get(selection[0])
        .map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
}

/// (objects, path) when the selection is one path + ≥1 objects, or a single
/// node carrying a path-effect form (returns its source + path).
pub fn selection_path_and_objects(
    project: &ProjectFile,
    selection: &[NodeId],
) -> Option<(Vec<NodeId>, NodeId)> {
    if selection.len() == 1 {
        if let Some(eff) = path_effect_by_form_node(&project.document.path_effects, selection[0]) {
            return Some((vec![eff.source_id], eff.path_id));
        }
    }
    let mut paths = Vec::new();
    let mut objects = Vec::new();
    for id in selection {
        let Some(node) = project.nodes.get(*id) else {
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

/// First (object, path) pair of [`selection_path_and_objects`].
pub fn selection_path_and_object(
    project: &ProjectFile,
    selection: &[NodeId],
) -> Option<(NodeId, NodeId)> {
    selection_path_and_objects(project, selection)
        .and_then(|(objs, path)| objs.first().copied().map(|o| (o, path)))
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

/// Selected ids usable as Tiling / CircularClone sources.
pub fn selection_tiling_circular_sources(
    project: &ProjectFile,
    selection: &[NodeId],
) -> Vec<NodeId> {
    selection
        .iter()
        .copied()
        .filter(|id| {
            project
                .nodes
                .get(*id)
                .is_some_and(is_tiling_circular_source)
        })
        .collect()
}

/// True when any selected object is the source of a tiling effect.
pub fn selection_has_tiling_effect(project: &ProjectFile, selection: &[NodeId]) -> bool {
    selection.iter().any(|&oid| {
        project
            .document
            .tiling_effects
            .values()
            .any(|e| e.source_id == oid)
    })
}

/// True when any selected object is the source of a circular-clone effect.
pub fn selection_has_circular_effect(project: &ProjectFile, selection: &[NodeId]) -> bool {
    selection.iter().any(|&oid| {
        project
            .document
            .circular_effects
            .values()
            .any(|e| e.source_id == oid)
    })
}

/// Panel context for on-path editing: form-node source, path+objects pair,
/// or objects linked to the selected path via effect links.
pub fn object_on_path_panel_context(
    project: &ProjectFile,
    selection: &[NodeId],
) -> Option<(Vec<NodeId>, NodeId)> {
    if selection.len() == 1 {
        if let Some(eff) = path_effect_by_form_node(&project.document.path_effects, selection[0]) {
            return Some((vec![eff.source_id], eff.path_id));
        }
    }
    if let Some(ctx) = selection_path_and_objects(project, selection) {
        return Some(ctx);
    }
    if selection.len() != 1 {
        return None;
    }
    let path_id = selection[0];
    let path_node = project.nodes.get(path_id)?;
    if !matches!(path_node.kind, NodeKind::Path { .. }) {
        return None;
    }
    let mut objects = Vec::new();
    for effect_id in &path_node.path_effect_links {
        let Some(effect) = project.document.path_effects.get(effect_id) else {
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

/// True when the panel-context objects already carry an on-path effect.
pub fn selection_has_object_on_path_effect(project: &ProjectFile, selection: &[NodeId]) -> bool {
    let Some((objects, path_id)) = object_on_path_panel_context(project, selection) else {
        return false;
    };
    has_effect_for_objects(&project.document.path_effects, &objects, path_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn empty_selection_has_no_bounds() {
        let project = Document::new_empty_project();
        assert!(selection_bounds(&project, &[]).is_none());
        assert!(selection_bounds_for_raster(&project, &[]).is_none());
        assert!(selected_layer_kind(&project, &[]).is_none());
        assert!(!selection_is_single_image(&project, &[]));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let project = Document::new_empty_project();
        let ghost = uuid::Uuid::new_v4();
        assert!(selection_bounds(&project, &[ghost]).is_none());
        assert!(!selection_is_single_image(&project, &[ghost]));
    }
}
