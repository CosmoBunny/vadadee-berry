//! Raster export through the live preview renderer (headless egui).
//!
//! Architecture (per review: one authoritative renderer, no SVG intermediate):
//!
//! ```text
//! Preview: Document → render::draw_nodes_ex ──▶ egui Painter ──▶ GPU screen
//! Export:  Document → render::draw_nodes_ex ──▶ headless Painter ──▶ tessellate ──▶ CPU raster ──▶ PNG/JPEG/BMP
//! ```
//!
//! Both paths invoke the SAME `render::draw_nodes_ex`, so text layout, glyph
//! selection, baselines, wrapping, rotation, transforms and bounds are
//! identical by construction. Only the output scale differs
//! (`scale = export_dpi / 96`, document units stay untouched).
//!
//! Text goes through egui's own layout (`painter.layout_job` inside render.rs)
//! against the same [`crate::fonts::FontRegistry`] faces the preview uses —
//! there is no export-side `<text>` reconstruction, no baseline compensation
//! and no DPI fudging of coordinates.
//!
//! Mesh rasterization below is a dumb triangle filler, NOT a second renderer:
//! it consumes egui-tessellated meshes (egui's own tessellator output) with
//! the font atlas egui itself produced.
//!
//! Coverage contract — what each entry point renders, so gaps stay explicit
//! instead of becoming silent divergences:
//!
//! ```text
//! render_document_rgba      vector scene + path/tiling/circular/clip effects
//!                           + groups + NE AppObjects (relocated to NE slot).
//!                           AV frames are pixel overlays (frame paths only);
//!                           WGSL shading needs GPU (video path); the CPU
//!                           preset path applies in frames fallback AND still
//!                           export via the shared helper. NE FilePath&bakes
//!                           composite in frames fallback and in
//!                           `io::export_document_raster` (full still res).
//!                           Skipped only over a translucent page (pixmap
//!                           round-trip is straight-safe solely over opaque
//!                           bg).
//! render_selection_rgba     same scene, cropped to bounds, transparent bg
//!                           (no overlays: selection semantics stay approximate).
//! render_static_order_rgba  explicit order, transparent bg; video fallback
//!                           segments (worker adds AV/shading/NE overlays
//!                           around them — never AppObjects ids: those are
//!                           hidden in segments, painted by the next line).
//! render_ne_appobjects_rgba NE AppObjects overlay with sources un-hidden
//!                           (mirrors canvas `hide.remove`); both frame paths
//!                           use this, never the segment renderer.
//! render_layer_base_rgba    base pass only, caller hidden set; preview cache
//!                           (canvas paints effect passes globally after).
//! ```
//!
//! Anything not listed here must go through one of these — never a new
//! SVG→resvg round-trip (deleted: `document_svg_single_image_layer`,
//! `document_svg_nodes_only`, `rasterize_image_layer`).
//!
//! [`PainterSession`] owns everything compilation-like across renders of a
//! frame/export: the headless [`egui::Context`], installed font families +
//! atlas, decoded image bytes and texture handles. Shapes are re-recorded
//! every render (the scene animates); nothing is recompiled. Hot loops hold
//! one session; one-shot callers get an ephemeral session at exactly the old
//! per-call cost. Image bytes are keyed by `NodeId` and assumed stable for
//! the session lifetime (animation mutates transforms, never bytes).

use std::collections::{HashMap, HashSet};

use egui::{Color32, ColorImage, Context, LayerId, Pos2, RawInput, Rect, TextureId, Vec2};

use crate::canvas::Viewport;
use crate::document::{NodeId, ProjectFile};
use crate::fonts::FontRegistry;

/// Fill for areas the document does not paint (matches old SVG export).
#[derive(Debug, Clone, Copy)]
pub enum ExportBackground {
    /// Full-document export: page color, like the old `<rect>` page background.
    Page,
    /// Selection export: transparent (old selection SVG had no background).
    Transparent,
}

/// Render the full document to RGBA8. `scale` = output pixels per doc unit
/// (`export_dpi / 96`). Returns `(width, height, rgba)`.
/// Dimensions come from the authoritative [`crate::render_pipeline::RenderTarget`].
/// Paint plan mirrors `canvas_ui` stacking (see
/// [`crate::render_pipeline::full_document_paint_plan`]). Pixel/GPU overlays
/// (AV, WGSL shading, NE FilePath/bakes) are composited by the frame paths
/// and by `io::export_document_raster`, not here.
pub fn render_document_rgba(project: &ProjectFile, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    PainterSession::new().render_document(project, scale)
}

/// Render selected nodes translated so `bounds` maps to the buffer origin.
/// Same renderer as preview; the viewport pan does the translating (no
/// document mutation, no coordinate rewriting).
pub fn render_selection_rgba(
    project: &ProjectFile,
    selection: &[NodeId],
    bounds: kurbo::Rect,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    PainterSession::new().render_selection(project, selection, bounds, scale)
}

/// Render an explicit node order into a full-target transparent buffer.
///
/// Video fallback compositor primitive: consecutive static layers
/// (Image/Flowchart, plus NodeEditor AppObjects overlays) are grouped into
/// segments and painted with the same `draw_nodes_ex` + effect passes as
/// preview, then blitted onto the frame pixmap in stack order. This replaces
/// the old `document_svg_*` → resvg round-trip for static content, so text
/// layout, clip, tiling and path effects match preview by construction.
/// `target` must be the frame's [`crate::render_pipeline::RenderTarget`].
pub fn render_static_order_rgba(
    project: &ProjectFile,
    order: &[NodeId],
    target: &crate::render_pipeline::RenderTarget,
) -> Option<Vec<u8>> {
    PainterSession::new().render_static_order(project, order, target)
}

/// Render a NodeEditor AppObjects overlay: base pass only, with the source
/// ids un-hidden.
///
/// Mirrors `canvas_ui` exactly: AppObjects sources are hidden in every base
/// pass but `hide.remove(id)`d for the NE-slot paint. Calling the plain
/// segment renderer here would paint nothing (the sources are hidden) —
/// that double-hide broke CPU fallback frames and thumbnails.
pub fn render_ne_appobjects_rgba(
    project: &ProjectFile,
    ids: &[NodeId],
    target: &crate::render_pipeline::RenderTarget,
) -> Option<Vec<u8>> {
    PainterSession::new().render_ne_appobjects(project, ids, target)
}

/// Render one layer's base nodes with the live-canvas painter (no SVG).
///
/// Preview-cache primitive: the canvas blits one texture per dense layer and
/// paints effect passes globally afterward, so the cached texture must hold
/// the base pass only — same `draw_nodes_ex`, same caller-provided `hidden`
/// set, same global loft set as `canvas_ui`. Transparent background (the old
/// SVG cache had no page rect either).
pub fn render_layer_base_rgba(
    project: &ProjectFile,
    order: &[NodeId],
    hidden: &HashSet<NodeId>,
    target: &crate::render_pipeline::RenderTarget,
) -> Option<Vec<u8>> {
    PainterSession::new().render_layer_base(project, order, hidden, target)
}

/// Frame-persistent painter compilation (review item 14: separate static
/// scene compilation/cache from per-frame state).
///
/// A fresh headless painter per segment per frame rebuilds everything: the
/// egui `Context`, 3 font-install pump passes, the font atlas, and a
/// PNG/JPEG decode per image node. A session owns all of that across renders
/// — shapes are still re-recorded every render (the scene animates), but
/// nothing is recompiled. Hot loops (video fallback frames) hold one session
/// per export; one-shot callers get an ephemeral session through the free
/// functions below at exactly today's cost.
///
/// Cache validity: image bytes are keyed by `NodeId` and assumed stable for
/// the session lifetime (animation mutates transforms, never bytes; bakes
/// flow through overlays, not base segments). Texture handles stay valid
/// because they belong to the session's own `Context`.
pub struct PainterSession {
    ctx: Context,
    fonts: FontRegistry,
    installed: HashSet<String>,
    decoded: HashMap<NodeId, image::RgbaImage>,
    handles: HashMap<NodeId, egui::TextureHandle>,
}

impl Default for PainterSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PainterSession {
    pub fn new() -> Self {
        Self {
            ctx: Context::default(),
            fonts: FontRegistry::new(),
            installed: HashSet::new(),
            decoded: HashMap::new(),
            handles: HashMap::new(),
        }
    }

    /// Core paint: full-document plan (AppObjects relocated to NE slot).
    pub fn render_document(
        &mut self,
        project: &ProjectFile,
        scale: f32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let target = crate::render_pipeline::RenderTarget::from_scale(
            project.document.width,
            project.document.height,
            scale,
        )?;
        let (w, h) = (target.width, target.height);
        let (order, hidden) = crate::render_pipeline::full_document_paint_plan(project);
        let loft = crate::render_pipeline::loft_form_paths(&project.document);
        let viewport = Viewport {
            pan: Vec2::ZERO,
            zoom: scale,
            ..Default::default()
        };
        self.render_order(ScenePaint {
            project,
            order: &order,
            viewport: &viewport,
            origin: Pos2::ZERO,
            w,
            h,
            bg: ExportBackground::Page,
            hidden,
            loft,
            effects: Some(&order),
        })
    }

    /// Cropped selection render (transparent bg).
    pub fn render_selection(
        &mut self,
        project: &ProjectFile,
        selection: &[NodeId],
        bounds: kurbo::Rect,
        scale: f32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        if selection.is_empty() {
            return None;
        }
        let target = crate::render_pipeline::RenderTarget::for_selection(bounds, scale)?;
        let (w, h) = (target.width, target.height);
        let order = crate::io::selection_paint_order(project, selection);
        // screen = origin + pan + doc * zoom → pan puts bounds at origin, via
        // the single doc→pixel conversion (not inline `* scale` math).
        let (bx, by) = target.doc_to_px(bounds.x0, bounds.y0);
        let viewport = Viewport {
            pan: Vec2::new(-bx, -by),
            zoom: scale,
            ..Default::default()
        };
        let hidden_all = crate::render_pipeline::hidden_effect_sources(project);
        let hidden: HashSet<NodeId> = hidden_all
            .into_iter()
            .filter(|id| order.contains(id))
            .collect();
        let loft = crate::render_pipeline::loft_form_paths(&project.document);
        self.render_order(ScenePaint {
            project,
            order: &order,
            viewport: &viewport,
            origin: Pos2::ZERO,
            w,
            h,
            bg: ExportBackground::Transparent,
            hidden,
            loft,
            effects: Some(&order),
        })
    }

    /// Explicit node order into a full-target transparent buffer (video
    /// fallback segments).
    pub fn render_static_order(
        &mut self,
        project: &ProjectFile,
        order: &[NodeId],
        target: &crate::render_pipeline::RenderTarget,
    ) -> Option<Vec<u8>> {
        if order.is_empty() {
            return None;
        }
        let viewport = Viewport {
            pan: Vec2::ZERO,
            zoom: target.scale,
            ..Default::default()
        };
        let hidden_all = crate::render_pipeline::hidden_effect_sources(project);
        let hidden: HashSet<NodeId> = hidden_all
            .into_iter()
            .filter(|id| order.contains(id))
            .collect();
        let loft = crate::render_pipeline::loft_form_paths(&project.document);
        let (_, _, rgba) = self.render_order(ScenePaint {
            project,
            order,
            viewport: &viewport,
            origin: Pos2::ZERO,
            w: target.width,
            h: target.height,
            bg: ExportBackground::Transparent,
            hidden,
            loft,
            effects: Some(order),
        })?;
        Some(rgba)
    }

    /// NE AppObjects overlay with sources un-hidden (mirrors canvas
    /// `hide.remove`).
    pub fn render_ne_appobjects(
        &mut self,
        project: &ProjectFile,
        ids: &[NodeId],
        target: &crate::render_pipeline::RenderTarget,
    ) -> Option<Vec<u8>> {
        if ids.is_empty() {
            return None;
        }
        let mut hidden = crate::render_pipeline::hidden_effect_sources(project);
        for id in ids {
            hidden.remove(id);
        }
        self.render_layer_base(project, ids, &hidden, target)
    }

    /// Base pass only with a caller-resolved hidden set (preview cache).
    pub fn render_layer_base(
        &mut self,
        project: &ProjectFile,
        order: &[NodeId],
        hidden: &HashSet<NodeId>,
        target: &crate::render_pipeline::RenderTarget,
    ) -> Option<Vec<u8>> {
        if order.is_empty() || target.width == 0 || target.height == 0 {
            return None;
        }
        // Mirror canvas_ui: every loft form path paints without fill.
        let loft = crate::render_pipeline::loft_form_paths(&project.document);
        let viewport = Viewport {
            pan: Vec2::ZERO,
            zoom: target.scale,
            ..Default::default()
        };
        let (_, _, rgba) = self.render_order(ScenePaint {
            project,
            order,
            viewport: &viewport,
            origin: Pos2::ZERO,
            w: target.width,
            h: target.height,
            bg: ExportBackground::Transparent,
            hidden: hidden.clone(),
            loft,
            effects: None,
        })?;
        Some(rgba)
    }
}

/// One paint request: which scene slice, how it maps to pixels, and which
/// passes run. Bundles the former 10-parameter render call so the entry
/// point reads as `render(paint)` — the review's `render(node, ctx)`
/// direction: one scene description in, one buffer out.
pub struct ScenePaint<'a> {
    pub project: &'a ProjectFile,
    pub order: &'a [NodeId],
    pub viewport: &'a Viewport,
    pub origin: Pos2,
    pub w: u32,
    pub h: u32,
    pub bg: ExportBackground,
    pub hidden: HashSet<NodeId>,
    pub loft: HashSet<NodeId>,
    pub effects: Option<&'a [NodeId]>,
}

impl PainterSession {
    /// One paint pass on the session-owned context/atlas/textures.
    pub fn render_order(&mut self, paint: ScenePaint<'_>) -> Option<(u32, u32, Vec<u8>)> {
        let ScenePaint {
            project,
            order,
            viewport,
            origin,
            w,
            h,
            bg,
            hidden,
            loft,
            effects,
        } = paint;
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32, h as f32));
        let raw = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        // Install only families the document actually uses (plus the default):
        // each install rebuilds the atlas, so system-wide installs are far too
        // slow and unnecessary. Preview resolves the same way per family.
        // egui applies staged font definitions across passes (preview gets this
        // via repaint frames); pump install passes before the render pass —
        // but only for families this context hasn't seen (later renders on a
        // reused session skip this entirely).
        let mut needed = vec![self.fonts.default_family()];
        for id in order {
            let Some(node) = project.nodes.get(*id) else {
                continue;
            };
            if let crate::document::NodeKind::Text { style, .. } = &node.kind {
                if !needed.contains(&style.font_family) {
                    needed.push(style.font_family.clone());
                }
            }
        }
        let fresh: Vec<String> = needed
            .into_iter()
            .filter(|f| !self.installed.contains(f))
            .collect();
        if !fresh.is_empty() {
            for _ in 0..3 {
                let raw = raw.clone();
                let _ = self.ctx.run_ui(raw, |ui| {
                    let ctx = ui.ctx().clone();
                    for fam in &fresh {
                        self.fonts.ensure_loaded(&ctx, fam);
                    }
                });
            }
            self.installed.extend(fresh);
        }

        // Image pixels decoded once per session (PNG/JPEG decode per frame
        // was pure waste); handles reused on the session context.
        let mut extra_textures: HashMap<TextureId, ColorImage> = HashMap::new();
        let mut image_textures: HashMap<NodeId, egui::TextureHandle> = HashMap::new();
        let output = self.ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            for id in order {
                let Some(node) = project.nodes.get(*id) else {
                    continue;
                };
                if let crate::document::NodeKind::Image { bytes, .. } = &node.kind {
                    if bytes.is_empty() {
                        continue;
                    }
                    let hit = self.decoded.contains_key(id);
                    let decoded = if hit {
                        &self.decoded[id]
                    } else {
                        let Ok(dyn_img) = image::load_from_memory(bytes) else {
                            continue;
                        };
                        self.decoded.insert(*id, dyn_img.to_rgba8());
                        &self.decoded[id]
                    };
                    let (iw, ih) = decoded.dimensions();
                    let ci = ColorImage::from_rgba_unmultiplied(
                        [iw as usize, ih as usize],
                        decoded.as_raw(),
                    );
                    let handle = match self.handles.get(id) {
                        Some(h) => h.clone(),
                        None => {
                            let handle = ctx.load_texture(
                                format!("export-img-{id}"),
                                ci.clone(),
                                egui::TextureOptions::LINEAR,
                            );
                            self.handles.insert(*id, handle.clone());
                            handle
                        }
                    };
                    extra_textures.insert(handle.id(), ci);
                    image_textures.insert(*id, handle);
                }
            }

            // Same renderer as the live canvas (hidden/loft/effects resolved by
            // the caller: full export = paint plan; segments = filtered plan;
            // preview cache = canvas hidden, base pass only).
            let painter = ctx.layer_painter(LayerId::background());
            let page_color = project.document.page_color_egui();
            crate::render::draw_nodes_ex(
                &painter,
                &project.nodes,
                order,
                viewport,
                origin,
                project.document.width as f32,
                project.document.height as f32,
                &[],
                &hidden,
                &loft,
                &self.fonts,
                &image_textures,
                page_color,
            );
            if let Some(effects) = effects {
                crate::render_pipeline::paint_document_effects(
                    &painter,
                    project,
                    Some(effects),
                    viewport,
                    origin,
                    &self.fonts,
                    &image_textures,
                );
            }
        });
        let primitives = self.ctx.tessellate(output.shapes, 1.0);

        // Font atlas produced by the same layout above.
        let atlas = self.ctx.fonts(|f| f.image());
        let page_color = project.document.page_color_egui();
        rasterize_primitives(&primitives, &atlas, &extra_textures, w, h, bg, page_color)
    }
}

/// Fill the output buffer: page color or transparent.
fn clear_buffer(w: u32, h: u32, bg: ExportBackground, page: Color32) -> Vec<u8> {
    let px = match bg {
        ExportBackground::Page => [page.r(), page.g(), page.b(), page.a()],
        ExportBackground::Transparent => [0, 0, 0, 0],
    };
    let mut buf = Vec::with_capacity((w * h) as usize * 4);
    for _ in 0..w * h {
        buf.extend_from_slice(&px);
    }
    buf
}

/// CPU-fill egui-tessellated meshes. Geometry comes from egui's own
/// tessellator; this only splats triangles (flat + font-atlas textured).
fn rasterize_primitives(
    primitives: &[egui::ClippedPrimitive],
    atlas: &ColorImage,
    extra: &HashMap<TextureId, ColorImage>,
    w: u32,
    h: u32,
    bg: ExportBackground,
    page: Color32,
) -> Option<(u32, u32, Vec<u8>)> {
    let mut buf = clear_buffer(w, h, bg, page);
    for prim in primitives {
        let (min_x, min_y, max_x, max_y) = clip_bounds(prim.clip_rect, w, h);
        if min_x >= max_x || min_y >= max_y {
            continue;
        }
        let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
            continue; // PaintCallback needs a GPU; not used by document rendering.
        };
        // Glyph-outline meshes (text_glyph.rs) carry a texture id but
        // degenerate (constant) UVs — there is no texture variation, so shade
        // flat with the vertex color. Genuine textured content (galley text,
        // images) always spans UV area.
        let mut u0 = f32::MAX;
        let mut u1 = f32::MIN;
        let mut v0 = f32::MAX;
        let mut v1 = f32::MIN;
        for v in &mesh.vertices {
            u0 = u0.min(v.uv.x);
            u1 = u1.max(v.uv.x);
            v0 = v0.min(v.uv.y);
            v1 = v1.max(v.uv.y);
        }
        let flat = (u1 - u0) < 1e-6 && (v1 - v0) < 1e-6;
        let image = if flat || mesh.texture_id == TextureId::default() {
            None
        } else if let Some(img) = extra.get(&mesh.texture_id) {
            Some(img)
        } else {
            Some(atlas)
        };
        for tri in mesh.indices.chunks_exact(3) {
            let v = [
                mesh.vertices[tri[0] as usize],
                mesh.vertices[tri[1] as usize],
                mesh.vertices[tri[2] as usize],
            ];
            fill_triangle(&mut buf, w, h, v, image, min_x, min_y, max_x, max_y);
        }
    }
    Some((w, h, buf))
}

fn clip_bounds(clip: Rect, w: u32, h: u32) -> (i32, i32, i32, i32) {
    let min_x = (clip.min.x.floor() as i32).clamp(0, w as i32);
    let min_y = (clip.min.y.floor() as i32).clamp(0, h as i32);
    let max_x = (clip.max.x.ceil() as i32).clamp(0, w as i32);
    let max_y = (clip.max.y.ceil() as i32).clamp(0, h as i32);
    (min_x, min_y, max_x, max_y)
}

#[allow(clippy::too_many_arguments)]
fn fill_triangle(
    buf: &mut [u8],
    w: u32,
    h: u32,
    v: [egui::epaint::Vertex; 3],
    image: Option<&ColorImage>,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
) {
    let p = [v[0].pos, v[1].pos, v[2].pos];
    let edge = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    // Backface-agnostic: normalize by signed area, accept either winding.
    let area = edge(p[0], p[1], p[2]);
    if area.abs() < 1e-9 {
        return;
    }
    let x0 = (p[0].x.min(p[1].x).min(p[2].x).floor() as i32).max(min_x);
    let y0 = (p[0].y.min(p[1].y).min(p[2].y).floor() as i32).max(min_y);
    let x1 = (p[0].x.max(p[1].x).max(p[2].x).ceil() as i32).min(max_x);
    let y1 = (p[0].y.max(p[1].y).max(p[2].y).ceil() as i32).min(max_y);
    for y in y0..y1 {
        for x in x0..x1 {
            let c = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(p[1], p[2], c);
            let w1 = edge(p[2], p[0], c);
            let w2 = edge(p[0], p[1], c);
            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };
            if !inside {
                continue;
            }
            let b0 = w0 / area;
            let b1 = w1 / area;
            let b2 = w2 / area;
            // Interpolated vertex color (straight sRGBA).
            let r = b0 * v[0].color.r() as f32
                + b1 * v[1].color.r() as f32
                + b2 * v[2].color.r() as f32;
            let g = b0 * v[0].color.g() as f32
                + b1 * v[1].color.g() as f32
                + b2 * v[2].color.g() as f32;
            let bl = b0 * v[0].color.b() as f32
                + b1 * v[1].color.b() as f32
                + b2 * v[2].color.b() as f32;
            let a = b0 * v[0].color.a() as f32
                + b1 * v[1].color.a() as f32
                + b2 * v[2].color.a() as f32;
            let (sr, sg, sb, sa) = match image {
                None => (r, g, bl, a),
                Some(img) => {
                    let u = b0 * v[0].uv.x + b1 * v[1].uv.x + b2 * v[2].uv.x;
                    let vv = b0 * v[0].uv.y + b1 * v[1].uv.y + b2 * v[2].uv.y;
                    let t = sample_bilinear(img, u, vv);
                    // Textured output = vertex color modulated by texel (egui convention).
                    (
                        r * t[0] / 255.0,
                        g * t[1] / 255.0,
                        bl * t[2] / 255.0,
                        a * t[3] / 255.0,
                    )
                }
            };
            blend_over(buf, w, h, x, y, sr, sg, sb, sa);
        }
    }
}

/// Bilinear sample of an sRGBA [`ColorImage`]; returns straight RGBA bytes.
fn sample_bilinear(img: &ColorImage, u: f32, v: f32) -> [f32; 4] {
    let (sw, sh) = (img.width() as f32, img.height() as f32);
    if sw < 1.0 || sh < 1.0 {
        return [255.0, 255.0, 255.0, 255.0];
    }
    let x = (u.clamp(0.0, 1.0) * (sw - 1.0)).clamp(0.0, sw - 1.0);
    let y = (v.clamp(0.0, 1.0) * (sh - 1.0)).clamp(0.0, sh - 1.0);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(img.width() - 1);
    let y1 = (y0 + 1).min(img.height() - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let px = |x: usize, y: usize| {
        let c = img[(x, y)];
        [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32]
    };
    let a = px(x0, y0);
    let b = px(x1, y0);
    let c = px(x0, y1);
    let d = px(x1, y1);
    let mut out = [0.0f32; 4];
    for i in 0..4 {
        out[i] = a[i] * (1.0 - fx) * (1.0 - fy)
            + b[i] * fx * (1.0 - fy)
            + c[i] * (1.0 - fx) * fy
            + d[i] * fx * fy;
    }
    out
}

/// Standard "over" compositing in straight sRGBA (exact for opaque source).
fn blend_over(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, r: f32, g: f32, b: f32, a: f32) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let sa = (a / 255.0).clamp(0.0, 1.0);
    if sa <= 0.0 {
        return;
    }
    let idx = ((y as u32 * w + x as u32) * 4) as usize;
    if sa >= 1.0 {
        buf[idx] = r.clamp(0.0, 255.0) as u8;
        buf[idx + 1] = g.clamp(0.0, 255.0) as u8;
        buf[idx + 2] = b.clamp(0.0, 255.0) as u8;
        buf[idx + 3] = 255;
        return;
    }
    let dr = buf[idx] as f32;
    let dg = buf[idx + 1] as f32;
    let db = buf[idx + 2] as f32;
    let da = buf[idx + 3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    buf[idx] = ((r * sa + dr * da * (1.0 - sa)) / out_a.max(1e-6)).clamp(0.0, 255.0) as u8;
    buf[idx + 1] = ((g * sa + dg * da * (1.0 - sa)) / out_a.max(1e-6)).clamp(0.0, 255.0) as u8;
    buf[idx + 2] = ((b * sa + db * da * (1.0 - sa)) / out_a.max(1e-6)).clamp(0.0, 255.0) as u8;
    buf[idx + 3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
}

// Regression tests: preview renderer ⇄ raster export consistency.
//
// These exercise the SAME `render::draw_nodes_ex` the live canvas uses
// (via headless egui):
// 1. plain text, 2. multiline/wrapped text box, 3. different fonts,
// 4. bold/italic, 5. rotated text, 6. scaled text, 7. fractional coords,
// 8. 1x DPI, 9. 2x DPI, 10. non-integer DPI scale.
//
// They assert geometry consistency (dims, determinism, ink-bbox scaling),
// not SVG contents — the export must be a different-resolution capture of
// the same painter output, not a reconstruction.

#[cfg(test)]
mod preview_export_consistency_tests {
    use super::*;
    use crate::document::{Document, Node, NodeKind, TextStyle};

    fn text_project(
        content: &str,
        x: f64,
        y: f64,
        font_size: f32,
        family: Option<&str>,
        bold: bool,
        italic: bool,
        width: f32,
        rotation_rad: f64,
        scale_xy: (f64, f64),
    ) -> ProjectFile {
        let mut project = Document::new_empty_project();
        let fam = family
            .map(|s| s.to_string())
            .unwrap_or_else(|| TextStyle::default().font_family);
        let style = TextStyle {
            content: content.into(),
            font_size,
            font_family: fam,
            bold,
            italic,
            width,
        };
        let mut node = Node::new(NodeKind::Text { x, y, style }, "t");
        node.transform.rotation_rad = rotation_rad;
        node.transform.scale = [scale_xy.0, scale_xy.1];
        let id = node.id;
        project.nodes.insert(node);
        project.document.append_to_active_layer(id);
        project
    }

    fn plain_project() -> ProjectFile {
        text_project(
            "Hello",
            50.0,
            60.0,
            24.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        )
    }

    fn render(project: &ProjectFile, scale: f32) -> (u32, u32, Vec<u8>) {
        render_document_rgba(project, scale).expect("render ok")
    }

    /// Ink bounding box vs the corner-pixel background.
    fn ink_bbox(w: u32, h: u32, rgba: &[u8]) -> Option<(u32, u32, u32, u32)> {
        let bg = [rgba[0] as i32, rgba[1] as i32, rgba[2] as i32];
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut found = false;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let d = (rgba[i] as i32 - bg[0]).abs()
                    + (rgba[i + 1] as i32 - bg[1]).abs()
                    + (rgba[i + 2] as i32 - bg[2]).abs();
                if d > 36 {
                    found = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        found.then_some((min_x, min_y, max_x, max_y))
    }

    fn ink_count(w: u32, h: u32, rgba: &[u8]) -> u64 {
        let bg = [rgba[0] as i32, rgba[1] as i32, rgba[2] as i32];
        let mut n = 0u64;
        for i in (0..(w * h) as usize).map(|p| p * 4) {
            let d = (rgba[i] as i32 - bg[0]).abs()
                + (rgba[i + 1] as i32 - bg[1]).abs()
                + (rgba[i + 2] as i32 - bg[2]).abs();
            if d > 36 {
                n += 1;
            }
        }
        n
    }

    /// 1. Plain text renders ink at 1x with exact output dims.

    #[test]
    fn triangle_filler_splats_pixels() {
        let mut buf = vec![0u8; 10 * 10 * 4];
        let v = [
            egui::epaint::Vertex {
                pos: Pos2::new(1.0, 1.0),
                uv: Pos2::ZERO,
                color: Color32::from_rgb(200, 10, 10),
            },
            egui::epaint::Vertex {
                pos: Pos2::new(8.0, 1.0),
                uv: Pos2::ZERO,
                color: Color32::from_rgb(200, 10, 10),
            },
            egui::epaint::Vertex {
                pos: Pos2::new(1.0, 8.0),
                uv: Pos2::ZERO,
                color: Color32::from_rgb(200, 10, 10),
            },
        ];
        fill_triangle(&mut buf, 10, 10, v, None, 0, 0, 10, 10);
        let painted = buf.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(painted > 10, "filler must paint, got {painted}");
    }

    #[test]
    fn plain_text_renders_with_exact_dims() {
        let p = plain_project();
        let (w, h, rgba) = render(&p, 1.0);
        assert_eq!((w, h), (p.document.width as u32, p.document.height as u32));
        assert!(ink_count(w, h, &rgba) > 50, "text must leave ink");
    }

    /// 8/9/10. Output dims follow scale; rendering is deterministic.
    #[test]
    fn dims_follow_scale_and_render_is_deterministic() {
        let p = plain_project();
        for scale in [1.0f32, 2.0, 1.5] {
            let (w, h, rgba) = render(&p, scale);
            assert_eq!(w, (p.document.width as f32 * scale).round() as u32);
            assert_eq!(h, (p.document.height as f32 * scale).round() as u32);
            let (_, _, rgba2) = render(&p, scale);
            assert_eq!(rgba, rgba2, "same doc+scale must be byte-identical");
        }
    }

    /// 9. 2x ink bbox ≈ 2× the 1x bbox: same scene, higher density.
    #[test]
    fn hires_bbox_scales_linearly() {
        let p = plain_project();
        let (w1, h1, r1) = render(&p, 1.0);
        let (w2, h2, r2) = render(&p, 2.0);
        let b1 = ink_bbox(w1, h1, &r1).expect("ink at 1x");
        let b2 = ink_bbox(w2, h2, &r2).expect("ink at 2x");
        for (a, b) in [(b1.0, b2.0), (b1.1, b2.1), (b1.2, b2.2), (b1.3, b2.3)] {
            assert!(
                ((b as f32) - (a as f32) * 2.0).abs() <= 3.0,
                "bbox must scale 2x: {b1:?} vs {b2:?}"
            );
        }
    }

    /// 2. Wrapped text box grows vertically vs auto-width single line.
    #[test]
    fn wrapped_text_box_wraps() {
        let long = "word ".repeat(40);
        let auto_p = text_project(
            &long,
            20.0,
            20.0,
            20.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let wrap_p = text_project(
            &long,
            20.0,
            20.0,
            20.0,
            None,
            false,
            false,
            200.0,
            0.0,
            (1.0, 1.0),
        );
        let (w1, h1, r1) = render(&auto_p, 1.0);
        let (w2, h2, r2) = render(&wrap_p, 1.0);
        let ha = ink_bbox(w1, h1, &r1).map(|b| b.3 - b.1).unwrap_or(0);
        let hw = ink_bbox(w2, h2, &r2).map(|b| b.3 - b.1).unwrap_or(0);
        assert!(hw > ha, "wrapped box must be taller: {hw} vs {ha}");
    }

    /// 3. Different font family still renders (falls back gracefully if missing).
    #[test]
    fn other_family_renders() {
        let mut fonts = FontRegistry::new();
        let fam = fonts
            .families()
            .iter()
            .find(|f| *f != &fonts.default_family())
            .cloned()
            .unwrap_or_else(|| fonts.default_family());
        let p = text_project(
            "Ag",
            50.0,
            60.0,
            24.0,
            Some(&fam),
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let (w, h, rgba) = render(&p, 1.0);
        assert!(ink_count(w, h, &rgba) > 20, "family {fam} must render");
    }

    /// 4. Bold/italic render ink without panic.
    #[test]
    fn bold_italic_render_ink() {
        let reg = text_project(
            "Bold?",
            50.0,
            60.0,
            28.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let bold = text_project(
            "Bold?",
            50.0,
            60.0,
            28.0,
            None,
            true,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let it = text_project(
            "Bold?",
            50.0,
            60.0,
            28.0,
            None,
            false,
            true,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let (w, h, r_reg) = render(&reg, 1.0);
        let (_, _, r_bold) = render(&bold, 1.0);
        let (_, _, r_it) = render(&it, 1.0);
        assert!(ink_count(w, h, &r_reg) > 20);
        assert!(ink_count(w, h, &r_bold) > 20);
        assert!(ink_count(w, h, &r_it) > 20);
        if r_bold != r_reg {
            assert!(ink_count(w, h, &r_bold) >= ink_count(w, h, &r_reg));
        }
    }

    /// 5. Rotated text keeps its ink (moves it) without panic.
    #[test]
    fn rotated_text_keeps_ink() {
        let p = text_project(
            "Rotate me",
            200.0,
            200.0,
            28.0,
            None,
            false,
            false,
            0.0,
            std::f64::consts::FRAC_PI_6,
            (1.0, 1.0),
        );
        let (w, h, rgba) = render(&p, 1.0);
        let n = ink_count(w, h, &rgba);
        assert!(n > 50, "rotated text must leave ink, got {n}");
        let flat = text_project(
            "Rotate me",
            200.0,
            200.0,
            28.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let (_, _, r_flat) = render(&flat, 1.0);
        assert_ne!(rgba, r_flat, "rotation must move pixels");
    }

    /// 6. Node `transform.scale` is ignored for text by the preview renderer
    /// (glyph size comes from `font_size`); export must match that behavior
    /// exactly rather than inventing its own scaling.
    #[test]
    fn scaled_transform_matches_preview_behavior() {
        let p1 = text_project(
            "Big",
            100.0,
            100.0,
            24.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        let p2 = text_project(
            "Big",
            100.0,
            100.0,
            24.0,
            None,
            false,
            false,
            0.0,
            0.0,
            (2.0, 2.0),
        );
        let (w, h, r1) = render(&p1, 1.0);
        let b1 = ink_bbox(w, h, &r1).expect("ink");
        let (_, _, r2) = render(&p2, 1.0);
        let b2 = ink_bbox(w, h, &r2).expect("ink scaled");
        assert_eq!(
            b1, b2,
            "transform.scale must behave identically: {b1:?} vs {b2:?}"
        );
    }

    /// 7. Fractional coordinates render without panic and leave ink.
    #[test]
    fn fractional_coords_render() {
        let p = text_project(
            "Frac",
            50.33,
            60.67,
            23.5,
            None,
            false,
            false,
            0.0,
            0.0,
            (1.0, 1.0),
        );
        for scale in [1.0f32, 1.5] {
            let (w, h, rgba) = render(&p, scale);
            assert!(ink_count(w, h, &rgba) > 20, "ink at scale {scale}");
        }
    }

    /// Selection render matches the same crop of the full render.
    /// Uses integer bounds so the crop aligns exactly (fractional bounds can
    /// shift coverage by a sub-pixel between the translated render and the
    /// integer crop grid — same renderer either way).
    #[test]
    fn selection_matches_full_render_crop() {
        let p = plain_project();
        let (fw, _fh, full) = render_document_rgba(&p, 1.0).unwrap();
        let ids: Vec<_> = p.document.layers[0].nodes.clone();
        let bounds = kurbo::Rect::new(40.0, 50.0, 220.0, 110.0);
        let (sw, sh, sel) = render_selection_rgba(&p, &ids, bounds, 1.0).unwrap();
        assert_eq!(sw, bounds.width().round() as u32);
        assert_eq!(sh, bounds.height().round() as u32);
        // Pixel-compare the crop region of the full render, restricted to
        // selection ink (selection exports stay transparent; full-doc has
        // page background — backgrounds intentionally differ).
        let x0 = bounds.x0 as u32;
        let y0 = bounds.y0 as u32;
        let mut diff = 0u64;
        let mut ink = 0u64;
        for y in 0..sh {
            for x in 0..sw {
                let si = ((y * sw + x) * 4) as usize;
                if sel[si + 3] < 10 {
                    continue;
                }
                ink += 1;
                let fi = (((y0 + y) * fw + (x0 + x)) * 4) as usize;
                for c in 0..4 {
                    diff += (sel[si + c] as i32 - full[fi + c] as i32).unsigned_abs() as u64;
                }
            }
        }
        assert!(ink > 50, "selection must contain ink");
        let per_px = diff as f64 / (ink as f64 * 4.0);
        assert!(per_px < 1.0, "selection must match full crop, got {per_px}");
    }

    /// Layer-base primitive (preview cache) composited over the page
    /// background must reproduce the full-document render: `over` is
    /// associative, so base-over-transparent-over-bg == paint-over-bg.
    #[test]
    fn layer_base_matches_full_render_over_background() {
        let p = plain_project();
        let (w, h, full) = render_document_rgba(&p, 1.0).unwrap();
        let target = crate::render_pipeline::RenderTarget {
            width: w,
            height: h,
            scale: 1.0,
        };
        let order: Vec<_> = p.document.layers[0].nodes.clone();
        let base =
            render_layer_base_rgba(&p, &order, &std::collections::HashSet::new(), &target).unwrap();
        // Corner pixel is untouched page background (text lives at 50,60).
        let bg = [full[0] as f32, full[1] as f32, full[2] as f32];
        let mut worst = 0i32;
        for i in (0..(w * h) as usize).map(|px| px * 4) {
            let sa = base[i + 3] as f32 / 255.0;
            for c in 0..3 {
                let expect = base[i + c] as f32 * sa + bg[c] * (1.0 - sa);
                worst = worst.max((expect.round() as i32 - full[i + c] as i32).abs());
            }
        }
        assert!(worst <= 2, "cache base must match export, worst {worst}");
    }

    /// Group parity with preview: children live in parent-local space and are
    /// hidden from the base pass (the parent paints them). Export must show
    /// ink at the transformed location and NONE at the children's local
    /// coords (previously double-painted both).
    #[test]
    fn grouped_children_paint_once_at_parent_transform() {
        use crate::document::{Fill, Node, Paint};

        let mut project = Document::new_empty_project();
        let red = Fill::Solid(Paint::from_hex(0xc0392b, 1.0));
        let c1 = Node::rect(10.0, 10.0, 30.0, 30.0, red.clone());
        let c1_id = c1.id;
        project.nodes.insert(c1);
        let c2 = Node::rect(50.0, 60.0, 30.0, 30.0, red);
        let c2_id = c2.id;
        project.nodes.insert(c2);
        let mut group = Node::group(vec![c1_id, c2_id], "g");
        group.transform.translation = [400.0, 300.0];
        let gid = group.id;
        project.nodes.insert(group);
        // Real layout keeps children in the layer list; canvas hides them.
        project.document.append_to_active_layer(c1_id);
        project.document.append_to_active_layer(c2_id);
        project.document.append_to_active_layer(gid);

        // Structural: children hidden, painted via parent only.
        let hidden = crate::render_pipeline::hidden_effect_sources(&project);
        assert!(hidden.contains(&c1_id) && hidden.contains(&c2_id));
        assert!(!hidden.contains(&gid));

        let (w, h, rgba) = render(&project, 1.0);
        let ink_in = |x0: u32, y0: u32, x1: u32, y1: u32| -> u64 {
            let bg = [rgba[0] as i32, rgba[1] as i32, rgba[2] as i32];
            let mut n = 0u64;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    let d = (rgba[i] as i32 - bg[0]).abs()
                        + (rgba[i + 1] as i32 - bg[1]).abs()
                        + (rgba[i + 2] as i32 - bg[2]).abs();
                    if d > 36 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(
            ink_in(405, 305, 490, 395) > 200,
            "group must paint children at parent transform"
        );
        assert_eq!(
            ink_in(0, 0, 100, 110),
            0,
            "children local coords must stay clean (were double-painted)"
        );
    }

    /// Clip effect: export must apply the same solid-face clip as preview.
    /// A 200px red image clipped to a 60px mask rect must leave ink inside
    /// the mask and page background outside it (where the unclipped image
    /// would otherwise paint).
    #[test]
    fn clip_effect_confines_export_ink_to_mask() {
        use crate::document::{ClipMaskEffect, Fill, Node, Paint};
        use image::ImageEncoder;

        // Solid red 64x64 PNG.
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([220, 30, 30, 255]));
        let mut png_bytes = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            let enc = image::codecs::png::PngEncoder::new(&mut cursor);
            enc.write_image(img.as_raw(), 64, 64, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let mut project = Document::new_empty_project();
        let src = Node::image(50.0, 50.0, 200.0, 200.0, png_bytes);
        let src_id = src.id;
        project.nodes.insert(src);
        let mask = Node::rect(
            100.0,
            100.0,
            60.0,
            60.0,
            Fill::Solid(Paint::from_hex(0x000000, 1.0)),
        );
        let mask_id = mask.id;
        project.nodes.insert(mask);
        project.document.append_to_active_layer(src_id);
        project.document.append_to_active_layer(mask_id);
        let effect = ClipMaskEffect {
            id: uuid::Uuid::new_v4(),
            source_id: src_id,
            mask_id,
            hide_mask: true,
        };
        project.document.clip_masks.insert(effect.id, effect);

        let (w, h, rgba) = render(&project, 1.0);
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        let bg = px(5, 5);
        // Center of mask: must show the red image.
        let inside = px(130, 130);
        let inside_diff = (inside[0] as i32 - bg[0] as i32).abs()
            + (inside[1] as i32 - bg[1] as i32).abs()
            + (inside[2] as i32 - bg[2] as i32).abs();
        assert!(
            inside_diff > 60,
            "mask interior must show image, got {inside:?} vs bg {bg:?}"
        );
        // Inside image but outside mask: must be page background (clipped away).
        let outside = px(60, 60);
        let outside_diff = (outside[0] as i32 - bg[0] as i32).abs()
            + (outside[1] as i32 - bg[1] as i32).abs()
            + (outside[2] as i32 - bg[2] as i32).abs();
        assert!(
            outside_diff < 24,
            "clipped-away region must match background, got {outside:?} vs bg {bg:?}"
        );
        let _ = h;
    }

    /// Session reuse must not change output: a persistent session renders
    /// byte-identical bytes to one-shot renders, and stays stable across
    /// interleaved scales (fonts/atlas/images persist, shapes re-record).
    #[test]
    fn session_reuse_matches_oneshot_renders() {
        let p = plain_project();
        let (w1, h1, r1) = render(&p, 1.0);
        let mut session = PainterSession::new();
        let (w2, h2, r2) = session.render_document(&p, 1.0).unwrap();
        assert_eq!((w1, h1), (w2, h2));
        assert_eq!(r1, r2, "first session render must equal one-shot");
        let _ = session.render_document(&p, 2.0).unwrap();
        let (_, _, r4) = session.render_document(&p, 1.0).unwrap();
        assert_eq!(r2, r4, "session must be stable across interleaved scales");
    }

    /// Session image cache: decode-once + handle reuse must match one-shot
    /// output (exercises the persisted-atlas/texture path with real pixels).
    #[test]
    fn session_image_renders_match_oneshot() {
        use crate::document::Node;
        use image::ImageEncoder;

        let img = image::RgbaImage::from_pixel(32, 32, image::Rgba([30, 120, 220, 255]));
        let mut png_bytes = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            let enc = image::codecs::png::PngEncoder::new(&mut cursor);
            enc.write_image(img.as_raw(), 32, 32, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let mut project = Document::new_empty_project();
        let node = Node::image(60.0, 60.0, 120.0, 120.0, png_bytes);
        let id = node.id;
        project.nodes.insert(node);
        project.document.append_to_active_layer(id);

        let (w, h, r1) = render(&project, 1.0);
        let mut session = PainterSession::new();
        let (_, _, r2) = session.render_document(&project, 1.0).unwrap();
        assert_eq!(r1, r2, "session image render must equal one-shot");
        let (_, _, r3) = session.render_document(&project, 1.0).unwrap();
        assert_eq!(r2, r3, "cached-handle re-render must be stable");
        assert!(ink_count(w, h, &r2) > 100, "image must leave ink");
    }
}
