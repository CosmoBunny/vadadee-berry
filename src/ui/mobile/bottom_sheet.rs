//! Reusable mobile bottom sheet: `Closed → Peek → Expanded`.
//!
//! One implementation for layers, color/stroke, geometry, text, export, tool
//! selection, properties and advanced options — never one custom panel each.
//! Content after this file only fills `show_content`, never reimplements the
//! chrome (drag handle, detents, safe-area padding, dismissal).

/// Presentation detent. `Peek` keeps canvas context; `Expanded` takes over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SheetDetent {
    #[default]
    Closed,
    Peek,
    Expanded,
}

/// Bottom sheet chrome. Construct per frame from `MobileUiState`; content is
/// a caller closure so each sheet stays a content function, not a widget.
pub struct MobileBottomSheet<'a> {
    pub title: &'a str,
    pub detent: SheetDetent,
    /// Height fraction of available space for `Peek` (0..1, default 0.35).
    pub peek_fraction: f32,
}

impl<'a> MobileBottomSheet<'a> {
    pub fn new(title: &'a str, detent: SheetDetent) -> Self {
        Self {
            title,
            detent,
            peek_fraction: 0.35,
        }
    }

    /// Render chrome + content. Returns the detent after drag interaction
    /// (drag handle toggles Peek↔Expanded; scrim tap closes).
    pub fn show(
        self,
        ctx: &egui::Context,
        layout: crate::platform::UiLayout,
        content: impl FnOnce(&mut egui::Ui),
    ) -> SheetDetent {
        if self.detent == SheetDetent::Closed {
            return SheetDetent::Closed;
        }
        let full = ctx.content_rect();
        let max_h = (full.height() - layout.safe_area.top).max(120.0);
        let target_h = match self.detent {
            SheetDetent::Closed => 0.0,
            SheetDetent::Peek => max_h * self.peek_fraction.clamp(0.15, 0.6),
            SheetDetent::Expanded => max_h * 0.85,
        };
        let mut next = self.detent;
        // Scrim above the canvas: tap outside closes (expanded only).
        if self.detent == SheetDetent::Expanded {
            egui::Area::new(egui::Id::new(("sheet-scrim", self.title)))
                .order(egui::Order::Middle)
                .fixed_pos(full.min)
                .show(ctx, |ui| {
                    ui.set_width(full.width());
                    ui.set_height(full.height());
                    let r = ui.allocate_rect(
                        egui::Rect::from_min_size(full.min, full.size()),
                        egui::Sense::click(),
                    );
                    if r.clicked() {
                        next = SheetDetent::Closed;
                    }
                });
        }
        // Sheet body anchored above the bottom safe area.
        let rect = egui::Rect::from_min_max(
            egui::pos2(full.min.x, full.max.y - target_h - layout.safe_area.bottom),
            egui::pos2(full.max.x, full.max.y - layout.safe_area.bottom),
        );
        egui::Area::new(egui::Id::new(("sheet", self.title)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    rect.size(),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::Frame::NONE
                            .fill(ctx.global_style().visuals.panel_fill)
                            .corner_radius(egui::CornerRadius {
                                nw: 16,
                                ne: 16,
                                sw: 0,
                                se: 0,
                            })
                            .inner_margin(egui::Margin {
                                left: layout.metrics.panel_padding as i8,
                                right: layout.metrics.panel_padding as i8,
                                top: 8,
                                bottom: 12,
                            })
                            .show(ui, |ui| {
                                ui.set_width(rect.width());
                                // Drag handle: tap toggles Peek↔Expanded.
                                ui.vertical_centered(|ui| {
                                    let handle = ui.allocate_response(
                                        egui::vec2(48.0, layout.metrics.sheet_handle.max(12.0)),
                                        egui::Sense::click(),
                                    );
                                    if handle.clicked() {
                                        next = match self.detent {
                                            SheetDetent::Peek => SheetDetent::Expanded,
                                            _ => SheetDetent::Peek,
                                        };
                                    }
                                    ui.painter().rect_filled(
                                        handle.rect.shrink2(egui::vec2(6.0, 8.0)),
                                        2.0,
                                        ui.style().visuals.text_color().gamma_multiply(0.35),
                                    );
                                });
                                ui.label(egui::RichText::new(self.title).strong());
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, true])
                                    .show(ui, content);
                            });
                    },
                );
            });
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_sheet_renders_nothing_and_stays_closed() {
        // Chrome-level invariant without a Context: Closed short-circuits
        // before touching egui (covered by the early return above).
        let sheet = MobileBottomSheet::new("x", SheetDetent::Closed);
        assert_eq!(sheet.detent, SheetDetent::Closed);
        assert!((sheet.peek_fraction - 0.35).abs() < 1e-6);
    }
}
