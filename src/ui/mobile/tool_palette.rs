//! Full tool palette grid for the `More` sheet.
//!
//! Every button maps onto an existing [`crate::tools::ToolKind`] —
//! including desktop/power tools, which surface here instead of being
//! duplicated as mobile-only tools.

use crate::app::VadadeeBerryApp;
use crate::platform::UiMetrics;
use crate::tools::ToolKind;
use crate::ui::mobile::tools::MobileTool;

/// All editor tools with phone-friendly labels, grouped phone-first.
const GROUPS: &[(&str, &[ToolKind])] = &[
    (
        "Draw & Select",
        &[
            ToolKind::Select,
            ToolKind::Node,
            ToolKind::Pen,
            ToolKind::Brush,
            ToolKind::RasterBrush,
            ToolKind::Text,
            ToolKind::Eyedropper,
        ],
    ),
    (
        "Shapes",
        &[
            ToolKind::Rectangle,
            ToolKind::Circle,
            ToolKind::Ellipse,
            ToolKind::Line,
            ToolKind::Polygon,
            ToolKind::Arc,
            ToolKind::Plotter,
        ],
    ),
    (
        "Paint",
        &[
            ToolKind::Eraser,
            ToolKind::BucketFill,
            ToolKind::Smudge,
            ToolKind::RasterSelect,
        ],
    ),
];

fn label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Select => "Select",
        ToolKind::Node => "Node",
        ToolKind::Rectangle => "Rect",
        ToolKind::Circle => "Circle",
        ToolKind::Ellipse => "Ellipse",
        ToolKind::Line => "Line",
        ToolKind::Polygon => "Polygon",
        ToolKind::Pen => "Pen",
        ToolKind::Text => "Text",
        ToolKind::Arc => "Arc",
        ToolKind::Plotter => "Plotter",
        ToolKind::Brush => "Brush",
        ToolKind::RasterBrush => "R.Brush",
        ToolKind::Eraser => "Eraser",
        ToolKind::BucketFill => "Fill",
        ToolKind::Smudge => "Smudge",
        ToolKind::RasterSelect => "R.Select",
        ToolKind::Eyedropper => "Picker",
    }
}

/// Render the grouped grid. Returns true when a tool was picked (caller
/// closes the sheet).
pub fn show_tool_palette(app: &mut VadadeeBerryApp, metrics: UiMetrics, ui: &mut egui::Ui) -> bool {
    let mut picked = false;
    for (group, kinds) in GROUPS {
        ui.label(egui::RichText::new(*group).small().strong());
        let cols = 4;
        egui::Grid::new(format!("tool-palette-{group}"))
            .num_columns(cols)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for (i, kind) in kinds.iter().enumerate() {
                    let selected = app.tools.active == *kind;
                    // Show which bar button (if any) owns this tool.
                    let bar = MobileTool::from_kind(*kind);
                    let text = if bar == MobileTool::More {
                        label(*kind).to_string()
                    } else {
                        format!("{} ·", label(*kind))
                    };
                    if ui
                        .add_sized(
                            [metrics.toolbar_button_size, metrics.toolbar_button_size],
                            egui::Button::selectable(selected, text),
                        )
                        .clicked()
                    {
                        app.tools.active = *kind;
                        picked = true;
                    }
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(metrics.section_spacing);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ToolKind appears exactly once across groups (no orphans, no
    /// duplicates) — the palette must cover the whole editor.
    #[test]
    fn palette_covers_all_tools_exactly_once() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut count = 0;
        for (_, kinds) in GROUPS {
            for kind in *kinds {
                count += 1;
                assert!(seen.insert(*kind), "duplicate tool: {kind:?}");
            }
        }
        assert_eq!(count, 18, "expected all 18 ToolKind variants, got {count}");
    }
}
