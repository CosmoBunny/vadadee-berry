//! Asset API (Phase 11): read-only project asset inventory + opaque handles.
//!
//! Lua shape:
//! ```lua
//! local items = vblua.assets.list()        -- [{ref, kind, name, ...}]
//! local info = vblua.assets.info(ref)     -- detail table | nil
//! vblua.assets.list("image")               -- kind filter
//! ```
//!
//! Handles are opaque strings (`node:<uuid>`, `clip:<uuid>`) — resolvable
//! host-side, never filesystem paths. `media_path` and raw bytes never
//! cross into Lua (paths leak host layout; bytes are a decode/DoS vector).
//!
//! `import` is deliberately denied: new bytes enter only through the
//! platform picker abstraction (Phase 12). This phase is inventory +
//! metadata so scripts can reason about project assets (Phase 15).

use crate::document::{NodeKind, ProjectFile};

/// One inventoried asset (no paths, no bytes).
#[derive(Debug, Clone)]
pub struct AssetItem {
    /// Opaque handle (`node:<uuid>` / `clip:<uuid>`).
    pub ref_id: String,
    /// `image` | `audio` | `video`.
    pub kind: String,
    pub name: String,
    /// Image canvas size (images only).
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Embedded byte size (images only; clips report duration instead).
    pub bytes: Option<usize>,
    /// Cached source duration seconds (media only).
    pub duration: Option<f32>,
}

/// Project asset inventory snapshot.
#[derive(Debug, Clone, Default)]
pub struct AssetSnapshot {
    pub items: Vec<AssetItem>,
}

impl AssetSnapshot {
    pub fn capture(project: &ProjectFile) -> Self {
        let mut items = Vec::new();
        for (id, node) in &project.nodes.map {
            if let NodeKind::Image {
                width,
                height,
                bytes,
                ..
            } = &node.kind
            {
                items.push(AssetItem {
                    ref_id: format!("node:{id}"),
                    kind: "image".to_string(),
                    name: node.name.clone(),
                    width: Some(*width),
                    height: Some(*height),
                    bytes: Some(bytes.len()),
                    duration: None,
                });
            }
        }
        for layer in &project.document.layers {
            for clip in &layer.av_clips {
                if clip.media_path.is_empty() {
                    continue;
                }
                items.push(AssetItem {
                    ref_id: format!("clip:{}", clip.id),
                    kind: if clip.is_audio_only() {
                        "audio"
                    } else if clip.is_still_image() {
                        "image"
                    } else {
                        "video"
                    }
                    .to_string(),
                    name: clip.name.clone(),
                    width: None,
                    height: None,
                    bytes: None,
                    duration: clip.media_source_duration,
                });
            }
        }
        // Stable order for scripts (nodes first, then clips, by name).
        items.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
        Self { items }
    }

    pub fn kinds_present(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self.items.iter().map(|i| i.kind.clone()).collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AvClip, Fill, Layer, Node, NodeStore};

    fn project_with_assets() -> ProjectFile {
        let doc = crate::document::Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![Layer::new_av_layer(
                uuid::Uuid::new_v4(),
                "AV".into(),
                String::new(),
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
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let mut img = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
        img.name = "Photo".into();
        img.kind = NodeKind::Image {
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
            bytes: vec![0u8; 128],
            collab_asset_sha256: None,
        };
        project.nodes.insert(img);
        project.document.layers[0].av_clips.push(AvClip {
            id: uuid::Uuid::new_v4(),
            name: "Take".into(),
            media_path: "/secret/host/path/take.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: 20.0,
            video_timeline_start: 5.0,
            media_source_duration: Some(30.0),
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        });
        project
    }

    #[test]
    fn inventory_lists_but_never_leaks_paths_or_bytes() {
        let project = project_with_assets();
        let snap = AssetSnapshot::capture(&project);
        assert_eq!(snap.items.len(), 2);
        let flat = format!("{snap:?}");
        assert!(!flat.contains("/secret/host/path"), "{flat}");
        assert!(!flat.contains("take.mp4"), "{flat}");
        let img = snap.items.iter().find(|i| i.kind == "image" && i.width == Some(640.0)).unwrap();
        assert!(img.ref_id.starts_with("node:"));
        assert_eq!(img.bytes, Some(128));
        let vid = snap.items.iter().find(|i| i.kind == "video").unwrap();
        assert!(vid.ref_id.starts_with("clip:"));
        assert_eq!(vid.duration, Some(30.0));
    }
}
