//! Inkscape-style cached rasterization for dense image layers.
//!
//! When a layer has many primitives, we rasterize it once (off the UI thread)
//! and blit the texture on pan/zoom. Selection and active edits stay vector.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use egui::{ColorImage, TextureHandle, TextureOptions};
use rayon::prelude::*;
use resvg::tiny_skia::{Color, Rect as SkRect};

use crate::document::{Fill, Layer, LayerKind, NodeId, NodeKind, ProjectFile};

/// Minimum visible nodes before switching from per-frame vector paint to raster cache.
pub const MIN_NODES_FOR_CACHE: usize = 150;

const TILE_SIZE: u32 = 256;

pub struct LayerRasterCacheEntry {
    pub texture: TextureHandle,
    pub revision: u64,
    pub anim_frame: usize,
}

pub struct LayerCacheResult {
    pub layer_id: uuid::Uuid,
    pub revision: u64,
    pub anim_frame: usize,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
struct RectItem {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
}

pub fn layer_has_blend_nodes(
    project: &ProjectFile,
    layer: &Layer,
    hidden: &HashSet<NodeId>,
) -> bool {
    use crate::document::BlendMode;
    layer.nodes.iter().any(|id| {
        if hidden.contains(id) {
            return false;
        }
        project
            .nodes
            .get(*id)
            .is_some_and(|n| n.style.blend_mode != BlendMode::Normal)
    })
}

pub fn layer_has_animated_nodes(project: &ProjectFile, layer: &Layer) -> bool {
    layer
        .nodes
        .iter()
        .any(|id| project.anim_timeline.nodes.contains_key(id))
}

pub fn layer_has_text_nodes(
    project: &ProjectFile,
    layer: &Layer,
    hidden: &HashSet<NodeId>,
) -> bool {
    layer.nodes.iter().any(|id| {
        if hidden.contains(id) {
            return false;
        }
        project
            .nodes
            .get(*id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
    })
}

pub fn should_cache_layer(
    project: &ProjectFile,
    layer: &Layer,
    hidden: &HashSet<NodeId>,
    cache_enabled: bool,
    dragging: bool,
    text_editing: bool,
    anim_playing: bool,
    bulk_insert_active: bool,
) -> bool {
    if !cache_enabled {
        return false;
    }
    if !layer.visible || layer.kind != LayerKind::Image || !layer.is_renderer {
        return false;
    }
    // SVG/resvg text baseline differs from live glyph paint — raster cache misaligns/blurs text.
    if layer_has_text_nodes(project, layer, hidden) {
        return false;
    }
    if dragging || text_editing || bulk_insert_active {
        return false;
    }
    let visible_count = layer
        .nodes
        .iter()
        .filter(|id| !hidden.contains(id))
        .count();
    if visible_count < MIN_NODES_FOR_CACHE {
        return false;
    }
    if layer_has_blend_nodes(project, layer, hidden) {
        return false;
    }
    if anim_playing && layer_has_animated_nodes(project, layer) {
        return false;
    }
    true
}

pub fn cache_entry_valid(
    entry: &LayerRasterCacheEntry,
    revision: u64,
    anim_frame: usize,
) -> bool {
    entry.revision == revision && entry.anim_frame == anim_frame
}

pub fn layer_is_solid_rect_only(
    project: &ProjectFile,
    layer: &Layer,
    hidden: &HashSet<NodeId>,
) -> bool {
    use crate::document::{BlendMode, Fill};
    layer.nodes.iter().all(|id| {
        if hidden.contains(id) {
            return true;
        }
        let Some(node) = project.nodes.get(*id) else {
            return false;
        };
        if node.style.blend_mode != BlendMode::Normal {
            return false;
        }
        if node.style.stroke.width > 0.0 {
            return false;
        }
        matches!(
            (&node.kind, &node.style.fill),
            (NodeKind::Rect { rx, .. }, Fill::Solid(_)) if *rx <= 0.0
        )
    })
}

fn fill_to_skia_color(fill: &Fill, opacity: f32) -> Color {
    match fill {
        Fill::Solid(p) => {
            let a = (p.rgba[3] * opacity).clamp(0.0, 1.0);
            Color::from_rgba(
                p.rgba[0].clamp(0.0, 1.0),
                p.rgba[1].clamp(0.0, 1.0),
                p.rgba[2].clamp(0.0, 1.0),
                a,
            )
            .unwrap_or(Color::TRANSPARENT)
        }
        _ => Color::TRANSPARENT,
    }
}

fn collect_rect_items(
    project: &ProjectFile,
    layer: &Layer,
    hidden: &HashSet<NodeId>,
) -> Vec<RectItem> {
    layer
        .nodes
        .iter()
        .filter_map(|id| {
            if hidden.contains(id) {
                return None;
            }
            let node = project.nodes.get(*id)?;
            let NodeKind::Rect { x, y, w, h, rx } = &node.kind else {
                return None;
            };
            if *rx > 0.0 {
                return None;
            }
            let Fill::Solid(_) = &node.style.fill else {
                return None;
            };
            Some(RectItem {
                x: *x as f32,
                y: *y as f32,
                w: *w as f32,
                h: *h as f32,
                color: fill_to_skia_color(&node.style.fill, node.style.opacity),
            })
        })
        .collect()
}

/// Rayon-parallel tile raster for solid-fill rectangles (pixel-art grids).
pub fn rasterize_rect_layer_parallel(
    rects: &[RectItem],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let tw = TILE_SIZE;
    let th = TILE_SIZE;
    let tiles_x = width.div_ceil(tw);
    let tiles_y = height.div_ceil(th);
    let tile_count = (tiles_x * tiles_y) as usize;

    let mut tile_pixels: Vec<(u32, u32, u32, u32, Vec<u8>)> = (0..tile_count)
        .into_par_iter()
        .map(|tile_idx| {
            let tx = (tile_idx as u32) % tiles_x;
            let ty = (tile_idx as u32) / tiles_x;
            let origin_x = tx * tw;
            let origin_y = ty * th;
            let tile_w = tw.min(width - origin_x);
            let tile_h = th.min(height - origin_y);
            let mut tile = vec![0u8; (tile_w * tile_h * 4) as usize];

            let Some(tile_doc) = SkRect::from_xywh(
                origin_x as f32,
                origin_y as f32,
                tile_w as f32,
                tile_h as f32,
            ) else {
                return (tx, ty, tile_w, tile_h, tile);
            };

            for item in rects {
                let Some(rect) = SkRect::from_xywh(item.x, item.y, item.w, item.h) else {
                    continue;
                };
                if let Some(inter) = tile_doc.intersect(&rect) {
                    let x0 = (inter.left() - origin_x as f32).floor().max(0.0) as u32;
                    let y0 = (inter.top() - origin_y as f32).floor().max(0.0) as u32;
                    let x1 = (inter.right() - origin_x as f32).ceil().min(tile_w as f32) as u32;
                    let y1 = (inter.bottom() - origin_y as f32).ceil().min(tile_h as f32) as u32;
                    let r = (item.color.red() * 255.0).round() as u8;
                    let g = (item.color.green() * 255.0).round() as u8;
                    let b = (item.color.blue() * 255.0).round() as u8;
                    let a = (item.color.alpha() * 255.0).round() as u8;
                    for py in y0..y1 {
                        for px in x0..x1 {
                            let i = ((py * tile_w + px) * 4) as usize;
                            tile[i] = r;
                            tile[i + 1] = g;
                            tile[i + 2] = b;
                            tile[i + 3] = a;
                        }
                    }
                }
            }
            (tx, ty, tile_w, tile_h, tile)
        })
        .collect();
    tile_pixels.sort_by_key(|(tx, ty, _, _, _)| (*ty, *tx));

    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (tx, ty, tile_w, tile_h, tile) in tile_pixels {
        let origin_x = tx * tw;
        let origin_y = ty * th;
        for row in 0..tile_h {
            let src = (row * tile_w * 4) as usize;
            let dst = (((origin_y + row) * width + origin_x) * 4) as usize;
            let len = (tile_w * 4) as usize;
            rgba[dst..dst + len].copy_from_slice(&tile[src..src + len]);
        }
    }
    Some(rgba)
}

pub fn spawn_layer_raster_job(
    project: ProjectFile,
    layer: Layer,
    hidden: HashSet<NodeId>,
    revision: u64,
    anim_frame: usize,
    result_tx: mpsc::Sender<LayerCacheResult>,
) {
    let layer_id = layer.id;
    let doc_w = project.document.width as u32;
    let doc_h = project.document.height as u32;
    let use_fast_rect = layer_is_solid_rect_only(&project, &layer, &hidden);
    let rect_items = if use_fast_rect {
        collect_rect_items(&project, &layer, &hidden)
    } else {
        Vec::new()
    };

    std::thread::Builder::new()
        .name("layer-raster-cache".into())
        .spawn(move || {
            let result = if use_fast_rect {
                rasterize_rect_layer_parallel(&rect_items, doc_w, doc_h)
                    .map(|rgba| (doc_w, doc_h, rgba))
            } else {
                crate::io::rasterize_image_layer(&project, &layer, &hidden, 1.0)
            };
            if let Some((w, h, rgba)) = result {
                let _ = result_tx.send(LayerCacheResult {
                    layer_id,
                    revision,
                    anim_frame,
                    width: w,
                    height: h,
                    rgba,
                });
            }
        })
        .ok();
}

pub fn install_cache_result(
    cache: &mut HashMap<uuid::Uuid, LayerRasterCacheEntry>,
    pending: &mut HashSet<uuid::Uuid>,
    ctx: &egui::Context,
    result: LayerCacheResult,
) {
    let LayerCacheResult {
        layer_id,
        revision,
        anim_frame,
        width,
        height,
        rgba,
    } = result;
    if width == 0 || height == 0 {
        pending.remove(&layer_id);
        return;
    }
    let color_image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
    let texture = ctx.load_texture(
        format!("layer-cache-{}-{}-{}", layer_id.as_simple(), revision, anim_frame),
        color_image,
        TextureOptions::LINEAR,
    );
    cache.insert(
        layer_id,
        LayerRasterCacheEntry {
            texture,
            revision,
            anim_frame,
        },
    );
    pending.remove(&layer_id);
}