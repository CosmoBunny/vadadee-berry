//! Application command layer (P0 refactor scaffold).
//!
//! Long-term direction (per architecture review): UI, MCP, collaboration and
//! keyboard input should all funnel through [`EditorCommand`]s handled by
//! [`CommandDispatcher`], instead of reaching directly into
//! `VadadeeBerryApp` and mutating document / cache / render state by hand.
//!
//! ```text
//! Mouse ─────┐
//! Keyboard ──┼──▶ EditorCommand ─▶ CommandDispatcher ─▶ Document (+ History)
//! MCP ───────┘                              │
//!                                           ▼
//!                                    DocumentChanged
//! ```
//!
//! This module is intentionally **additive**: it wraps the existing
//! [`crate::history::History`]/[`crate::history::ProjectEdit`] machinery
//! without changing its behavior. Call sites migrate one at a time.

use crate::document::ProjectFile;
use crate::history::{History, ProjectEdit};

/// Coarse dirty flags describing what a command touched.
///
/// Consumers (renderer, caches, UI sync, collaboration) will eventually
/// subscribe to these instead of `app.rs` manually remembering every
/// invalidation (review §11). For now the dispatcher computes them and
/// returns them to the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentChanged {
    /// Node set / document structure changed (insert/remove/reorder).
    pub document: bool,
    /// Node geometry changed.
    pub geometry: bool,
    /// Style / paint changed.
    pub style: bool,
    /// Animation timelines / keyframes changed.
    pub animation: bool,
    /// Layer list / active layer changed.
    pub layers: bool,
    /// GPU-resident state (textures, pipelines) may need refresh.
    pub gpu: bool,
    /// UI panels (inspector, layers, timeline) should re-sync.
    pub ui: bool,
}

impl DocumentChanged {
    pub fn structure() -> Self {
        Self {
            document: true,
            geometry: true,
            gpu: true,
            ui: true,
            ..Default::default()
        }
    }

    pub fn nodes() -> Self {
        Self {
            geometry: true,
            style: true,
            gpu: true,
            ui: true,
            ..Default::default()
        }
    }

    pub fn timeline() -> Self {
        Self {
            animation: true,
            ui: true,
            ..Default::default()
        }
    }

    pub fn everything() -> Self {
        Self {
            document: true,
            geometry: true,
            style: true,
            animation: true,
            layers: true,
            gpu: true,
            ui: true,
        }
    }
}

/// Map a recorded edit to the change flags it implies.
pub fn changes_for_edit(edit: &ProjectEdit) -> DocumentChanged {
    match edit {
        ProjectEdit::InsertNode { .. }
        | ProjectEdit::InsertNodes { .. }
        | ProjectEdit::InsertNodesApplied { .. }
        | ProjectEdit::RemoveNodes { .. }
        | ProjectEdit::ReorderNodes { .. } => DocumentChanged::structure(),
        ProjectEdit::PatchNode { .. } | ProjectEdit::PatchNodes { .. } => DocumentChanged::nodes(),
        ProjectEdit::PatchTimeline { .. } => DocumentChanged::timeline(),
        ProjectEdit::PatchDocument { .. } | ProjectEdit::SetDocument { .. } => {
            DocumentChanged::everything()
        }
    }
}

/// Narrow context a command is allowed to touch.
///
/// Deliberately much smaller than `&mut VadadeeBerryApp`: commands receive
/// the project plus history only. UI/render/cache reactions happen via the
/// returned [`DocumentChanged`], handled by the caller (later: subscribers).
pub struct CommandContext<'a> {
    pub project: &'a mut ProjectFile,
    pub history: &'a mut History,
}

/// All mutations of project state, addressable by UI, MCP, collab and tests.
///
/// Currently every variant funnels into the existing undoable
/// [`ProjectEdit`] machinery. Non-undoable UI intents (selection, playback)
/// will gain variants here in a follow-up so they stop bypassing the layer.
#[derive(Debug)]
pub enum EditorCommand {
    /// Undoable document edit, recorded in [`History`].
    Edit(ProjectEdit),
    /// Undoable edit whose forward apply is already reflected in the project.
    EditApplied(ProjectEdit),
    /// Undo the last edit. Returns true when something was undone.
    Undo,
    /// Redo the last undone edit. Returns true when something was redone.
    Redo,
}

/// Routes [`EditorCommand`]s through [`History`] and reports what changed.
///
/// Stateless: all state lives in the [`CommandContext`]. One shared choke
/// point where UI / MCP / collaboration / keyboard input converge, so future
/// subscribers (render invalidation, collab op broadcast, metrics) hook in
/// once instead of at every call site.
pub struct CommandDispatcher;

impl CommandDispatcher {
    /// Dispatch a command. Returns `(did_something, changes)`.
    pub fn dispatch(ctx: CommandContext<'_>, cmd: EditorCommand) -> (bool, DocumentChanged) {
        match cmd {
            EditorCommand::Edit(edit) => {
                let changes = changes_for_edit(&edit);
                ctx.history.push(ctx.project, edit);
                (true, changes)
            }
            EditorCommand::EditApplied(edit) => {
                let changes = changes_for_edit(&edit);
                ctx.history.push_applied(ctx.project, edit);
                (true, changes)
            }
            EditorCommand::Undo => {
                let did = ctx.history.undo(ctx.project);
                (did, DocumentChanged::everything())
            }
            EditorCommand::Redo => {
                let did = ctx.history.redo(ctx.project);
                (did, DocumentChanged::everything())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::history::snapshot_document;

    fn test_ctx() -> (ProjectFile, History) {
        (Document::new_empty_project(), History::default())
    }

    #[test]
    fn dispatch_edit_records_undoable_change() {
        let (mut project, mut history) = test_ctx();
        let before = snapshot_document(&project.document);
        project.document.add_layer("L2");
        let after = snapshot_document(&project.document);
        let (did, changes) = CommandDispatcher::dispatch(
            CommandContext {
                project: &mut project,
                history: &mut history,
            },
            EditorCommand::Edit(ProjectEdit::PatchDocument { before, after }),
        );
        assert!(did);
        assert!(changes.document && changes.ui);
        assert!(history.can_undo());
    }

    #[test]
    fn changes_for_edit_maps_variants_to_flags() {
        let doc = snapshot_document(&Document::new_empty_project().document);
        let node_patch = ProjectEdit::PatchDocument {
            before: doc.clone(),
            after: doc.clone(),
        };
        assert!(changes_for_edit(&node_patch).document);
        let tl = ProjectEdit::PatchTimeline {
            before: crate::document::AnimationTimeline::default(),
            after: crate::document::AnimationTimeline::default(),
        };
        let c = changes_for_edit(&tl);
        assert!(c.animation && c.ui && !c.document);
    }

    #[test]
    fn dispatch_undo_redo_roundtrip() {
        let (mut project, mut history) = test_ctx();
        let before = snapshot_document(&project.document);
        project.document.add_layer("L2");
        let after = snapshot_document(&project.document);
        let dispatch = |project: &mut ProjectFile,
                        history: &mut History,
                        cmd: EditorCommand| {
            CommandDispatcher::dispatch(
                CommandContext { project, history },
                cmd,
            )
        };
        dispatch(
            &mut project,
            &mut history,
            EditorCommand::Edit(ProjectEdit::PatchDocument {
                before: before.clone(),
                after: after.clone(),
            }),
        );
        let (undid, _) = dispatch(&mut project, &mut history, EditorCommand::Undo);
        assert!(undid);
        assert_eq!(project.document.layers.len(), before.layers.len());
        let (redid, _) = dispatch(&mut project, &mut history, EditorCommand::Redo);
        assert!(redid);
        assert_eq!(project.document.layers.len(), after.layers.len());
    }
}
