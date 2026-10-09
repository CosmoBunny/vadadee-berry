//! UI scripting (Phase 13): controlled Lua extension panels.
//!
//! Lua shape:
//! ```lua
//! vblua.ui.panel("tools", "My Tool")
//! vblua.ui.text("tools", "Pick an option")
//! vblua.ui.button("tools", "Generate", function()
//!   local n = vblua.graph.create("Blur")
//!   vblua.log("made " .. n)
//! end)
//! vblua.ui.slider("tools", "Amount", 0, 100, 50, function(v)
//!   vblua.log("amount " .. v)
//! end)
//! ```
//!
//! Rules (spec: Rust remains the host):
//! - Lua declares panels/widgets; the host renders them in a floating
//!   "VBLua Panels" window. No fullscreen takeover, no core UI replacement.
//! - Widget definitions apply immediately (host chrome, not document state —
//!   no undo). Callbacks run through the registry and may queue document
//!   commands, applied + undo-recorded like console runs.
//! - Requires the `ui` capability (default policy denies it).
//! - Budgets: 32 panels, 128 widgets per panel.

use std::collections::HashMap;

/// Host-consumed widget interaction (queued by the renderer, run by poll).
#[derive(Debug, Clone)]
pub enum UiEvent {
    Clicked(u64),
    NumChanged(u64, f64),
    BoolChanged(u64, bool),
}

pub const MAX_PANELS: usize = 32;
pub const MAX_WIDGETS_PER_PANEL: usize = 128;

/// Widget kind (retained descriptor; the host draws it every frame).
#[derive(Debug, Clone)]
pub enum UiWidgetKind {
    Text { content: String },
    Button { label: String },
    Slider { label: String, min: f64, max: f64 },
    Checkbox { label: String },
}

/// One retained widget. Live values live here (sliders/checkboxes).
#[derive(Debug, Clone)]
pub struct UiWidget {
    pub id: u64,
    pub kind: UiWidgetKind,
    pub num_value: f64,
    pub bool_value: bool,
    pub has_callback: bool,
}

/// One Lua-declared panel.
#[derive(Debug, Clone, Default)]
pub struct UiPanel {
    pub id: String,
    pub title: String,
    pub widgets: Vec<UiWidget>,
}

/// Panel registry (host state, persists across script runs).
#[derive(Debug, Clone, Default)]
pub struct UiRegistry {    pub panels: HashMap<String, UiPanel>,
    pub next_widget_id: u64,
}

impl UiRegistry {
    /// Get or create a panel (called by `vblua.ui.panel`).
    pub fn panel(&mut self, id: &str, title: &str) -> Result<(), String> {
        if id.is_empty() || id.len() > 64 {
            return Err("panel id must be 1..64 chars".to_string());
        }
        if title.len() > 128 {
            return Err("panel title must be <= 128 chars".to_string());
        }
        if !self.panels.contains_key(id) {
            if self.panels.len() >= MAX_PANELS {
                return Err(format!("too many panels (max {MAX_PANELS})"));
            }
            self.panels.insert(
                id.to_string(),
                UiPanel {
                    id: id.to_string(),
                    title: title.to_string(),
                    widgets: Vec::new(),
                },
            );
        } else if let Some(p) = self.panels.get_mut(id) {
            p.title = title.to_string();
        }
        Ok(())
    }

    pub fn drop_panel(&mut self, id: &str) -> bool {
        self.panels.remove(id).is_some()
    }

    pub fn clear_panel(&mut self, id: &str) -> Result<(), String> {
        match self.panels.get_mut(id) {
            Some(p) => {
                p.widgets.clear();
                Ok(())
            }
            None => Err("unknown panel id".to_string()),
        }
    }

    /// Push a widget; returns its id. Enforces the per-panel budget.
    pub fn push_widget(&mut self, panel_id: &str, kind: UiWidgetKind) -> Result<u64, String> {
        let panel = self
            .panels
            .get_mut(panel_id)
            .ok_or_else(|| "unknown panel id — call vblua.ui.panel first".to_string())?;
        if panel.widgets.len() >= MAX_WIDGETS_PER_PANEL {
            return Err(format!("too many widgets (max {MAX_WIDGETS_PER_PANEL})"));
        }
        self.next_widget_id += 1;
        let id = self.next_widget_id;
        let (num_value, bool_value) = match &kind {
            UiWidgetKind::Slider { min, max, .. } => ((min + max) / 2.0, false),
            UiWidgetKind::Checkbox { .. } => (0.0, false),
            _ => (0.0, false),
        };
        panel.widgets.push(UiWidget {
            id,
            kind,
            num_value,
            bool_value,
            has_callback: false,
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_and_widget_budgets() {
        let mut r = UiRegistry::default();
        assert!(r.panel("", "t").is_err());
        r.panel("p", "Title").unwrap();
        let id = r
            .push_widget("p", UiWidgetKind::Button { label: "Go".into() })
            .unwrap();
        assert_eq!(id, 1);
        assert!(r.push_widget("nope", UiWidgetKind::Text { content: "x".into() }).is_err());
        assert!(r.drop_panel("p"));
        assert!(!r.drop_panel("p"));
    }
}
