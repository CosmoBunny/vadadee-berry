//! Mobile layer sheet: large touch targets, clear selection, visibility,
//! lock, rename, delete, reorder, add.
//!
//! Pure presentation returning [`LayerAction`]s; the shell applies them to
//! the shared history-backed layer commands (`set_active_layer`,
//! `set_layer_visible`, `set_layer_locked`, `rename_layer`, `delete_layer`,
//! `nudge_layer_order`, `add_layer`). No document mutation here.
//!
//! Duplicate is deliberately absent: duplicating a layer requires deep node
//! remapping (shared `NodeStore`, groups, effects) — a shared-command design
//! of its own, not a sheet concern.

use crate::document::LayerKind;
use crate::platform::UiMetrics;

/// Intent from the layer sheet; applied by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LayerAction {
    #[default]
    None,
    SetActive(usize),
    ToggleVisible(usize),
    ToggleLocked(usize),
    /// Commit a finished rename (single history entry, never per keystroke).
    CommitRename(usize, String),
    Delete(usize),
    MoveUp(usize),
    MoveDown(usize),
    Add,
}

/// One row's worth of layer data, copied out of the document so the sheet
/// holds no document borrows while emitting actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRow {
    pub index: usize,
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    pub active: bool,
    pub node_count: usize,
}

/// Snapshot the document's layers top-first (index 0 paints bottom).
pub fn layer_rows(project: &crate::document::ProjectFile) -> Vec<LayerRow> {
    let active = project.document.active_layer_index;
    let mut rows: Vec<LayerRow> = project
        .document
        .layers
        .iter()
        .enumerate()
        .map(|(index, l)| LayerRow {
            index,
            name: l.name.clone(),
            kind: l.kind,
            visible: l.visible,
            locked: l.locked,
            active: index == active,
            node_count: l.nodes.len(),
        })
        .collect();
    rows.reverse();
    rows
}

/// Render rows + add button. `rename` is `(index, buffer)` while a row is
/// being renamed; commit/cancel buttons finish the edit.
#[allow(clippy::too_many_arguments)]
pub fn show_layer_sheet(
    rows: &[LayerRow],
    can_delete: bool,
    rename: &mut Option<(usize, String)>,
    metrics: UiMetrics,
    ui: &mut egui::Ui,
) -> LayerAction {
    let mut action = LayerAction::None;
    let btn = [metrics.touch_target, metrics.touch_target];
    for row in rows {
        ui.horizontal(|ui| {
            // Select whole row → active layer.
            let name_btn = egui::Button::selectable(
                row.active,
                format!(
                    "{} {} ({})",
                    if row.active { "●" } else { "○" },
                    row.name,
                    row.node_count
                ),
            );
            if ui
                .add_sized([ui.available_width() * 0.4, btn[1]], name_btn)
                .clicked()
            {
                action = LayerAction::SetActive(row.index);
            }
            // Rename editor replaces the row label while editing.
            if let Some((edit_index, buf)) = rename
                && *edit_index == row.index
            {
                ui.text_edit_singleline(buf);
                if ui
                    .add_sized([btn[0] * 0.7, btn[1]], egui::Button::new("✔"))
                    .clicked()
                {
                    action = LayerAction::CommitRename(row.index, buf.clone());
                }
                if ui
                    .add_sized([btn[0] * 0.7, btn[1]], egui::Button::new("✕"))
                    .clicked()
                {
                    *rename = None;
                }
                return;
            }
            if ui
                .add_sized(btn, egui::Button::new(if row.visible { "👁" } else { "🚫" }))
                .on_hover_text("Visibility")
                .clicked()
            {
                action = LayerAction::ToggleVisible(row.index);
            }
            if ui
                .add_sized(btn, egui::Button::new(if row.locked { "🔒" } else { "🔓" }))
                .on_hover_text("Lock")
                .clicked()
            {
                action = LayerAction::ToggleLocked(row.index);
            }
            if ui
                .add_sized(btn, egui::Button::new("✎"))
                .on_hover_text("Rename")
                .clicked()
            {
                *rename = Some((row.index, row.name.clone()));
            }
            if ui
                .add_sized(btn, egui::Button::new("▲"))
                .on_hover_text("Move up")
                .clicked()
            {
                action = LayerAction::MoveUp(row.index);
            }
            if ui
                .add_sized(btn, egui::Button::new("▼"))
                .on_hover_text("Move down")
                .clicked()
            {
                action = LayerAction::MoveDown(row.index);
            }
            if can_delete
                && ui
                    .add_sized(btn, egui::Button::new("🗑"))
                    .on_hover_text("Delete")
                    .clicked()
            {
                action = LayerAction::Delete(row.index);
            }
        });
    }
    ui.add_space(metrics.section_spacing);
    if ui.button("＋ Add layer").clicked() {
        action = LayerAction::Add;
    }
    // A rename left open on another row is cancelled when any action fires.
    if !matches!(action, LayerAction::None) {
        *rename = None;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn rows_of(doc_layers: usize) -> Vec<LayerRow> {
        let mut project = Document::new_empty_project();
        while project.document.layers.len() < doc_layers {
            let id = uuid::Uuid::new_v4();
            project
                .document
                .layers
                .push(crate::document::Layer::new_image(
                    id,
                    format!("L{}", project.document.layers.len()),
                    true,
                    false,
                    vec![],
                ));
        }
        layer_rows(&project)
    }

    #[test]
    fn rows_list_top_first_with_active_flag() {
        let rows = rows_of(3);
        assert_eq!(rows.len(), 3);
        // Top-first: last document layer renders first.
        assert_eq!(rows[0].index, 2);
        assert_eq!(rows[2].index, 0);
        // Fresh project: layer 0 active.
        assert!(rows[2].active);
        assert!(!rows[0].active);
    }

    #[test]
    fn layer_kind_survives_snapshot() {
        let project = Document::new_empty_project();
        let rows = layer_rows(&project);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, LayerKind::Image);
        assert!(rows[0].visible);
        assert!(!rows[0].locked);
    }
}
