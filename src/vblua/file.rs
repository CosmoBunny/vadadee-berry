//! File handler + picker integration (Phase 12): the platform file abstraction.
//!
//! Lua shape:
//! ```lua
//! if vblua.file.status().picker then
//!   local img = vblua.file.pick({ filters = { "png", "jpg" } }) -- -> node id
//! end
//! ```
//!
//! Architecture (the whole point of this phase):
//! ```text
//! Lua pick() -> VBLua API -> PickerHook -> platform implementation
//!                                  ├─ desktop: rfd file dialog (wired in app)
//!                                  ├─ Android: SAF (host wires later)
//!                                  └─ iOS: document picker (host wires later)
//! ```
//! Lua never sees paths — the hook returns **bytes + display name**; the
//! runtime decodes, bounds, and inserts an Image node. Same script on every
//! platform; `status().picker == false` where unwired (mobile today).
//!
//! Guards: `filesystem.read` capability, 8 MiB byte cap, decodable-image
//! only (png/jpeg/bmp — the enabled `image` features), 4096px dimension cap,
//! target = active Image layer else first Image layer.

use std::sync::Arc;

use crate::document::{Document, LayerKind, Node, NodeStore};

/// Decodable image extensions (matches the enabled `image` crate features).
pub const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "bmp"];
/// Picker byte budget (decode bomb guard).
pub const MAX_PICK_BYTES: usize = 8 * 1024 * 1024;
/// Decoded dimension budget per side.
pub const MAX_PICK_DIM: u32 = 4096;

/// What the platform picker returned (bytes, never a path to Lua).
#[derive(Debug, Clone)]
pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Host-provided picker: platform dialog → bytes. Desktop wires rfd;
/// mobile wires SAF / document picker; tests wire a fake.
pub type PickerHook = Arc<dyn Fn(&PickFilter) -> Result<PickedFile, String> + Send + Sync>;

/// Script picker request (already validated extensions).
#[derive(Debug, Clone)]
pub struct PickFilter {
    pub filters: Vec<String>,
    pub title: String,
}

impl PickFilter {
    pub fn from_lua(filters: Vec<String>, title: Option<String>) -> Result<Self, String> {
        if filters.is_empty() {
            return Err("pick needs filters, e.g. { filters = { \"png\", \"jpg\" } }".to_string());
        }
        if filters.len() > 32 {
            return Err("at most 32 filters".to_string());
        }
        let mut clean = Vec::new();
        for f in filters {
            let f = f.trim().trim_start_matches('.').to_ascii_lowercase();
            if !IMAGE_FILTERS.contains(&f.as_str()) {
                return Err(format!(
                    "unsupported filter '{f}' — decodable here: {}",
                    IMAGE_FILTERS.join(", ")
                ));
            }
            if !clean.contains(&f) {
                clean.push(f);
            }
        }
        let title = title.unwrap_or_else(|| "Pick an image".to_string());
        Ok(Self {
            filters: clean,
            title: title.chars().take(128).collect(),
        })
    }
}

/// Decode picker bytes into (width, height), enforcing budgets.
pub fn decode_pick(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() > MAX_PICK_BYTES {
        return Err(format!("file exceeds {} bytes", MAX_PICK_BYTES));
    }
    if bytes.is_empty() {
        return Err("picker returned empty file".to_string());
    }
    let img = image::load_from_memory(bytes)
        .map_err(|_| "not a decodable image (png/jpeg/bmp only)".to_string())?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || w > MAX_PICK_DIM || h > MAX_PICK_DIM {
        return Err(format!("image dimensions out of range (max {MAX_PICK_DIM}px per side)"));
    }
    Ok((w, h))
}

/// One staged file mutation. The node id is preallocated so scripts can use
/// it in the same run (snapshot updated optimistically like graph creates).
#[derive(Debug, Clone)]
pub enum FileCommand {
    ImportImage {
        id: uuid::Uuid,
        layer_id: uuid::Uuid,
        name: String,
        width: f64,
        height: f64,
        bytes: Vec<u8>,
    },
}

impl FileCommand {
    /// Apply to the full project (needs the node store). Returns the created
    /// node on success so the host can record undo.
    pub fn apply_to_project(
        &self,
        doc: &mut Document,
        nodes: &mut NodeStore,
    ) -> Option<Node> {
        match self {
            FileCommand::ImportImage {
                id,
                layer_id,
                name,
                width,
                height,
                bytes,
            } => {
                if nodes.map.contains_key(id) {
                    return None;
                }
                let Some(layer) = doc.layers.iter_mut().find(|l| l.id == *layer_id) else {
                    return None;
                };
                if layer.kind != LayerKind::Image {
                    return None;
                }
                let mut node = Node::image(0.0, 0.0, *width, *height, bytes.clone());
                node.id = *id;
                node.name = name.clone();
                nodes.insert(node.clone());
                if !layer.nodes.contains(id) {
                    layer.nodes.push(*id);
                }
                Some(node)
            }
        }
    }
}

/// Resolve the import target: active Image layer, else first Image layer.
pub fn resolve_image_layer(doc: &Document) -> Result<uuid::Uuid, String> {
    if let Some(l) = doc.layers.get(doc.active_layer_index)
        && l.kind == LayerKind::Image
    {
        return Ok(l.id);
    }
    doc.layers
        .iter()
        .find(|l| l.kind == LayerKind::Image)
        .map(|l| l.id)
        .ok_or_else(|| "no image layer to import into".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_validation() {
        assert!(PickFilter::from_lua(vec![], None).is_err());
        assert!(PickFilter::from_lua(vec!["exe".into()], None).is_err());
        assert!(PickFilter::from_lua(vec![".PNG".into()], None).is_ok());
        assert!(PickFilter::from_lua(vec!["svg".into()], None).is_err());
    }

    #[test]
    fn decode_rejects_garbage_and_caps() {
        assert!(decode_pick(&[]).is_err());
        assert!(decode_pick(&[0u8; 16]).is_err());
        assert!(decode_pick(&vec![0u8; MAX_PICK_BYTES + 1]).is_err());
        // Minimal 1x1 PNG.
        let png = make_png(1, 1);
        assert_eq!(decode_pick(&png).unwrap(), (1, 1));
    }

    #[test]
    fn import_applies_to_project() {
        use crate::document::Layer;
        let mut doc = crate::document::Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![Layer::new_image(
                uuid::Uuid::new_v4(),
                "L".into(),
                true,
                false,
                vec![],
            )],
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
        };
        let mut nodes = NodeStore::default();
        let png = make_png(2, 3);
        let (w, h) = decode_pick(&png).unwrap();
        let id = uuid::Uuid::new_v4();
        let layer_id = resolve_image_layer(&doc).unwrap();
        let node = FileCommand::ImportImage {
            id,
            layer_id,
            name: "pick.png".into(),
            width: w as f64,
            height: h as f64,
            bytes: png,
        }
        .apply_to_project(&mut doc, &mut nodes)
        .unwrap();
        assert_eq!(node.id, id);
        assert!(doc.layers[0].nodes.contains(&id));
    }

    /// Encode a w×h test PNG (no fixture files).
    fn make_png(w: u32, h: u32) -> Vec<u8> {
        use image::ImageEncoder;
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut buf);
        enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }
}
