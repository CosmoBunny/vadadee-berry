//! Phone/tablet shell: top bar, canvas, bottom toolbar, sheets.
//!
//! Layout model (§5):
//!
//! ```text
//! ┌──────────────────────────────┐
//! │ ←  Project       ⋮   Export │  Top bar
//! ├──────────────────────────────┤
//! │                              │
//! │            CANVAS            │  existing canvas_ui, unmodified
//! │                              │
//! ├──────────────────────────────┤
//! │ Select Move Text Brush …  +  │  Bottom toolbar
//! ├──────────────────────────────┤
//! │       optional timeline      │  later phase
//! └──────────────────────────────┘
//! ```
//!
//! No desktop dock, no action strip, no desktop dialogs. Sheets host all
//! secondary UI through the single [`MobileBottomSheet`] chrome.
//!
//! Chrome functions are pure presentation returning intents; this shell is
//! the only place that turns intents into shared commands on the app.

use crate::app::VadadeeBerryApp;
use crate::platform::{SafeAreaInsets, UiDeviceClass, UiLayout, classify_device};
use crate::ui::mobile::bottom_sheet::{MobileBottomSheet, SheetDetent};
use crate::ui::mobile::bottom_toolbar::{ToolbarAction, show_bottom_toolbar};
use crate::ui::mobile::state::MobileSheet;
use crate::ui::mobile::top_bar::{TopBarAction, show_top_bar};

pub struct MobileShell;

impl MobileShell {
    /// Render the whole phone/tablet frame. `Desktop` viewports never reach
    /// here (dispatch keeps them on desktop chrome).
    pub fn show(app: &mut VadadeeBerryApp, ui: &mut egui::Ui) {
        let viewport = ui.ctx().content_rect().size();
        let device = classify_device(viewport);
        debug_assert_ne!(
            device,
            UiDeviceClass::Desktop,
            "mobile shell got a desktop viewport"
        );
        let insets = SafeAreaInsets::from_context(ui.ctx());
        let layout = UiLayout::resolve(device, insets, viewport);
        app.mobile_ui.viewport = viewport;

        // Top bar (reads title + undo state, then acts).
        let title = app.project.document.title.clone();
        let can_undo = app.history.can_undo();
        let can_redo = app.history.can_redo();
        let top_action = egui::Panel::top("mobile_top_bar")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.add_space(insets.top);
                show_top_bar(&title, can_undo, can_redo, layout, ui)
            })
            .inner;
        match top_action {
            TopBarAction::None => {}
            TopBarAction::OpenMenu => app.mobile_ui.toggle_sheet(MobileSheet::Menu),
            TopBarAction::Undo => app.do_undo(),
            TopBarAction::Redo => app.do_redo(),
            TopBarAction::Export => app.request_export_image(),
        }

        // Bottom toolbar.
        let active_tool = app.tools.active;
        let toolbar_action = egui::Panel::bottom("mobile_bottom_toolbar")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                let action = show_bottom_toolbar(active_tool, layout, ui);
                ui.add_space(insets.bottom);
                action
            })
            .inner;
        match toolbar_action {
            ToolbarAction::None => {}
            ToolbarAction::SetTool(kind) => {
                app.tools.active = kind;
                app.mobile_ui.close_sheet();
            }
            ToolbarAction::OpenTools => app.mobile_ui.toggle_sheet(MobileSheet::Tools),
        }

        // Canvas fills the middle (existing implementation, unmodified).
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let _ = app.canvas_ui(ui);
        });

        // Active sheet on top.
        let ctx = ui.ctx().clone();
        Self::show_sheet(app, &ctx, layout);
    }

    fn show_sheet(app: &mut VadadeeBerryApp, ctx: &egui::Context, layout: UiLayout) {
        let sheet = app.mobile_ui.active_sheet;
        let title = match sheet {
            MobileSheet::None => return,
            MobileSheet::Tools => "Tools",
            MobileSheet::Layers => "Layers",
            MobileSheet::Inspector => "Properties",
            MobileSheet::Timeline => "Timeline",
            MobileSheet::Export => "Export",
            MobileSheet::Menu => "Menu",
        };
        let next = MobileBottomSheet::new(title, SheetDetent::Peek).show(ctx, layout, |ui| {
            Self::sheet_content(app, sheet, layout, ui)
        });
        if next == SheetDetent::Closed {
            app.mobile_ui.close_sheet();
        }
    }

    /// Per-sheet content (tool palette live; layers/inspector/text/export/
    /// timeline fill in through Phases 4–7).
    fn sheet_content(
        app: &mut VadadeeBerryApp,
        sheet: MobileSheet,
        layout: crate::platform::UiLayout,
        ui: &mut egui::Ui,
    ) {
        match sheet {
            MobileSheet::None => {}
            MobileSheet::Tools => {
                if super::tool_palette::show_tool_palette(app, layout.metrics, ui) {
                    app.mobile_ui.close_sheet();
                }
            }
            MobileSheet::Layers => {
                use super::layers::{LayerAction, layer_rows, show_layer_sheet};
                let rows = layer_rows(&app.project);
                let can_delete = rows.len() > 1;
                let action = show_layer_sheet(
                    &rows,
                    can_delete,
                    &mut app.mobile_ui.layer_rename,
                    layout.metrics,
                    ui,
                );
                // Borrow ends before applying: rows/action are owned.
                match action {
                    LayerAction::None => {}
                    LayerAction::SetActive(i) => app.set_active_layer(i),
                    LayerAction::ToggleVisible(i) => {
                        if let Some(row) = rows.iter().find(|r| r.index == i) {
                            app.set_layer_visible(i, !row.visible);
                        }
                    }
                    LayerAction::ToggleLocked(i) => {
                        if let Some(row) = rows.iter().find(|r| r.index == i) {
                            app.set_layer_locked(i, !row.locked);
                        }
                    }
                    LayerAction::CommitRename(i, name) => {
                        if !name.trim().is_empty() {
                            app.rename_layer(i, name);
                        }
                    }
                    LayerAction::Delete(i) => app.delete_layer(i),
                    LayerAction::MoveUp(i) => app.nudge_layer_order(i, 1),
                    LayerAction::MoveDown(i) => app.nudge_layer_order(i, -1),
                    LayerAction::Add => {
                        let n = app.project.document.layers.len() + 1;
                        app.add_layer(&format!("Layer {n}"));
                    }
                }
            }
            MobileSheet::Inspector => {
                ui.label("Mobile inspector arrives in Phase 5.");
            }
            MobileSheet::Timeline => {
                ui.label("Compact timeline arrives in Phase 6.");
            }
            MobileSheet::Export => {
                if ui.button("Export PNG").clicked() {
                    app.request_export_image();
                }
                ui.label("Mobile export sheet arrives in Phase 7.");
            }
            MobileSheet::Menu => {
                if ui.button("Undo").clicked() {
                    app.do_undo();
                }
                if ui.button("Redo").clicked() {
                    app.do_redo();
                }
            }
        }
    }
}
