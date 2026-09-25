//! Authoritative render contract: ONE definition of "what this document looks like".
//!
//! ```text
//!                          Project / Document
//!                                  │
//!                                  ▼
//!                       RenderContext + RenderTarget
//!                                  │
//!                                  ▼
//!                    render::draw_nodes_ex (sole scene interpreter)
//!                                  │
//!                  ┌───────────────┼───────────────┐
//!                  ▼               ▼               ▼
//!           Preview Buffer   Export Buffer    Video Frame
//! ```
//!
//! Every consumer (canvas preview, PNG/JPEG/BMP, clipboard, selection,
//! thumbnail, bake, video frame) must go through [`RenderTarget`] for size
//! and [`RenderContext`] for frame/time. No consumer reinterprets DPI,
//! coordinates, or layer semantics on its own.
//!
//! * DPI is just a scale: `scale = dpi / DOC_PX_PER_INCH` (96).
//! * Document units are never rewritten; the viewport/transform applies scale.
//! * Overlay compositing (AV frames, NodeEditor bakes, shading) shares the
//!   helpers [`RenderTarget::doc_to_px`] and [`pixmap_transform`] so all
//!   layer kinds convert coordinates through ONE path.
//!
//! Legacy SVG→resvg raster paths are gone: static vector content in every
//! frame compositor goes through `export_render` (same `draw_nodes_ex` as
//! preview). The remaining pixmap overlays (decoded AV pixels, CPU shading
//! heuristics, NodeEditor bakes) share `RenderTarget` sizes and
//! `pixmap_transform`, with one implementation each in `io`
//! (`flush_static_segment`, `blit_av_layer`, `blit_transparent_full_frame`).

/// Document pixels per inch. 96 = 1 doc px per output pixel at 1x.
pub const DOC_PX_PER_INCH: f32 = 96.0;

/// Where the rendered pixels go and at what density.
#[derive(Debug, Clone, Copy)]
pub struct RenderTarget {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Output pixels per document unit (`dpi / 96`).
    pub scale: f32,
}

impl RenderTarget {
    /// Full-document target from an explicit scale (`dpi / 96`).
    pub fn from_scale(doc_w: f64, doc_h: f64, scale: f32) -> Option<Self> {
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let w = (doc_w as f32 * scale).round().max(1.0) as u32;
        let h = (doc_h as f32 * scale).round().max(1.0) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        Some(Self {
            width: w,
            height: h,
            scale,
        })
    }

    /// Full-document target from a user-facing DPI value.
    pub fn from_dpi(doc_w: f64, doc_h: f64, dpi: f32) -> Option<Self> {
        Self::from_scale(doc_w, doc_h, dpi / DOC_PX_PER_INCH)
    }

    /// Full-document target for video encoders (forces even dimensions).
    pub fn for_video(doc_w: f64, doc_h: f64, scale: f32) -> Option<Self> {
        let mut t = Self::from_scale(doc_w, doc_h, scale)?;
        if t.width % 2 != 0 {
            t.width = t.width.saturating_sub(1);
        }
        if t.height % 2 != 0 {
            t.height = t.height.saturating_sub(1);
        }
        if t.width == 0 || t.height == 0 {
            return None;
        }
        Some(t)
    }

    /// Selection/crop target: `bounds` (document units) at `scale`.
    pub fn for_selection(bounds: kurbo::Rect, scale: f32) -> Option<Self> {
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let w = (bounds.width() as f32 * scale).round().max(1.0) as u32;
        let h = (bounds.height() as f32 * scale).round().max(1.0) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        Some(Self {
            width: w,
            height: h,
            scale,
        })
    }

    /// Map a document-space point to output pixels. THE single conversion.
    pub fn doc_to_px(&self, x: f64, y: f64) -> (f32, f32) {
        ((x as f32) * self.scale, (y as f32) * self.scale)
    }

    /// Map a document-space size to output pixels.
    pub fn size_to_px(&self, w: f64, h: f64) -> (f32, f32) {
        ((w as f32) * self.scale, (h as f32) * self.scale)
    }
}

/// Per-frame render inputs: animation frame + timeline time. Static scene +
/// per-frame state stay separate so video can reuse compilation and only
/// vary frame/time. Output density lives in [`RenderTarget`], never here —
/// one size promise per render, not two numbers that can disagree.
#[derive(Debug, Clone, Copy)]
pub struct RenderContext {
    /// Animation frame index.
    pub frame: usize,
    /// Timeline seconds (shading/video).
    pub time_secs: f32,
}

impl RenderContext {
    pub fn new(frame: usize, time_secs: f32) -> Self {
        Self { frame, time_secs }
    }

    /// Still-image export / thumbnails at frame 0, frozen time.
    pub fn still() -> Self {
        Self::new(0, 0.0)
    }
}

/// Build the tiny-skia transform used to composite an overlay pixmap
/// (decoded AV frame, NodeEditor bake) at document rect `(dx,dy,dw,dh)`
/// with rotation degrees and source size `(sw,sh)`.
///
/// Single implementation so AV / NodeEditor / selection overlays cannot
/// drift apart with duplicated `x = dx*scale` math.
pub fn pixmap_transform(
    target: &RenderTarget,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
    rot_deg: f32,
    sw: u32,
    sh: u32,
) -> resvg::tiny_skia::Transform {
    use resvg::tiny_skia::Transform;
    let (x, y) = target.doc_to_px(dx, dy);
    let (w, h) = target.size_to_px(dw, dh);
    let sx = w / sw.max(1) as f32;
    let sy = h / sh.max(1) as f32;
    if rot_deg.abs() > 1e-4 {
        Transform::from_translate(x, y).pre_concat(
            Transform::from_translate(w / 2.0, h / 2.0)
                .pre_rotate(rot_deg)
                .pre_translate(-w / 2.0, -h / 2.0)
                .pre_scale(sx, sy),
        )
    } else {
        Transform::from_translate(x, y).pre_scale(sx, sy)
    }
}

/// Convert a user DPI to the renderer scale. Clamped to the same range the
/// app already exposes (24–1536 DPI → 0.25–16x).
pub fn dpi_to_scale(dpi: f32) -> f32 {
    (dpi / DOC_PX_PER_INCH).clamp(0.25, 16.0)
}

/// Nodes the base pass must skip because another pass paints them.
///
/// Single implementation of what `app::hidden_canvas_sources` does for the
/// live canvas: path-effect hidden sources + form nodes, tiling/circular
/// `hide_source`, clip source (+ mask when `hide_mask`), boolean operands,
/// group children (painted via the parent), and NE AppObjects sources
/// (painted at the NE layer slot). Export must hide the same nodes,
/// otherwise preview≠export (e.g. grouped children double-painting).
pub fn hidden_effect_sources(
    project: &crate::document::ProjectFile,
) -> std::collections::HashSet<crate::document::NodeId> {
    let doc = &project.document;
    let mut hidden = crate::document::hidden_effect_sources(&doc.path_effects);
    hidden.extend(crate::document::path_effect_form_node_ids(
        &doc.path_effects,
    ));
    for e in doc.tiling_effects.values() {
        if e.hide_source {
            hidden.insert(e.source_id);
        }
    }
    for e in doc.circular_effects.values() {
        if e.hide_source {
            hidden.insert(e.source_id);
        }
    }
    for cm in doc.clip_masks.values() {
        hidden.insert(cm.source_id);
        if cm.hide_mask {
            hidden.insert(cm.mask_id);
        }
    }
    for e in doc.boolean_effects.values() {
        if e.hide_operands {
            hidden.insert(e.a_id);
            hidden.insert(e.b_id);
        }
    }
    // Group children live in parent-local space; the parent paints them.
    for n in project.nodes.map.values() {
        if let crate::document::NodeKind::Group { children } = &n.kind {
            for cid in children {
                hidden.insert(*cid);
            }
        }
    }
    // App objects feeding a visible NodeEditor output are drawn by the NE
    // composite only (canvas P6c) — hide originals so they don't double-draw.
    for layer in &doc.layers {
        if !layer.visible || layer.kind != crate::document::LayerKind::NodeEditor {
            continue;
        }
        let Some(g) = layer.node_graph.as_ref() else {
            continue;
        };
        let eval = g.resolve_output_image();
        if let crate::document::GraphImageSource::AppObjects(ids) = &eval.image {
            for id in ids {
                hidden.insert(*id);
            }
        }
    }
    hidden
}

/// Loft form paths paint without fill in preview (`canvas_ui` passes this
/// global set to every layer draw). Export must use the same set, not a
/// per-order subset, or loft forms fill in export but not preview.
pub fn loft_form_paths(
    doc: &crate::document::Document,
) -> std::collections::HashSet<crate::document::NodeId> {
    doc.path_effects
        .values()
        .filter(|e| e.mode == crate::document::OnPathMode::Loft)
        .map(|e| e.path_id)
        .collect()
}

/// NodeIds feeding visible NodeEditor AppObjects outputs, in canvas draw
/// order (flat visible order intersected, then any missing appended).
fn ne_appobjects_order(project: &crate::document::ProjectFile) -> Vec<crate::document::NodeId> {
    use std::collections::HashSet;
    let flat: Vec<crate::document::NodeId> = project
        .document
        .layers
        .iter()
        .filter(|l| l.visible && l.is_renderer)
        .flat_map(|l| l.nodes.iter().copied())
        .collect();
    let in_flat: HashSet<_> = flat.iter().copied().collect();
    let mut ids: Vec<crate::document::NodeId> = Vec::new();
    for layer in &project.document.layers {
        if !layer.visible
            || !layer.is_renderer
            || layer.kind != crate::document::LayerKind::NodeEditor
        {
            continue;
        }
        let Some(g) = layer.node_graph.as_ref() else {
            continue;
        };
        let eval = g.resolve_output_image();
        if let crate::document::GraphImageSource::AppObjects(eval_ids) = &eval.image {
            for id in flat.iter().filter(|id| eval_ids.contains(id)) {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            for id in eval_ids {
                if !in_flat.contains(id) && !ids.contains(id) {
                    ids.push(*id);
                }
            }
        }
    }
    ids
}

/// Full-document paint plan mirroring `canvas_ui` layer stacking: flat
/// visible order with AppObjects sources relocated to their NE layer's
/// stack slot, plus the hidden set with those ids un-hidden (canvas
/// `hide.remove`). Groups stay hidden (parent paints them).
pub fn full_document_paint_plan(
    project: &crate::document::ProjectFile,
) -> (
    Vec<crate::document::NodeId>,
    std::collections::HashSet<crate::document::NodeId>,
) {
    use std::collections::HashSet;
    let moved: HashSet<crate::document::NodeId> =
        ne_appobjects_order(project).into_iter().collect();
    let mut order: Vec<crate::document::NodeId> = Vec::new();
    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer {
            continue;
        }
        if layer.kind == crate::document::LayerKind::NodeEditor {
            if let Some(g) = layer.node_graph.as_ref() {
                let eval = g.resolve_output_image();
                if let crate::document::GraphImageSource::AppObjects(eval_ids) = &eval.image {
                    if !eval_ids.is_empty() {
                        // Canvas order: draw-order intersection, then missing.
                        let flat = full_order(project);
                        for id in flat.iter().filter(|id| eval_ids.contains(id)) {
                            if !order.contains(id) {
                                order.push(*id);
                            }
                        }
                        for id in eval_ids {
                            if !order.contains(id) {
                                order.push(*id);
                            }
                        }
                        continue;
                    }
                }
            }
        }
        for id in &layer.nodes {
            if moved.contains(id) {
                continue;
            }
            order.push(*id);
        }
    }
    let mut hidden = hidden_effect_sources(project);
    for id in &moved {
        hidden.remove(id);
    }
    (order, hidden)
}

fn full_order(project: &crate::document::ProjectFile) -> Vec<crate::document::NodeId> {
    project
        .document
        .layers
        .iter()
        .filter(|l| l.visible && l.is_renderer)
        .flat_map(|l| l.nodes.iter().copied())
        .collect()
}

/// Effect passes in live-canvas order (path → tiling → circular → clip).
///
/// `order` restricts effects to those touching the rendered node set so
/// selection/crop exports don't leak unselected effects. Pass `None` for
/// full-document renders (all effects). This is the same call sequence as
/// `app::canvas_ui` and the GPU video path.
#[allow(clippy::too_many_arguments)]
pub fn paint_document_effects(
    painter: &egui::Painter,
    project: &crate::document::ProjectFile,
    order: Option<&[crate::document::NodeId]>,
    viewport: &crate::canvas::Viewport,
    origin: egui::Pos2,
    fonts: &crate::fonts::FontRegistry,
    image_textures: &std::collections::HashMap<crate::document::NodeId, egui::TextureHandle>,
) {
    use std::collections::HashSet;
    let allow: Option<HashSet<crate::document::NodeId>> =
        order.map(|o| o.iter().copied().collect());
    let touches = |ids: &[crate::document::NodeId]| -> bool {
        match &allow {
            None => true,
            Some(set) => ids.iter().any(|id| set.contains(id)),
        }
    };

    // Path effects: keep only effects whose source/path/form is rendered.
    let mut path_fx = indexmap::IndexMap::new();
    for (k, v) in project.document.path_effects.iter() {
        let mut ids = vec![v.source_id, v.path_id];
        if let Some(f) = v.form_node_id {
            ids.push(f);
        }
        if touches(&ids) {
            path_fx.insert(*k, v.clone());
        }
    }
    crate::render::draw_path_effects(
        painter,
        &project.nodes,
        &path_fx,
        viewport,
        origin,
        fonts,
        image_textures,
        &[],
    );

    let mut tiling = indexmap::IndexMap::new();
    for (k, v) in project.document.tiling_effects.iter() {
        if touches(&[v.source_id]) {
            tiling.insert(*k, v.clone());
        }
    }
    crate::render::draw_tiling_effects(
        painter,
        &project.nodes,
        &tiling,
        viewport,
        origin,
        fonts,
        image_textures,
        &[],
    );

    let mut circular = indexmap::IndexMap::new();
    for (k, v) in project.document.circular_effects.iter() {
        if touches(&[v.source_id]) {
            circular.insert(*k, v.clone());
        }
    }
    crate::render::draw_circular_effects(
        painter,
        &project.nodes,
        &circular,
        viewport,
        origin,
        fonts,
        image_textures,
        &[],
    );

    let mut clips = indexmap::IndexMap::new();
    for (k, v) in project.document.clip_masks.iter() {
        if touches(&[v.source_id, v.mask_id]) {
            clips.insert(*k, v.clone());
        }
    }
    crate::render::draw_clip_mask_effects(
        painter,
        &project.nodes,
        &clips,
        viewport,
        origin,
        fonts,
        image_textures,
        &[],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_scale_roundtrip() {
        assert!((dpi_to_scale(96.0) - 1.0).abs() < 1e-6);
        assert!((dpi_to_scale(192.0) - 2.0).abs() < 1e-6);
        let t = RenderTarget::from_dpi(794.0, 1123.0, 192.0).unwrap();
        assert_eq!(t.scale, 2.0);
        assert_eq!(t.width, (794.0f32 * 2.0).round() as u32);
    }

    #[test]
    fn video_forces_even() {
        let t = RenderTarget::for_video(101.0, 101.0, 1.0).unwrap();
        assert_eq!(t.width % 2, 0);
        assert_eq!(t.height % 2, 0);
    }

    #[test]
    fn single_coordinate_path() {
        let t = RenderTarget::from_scale(100.0, 100.0, 2.0).unwrap();
        assert_eq!(t.doc_to_px(10.0, 20.0), (20.0, 40.0));
        assert_eq!(t.size_to_px(5.0, 5.0), (10.0, 10.0));
        let tr = pixmap_transform(&t, 10.0, 20.0, 50.0, 50.0, 0.0, 100, 100);
        let mut p = resvg::tiny_skia::Point::from_xy(0.0, 0.0);
        tr.map_point(&mut p);
        assert!((p.x - 20.0).abs() < 1e-3 && (p.y - 40.0).abs() < 1e-3);
    }
}
