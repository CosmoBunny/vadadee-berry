//! Strip heavy assets for wire sync; merge remote changes into local RAM cache.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::document::{NodeKind, ProjectFile};

pub fn asset_sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Clone project for wire: image bytes → SHA256 key; video/audio layers keep paths only.
pub fn strip_for_wire(project: &ProjectFile, cache: &mut HashMap<String, Vec<u8>>) -> ProjectFile {
    let mut out = project.clone();
    for node in out.nodes.map.values_mut() {
        if let NodeKind::Image {
            bytes,
            collab_asset_sha256,
            ..
        } = &mut node.kind
        {
            if bytes.is_empty() {
                continue;
            }
            let key = asset_sha256_hex(bytes);
            cache.insert(key.clone(), bytes.clone());
            *collab_asset_sha256 = Some(key);
            bytes.clear();
        }
    }
    out
}

/// Fill empty image bytes from local project + RAM cache; then replace document/nodes.
pub fn merge_remote(
    local: &mut ProjectFile,
    mut remote: ProjectFile,
    cache: &mut HashMap<String, Vec<u8>>,
) {
    hydrate_images(&mut remote, local, cache);
    local.document = remote.document;
    local.nodes = remote.nodes;
    local.anim_timeline = remote.anim_timeline;
}

fn hydrate_images(
    project: &mut ProjectFile,
    local: &ProjectFile,
    cache: &mut HashMap<String, Vec<u8>>,
) {
    for node in project.nodes.map.values_mut() {
        let NodeKind::Image {
            bytes,
            collab_asset_sha256,
            ..
        } = &mut node.kind
        else {
            continue;
        };
        if !bytes.is_empty() {
            continue;
        }
        let Some(key) = collab_asset_sha256.as_ref() else {
            continue;
        };
        if let Some(b) = cache.get(key) {
            *bytes = b.clone();
            continue;
        }
        if let Some(b) = find_local_image_bytes(local, key) {
            cache.insert(key.clone(), b.clone());
            *bytes = b;
            continue;
        }
    }
}

fn find_local_image_bytes(local: &ProjectFile, key: &str) -> Option<Vec<u8>> {
    for node in local.nodes.map.values() {
        if let NodeKind::Image {
            bytes,
            collab_asset_sha256,
            ..
        } = &node.kind
        {
            if !bytes.is_empty() && collab_asset_sha256.as_deref() == Some(key) {
                return Some(bytes.clone());
            }
            if !bytes.is_empty() && asset_sha256_hex(bytes) == key {
                return Some(bytes.clone());
            }
        }
    }
    None
}

pub fn project_wire_hash(project: &ProjectFile) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    project.document.active_layer_index.hash(&mut h);
    for id in project.document.ordered_node_ids() {
        id.hash(&mut h);
        if let Some(n) = project.nodes.get(id) {
            n.name.hash(&mut h);
            for (x, y) in n.edit_handles() {
                x.to_bits().hash(&mut h);
                y.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}