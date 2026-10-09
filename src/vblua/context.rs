//! Script-visible document handle (Phase 4).
//!
//! Lua never owns the [`Document`]. Before each `execute` the runtime copies a
//! small [`DocumentSnapshot`] into the host; Lua reads from the snapshot and
//! stages mutations as [`DocumentCommand`]s. After the script returns, Rust
//! applies the queue in one batch (one undo transaction — Phase 16 groundwork).
//! IDs cross as strings; Lua never sees pointers.

use crate::document::{Document, LayerKind};

/// Read-only document view handed to Lua as a table.
#[derive(Debug, Clone, Default)]
pub struct DocumentSnapshot {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub layers: Vec<LayerInfo>,
    /// Active layer id (import targets prefer it).
    pub active_layer_id: Option<String>,
}

/// Read-only layer row.
#[derive(Debug, Clone)]
pub struct LayerInfo {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub kind: String,
    pub node_count: usize,
}

impl DocumentSnapshot {
    pub fn capture(doc: &Document) -> Self {
        Self {
            title: doc.title.clone(),
            width: doc.width,
            height: doc.height,
            layers: doc
                .layers
                .iter()
                .map(|l| LayerInfo {
                    id: l.id.to_string(),
                    name: l.name.clone(),
                    visible: l.visible,
                    kind: match l.kind {
                        LayerKind::Image => "image",
                        LayerKind::AV => "av",
                        LayerKind::Shading => "shading",
                        LayerKind::Flowchart => "flowchart",
                        LayerKind::NodeEditor => "node_editor",
                        LayerKind::ScreenRecord => "screen_record",
                    }
                    .to_string(),
                    node_count: l.nodes.len(),
                })
                .collect(),
            active_layer_id: doc
                .layers
                .get(doc.active_layer_index)
                .map(|l| l.id.to_string()),
        }
    }

    /// Push this snapshot as a Lua table (`document.current()` shape).
    pub fn into_lua_table(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        let t = lua.create_table()?;
        t.set("name", self.title.clone())?;
        t.set("title", self.title.clone())?;
        t.set("width", self.width)?;
        t.set("height", self.height)?;
        let layers = lua.create_table()?;
        for (i, l) in self.layers.iter().enumerate() {
            let lt = lua.create_table()?;
            lt.set("id", l.id.clone())?;
            lt.set("name", l.name.clone())?;
            lt.set("visible", l.visible)?;
            lt.set("kind", l.kind.clone())?;
            lt.set("node_count", l.node_count as i64)?;
            layers.set(i + 1, lt)?;
        }
        t.set("layers", layers)?;
        t.set("layer_count", self.layers.len() as i64)?;
        Ok(t)
    }
}

/// One staged document mutation. Applied by Rust after the script returns.
#[derive(Debug, Clone)]
pub enum DocumentCommand {
    Rename { title: String },
    Resize { width: f64, height: f64 },
    SetLayerVisible { layer_id: String, visible: bool },
    CreateLayer {
        layer_id: uuid::Uuid,
        name: String,
        kind: LayerKind,
    },
}

/// Parse a script layer `type` into `(LayerKind, canonical name)`.
/// Canonical names match the snapshot `kind` strings, so idempotency
/// checks compare like with like.
pub fn parse_layer_kind(s: &str) -> Option<(LayerKind, &'static str)> {
    match s.trim().to_lowercase().as_str() {
        "image" | "images" => Some((LayerKind::Image, "image")),
        "av" | "video" | "audio" | "media" => Some((LayerKind::AV, "av")),
        "shading" | "shader" => Some((LayerKind::Shading, "shading")),
        "flowchart" | "flow" | "diagram" => Some((LayerKind::Flowchart, "flowchart")),
        "node_editor" | "nodeeditor" | "nodes" | "graph" => {
            Some((LayerKind::NodeEditor, "node_editor"))
        }
        "screen_record" | "screenrecord" | "record" | "capture" => {
            Some((LayerKind::ScreenRecord, "screen_record"))
        }
        _ => None,
    }
}

impl DocumentCommand {
    /// Apply one command. Returns `true` if the document changed.
    /// File I/O (`open`/`save`) deliberately has no command: it must go
    /// through the platform file abstraction (Phase 12), not Lua strings.
    pub fn apply_to(&self, doc: &mut Document) -> bool {
        match self {
            DocumentCommand::Rename { title } => {
                if title.is_empty() || title.len() > 256 {
                    return false;
                }
                doc.title = title.clone();
                true
            }
            DocumentCommand::Resize { width, height } => {
                let (w, h) = (width.max(1.0).min(16384.0), height.max(1.0).min(16384.0));
                if (doc.width - w).abs() < f64::EPSILON && (doc.height - h).abs() < f64::EPSILON {
                    return false;
                }
                doc.width = w;
                doc.height = h;
                true
            }
            DocumentCommand::SetLayerVisible { layer_id, visible } => {
                let Ok(id) = layer_id.parse::<uuid::Uuid>() else {
                    return false;
                };
                let Some(l) = doc.layers.iter_mut().find(|l| l.id == id) else {
                    return false;
                };
                l.visible = *visible;
                true
            }
            DocumentCommand::CreateLayer { layer_id, name, kind } => {
                if doc.layers.iter().any(|l| l.id == *layer_id) {
                    return false;
                }
                let name: String = name.chars().take(128).collect();
                if name.is_empty() {
                    return false;
                }
                doc.layers.push(match kind {
                    LayerKind::Image => {
                        crate::document::Layer::new_image(*layer_id, name, true, false, vec![])
                    }
                    LayerKind::AV => {
                        crate::document::Layer::new_empty_av_layer(*layer_id, name)
                    }
                    LayerKind::Shading => {
                        crate::document::Layer::new_shading_layer(*layer_id, name)
                    }
                    LayerKind::Flowchart => {
                        crate::document::Layer::new_flowchart_layer(*layer_id, name)
                    }
                    LayerKind::NodeEditor => {
                        crate::document::Layer::new_node_editor_layer(*layer_id, name)
                    }
                    LayerKind::ScreenRecord => {
                        crate::document::Layer::new_screen_record_layer(*layer_id, name)
                    }
                });
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_doc() -> Document {
        Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        }
    }

    #[test]
    fn rename_and_resize_apply() {
        let mut d = test_doc();
        assert!(DocumentCommand::Rename { title: "n".into() }.apply_to(&mut d));
        assert_eq!(d.title, "n");
        assert!(DocumentCommand::Resize { width: 50.0, height: 60.0 }.apply_to(&mut d));
        assert_eq!((d.width, d.height), (50.0, 60.0));
    }

    #[test]
    fn create_layer_apply_is_idempotent_by_id() {
        let mut d = test_doc();
        let id = uuid::Uuid::new_v4();
        assert!(DocumentCommand::CreateLayer {
            layer_id: id,
            name: "Images".into(),
            kind: LayerKind::Image,
        }
        .apply_to(&mut d));
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].name, "Images");
        // Same id twice: second apply is a no-op (same-run double create).
        assert!(!DocumentCommand::CreateLayer {
            layer_id: id,
            name: "Images".into(),
            kind: LayerKind::Image,
        }
        .apply_to(&mut d));
        assert_eq!(d.layers.len(), 1);
        // Empty name refused.
        assert!(!DocumentCommand::CreateLayer {
            layer_id: uuid::Uuid::new_v4(),
            name: "".into(),
            kind: LayerKind::AV,
        }
        .apply_to(&mut d));
    }

    #[test]
    fn parse_layer_kind_aliases() {
        assert_eq!(
            parse_layer_kind("VIDEO"),
            Some((LayerKind::AV, "av"))
        );
        assert_eq!(
            parse_layer_kind("graph"),
            Some((LayerKind::NodeEditor, "node_editor"))
        );
        assert_eq!(parse_layer_kind("nope"), None);
    }

    #[test]
    fn bad_commands_are_noops() {
        let mut d = test_doc();
        assert!(!DocumentCommand::Rename { title: String::new() }.apply_to(&mut d));
        assert!(!DocumentCommand::SetLayerVisible {
            layer_id: "not-a-uuid".into(),
            visible: false
        }
        .apply_to(&mut d));
    }
}
