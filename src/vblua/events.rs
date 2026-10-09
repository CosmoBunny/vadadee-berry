//! Document scripting events (Phase 28 core) + addon hot-reload (Phase 27 lite).
//!
//! Lua shape:
//! ```lua
//! vblua.events.on("node-created", function(ids) vblua.log("new: " .. #ids) end)
//! vblua.events.on("error", function(msg) vblua.log("broke: " .. msg) end)
//! vblua.addons.reload("example.glow")  -- re-read + re-run addon.lua
//! ```
//!
//! Rules (spec: no infinite event loops):
//! - The runtime fires events from the outer batch ONLY: `node-created`,
//!   `node-deleted` (from the node report), `addon-enabled`, `error`,
//!   `after_run`. Callbacks may queue commands (applied in a second drain)
//!   but never fire further events — one level, no recursion.
//! - App-level events (`document-open`, `before-export`, `selection-change`)
//!   need app dispatch points and are NOT faked here; the bus accepts them
//!   from future host code via `queue_event`.

use std::collections::HashMap;

/// Runtime-fired events (app dispatch points add more later).
pub const KNOWN_EVENTS: &[&str] = &[
    "after_run",
    "node-created",
    "node-deleted",
    "addon-enabled",
    "error",
    "document-open",
    "before-export",
];

/// One fired event (collected per batch, dispatched once).
#[derive(Debug, Clone)]
pub enum FiredEvent {
    AfterRun,
    NodesCreated(Vec<String>),
    NodesDeleted(Vec<String>),
    AddonEnabled(String),
    ScriptError(String),
    /// Host lifecycle hooks fired by the application (open/export).
    App(String),
}

impl FiredEvent {
    pub fn name(&self) -> String {
        match self {
            FiredEvent::AfterRun => "after_run".to_string(),
            FiredEvent::NodesCreated(_) => "node-created".to_string(),
            FiredEvent::NodesDeleted(_) => "node-deleted".to_string(),
            FiredEvent::AddonEnabled(_) => "addon-enabled".to_string(),
            FiredEvent::ScriptError(_) => "error".to_string(),
            FiredEvent::App(name) => name.clone(),
        }
    }
}

/// Subscription registry (host state, persists across runs).
#[derive(Debug, Default)]
pub struct EventBus {
    pub subscribers: HashMap<String, Vec<mlua::Function>>,
}

impl EventBus {
    pub fn subscribe(
        &mut self,
        event: &str,
        func: mlua::Function,
    ) -> Result<(), String> {
        if !KNOWN_EVENTS.contains(&event) {
            return Err(format!(
                "unknown event '{event}' — known: {}",
                KNOWN_EVENTS.join(", ")
            ));
        }
        let list = self.subscribers.entry(event.to_string()).or_default();
        if list.len() >= 16 {
            return Err(format!("too many subscribers for '{event}' (max 16)"));
        }
        list.push(func);
        Ok(())
    }

    pub fn unsubscribe(&mut self, event: &str) -> bool {
        self.subscribers.remove(event).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_events() {
        let lua = mlua::Lua::new();
        let mut bus = EventBus::default();
        let f: mlua::Function = lua.load("return function() end").eval().unwrap();
        assert!(bus.subscribe("nope", f).is_err());
        let f: mlua::Function = lua.load("return function() end").eval().unwrap();
        assert!(bus.subscribe("after_run", f).is_ok());
        assert!(bus.unsubscribe("after_run"));
        assert!(!bus.unsubscribe("after_run"));
    }
}
