//! Mobile top bar: back/title on the left, undo/redo + export + overflow.
//!
//! Takes the app for actions (title, undo/redo, export trigger) — the same
//! calls the desktop chrome makes. A narrower `EditorContext` replaces this
//! parameter when §25 lands; until then the calls stay shared commands.

use crate::platform::UiLayout;

/// Intent from the top bar; the shell applies it to shared commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopBarAction {
    #[default]
    None,
    OpenMenu,
    Undo,
    Redo,
}

/// Render the phone top bar: [Menu] [title] [Undo] [Redo].
/// Pure presentation: reads title + undo/redo availability, returns what
/// the user asked for. Export lives in the Export sheet, not the bar.
pub fn show_top_bar(
    title: &str,
    can_undo: bool,
    can_redo: bool,
    layout: UiLayout,
    ui: &mut egui::Ui,
) -> TopBarAction {
    let m = layout.metrics;
    let mut action = TopBarAction::None;
    ui.horizontal(|ui| {
        ui.add_space(layout.safe_area.left);
        if ui
            .add_sized(
                [m.toolbar_button_size, m.toolbar_button_size],
                egui::Button::new("‹"),
            )
            .on_hover_text("Menu")
            .clicked()
        {
            action = TopBarAction::OpenMenu;
        }
        ui.label(egui::RichText::new(title).strong().size(17.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(layout.safe_area.right);
            if ui
                .add_enabled(can_redo, egui::Button::new("↷"))
                .on_hover_text("Redo")
                .clicked()
            {
                action = TopBarAction::Redo;
            }
            if ui
                .add_enabled(can_undo, egui::Button::new("↶"))
                .on_hover_text("Undo")
                .clicked()
            {
                action = TopBarAction::Undo;
            }
        });
    });
    action
}
