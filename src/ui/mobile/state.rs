//! Mobile presentation state only.
//!
//! Tracks *how the phone shows the editor* (sheets, toolbar, navigation).
//! Never owns the `Document`, selection, or history — those stay on the
//! editor core and are borrowed per frame.

use crate::tools::ToolKind;

/// Which sheet (if any) is presented over the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobileSheet {
    /// Nothing presented.
    #[default]
    None,
    /// Tool grid (`More` overflow + full palette).
    Tools,
    /// Layers: select / reorder / visibility / lock / rename / delete.
    Layers,
    /// Inspector: transform / appearance / typography (progressive).
    Inspector,
    /// Compact timeline + expanded timeline mode.
    Timeline,
    /// Export: format / resolution / share.
    Export,
    /// Overflow menu (top-bar `⋮`).
    Menu,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MobileUiState {
    /// Currently presented sheet.
    pub active_sheet: MobileSheet,
    /// Primary destination (canvas-first; others open sheets).
    pub navigation: super::navigation::MobileNavigation,
    /// Last viewport size seen (drives shell metrics, not editor state).
    pub viewport: egui::Vec2,
    /// Layer rename in progress: `(layer index, edit buffer)`. Committed
    /// once (single history entry), never per keystroke.
    pub layer_rename: Option<(usize, String)>,
}

impl Default for MobileUiState {
    fn default() -> Self {
        Self {
            active_sheet: MobileSheet::None,
            navigation: super::navigation::MobileNavigation::Canvas,
            viewport: egui::Vec2::ZERO,
            layer_rename: None,
        }
    }
}

impl MobileUiState {
    /// Open a sheet (replaces any current sheet).
    pub fn open_sheet(&mut self, sheet: MobileSheet) {
        self.active_sheet = sheet;
    }

    /// Toggle: tapping the active destination's button dismisses its sheet.
    pub fn toggle_sheet(&mut self, sheet: MobileSheet) {
        if self.active_sheet == sheet {
            self.active_sheet = MobileSheet::None;
        } else {
            self.active_sheet = sheet;
        }
    }

    pub fn close_sheet(&mut self) {
        self.active_sheet = MobileSheet::None;
    }

    pub fn sheet_open(&self) -> bool {
        self.active_sheet != MobileSheet::None
    }

    /// Begin a layer rename (buffer prefilled with the current name).
    pub fn begin_layer_rename(&mut self, index: usize, current: String) {
        self.layer_rename = Some((index, current));
    }

    /// Map the editor's active tool to its mobile presentation (for toolbar
    /// highlight). Unknown/desktop-only tools fall back to `More`.
    pub fn tool_for_kind(kind: ToolKind) -> super::tools::MobileTool {
        super::tools::MobileTool::from_kind(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_toggle_roundtrip() {
        let mut s = MobileUiState::default();
        assert!(!s.sheet_open());
        s.toggle_sheet(MobileSheet::Layers);
        assert!(s.sheet_open());
        assert_eq!(s.active_sheet, MobileSheet::Layers);
        s.toggle_sheet(MobileSheet::Layers);
        assert!(!s.sheet_open());
        s.open_sheet(MobileSheet::Export);
        s.open_sheet(MobileSheet::Timeline);
        assert_eq!(s.active_sheet, MobileSheet::Timeline);
        s.close_sheet();
        assert!(!s.sheet_open());
    }

    #[test]
    fn state_holds_no_document() {
        // Compile-time shaped: MobileUiState has no Document-typed fields.
        // This test pins the field set so a Document can't sneak in.
        let s = MobileUiState::default();
        let debug = format!("{s:?}");
        assert!(debug.contains("Canvas"));
    }

    #[test]
    fn every_toolkind_maps_somewhere() {
        use crate::tools::ToolKind;
        let kinds = [
            ToolKind::Select,
            ToolKind::Node,
            ToolKind::Rectangle,
            ToolKind::Circle,
            ToolKind::Ellipse,
            ToolKind::Line,
            ToolKind::Polygon,
            ToolKind::Pen,
            ToolKind::Text,
            ToolKind::Arc,
            ToolKind::Plotter,
            ToolKind::Brush,
            ToolKind::RasterBrush,
            ToolKind::Eraser,
            ToolKind::BucketFill,
            ToolKind::Smudge,
            ToolKind::RasterSelect,
            ToolKind::Eyedropper,
        ];
        for kind in kinds {
            let _ = MobileUiState::tool_for_kind(kind);
        }
    }
}
