//! Uniform-grid spatial index for O(1) hit testing on dense rect layers.

use std::collections::{HashMap, HashSet};

use kurbo::Shape;
use rayon::prelude::*;

use crate::document::{NodeId, NodeKind, ProjectFile};

pub const GRID_CELL: f64 = 8.0;
pub const MIN_NODES_FOR_SPATIAL: usize = 150;

#[derive(Clone, Debug, Default)]
pub struct SpatialIndex {
    pub revision: u64,
    cells: HashMap<(i32, i32), Vec<NodeId>>,
    z_rank: HashMap<NodeId, u32>,
    flat_order: Vec<NodeId>,
    enabled: bool,
}

impl SpatialIndex {
    pub fn disabled(revision: u64) -> Self {
        Self {
            revision,
            enabled: false,
            ..Default::default()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn rebuild(
        project: &ProjectFile,
        hidden: &HashSet<NodeId>,
        revision: u64,
    ) -> Self {
        let flat_order: Vec<NodeId> = project.document.ordered_node_ids();
        if flat_order.len() < MIN_NODES_FOR_SPATIAL {
            return Self::disabled(revision);
        }

        let z_rank: HashMap<NodeId, u32> = flat_order
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i as u32))
            .collect();

        let pairs: Vec<((i32, i32), NodeId)> = flat_order
            .par_iter()
            .flat_map(|id| {
                if hidden.contains(id) {
                    return vec![];
                }
                let Some(node) = project.nodes.get(*id) else {
                    return vec![];
                };
                let b =
                    crate::document::spatial_index_bounds(node, &project.document, &project.nodes);
                cells_for_bounds(b).into_iter().map(|c| (c, *id)).collect()
            })
            .collect();

        let mut cells: HashMap<(i32, i32), Vec<NodeId>> = HashMap::new();
        for (cell, id) in pairs {
            cells.entry(cell).or_default().push(id);
        }
        for ids in cells.values_mut() {
            ids.sort_by_key(|id| z_rank.get(id).copied().unwrap_or(0));
            ids.dedup();
        }

        Self {
            revision,
            cells,
            z_rank,
            flat_order,
            enabled: true,
        }
    }

    pub fn pick_topmost_with_document(
        &self,
        project: &ProjectFile,
        hidden: &HashSet<NodeId>,
        doc: (f64, f64),
        slop: f64,
        node_uses_extended_bounds: impl Fn(NodeId) -> bool,
    ) -> (Option<NodeId>, Option<NodeId>) {
        if !self.enabled {
            return (None, None);
        }
        let mut hit: Option<NodeId> = None;
        let mut bbox_only: Option<NodeId> = None;
        let candidates = self.candidates_near(doc, slop);
        for id in candidates.into_iter().rev() {
            if hidden.contains(&id) {
                continue;
            }
            let Some(node) = project.nodes.get(id) else {
                continue;
            };
            let does_hit = if node_uses_extended_bounds(id) {
                let eb =
                    crate::document::get_effective_bounds(node, &project.document, &project.nodes);
                let pt = kurbo::Point::new(doc.0, doc.1);
                eb.inflate(slop, slop).contains(pt)
            } else {
                node.hit_test_with_store(&project.nodes, doc.0, doc.1, slop)
            };
            if !does_hit {
                continue;
            }
            let pt = kurbo::Point::new(doc.0, doc.1);
            let precise = if node_uses_extended_bounds(id) {
                true
            } else {
                node.bez_path().contains(pt)
                    || matches!(node.kind, NodeKind::Text { .. })
                    || matches!(node.kind, NodeKind::Image { .. })
            };
            if precise {
                hit = Some(id);
                break;
            } else if bbox_only.is_none() && !matches!(node.kind, NodeKind::Image { .. }) {
                bbox_only = Some(id);
            }
        }
        if hit.is_none() {
            hit = bbox_only;
        }
        (hit, bbox_only)
    }

    fn candidates_near(&self, doc: (f64, f64), slop: f64) -> Vec<NodeId> {
        let pad = slop.ceil() as i32 + 1;
        let cx = (doc.0 / GRID_CELL).floor() as i32;
        let cy = (doc.1 / GRID_CELL).floor() as i32;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for dy in -pad..=pad {
            for dx in -pad..=pad {
                if let Some(ids) = self.cells.get(&(cx + dx, cy + dy)) {
                    for id in ids {
                        if seen.insert(*id) {
                            out.push(*id);
                        }
                    }
                }
            }
        }
        out.sort_by_key(|id| self.z_rank.get(id).copied().unwrap_or(0));
        out
    }

    pub fn flat_order(&self) -> &[NodeId] {
        &self.flat_order
    }

    /// Nodes whose bounds intersect a document-space marquee rectangle.
    pub fn nodes_in_marquee(
        &self,
        project: &ProjectFile,
        hidden: &HashSet<NodeId>,
        marquee: kurbo::Rect,
    ) -> Vec<NodeId> {
        if !self.enabled {
            return Vec::new();
        }
        let x0 = (marquee.x0 / GRID_CELL).floor() as i32;
        let y0 = (marquee.y0 / GRID_CELL).floor() as i32;
        let x1 = (marquee.x1 / GRID_CELL).floor() as i32;
        let y1 = (marquee.y1 / GRID_CELL).floor() as i32;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                if let Some(ids) = self.cells.get(&(cx, cy)) {
                    for id in ids {
                        if seen.insert(*id) && !hidden.contains(id) {
                            if let Some(node) = project.nodes.get(*id) {
                                let b = crate::document::spatial_index_bounds(
                                    node,
                                    &project.document,
                                    &project.nodes,
                                );
                                let hit = b.intersect(marquee);
                                if hit.width() > 0.0 && hit.height() > 0.0 {
                                    out.push(*id);
                                }
                            }
                        }
                    }
                }
            }
        }
        out.sort_by_key(|id| self.z_rank.get(id).copied().unwrap_or(0));
        out
    }
}

fn cells_for_bounds(b: kurbo::Rect) -> Vec<(i32, i32)> {
    let x0 = (b.x0 / GRID_CELL).floor() as i32;
    let y0 = (b.y0 / GRID_CELL).floor() as i32;
    let x1 = (b.x1 / GRID_CELL).floor() as i32;
    let y1 = (b.y1 / GRID_CELL).floor() as i32;
    let mut out = Vec::new();
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            out.push((cx, cy));
        }
    }
    out
}