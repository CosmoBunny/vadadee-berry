//! Mobile bottom toolbar: phone-priority tools + `More` overflow.
//!
//! Buttons drive the shared [`crate::tools::ToolKind`] — tapping never edits
//! anything itself. Touch targets come from [`crate::platform::UiMetrics`],
//! never from scaled-down desktop sizes.

use crate::platform::UiLayout;
use crate::tools::ToolKind;
use crate::ui::mobile::tools::MobileTool;

/// Intent from the bottom toolbar; the shell applies it to shared commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarAction {
    #[default]
    None,
    SetTool(ToolKind),
    OpenTools,
}

/// Render the bottom toolbar: 5 phone tools + `More` overflow. Highlights the
/// button matching the editor's active tool. Pure presentation.
pub fn show_bottom_toolbar(
    active_kind: ToolKind,
    layout: UiLayout,
    ui: &mut egui::Ui,
) -> ToolbarAction {
    let m = layout.metrics;
    let active = MobileTool::from_kind(active_kind);
    let mut action = ToolbarAction::None;
    ui.horizontal_centered(|ui| {
        ui.add_space(layout.safe_area.left);
        for tool in MobileTool::BAR {
            let selected = active == tool;
            let label = if selected {
                format!("● {}", tool.label())
            } else {
                tool.label().to_string()
            };
            if ui
                .add_sized(
                    [m.toolbar_button_size, m.toolbar_button_size],
                    egui::Button::selectable(selected, label),
                )
                .on_hover_text(tool.label())
                .clicked()
            {
                action = ToolbarAction::SetTool(tool.tool_kind());
            }
        }
        if ui
            .add_sized(
                [m.toolbar_button_size, m.toolbar_button_size],
                egui::Button::new("+"),
            )
            .on_hover_text("More tools")
            .clicked()
        {
            action = ToolbarAction::OpenTools;
        }
        ui.add_space(layout.safe_area.right);
    });
    action
}
