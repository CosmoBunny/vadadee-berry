use eframe::egui;
use egui::{Context, Event, Key, Pos2, Sense, Ui};
use kurbo::Shape;
use crate::animation::UiAnimation;
use crate::canvas::Viewport;
use crate::fonts::FontRegistry;
use crate::document::{
    default_gradient_stops, default_loft_gap_for_node, effect_placements, find_effect_for_pair,
    loft_sweep_node,
    has_effect_for_objects, hidden_effect_sources, node_at_placement, BezierHandleMode, Document,
    FaceRenderable, Fill, FillKind,
    GradientStop, Node, NodeId, NodeKind, ObjectOnPathEffect, OnPathMode, Paint, PathData, PathMagic, PathPlacement, Tiling, TilingEffect, CircularClone, CircularCloneEffect,
    PathEditTarget, ProjectFile, Stroke, TextStyle, text_display_name,
};
use crate::history::{snapshot_document, snapshot_project, History, ProjectEdit};
use crate::io;
use crate::render;
use crate::theme;
use crate::tools::{self, DragNewShape, MarqueeSelect, SelectDrag, ToolKind, ToolState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GradientFlowTarget {
    Fill,
    Stroke,
}

#[derive(Debug, Clone, Copy)]
struct GradientFlowDrag {
    target: GradientFlowTarget,
    handle: crate::gradient_ui::GradientLineHandle,
    line_at_press: (f32, f32, f32, f32),
    doc_at_press: (f64, f64),
}
use crate::ui;

#[derive(Debug, Clone)]
struct ImagePastePlacement {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug)]
enum PasteTask {
    SystemImage {
        step: u8,
        rgba: Option<image::RgbaImage>,
        png: Option<Vec<u8>>,
        placement: Option<ImagePastePlacement>,
    },
    Objects {
        nodes: Vec<Node>,
        offset: (f64, f64),
        index: usize,
        new_sel: Vec<NodeId>,
    },
}

#[derive(Debug)]
struct PasteProgress {
    label: String,
    task: PasteTask,
}

pub struct VadadeeBerryApp {
    pub project: ProjectFile,
    pub viewport: Viewport,
    pub tools: ToolState,
    pub selection: Vec<NodeId>,
    pub history: History,
    pub ui_fill_stops: Vec<GradientStop>,
    pub ui_fill_stop_sel: usize,
    pub ui_fill_edit_gradient_line: bool,
    pub ui_fill_kind: FillKind,
    pub ui_gradient_angle: f32,
    pub ui_fill_line_x0: f32,
    pub ui_fill_line_y0: f32,
    pub ui_fill_line_x1: f32,
    pub ui_fill_line_y1: f32,
    pub ui_radial_cx: f32,
    pub ui_radial_cy: f32,
    pub polygon_sides: u32,
    pub ui_stroke_stops: Vec<GradientStop>,
    pub ui_stroke_stop_sel: usize,
    pub ui_stroke_edit_gradient_line: bool,
    pub ui_stroke_line_join: crate::document::LineJoin,
    pub ui_stroke_line_cap: crate::document::LineCap,
    pub ui_stroke_kind: FillKind,
    pub ui_stroke_angle: f32,
    pub ui_stroke_line_x0: f32,
    pub ui_stroke_line_y0: f32,
    pub ui_stroke_line_x1: f32,
    pub ui_stroke_line_y1: f32,
    pub ui_stroke_radial_cx: f32,
    pub ui_stroke_radial_cy: f32,
    pub ui_stroke_width: f32,
    pub ui_text_content: String,
    pub ui_text_font_size: f32,
    pub ui_text_font_family: String,
    pub fonts: FontRegistry,
    pub ui_text_bold: bool,
    pub ui_text_italic: bool,
    pub fill_enabled: bool,
    pub stroke_enabled: bool,
    pub status_message: String,
    clipboard: Vec<Node>,
    /// After tab promote-to-front, animate scroll strip back to the first tab.
    pub action_tab_scroll_home: bool,
    /// Inline text editor over the canvas (no Geometry tab required).
    pub on_page_text_edit: Option<NodeId>,
    pub(crate) on_page_text_focus_pending: bool,
    on_page_text_before: Option<Node>,
    on_page_text_newly_created: bool,
    pub cursor_doc: Option<(f64, f64)>,
    pub action_bar_open: bool,
    pub action_bar_width: f32,
    pub action_tab: ui::ActionTab,
    pub action_tab_order: Vec<ui::ActionTab>,
    /// Object-on-path effect editor (Path Magic tab).
    pub ui_on_path_mode: OnPathMode,
    pub ui_on_path_gap: f64,
    pub ui_on_path_count: usize,
    pub ui_on_path_cyclic: bool,
    pub ui_on_path_rotate: bool,
    pub ui_on_path_loft_scale: f32,
    pub ui_on_path_loft_opacity: f32,
    /// Measured height of the Object on Path panel (drives expand animation).
    pub ui_on_path_container_h: f32,
    // Tiling params (2D)
    pub ui_tiling_rows: usize,
    pub ui_tiling_cols: usize,
    pub ui_tiling_offset_x: f64,
    pub ui_tiling_offset_y: f64,
    pub ui_tiling_row_rot: f64,
    pub ui_tiling_col_rot: f64,
    pub ui_tiling_row_scale: f64,
    pub ui_tiling_col_scale: f64,
    pub ui_tiling_gap_x: f64,
    pub ui_tiling_gap_y: f64,
    // CircularClone params
    pub ui_circular_copies: usize,
    pub ui_circular_angle_offset: f64,
    pub ui_circular_origin_x: f64,
    pub ui_circular_origin_y: f64,
    pub ui_anim: UiAnimation,
    pub gradient_editor_focus: crate::gradient_ui::GradientEditorFocus,
    /// Cached textures for Image nodes (keyed by NodeId). Reloaded from .bytes on demand.
    image_textures: std::collections::HashMap<NodeId, egui::TextureHandle>,
    gradient_flow_drag: Option<GradientFlowDrag>,
    canvas_screen_rect: Option<egui::Rect>,
    canvas_origin: Pos2,
    pending_open_svg: bool,
    pending_save_project: bool,
    pending_export_svg: bool,
    /// Tracks Ctrl+V for paste fallback when egui-winit swallows the hotkey (image-only clipboard).
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    paste_hotkey_was_down: bool,
    /// Multi-frame paste shown on the 2nd status-bar label ("Pasting…").
    paste_progress: Option<PasteProgress>,
    pub toolbar_expanded: bool,
    pub toolbar_drag_active: bool,
    pub text_editor_rect: Option<egui::Rect>,
    pub last_android_text: String,
    pub path_overlay_rect: Option<egui::Rect>,
}

impl VadadeeBerryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let fonts = FontRegistry::new();
        let default_font = fonts.default_family();
        Self {
            project: Document::new_default_project(),
            viewport: Viewport::default(),
            tools: ToolState {
                active: ToolKind::Select,
                ..Default::default()
            },
            selection: vec![],
            history: History::default(),
            ui_fill_stops: default_gradient_stops(),
            ui_fill_stop_sel: 0,
            ui_fill_edit_gradient_line: false,
            ui_fill_kind: FillKind::Solid,
            ui_gradient_angle: 90.0,
            ui_fill_line_x0: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.0
            },
            ui_fill_line_y0: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.1
            },
            ui_fill_line_x1: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.2
            },
            ui_fill_line_y1: {
                let l = crate::document::linear_line_spanning_bbox(90.0);
                l.3
            },
            ui_radial_cx: 0.5,
            ui_radial_cy: 0.5,
            polygon_sides: 6,
            ui_stroke_stops: vec![
                GradientStop::new(0.0, Paint::from_hex(0x1a1f2e, 1.0)),
                GradientStop::new(1.0, Paint::from_hex(0x1a1f2e, 1.0)),
            ],
            ui_stroke_stop_sel: 0,
            ui_stroke_edit_gradient_line: false,
            ui_stroke_line_join: crate::document::LineJoin::Miter,
            ui_stroke_line_cap: crate::document::LineCap::Butt,
            ui_stroke_kind: FillKind::Solid,
            ui_stroke_angle: 0.0,
            ui_stroke_line_x0: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.0
            },
            ui_stroke_line_y0: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.1
            },
            ui_stroke_line_x1: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.2
            },
            ui_stroke_line_y1: {
                let l = crate::document::linear_line_spanning_bbox(0.0);
                l.3
            },
            ui_stroke_radial_cx: 0.5,
            ui_stroke_radial_cy: 0.5,
            ui_stroke_width: 2.0,
            ui_text_content: "Text".into(),
            ui_text_font_size: 24.0,
            ui_text_font_family: default_font,
            fonts,
            ui_text_bold: false,
            ui_text_italic: false,
            fill_enabled: true,
            stroke_enabled: true,
            status_message: "Idle".into(),
            clipboard: Vec::new(),
            action_tab_scroll_home: false,
            on_page_text_edit: None,
            on_page_text_focus_pending: false,
            on_page_text_before: None,
            on_page_text_newly_created: false,
            image_textures: std::collections::HashMap::new(),
            cursor_doc: None,
            action_bar_open: true,
            action_bar_width: 300.0,
            action_tab: ui::ActionTab::default(),
            action_tab_order: ui::ActionTab::all_tabs(),
            ui_on_path_mode: OnPathMode::GapDuplicate,
            ui_on_path_gap: 48.0,
            ui_on_path_count: 5,
            ui_on_path_cyclic: true,
            ui_on_path_rotate: true,
            ui_on_path_loft_scale: 1.0,
            ui_on_path_loft_opacity: 0.75,
            ui_on_path_container_h: 280.0,
            ui_tiling_rows: 3,
            ui_tiling_cols: 3,
            ui_tiling_offset_x: 0.0,
            ui_tiling_offset_y: 0.0,
            ui_tiling_row_rot: 0.0,
            ui_tiling_col_rot: 0.0,
            ui_tiling_row_scale: 0.0,
            ui_tiling_col_scale: 0.0,
            ui_tiling_gap_x: 48.0,
            ui_tiling_gap_y: 48.0,
            ui_circular_copies: 6,
            ui_circular_angle_offset: 0.0,
            ui_circular_origin_x: 0.0,
            ui_circular_origin_y: 0.0,
            ui_anim: {
                let mut anim = UiAnimation::new();
                anim.seed_status_board("Idle", 80.0, 56.0);
                anim
            },
            gradient_editor_focus: crate::gradient_ui::GradientEditorFocus::None,
            gradient_flow_drag: None,
            canvas_screen_rect: None,
            canvas_origin: Pos2::ZERO,
            pending_open_svg: false,
            pending_save_project: false,
            pending_export_svg: false,
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
            paste_hotkey_was_down: false,
            paste_progress: None,
            toolbar_expanded: false,
            toolbar_drag_active: false,
            text_editor_rect: None,
            last_android_text: String::new(),
            path_overlay_rect: None,
        }
    }

    pub fn new_document(&mut self) {
        let before = snapshot_project(&self.project);
        let after = Document::new_empty_project();
        self.history.push(
            &mut self.project,
            ProjectEdit::SetDocument { before, after },
        );
        self.selection.clear();
        self.viewport.pan = egui::vec2(48.0, 48.0);
        self.viewport.zoom = 0.85;
        self.status_message = "New A4 document".into();
        self.ui_anim.replay_intro();
    }

    pub fn request_open_svg(&mut self) {
        self.pending_open_svg = true;
    }

    pub fn request_import_image(&mut self) {
        #[cfg(target_os = "android")]
        {
            self.status_message = "Image import from files is not available on Android yet".into();
            return;
        }
        #[cfg(not(target_os = "android"))]
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            if let Ok(bytes) = std::fs::read(&path) {
                // Place near view "center" (rough, user can drag)
                let cx = 200.0;
                let cy = 150.0;
                let w = 320.0;
                let h = 240.0;
                self.insert_image(cx - w / 2.0, cy - h / 2.0, w, h, bytes);
            }
        }
    }
    pub fn request_save_project(&mut self) {
        self.pending_save_project = true;
    }
    pub fn request_export_svg(&mut self) {
        self.pending_export_svg = true;
    }

    pub fn do_undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.selection.clear();
            self.clear_transient_tool_state();
            self.status_message = "Undo".into();
            self.sync_inspector_from_selection();
        }
    }

    pub fn do_redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.selection.clear();
            self.clear_transient_tool_state();
            self.status_message = "Redo".into();
            self.sync_inspector_from_selection();
        }
    }

    fn clear_transient_tool_state(&mut self) {
        self.tools.drag_shape = None;
        self.tools.select.drag_mode = None;
        self.tools.select.marquee = None;
        self.tools.select.drag_snapshot.clear();
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
        self.tools.canvas_pan_drag = false;
        self.dismiss_on_page_text_edit_without_history();
    }

    /// Drop on-page editor without pushing undo history (e.g. after undo/redo).
    fn dismiss_on_page_text_edit_without_history(&mut self) {
        self.on_page_text_edit = None;
        self.on_page_text_before = None;
        self.on_page_text_focus_pending = false;
        self.on_page_text_newly_created = false;
    }

    pub fn set_selection(&mut self, ids: Vec<NodeId>) {
        self.selection = ids;
        self.gradient_editor_focus = crate::gradient_ui::GradientEditorFocus::None;
        self.sync_inspector_from_selection();
    }

    pub fn try_delete_focused_gradient_stop(&mut self) -> bool {
        use crate::document::normalize_stops;
        use crate::gradient_ui::GradientEditorFocus;
        if self.action_tab != ui::ActionTab::ColorStroke {
            return false;
        }
        match self.gradient_editor_focus {
            GradientEditorFocus::Fill if self.ui_fill_stops.len() > 2 => {
                let i = self
                    .ui_fill_stop_sel
                    .min(self.ui_fill_stops.len().saturating_sub(1));
                self.ui_fill_stops.remove(i);
                normalize_stops(&mut self.ui_fill_stops);
                self.ui_fill_stop_sel = self
                    .ui_fill_stop_sel
                    .min(self.ui_fill_stops.len().saturating_sub(1));
                self.apply_fill_to_selection();
                true
            }
            GradientEditorFocus::Stroke if self.ui_stroke_stops.len() > 2 => {
                let i = self
                    .ui_stroke_stop_sel
                    .min(self.ui_stroke_stops.len().saturating_sub(1));
                self.ui_stroke_stops.remove(i);
                normalize_stops(&mut self.ui_stroke_stops);
                self.ui_stroke_stop_sel = self
                    .ui_stroke_stop_sel
                    .min(self.ui_stroke_stops.len().saturating_sub(1));
                self.apply_stroke_to_selection();
                true
            }
            _ => false,
        }
    }

    fn sync_inspector_from_selection(&mut self) {
        if let Some(id) = self.selection.first() {
            if let Some(n) = self.project.nodes.get(*id) {
                if !matches!(n.kind, NodeKind::Path { .. }) {
                    self.tools.select.clear_path_point_selection();
                }
                self.ui_fill_stops = n.style.fill.stops();
                self.ui_fill_stop_sel = 0;
                self.ui_fill_kind = n.style.fill.kind();
                self.ui_gradient_angle = n.style.fill.linear_angle_deg();
                let (lx0, ly0, lx1, ly1) = n.style.fill.linear_line();
                self.ui_fill_line_x0 = lx0;
                self.ui_fill_line_y0 = ly0;
                self.ui_fill_line_x1 = lx1;
                self.ui_fill_line_y1 = ly1;
                if n.style.fill.kind() == FillKind::LinearGradient {
                    let line_angle =
                        crate::document::linear_angle_from_line(lx0, ly0, lx1, ly1);
                    let len = (lx1 - lx0).hypot(ly1 - ly0);
                    if len < 0.2
                        || (line_angle - self.ui_gradient_angle).abs() > 2.0
                            && (lx0 - 0.5).hypot(ly0 - 0.5) < 0.05
                    {
                        let span =
                            crate::document::linear_line_spanning_bbox(self.ui_gradient_angle);
                        self.ui_fill_line_x0 = span.0;
                        self.ui_fill_line_y0 = span.1;
                        self.ui_fill_line_x1 = span.2;
                        self.ui_fill_line_y1 = span.3;
                    }
                }
                let (rcx, rcy) = n.style.fill.radial_center();
                self.ui_radial_cx = rcx;
                self.ui_radial_cy = rcy;
                self.ui_stroke_stops = n.style.stroke.style.stops();
                self.ui_stroke_stop_sel = 0;
                self.ui_stroke_kind = n.style.stroke.style.kind();
                self.ui_stroke_angle = n.style.stroke.style.linear_angle_deg();
                let (sx0, sy0, sx1, sy1) = n.style.stroke.style.linear_line();
                self.ui_stroke_line_x0 = sx0;
                self.ui_stroke_line_y0 = sy0;
                self.ui_stroke_line_x1 = sx1;
                self.ui_stroke_line_y1 = sy1;
                if n.style.stroke.style.kind() == FillKind::LinearGradient {
                    let line_angle =
                        crate::document::linear_angle_from_line(sx0, sy0, sx1, sy1);
                    let len = (sx1 - sx0).hypot(sy1 - sy0);
                    if len < 0.2
                        || (line_angle - self.ui_stroke_angle).abs() > 2.0
                            && (sx0 - 0.5).hypot(sy0 - 0.5) < 0.05
                    {
                        let span =
                            crate::document::linear_line_spanning_bbox(self.ui_stroke_angle);
                        self.ui_stroke_line_x0 = span.0;
                        self.ui_stroke_line_y0 = span.1;
                        self.ui_stroke_line_x1 = span.2;
                        self.ui_stroke_line_y1 = span.3;
                    }
                }
                let (scx, scy) = n.style.stroke.style.radial_center();
                self.ui_stroke_radial_cx = scx;
                self.ui_stroke_radial_cy = scy;
                self.ui_stroke_width = n.style.stroke.width;
                self.ui_stroke_line_join = n.style.stroke.line_join;
                self.ui_stroke_line_cap = n.style.stroke.line_cap;
                self.fill_enabled = n.style.fill.is_visible();
                if let NodeKind::Polygon { sides, .. } = &n.kind {
                    self.polygon_sides = *sides;
                }
                if let NodeKind::Text { style, .. } = &n.kind {
                    self.ui_text_content = style.content.clone();
                    self.ui_text_font_size = style.font_size;
                    self.ui_text_font_family = style.font_family.clone();
                    self.ui_text_bold = style.bold;
                    self.ui_text_italic = style.italic;
                }
                self.stroke_enabled = n.style.stroke.width > 0.01;
            }
        }
        self.sync_on_path_ui_from_selection();
    }

    pub fn inspector_opacity(&self) -> f32 {
        self.selection
            .first()
            .and_then(|id| self.project.nodes.get(*id))
            .map(|n| n.style.opacity)
            .unwrap_or(1.0)
    }

    pub fn apply_fill_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.fill = self.build_ui_fill();
            if let NodeKind::Path { path } = &mut after.kind {
                if self.fill_enabled && !path.is_closed() && path.points.len() >= 3 {
                    path.set_closed(true);
                }
            }
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn reverse_path(&mut self, id: NodeId) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.reverse();
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = "Reversed path".into();
        }
    }

    pub fn set_all_path_anchors_smooth(&mut self, id: NodeId, smooth: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_all_anchors_smooth(smooth);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = if smooth {
                "Smoothed all corners".into()
            } else {
                "Sharpened all corners".into()
            };
        }
    }

    pub fn simplify_path(&mut self, id: NodeId) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.simplify_collinear(0.5);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.status_message = "Simplified path".into();
        }
    }

    pub fn set_path_closed(&mut self, id: NodeId, closed: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_closed(closed);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_circle_geometry(&mut self, id: NodeId, cx: f64, cy: f64, radius: f64) {
        self.set_ellipse_geometry(id, cx, cy, radius.max(0.5), radius.max(0.5));
    }

    pub fn set_polygon_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        r: f64,
        sides: u32,
        rotation_deg: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Polygon {
            cx: pcx,
            cy: pcy,
            r: pr,
            sides: ps,
            rotation_rad,
        } = &mut after.kind
        {
            *pcx = cx;
            *pcy = cy;
            *pr = r.max(1.0);
            *ps = sides.max(3);
            *rotation_rad = rotation_deg.to_radians();
            after.name = format!("Polygon ({})", *ps);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn build_ui_fill(&self) -> Fill {
        Fill::build(
            self.ui_fill_kind,
            self.fill_enabled,
            &self.ui_fill_stops,
            self.ui_gradient_angle,
            self.ui_fill_line_x0,
            self.ui_fill_line_y0,
            self.ui_fill_line_x1,
            self.ui_fill_line_y1,
            self.ui_radial_cx,
            self.ui_radial_cy,
        )
    }

    pub fn build_ui_stroke(&self) -> Stroke {
        Stroke {
            style: Fill::build(
                self.ui_stroke_kind,
                self.stroke_enabled,
                &self.ui_stroke_stops,
                self.ui_stroke_angle,
                self.ui_stroke_line_x0,
                self.ui_stroke_line_y0,
                self.ui_stroke_line_x1,
                self.ui_stroke_line_y1,
                self.ui_stroke_radial_cx,
                self.ui_stroke_radial_cy,
            ),
            width: if self.stroke_enabled {
                self.ui_stroke_width.max(0.5)
            } else {
                0.0
            },
            line_join: self.ui_stroke_line_join,
            line_cap: self.ui_stroke_line_cap,
        }
    }

    pub fn apply_stroke_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke = self.build_ui_stroke();
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn apply_stroke_width_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke.width = self.ui_stroke_width;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn apply_no_stroke_to_selection(&mut self) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.stroke.width = 0.0;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_selection_opacity(&mut self, opacity: f32) {
        for id in self.selection.clone() {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            after.style.opacity = opacity;
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn rename_node(&mut self, id: NodeId, name: String) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        after.name = name;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_rect_geometry(
        &mut self,
        id: NodeId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        rx: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Rect {
            x: rx0,
            y: ry0,
            w: rw,
            h: rh,
            rx: rrx,
        } = &mut after.kind
        {
            *rx0 = x;
            *ry0 = y;
            *rw = w.max(1.0);
            *rh = h.max(1.0);
            *rrx = rx.max(0.0);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_ellipse_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Ellipse {
            cx: ecx,
            cy: ecy,
            rx: erx,
            ry: ery,
        } = &mut after.kind
        {
            *ecx = cx;
            *ecy = cy;
            *erx = rx.max(0.5);
            *ery = ry.max(0.5);
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_line_geometry(
        &mut self,
        id: NodeId,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.points = vec![[x0, y0], [x1, y1]];
            path.verbs = vec![0, 1];
            path.closed = false;
            path.smooth_anchors.clear();
            path.handle_out_offset.clear();
            path.handle_in_offset.clear();
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_arc_geometry(
        &mut self,
        id: NodeId,
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle_deg: f64,
        sweep_angle_deg: f64,
        join: crate::document::ArcJoin,
    ) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Arc {
            cx: acx,
            cy: acy,
            radius: ar,
            start_angle_rad,
            sweep_angle_rad,
            join: ajoin,
        } = &mut after.kind
        {
            *acx = cx;
            *acy = cy;
            *ar = radius.max(0.5);
            *start_angle_rad = start_angle_deg.to_radians();
            *sweep_angle_rad = sweep_angle_deg.to_radians();
            *ajoin = join;
            after.name = match join {
                crate::document::ArcJoin::NoJoin => "Arc".into(),
                crate::document::ArcJoin::Chord => "Chord".into(),
                crate::document::ArcJoin::ToOrigin => "Pie".into(),
            };
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
    }

    pub fn set_document_title(&mut self, title: String) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.title = title;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn set_page_size(&mut self, width: f64, height: f64) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.width = width;
        after.height = height;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn add_layer(&mut self, name: &str) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        let idx = after.add_layer(name);
        after.active_layer_index = idx;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn set_active_layer(&mut self, index: usize) {
        if index >= self.project.document.layers.len() {
            return;
        }
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        after.active_layer_index = index;
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
        self.selection.clear();
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.visible = visible;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn set_layer_locked(&mut self, index: usize, locked: bool) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.locked = locked;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    pub fn rename_layer(&mut self, index: usize, name: String) {
        let before = snapshot_document(&self.project.document);
        let mut after = before.clone();
        if let Some(l) = after.layers.get_mut(index) {
            l.name = name;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before, after },
        );
    }

    fn live_action_status(&self, ctx: &Context) -> Option<String> {
        if self.tools.space_pan || self.tools.canvas_pan_drag {
            return Some("Panning".into());
        }
        if let Some(drag) = &self.tools.drag_shape {
            if let Some(kind) = drag.kind {
                return Some(format!("Creating {}", kind.label()));
            }
        }
        if self.tools.active == ToolKind::Pen {
            if !self.tools.pen.is_empty() {
                return Some("Creating path".into());
            }
            return Some("Click to place path points".into());
        }
        if self.on_page_text_edit.is_some() && ctx.wants_keyboard_input() {
            return Some("Editing text".into());
        }
        if self.tools.select.node_drag_active {
            if let Some(target) = self.tools.select.node_edit_target {
                let what = match target {
                    PathEditTarget::Anchor(i) => format!("point {i}"),
                    PathEditTarget::HandleOut(i) => format!("handle out {i}"),
                    PathEditTarget::HandleIn(i) => format!("handle in {i}"),
                };
                return Some(format!("Dragging {what}"));
            }
        }
        if let Some(mode) = self.tools.select.drag_mode {
            return Some(match mode {
                SelectDrag::Move => {
                    if self.selection.len() == 1 {
                        if let Some(id) = self.selection.first() {
                            if let Some(n) = self.project.nodes.get(*id) {
                                return Some(format!("Moving {}", n.name));
                            }
                        }
                    }
                    "Moving selection".into()
                }
                SelectDrag::Resize(_) => "Resizing".into(),
                SelectDrag::TilingGizmo(_) | SelectDrag::CircularGizmo(_) => "Editing effect".into(),
            });
        }
        if self.tools.select.marquee.is_some() {
            return Some("Selecting".into());
        }
        None
    }

    pub(crate) fn is_ephemeral_status_event(msg: &str) -> bool {
        msg == "Undo"
            || msg == "Redo"
            || msg == "Pasted"
            || msg == "Pasted image"
            || msg == "Nothing to paste"
            || msg == "Layer locked"
            || msg.starts_with("Copied")
            || msg.starts_with("Cut ")
            || msg.starts_with("Open")
            || msg.starts_with("Save")
            || msg.starts_with("Export")
            || msg.starts_with("New ")
            || msg.contains("failed")
            || msg.starts_with("Pen cancelled")
            || msg.starts_with("Removed point")
            || msg.starts_with("Polyline cleared")
    }

    /// Second status-bar segment: live action, short event line, else **Idle**.
    pub fn derive_action_status(&self, ctx: &Context) -> String {
        if let Some(progress) = &self.paste_progress {
            return progress.label.clone();
        }
        if let Some(live) = self.live_action_status(ctx) {
            return live;
        }
        if Self::is_ephemeral_status_event(&self.status_message) {
            return self.status_message.clone();
        }
        "Idle".into()
    }

    pub fn copy_selection(&mut self) {
        log::info!("CLIPBOARD: copy_selection called, selection.len()={}", self.selection.len());
        if self.selection.is_empty() {
            log::info!("CLIPBOARD: copy skipped, empty selection");
            return;
        }
        self.clipboard = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .collect();
        let n = self.clipboard.len();
        self.status_message = if n == 1 {
            format!("Copied {}", self.clipboard[0].name)
        } else {
            format!("Copied {n} objects")
        };
        log::info!("CLIPBOARD: copied {} objects to internal clipboard", n);
    }

    pub fn cut_selection(&mut self) {
        log::info!("CLIPBOARD: cut_selection called, selection.len()={}", self.selection.len());
        if self.selection.is_empty() {
            log::info!("CLIPBOARD: cut skipped, empty selection");
            return;
        }
        let names: Vec<String> = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id))
            .map(|n| n.name.clone())
            .collect();
        self.clipboard = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .collect();
        self.delete_selection();
        self.status_message = if names.len() == 1 {
            format!("Cut {}", names[0])
        } else {
            format!("Cut {} objects", names.len())
        };
        log::info!("CLIPBOARD: cut {} objects", names.len());
    }

    fn image_paste_doc_center(&self) -> (f64, f64) {
        if let Some((cx, cy)) = self.cursor_doc {
            return (cx, cy);
        }
        if let (Some(rect), origin) = (self.canvas_screen_rect, self.canvas_origin) {
            let center_screen = rect.center();
            return tools::doc_point_from_screen(
                center_screen,
                origin,
                self.viewport.pan,
                self.viewport.zoom,
            );
        }
        (180.0, 120.0)
    }

    fn object_paste_offset(&self) -> (f64, f64) {
        if let Some((cx, cy)) = self.cursor_doc {
            if let Some(first) = self.clipboard.first() {
                let pts = first.edit_handles();
                if let Some(&(fx, fy)) = pts.first() {
                    return (cx - fx + 16.0, cy - fy + 16.0);
                }
            }
            return (24.0, 24.0);
        }
        if let (Some(rect), origin) = (self.canvas_screen_rect, self.canvas_origin) {
            let center_screen = rect.center();
            let (cx, cy) =
                tools::doc_point_from_screen(center_screen, origin, self.viewport.pan, self.viewport.zoom);
            if let Some(first) = self.clipboard.first() {
                let pts = first.edit_handles();
                if let Some(&(fx, fy)) = pts.first() {
                    return (cx - fx + 16.0, cy - fy + 16.0);
                }
            }
            return (24.0, 24.0);
        }
        (24.0, 24.0)
    }

    fn begin_system_image_paste(&mut self) {
        self.paste_progress = Some(PasteProgress {
            label: "Pasting… 1/3 reading clipboard".into(),
            task: PasteTask::SystemImage {
                step: 1,
                rgba: None,
                png: None,
                placement: None,
            },
        });
    }

    fn begin_object_paste(&mut self, offset: (f64, f64)) {
        let nodes = self.clipboard.clone();
        let total = nodes.len();
        self.paste_progress = Some(PasteProgress {
            label: format!("Pasting… 0/{total} objects"),
            task: PasteTask::Objects {
                nodes,
                offset,
                index: 0,
                new_sel: Vec::new(),
            },
        });
    }

    fn finish_paste(&mut self, message: String) {
        self.paste_progress = None;
        self.status_message = message;
    }

    fn advance_paste_operation(&mut self, ctx: &Context) {
        let Some(mut progress) = self.paste_progress.take() else {
            return;
        };

        match &mut progress.task {
            PasteTask::SystemImage {
                step,
                rgba,
                png,
                placement,
            } => match *step {
                1 => {
                    log::info!("CLIPBOARD: paste step 1/3 reading clipboard");
                    if !self.layer_editable() {
                        self.finish_paste("Layer locked".into());
                        return;
                    }
                    #[cfg(target_os = "android")]
                    {
                        self.finish_paste("System image paste is not available on Android".into());
                        return;
                    }
                    #[cfg(not(target_os = "android"))]
                    {
                        let Ok(mut cb) = arboard::Clipboard::new() else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let Ok(img) = cb.get_image() else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let w = img.width as u32;
                        let h = img.height as u32;
                        if w == 0 || h == 0 {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let Some(rgba_img) =
                            image::RgbaImage::from_raw(w, h, img.bytes.into_owned())
                        else {
                            self.finish_paste("Nothing to paste".into());
                            return;
                        };
                        let (cx, cy) = self.image_paste_doc_center();
                        let disp_w = (w as f64).min(400.0);
                        let disp_h = disp_w * (h as f64 / w.max(1) as f64);
                        *rgba = Some(rgba_img);
                        *placement = Some(ImagePastePlacement {
                            x: cx - disp_w / 2.0,
                            y: cy - disp_h / 2.0,
                            width: disp_w,
                            height: disp_h,
                        });
                        *step = 2;
                        progress.label = "Pasting… 2/3 processing image".into();
                        self.paste_progress = Some(progress);
                        ctx.request_repaint();
                    }
                }
                2 => {
                    log::info!("CLIPBOARD: paste step 2/3 processing image");
                    let Some(rgba_img) = rgba.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    let mut out = Vec::new();
                    let ok = rgba_img
                        .write_to(
                            &mut std::io::Cursor::new(&mut out),
                            image::ImageFormat::Png,
                        )
                        .is_ok()
                        && !out.is_empty();
                    if !ok {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    }
                    *png = Some(out);
                    *step = 3;
                    progress.label = "Pasting… 3/3 placing on canvas".into();
                    self.paste_progress = Some(progress);
                    ctx.request_repaint();
                }
                3 => {
                    log::info!("CLIPBOARD: paste step 3/3 placing on canvas");
                    let Some(bytes) = png.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    let Some(place) = placement.take() else {
                        self.finish_paste("Nothing to paste".into());
                        return;
                    };
                    self.insert_image(place.x, place.y, place.width, place.height, bytes);
                    self.finish_paste("Pasted image".into());
                    log::info!("CLIPBOARD: pasted image from system clipboard");
                    ctx.request_repaint();
                }
                _ => {
                    self.finish_paste("Nothing to paste".into());
                }
            },
            PasteTask::Objects {
                nodes,
                offset,
                index,
                new_sel,
            } => {
                let total = nodes.len();
                if *index < total {
                    let mut node = nodes[*index].clone();
                    node.translate(offset.0, offset.1);
                    let dup = node.duplicate();
                    let id = dup.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node: dup });
                    new_sel.push(id);
                    *index += 1;
                    progress.label = format!("Pasting… {}/{total} objects", *index);
                    if *index >= total {
                        self.selection = new_sel.clone();
                        let done = if total == 1 {
                            "Pasted".into()
                        } else {
                            format!("Pasted {total} objects")
                        };
                        self.finish_paste(done);
                        log::info!("CLIPBOARD: pasted {total} objects from internal clipboard");
                    } else {
                        self.paste_progress = Some(progress);
                    }
                    ctx.request_repaint();
                } else {
                    self.finish_paste("Nothing to paste".into());
                }
            }
        }
    }

    pub fn is_pasting(&self) -> bool {
        self.paste_progress.is_some()
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn system_clipboard_has_image(&self) -> bool {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_image().ok())
            .is_some_and(|img| img.width > 0 && img.height > 0)
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn system_clipboard_has_image(&self) -> bool {
        false
    }

    /// `prefer_system_image`: true when egui-winit did not deliver Paste (image-only OS clipboard).
    pub fn paste_clipboard(&mut self, prefer_system_image: bool) {
        if self.paste_progress.is_some() {
            return;
        }
        log::info!(
            "CLIPBOARD: paste_clipboard called, internal={} prefer_system_image={}",
            self.clipboard.len(),
            prefer_system_image
        );
        if !self.layer_editable() {
            self.status_message = "Layer locked".into();
            log::info!("CLIPBOARD: paste blocked, layer not editable");
            return;
        }
        if prefer_system_image && self.system_clipboard_has_image() {
            self.begin_system_image_paste();
            return;
        }
        if !self.clipboard.is_empty() {
            let offset = self.object_paste_offset();
            self.begin_object_paste(offset);
            return;
        }
        if self.system_clipboard_has_image() {
            self.begin_system_image_paste();
        } else {
            self.status_message = "Nothing to paste".into();
        }
    }

    pub fn duplicate_selection(&mut self) {
        let copies: Vec<Node> = self
            .selection
            .iter()
            .filter_map(|id| self.project.nodes.get(*id).cloned())
            .map(|mut n| {
                n.translate(24.0, 24.0);
                n.duplicate()
            })
            .collect();
        let mut new_sel = Vec::new();
        for node in copies {
            let id = node.id;
            self.history
                .push(&mut self.project, ProjectEdit::InsertNode { node });
            new_sel.push(id);
        }
        self.selection = new_sel;
    }

    pub fn nudge_z_order(&mut self, delta: isize) {
        let idx = self.project.document.active_layer_index;
        let before = self
            .project
            .document
            .layers
            .get(idx)
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut after = before.clone();
        for id in self.selection.clone() {
            if let Some(pos) = after.iter().position(|n| *n == id) {
                let new_pos = (pos as isize + delta).clamp(0, after.len() as isize - 1) as usize;
                if new_pos != pos {
                    let item = after.remove(pos);
                    after.insert(new_pos, item);
                }
            }
        }
        if after != before {
            self.history.push(
                &mut self.project,
                ProjectEdit::ReorderNodes {
                    layer_index: idx,
                    before,
                    after,
                },
            );
        }
    }

    fn layer_editable(&self) -> bool {
        self.project
            .document
            .active_layer()
            .is_some_and(|l| l.visible && !l.locked)
    }

    fn process_file_dialogs(&mut self) {
        #[cfg(target_os = "android")]
        {
            if self.pending_open_svg || self.pending_save_project || self.pending_export_svg {
                self.pending_open_svg = false;
                self.pending_save_project = false;
                self.pending_export_svg = false;
                self.status_message =
                    "Project/SVG file dialogs are not available on Android yet".into();
            }
            return;
        }
        #[cfg(not(target_os = "android"))]
        {
            if self.pending_open_svg {
                self.pending_open_svg = false;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .pick_file()
                {
                    match io::import_svg(&path) {
                        Ok(mut p) => {
                            let before = snapshot_project(&self.project);
                            p.document.title = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("SVG")
                                .to_string();
                            let after = p;
                            self.history.push(
                                &mut self.project,
                                ProjectEdit::SetDocument { before, after },
                            );
                            self.selection.clear();
                            self.status_message = format!("Opened {}", path.display());
                        }
                        Err(e) => self.status_message = format!("Open failed: {e}"),
                    }
                }
            }
            if self.pending_save_project {
                self.pending_save_project = false;
                let default_name = io::default_project_filename(&self.project.document.title);
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("Vadadee Berry project", &[io::PROJECT_FILE_EXTENSION])
                    .save_file()
                {
                    match io::save_project(&path, &self.project) {
                        Ok(()) => self.status_message = format!("Saved {}", path.display()),
                        Err(e) => self.status_message = format!("Save failed: {e}"),
                    }
                }
            }
            if self.pending_export_svg {
                self.pending_export_svg = false;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .save_file()
                {
                    match io::export_svg(&path, &self.project) {
                        Ok(()) => self.status_message = format!("Exported {}", path.display()),
                        Err(e) => self.status_message = format!("Export failed: {e}"),
                    }
                }
            }
        }
    }

    fn object_clipboard_blocked(&self, ctx: &Context) -> bool {
        self.on_page_text_edit.is_some() && ctx.wants_keyboard_input()
    }

    /// Called early in chrome (right after menubar) so that state changes from
    /// keyboard C/V/X are visible in the same frame's status_bar and canvas_ui,
    /// exactly like when the user clicks the menubar items.  The blocked check
    /// uses the focus state at the beginning of the frame (i.e. the state when
    /// the key event actually arrived).
    /// Returns `true` when paste was triggered from an egui input event this frame.
    pub fn handle_object_clipboard_shortcuts(&mut self, ctx: &Context) -> bool {
        if self.object_clipboard_blocked(ctx) {
            log::debug!("CLIPBOARD: blocked (on_page_text_edit or wants_keyboard_input)");
            return false;
        }

        // egui-winit turns Ctrl+C/V/X into Event::Copy/Cut/Paste (not Event::Key), so we must
        // listen for both. Ctrl+D/Z still arrive as Key events, which is why those worked.
        let (want_copy, want_cut, want_paste) = ctx.input(|i| {
            let has_cmd = i.modifiers.command || i.modifiers.ctrl;
            let mut copy = false;
            let mut cut = false;
            let mut paste = false;
            for event in &i.events {
                match event {
                    Event::Copy => copy = true,
                    Event::Cut => cut = true,
                    Event::Paste(_) => paste = true,
                    Event::Key {
                        key: Key::C,
                        pressed: true,
                        ..
                    } if has_cmd => copy = true,
                    Event::Key {
                        key: Key::X,
                        pressed: true,
                        ..
                    } if has_cmd => cut = true,
                    Event::Key {
                        key: Key::V,
                        pressed: true,
                        ..
                    } if has_cmd => paste = true,
                    _ => {}
                }
            }
            (copy, cut, paste)
        });

        if !(want_copy || want_cut || want_paste) {
            return false;
        }

        ctx.input_mut(|i| {
            i.events.retain(|event| {
                !matches!(
                    event,
                    Event::Copy | Event::Cut | Event::Paste(_)
                )
            });
            if want_copy {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::C);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::C);
            }
            if want_cut {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::X);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::X);
            }
            if want_paste {
                let _ = i.consume_key(egui::Modifiers::COMMAND, Key::V);
                let _ = i.consume_key(egui::Modifiers::CTRL, Key::V);
            }
        });

        if want_copy {
            log::info!("CLIPBOARD: detected copy shortcut");
            self.copy_selection();
            let txt = if self.clipboard.len() == 1 {
                self.clipboard[0].name.clone()
            } else {
                format!("{} objects", self.clipboard.len())
            };
            ctx.output_mut(|o| {
                o.commands.push(egui::OutputCommand::CopyText(txt));
            });
            ctx.request_repaint();
            return false;
        }
        if want_cut {
            log::info!("CLIPBOARD: detected cut shortcut");
            self.cut_selection();
            let txt = if self.clipboard.len() == 1 {
                self.clipboard[0].name.clone()
            } else {
                format!("{} objects", self.clipboard.len())
            };
            ctx.output_mut(|o| {
                o.commands.push(egui::OutputCommand::CopyText(txt));
            });
            ctx.request_repaint();
            return false;
        }
        if want_paste {
            log::info!("CLIPBOARD: detected paste shortcut");
            self.paste_clipboard(false);
            ctx.request_repaint();
            return true;
        }
        false
    }

    /// egui-winit drops Ctrl+V when the clipboard has only image/png (no text), so no
    /// Event::Paste or Key::V reaches egui. Poll the physical hotkey as a fallback.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn handle_paste_hotkey_fallback(&mut self, ctx: &Context, events_handled_paste: bool) {
        use device_query::{DeviceQuery, DeviceState, Keycode};

        let keys = DeviceState::new().get_keys();
        let down = keys.contains(&Keycode::V)
            && (keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl));
        let edge = down && !self.paste_hotkey_was_down;
        self.paste_hotkey_was_down = down;

        if events_handled_paste || self.object_clipboard_blocked(ctx) {
            return;
        }
        if edge {
            log::info!("CLIPBOARD: paste hotkey fallback (image-only system clipboard)");
            self.paste_clipboard(true);
            ctx.request_repaint();
        }
    }

    fn keyboard_shortcuts(&mut self, ctx: &Context) {
        let text_focused = ctx.wants_keyboard_input();
        ctx.input_mut(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            if cmd {
                if i.modifiers.shift && i.key_pressed(Key::Z) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Z);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Z);
                    self.do_redo();
                } else if i.key_pressed(Key::Z) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Z);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Z);
                    if self.tools.active == ToolKind::Pen && !self.tools.pen.is_empty() {
                        self.tools.pen.pop_anchor();
                        self.status_message = if self.tools.pen.is_empty() {
                            "Polyline cleared — Esc to exit pen".into()
                        } else {
                            format!(
                                "Removed point ({} remaining)",
                                self.tools.pen.len()
                            )
                        };
                    } else {
                        self.do_undo();
                    }
                }
                if i.key_pressed(Key::Y) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::Y);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::Y);
                    self.do_redo();
                }
                if i.key_pressed(Key::O) {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::O);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::O);
                    self.request_open_svg();
                }
                if i.key_pressed(Key::S) {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::S);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::S);
                    self.request_save_project();
                }
                if i.key_pressed(Key::N) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::N);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::N);
                    self.new_document();
                }
                if i.key_pressed(Key::D) && !text_focused {
                    let _ = i.consume_key(egui::Modifiers::COMMAND, Key::D);
                    let _ = i.consume_key(egui::Modifiers::CTRL, Key::D);
                    self.duplicate_selection();
                }
            }
            if i.key_pressed(Key::Enter) && self.tools.active == ToolKind::Pen {
                self.finish_pen_path(self.tools.pen.was_closed);
            } else if i.key_pressed(Key::Escape) {
                if self.on_page_text_edit.is_some() {
                    self.finish_on_page_text_edit();
                } else {
                    self.cancel_tool_to_select();
                }
            } else if (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
                && self.tools.active == ToolKind::Pen
                && !self.tools.pen.is_empty()
                && !text_focused
            {
                self.tools.pen.pop_anchor();
                self.status_message = if self.tools.pen.is_empty() {
                    "Polyline cleared — Esc to exit pen".into()
                } else {
                    format!(
                        "Removed point ({} remaining)",
                        self.tools.pen.len()
                    )
                };
            } else if (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) && !text_focused
            {
                if self.tools.active == ToolKind::Node
                    && !self.tools.select.selected_path_points.is_empty()
                    && self.remove_selected_path_points()
                {
                    // removed path anchors
                } else if !self.try_delete_focused_gradient_stop() {
                    self.delete_selection();
                }
            }
        });
    }

    /// Cancel current tool interaction and switch to Select.
    /// For Pen: zero the points (cancel polyline immediately).
    pub fn cancel_tool_to_select(&mut self) {
        let was_pen = self.tools.active == ToolKind::Pen;
        if was_pen {
            self.tools.pen.anchors.clear();
            self.tools.pen.smooth_anchors.clear();
            self.tools.pen.handle_out_offset.clear();
            self.tools.pen.handle_in_offset.clear();
            self.tools.pen.curve_adjust = None;
        }
        self.tools.pen = Default::default();
        self.tools.drag_shape = None;
        self.tools.select.marquee = None;
        self.tools.select.drag_snapshot.clear();
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
        self.tools.select.drag_mode = None;
        if self.tools.active != ToolKind::Select {
            self.tools.active = ToolKind::Select;
            self.status_message = if was_pen {
                "Pen cancelled".into()
            } else {
                "Select".into()
            };
        }
    }

    pub fn delete_selection_public(&mut self) {
        self.delete_selection();
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() || !self.layer_editable() {
            return;
        }
        let layer_index = self.project.document.active_layer_index;
        let layer_nodes_before = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut removed = Vec::new();
        for id in &self.selection {
            if let Some(node) = self.project.nodes.get(*id).cloned() {
                removed.push((*id, node));
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::RemoveNodes {
                removed,
                layer_index,
                layer_nodes_before,
            },
        );
        self.selection.clear();
    }

    fn insert_node(&mut self, node: Node) {
        let id = node.id;
        self.history
            .push(&mut self.project, ProjectEdit::InsertNode { node });
        self.selection = vec![id];
        self.sync_inspector_from_selection();
    }

    /// Load (or reload) texture for an Image node from its embedded bytes.
    fn ensure_image_texture(&mut self, id: NodeId, bytes: &[u8], ctx: &Context) {
        if self.image_textures.contains_key(&id) {
            return;
        }
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let rgba = dyn_img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            let handle = ctx.load_texture(
                format!("vadadee-berry-img-{}", id),
                color_image,
                egui::TextureOptions::default(),
            );
            self.image_textures.insert(id, handle);
        }
    }

    pub fn insert_image(&mut self, x: f64, y: f64, width: f64, height: f64, bytes: Vec<u8>) {
        let node = self.styled_shape_node(Node::image(x, y, width, height, bytes));
        self.insert_node(node);
        ui::promote_action_tab(self, ui::ActionTab::ColorStroke);
    }

    fn finish_pen_path(&mut self, close: bool) {
        let pen = self.tools.pen.clone();
        if pen.anchors.len() < 2 {
            self.tools.pen = Default::default();
            return;
        }
        let path = PathData::from_anchor_data(
            &pen.anchors,
            &pen.smooth_anchors,
            pen.handle_out_offset,
            pen.handle_in_offset,
            close,
        );
        if let Some(id) = pen.continue_node {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                self.tools.pen = Default::default();
                return;
            };
            let mut after = before.clone();
            after.kind = NodeKind::Path { path };
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
            self.selection = vec![id];
            self.status_message = "Path updated".into();
        } else {
            let mut node = Node::path_from_bez(path.to_bez(), "Path");
            node.style.fill = self.build_ui_fill();
            node.style.stroke = self.build_ui_stroke();
            node.kind = NodeKind::Path { path };
            self.insert_node(node);
        }
        self.tools.pen = Default::default();
    }

    fn sync_pen_continue_from_selection(&mut self) {
        if !self.tools.pen.is_empty() || self.tools.pen.continue_node.is_some() {
            return;
        }
        if self.selection.len() != 1 {
            return;
        }
        let id = self.selection[0];
        let Some(node) = self.project.nodes.get(id) else {
            return;
        };
        let NodeKind::Path { path } = &node.kind else {
            return;
        };
        if path.anchor_positions().len() < 2 {
            return;
        }
        let anchors = path.anchor_positions();
        self.tools.pen.anchors = anchors;
        self.tools.pen.smooth_anchors = path.smooth_anchors.clone();
        self.tools.pen.handle_out_offset = path.handle_out_offset.clone();
        self.tools.pen.handle_in_offset = path.handle_in_offset.clone();
        self.tools.pen.continue_node = Some(id);
        self.tools.pen.extend_from_start = false;
        self.tools.pen.join_anchor = None;
        self.tools.pen.was_closed = path.is_closed();
        self.status_message = if path.is_closed() {
            "Pen: add points to closed path, or click near start to re-close".into()
        } else {
            "Pen: click an end point to continue, or add points".into()
        };
    }

    pub fn canvas_ui(&mut self, ui: &mut Ui) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let origin = rect.min;
        self.canvas_screen_rect = Some(rect);
        self.canvas_origin = origin;

        // Handle dropped image files (png/jpeg) -> create Image node at drop location or center
        let drops: Vec<_> = ui.input(|i| i.raw.dropped_files.clone());
        for f in drops {
            let bytes: Vec<u8> = if let Some(b) = &f.bytes {
                b.to_vec()
            } else if let Some(p) = &f.path {
                std::fs::read(p).ok().unwrap_or_default()
            } else {
                vec![]
            };
            if bytes.is_empty() { continue; }
            let name = f.name.to_lowercase();
            if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")
                || bytes.starts_with(b"\x89PNG") || bytes.starts_with(b"\xFF\xD8")
            {
                let pos = rect.center();
                let doc = tools::doc_point_from_screen(pos, origin, self.viewport.pan, self.viewport.zoom);
                let disp_w = 320.0;
                let disp_h = 240.0;
                self.insert_image(doc.0 - disp_w / 2.0, doc.1 - disp_h / 2.0, disp_w, disp_h, bytes);
            }
        }

        let page = self.viewport.page_rect(
            origin,
            self.project.document.width as f32,
            self.project.document.height as f32,
        );

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::colors::CANVAS_BG);
            render::draw_grid(&painter, &self.viewport, origin, page);
            render::draw_page_shadow(&painter, page);

            let order = self.project.document.ordered_node_ids();
            let ctx = ui.ctx().clone();
            for id in &order {
                if let Some(node) = self.project.nodes.get(*id) {
                    if let NodeKind::Text { style, .. } = &node.kind {
                        self.fonts.ensure_loaded(&ctx, &style.font_family);
                    }
                }
            }
            self.fonts
                .ensure_loaded(&ctx, &self.ui_text_font_family);
            // While on-page editing a text, suppress its normal draw so the in-place editor provides
            // the visible glyphs + caret with no duplicate/offset.
            let draw_order: Vec<NodeId> = if let Some(edit_id) = self.on_page_text_edit {
                order.into_iter().filter(|&iid| iid != edit_id).collect()
            } else {
                order
            };
            // Ensure textures for any Image nodes (decode from embedded bytes if needed)
            let image_ids: Vec<_> = self.project.document.ordered_node_ids().into_iter().filter(|id| {
                self.project.nodes.get(*id).map_or(false, |n| matches!(n.kind, NodeKind::Image { .. }))
            }).collect();
            for id in image_ids {
                if let Some(bytes) = self.project.nodes.get(id).and_then(|n| {
                    if let NodeKind::Image { bytes, .. } = &n.kind {
                        Some(bytes.clone())
                    } else {
                        None
                    }
                }) {
                    self.ensure_image_texture(id, &bytes, &ctx);
                }
            }

            let hidden_sources =
                hidden_effect_sources(&self.project.document.path_effects);
            let mut hidden_sources = hidden_sources;
            for e in self.project.document.tiling_effects.values() {
                if e.hide_source { hidden_sources.insert(e.source_id); }
            }
            for e in self.project.document.circular_effects.values() {
                if e.hide_source { hidden_sources.insert(e.source_id); }
            }
            let loft_paths: std::collections::HashSet<NodeId> = self.project.document.path_effects.values()
                .filter(|e| e.mode == OnPathMode::Loft)
                .map(|e| e.path_id)
                .collect();
            render::draw_nodes(
                &painter,
                &self.project.nodes,
                &draw_order,
                &self.viewport,
                origin,
                &self.selection,
                &hidden_sources,
                &loft_paths,
                &self.fonts,
                &self.image_textures,
            );

            // Draw large selection outline for Tiling/Circular sources using effective bounds
            for &id in &self.selection {
                if self.node_has_tiling_or_circular(id) {
                    if let Some(node) = self.project.nodes.get(id) {
                        let eb = crate::document::get_effective_bounds(node, &self.project.document);
                        let tl = self.viewport.doc_to_screen((eb.x0, eb.y0), origin);
                        let br = self.viewport.doc_to_screen((eb.x1, eb.y1), origin);
                        let r = egui::Rect::from_min_max(tl, br);
                        painter.rect_stroke(
                            r.expand(2.0),
                            0.0,
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 120, 215)),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
            }
            render::draw_path_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.path_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            render::draw_tiling_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.tiling_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );
            render::draw_circular_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.circular_effects,
                &self.viewport,
                origin,
                &self.fonts,
                &self.image_textures,
                &self.selection,
            );

            if self.tools.active == ToolKind::Select && self.tools.select.marquee.is_none() {
                if self.selection.len() == 1 {
                    if let Some(id) = self.selection.first() {
                        if let Some(node) = self.project.nodes.get(*id) {
                            let eb = crate::document::get_effective_bounds(node, &self.project.document);
                            let tl = self.viewport.doc_to_screen((eb.x0, eb.y0), origin);
                            let br = self.viewport.doc_to_screen((eb.x1, eb.y1), origin);
                            let sr = egui::Rect::from_min_max(tl, br);
                            render::draw_transform_handles(&painter, sr);
                        }
                    }
                } else if self.selection.len() > 1 {
                    if let Some(sr) = render::selection_union_screen_rect(
                        &self.project.nodes,
                        &self.selection,
                        &self.viewport,
                        origin,
                        &self.project.document.tiling_effects,
                        &self.project.document.circular_effects,
                    ) {
                        render::draw_group_selection_bounds(&painter, sr);
                    }
                }
            }

            if let Some(m) = &self.tools.select.marquee {
                if tools::marquee_is_drag(m.origin_doc, m.current_doc) {
                    render::draw_marquee_rect(
                        &painter,
                        &self.viewport,
                        origin,
                        m.origin_doc,
                        m.current_doc,
                    );
                }
            }

            if self.tools.active == ToolKind::Node {
                for id in &self.selection {
                    if let Some(node) = self.project.nodes.get(*id) {
                        render::draw_node_handles(
                            &painter,
                            node,
                            &self.viewport,
                            origin,
                            &self.tools.select.selected_path_points,
                            self.tools.select.selected_path_segment,
                        );
                    }
                }
            }

            if self.action_tab == ui::ActionTab::ColorStroke && self.selection.len() == 1 {
                if let Some(id) = self.selection.first() {
                    if let Some(node) = self.project.nodes.get(*id) {
                        let bounds = node.bounds();
                        if self.ui_fill_edit_gradient_line
                            && self.fill_enabled
                            && matches!(
                                self.ui_fill_kind,
                                FillKind::LinearGradient | FillKind::RadialGradient
                            )
                        {
                            render::draw_gradient_flow_overlay(
                                &painter,
                                &self.viewport,
                                origin,
                                bounds,
                                self.ui_fill_kind,
                                (
                                    self.ui_fill_line_x0,
                                    self.ui_fill_line_y0,
                                    self.ui_fill_line_x1,
                                    self.ui_fill_line_y1,
                                ),
                                self.ui_radial_cx,
                                self.ui_radial_cy,
                            );
                        }
                        if self.ui_stroke_edit_gradient_line
                            && self.stroke_enabled
                            && matches!(
                                self.ui_stroke_kind,
                                FillKind::LinearGradient | FillKind::RadialGradient
                            )
                        {
                            render::draw_gradient_flow_overlay(
                                &painter,
                                &self.viewport,
                                origin,
                                bounds,
                                self.ui_stroke_kind,
                                (
                                    self.ui_stroke_line_x0,
                                    self.ui_stroke_line_y0,
                                    self.ui_stroke_line_x1,
                                    self.ui_stroke_line_y1,
                                ),
                                self.ui_stroke_radial_cx,
                                self.ui_stroke_radial_cy,
                            );
                        }
                    }
                }
            }

            if let Some(drag) = &self.tools.drag_shape {
                match drag.kind {
                    Some(ToolKind::Rectangle) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        render::draw_preview_rect(&painter, &self.viewport, origin, x, y, w, h);
                    }
                    Some(ToolKind::Circle) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let side = w.min(h);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        render::draw_preview_ellipse(
                            &painter, &self.viewport, origin, cx, cy, r, r,
                        );
                    }
                    Some(ToolKind::Ellipse) | Some(ToolKind::Arc) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        render::draw_preview_ellipse(
                            &painter, &self.viewport, origin, cx, cy, w / 2.0, h / 2.0,
                        );
                    }
                    Some(ToolKind::Line) => {
                        render::draw_preview_line(
                            &painter,
                            &self.viewport,
                            origin,
                            drag.origin_doc,
                            drag.current_doc,
                        );
                    }
                    Some(ToolKind::Polygon) => {
                        let (x, y, w, h) =
                            tools::normalize_rect(drag.origin_doc, drag.current_doc);
                        let side = w.min(h);
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        render::draw_preview_polygon(
                            &painter,
                            &self.viewport,
                            origin,
                            cx,
                            cy,
                            side / 2.0,
                            self.polygon_sides,
                        );
                    }
                    _ => {}
                }
            }

            render::draw_pen_preview(
                &painter,
                &self.viewport,
                origin,
                &self.tools.pen,
                self.cursor_doc,
            );

            if self.tools.active == ToolKind::Brush && !self.tools.brush.points.is_empty() {
                let stroke_color = match &self.build_ui_stroke().style {
                    Fill::Solid(p) => p.to_egui(),
                    Fill::LinearGradient { stops, .. } | Fill::RadialGradient { stops, .. } => {
                        if let Some(s) = stops.first() {
                            s.color.to_egui()
                        } else {
                            egui::Color32::from_rgb(0, 120, 215)
                        }
                    }
                    Fill::None => egui::Color32::from_rgb(0, 120, 215),
                };
                render::draw_brush_preview(
                    &painter,
                    &self.viewport,
                    origin,
                    &self.tools.brush.points,
                    stroke_color,
                );
            }
        }

        let mut path_rect = None;
        let mut pen_finished = false;
        let mut pen_cancelled = false;
        if self.tools.active == ToolKind::Pen && !self.tools.pen.is_empty() {
            let x = rect.center().x;
            let y = rect.max.y - 80.0;
            let overlay_pos = egui::pos2(x, y);
            
            egui::Area::new(egui::Id::new("path_drawing_overlay"))
                .fixed_pos(overlay_pos)
                .pivot(egui::Align2::CENTER_CENTER)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    let inner_resp = egui::Frame::NONE
                        .fill(egui::Color32::from_black_alpha(220))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(16, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let tick_btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✔")
                                            .color(egui::Color32::from_rgb(0, 230, 118))
                                            .strong()
                                            .size(20.0)
                                    )
                                    .frame(false)
                                );
                                if tick_btn.clicked() {
                                    pen_finished = true;
                                }
                                tick_btn.on_hover_text("Complete path drawing");

                                ui.add_space(16.0);

                                let cross_btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✖")
                                            .color(egui::Color32::from_rgb(255, 23, 68))
                                            .strong()
                                            .size(20.0)
                                    )
                                    .frame(false)
                                );
                                if cross_btn.clicked() {
                                    pen_cancelled = true;
                                }
                                cross_btn.on_hover_text("Cancel path drawing");
                            });
                        });
                    path_rect = Some(inner_resp.response.rect);
                });
        }
        self.path_overlay_rect = path_rect;
        if pen_finished {
            self.finish_pen_path(self.tools.pen.was_closed);
        }
        if pen_cancelled {
            self.tools.pen = Default::default();
            self.status_message = "Path cancelled".into();
        }

        ui::show_on_page_text_editor(self, ui, &response, origin);
        self.handle_canvas_input(&response, origin);
        response
    }

    fn handle_canvas_input(&mut self, response: &egui::Response, origin: Pos2) {
        if response.ctx.input(|i| i.multi_touch().is_some()) {
            self.tools.brush.points.clear();
            return;
        }
        if let Some(editor_rect) = self.text_editor_rect {
            if let Some(pointer_pos) = response.ctx.input(|i| i.pointer.interact_pos()) {
                if editor_rect.contains(pointer_pos) {
                    return;
                }
            }
        }
        if let Some(overlay_rect) = self.path_overlay_rect {
            if let Some(pointer_pos) = response.ctx.input(|i| i.pointer.interact_pos()) {
                if overlay_rect.contains(pointer_pos) {
                    return;
                }
            }
        }
        let pointer = response.interact_pointer_pos();
        let primary_down = response.is_pointer_button_down_on();
        let primary_pressed = response.ctx.input(|i| {
            i.pointer.button_pressed(egui::PointerButton::Primary)
        }) && response.contains_pointer();
        let primary_released = response.ctx.input(|i| {
            i.pointer.button_released(egui::PointerButton::Primary)
        }) && response.contains_pointer();
        let primary_released_anywhere = response.ctx.input(|i| {
            i.pointer.button_released(egui::PointerButton::Primary)
        });
        let double_clicked = response.double_clicked()
            || (response.contains_pointer()
                && response.ctx.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                }));

        let pan_active = self.tools.space_pan
            || response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary);

        self.tools.canvas_pan_drag = pan_active;
        if pan_active {
            let delta = response.drag_delta();
            self.viewport.pan += delta;
            return;
        }

        let Some(pos) = pointer else {
            self.cursor_doc = None;
            self.gradient_flow_drag = None;
            return;
        };
        let mut doc = self.viewport.screen_to_doc(pos, origin);
        doc = self.viewport.snap(doc);
        self.cursor_doc = Some(doc);

        if self.handle_gradient_flow_input(
            origin,
            pos,
            doc,
            primary_pressed,
            primary_down,
            primary_released,
        ) {
            return;
        }

        if !self.layer_editable() {
            return;
        }

        if self.tools.active == ToolKind::Pen {
            self.sync_pen_continue_from_selection();
        }

        let shift = response.ctx.input(|i| i.modifiers.shift);
        let ctrl = response.ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
        match self.tools.active {
            ToolKind::Select => self.tool_select(
                pos,
                origin,
                doc,
                shift,
                primary_pressed,
                primary_down,
                primary_released,
                double_clicked,
            ),
            ToolKind::Rectangle
            | ToolKind::Circle
            | ToolKind::Ellipse
            | ToolKind::Line
            | ToolKind::Polygon
            | ToolKind::Arc => {
                self.tool_drag_shape(doc, primary_down, primary_released);
            }
            ToolKind::Pen => {
                let ctrl = response.ctx.input(|i| i.modifiers.ctrl);
                let primary_released_pen = primary_released_anywhere;
                self.tool_pen(
                    doc,
                    primary_pressed,
                    primary_down,
                    primary_released_pen,
                    ctrl,
                );
            }
            ToolKind::Text => self.tool_text(doc, primary_pressed),
            ToolKind::Brush => {
                let time = response.ctx.input(|i| i.time);
                self.tool_brush(
                    doc,
                    time,
                    primary_pressed,
                    primary_down,
                    primary_released_anywhere,
                );
            }
            ToolKind::Node => self.tool_node(
                pos,
                origin,
                doc,
                shift,
                ctrl,
                primary_pressed,
                primary_down,
                primary_released,
                primary_released_anywhere,
                double_clicked,
            ),
        }

        if primary_released_anywhere
            && self.tools.active == ToolKind::Node
            && !self.tools.select.drag_snapshot.is_empty()
        {
            self.commit_drag_edits();
        }
    }

    fn commit_drag_edits(&mut self) {
        if self.tools.select.node_drag_active {
            if let Some(target) = self.tools.select.node_edit_target {
                if let Some(&(id, _)) = self.tools.select.drag_snapshot.first() {
                    if self.tools.select.selected_path_points.len() <= 1 {
                        self.tools.select
                            .set_single_path_point(id, target.anchor_index());
                    }
                }
            }
        }
        for (id, before) in self.tools.select.drag_snapshot.drain(..) {
            let Some(after) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            if before != after {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
            }
        }
        self.tools.select.drag_mode = None;
        self.tools.select.node_edit_target = None;
        self.tools.select.node_drag_origin = None;
        self.tools.select.node_drag_active = false;
    }

    pub fn set_path_handle_mode(&mut self, id: NodeId, anchor_idx: usize, mode: BezierHandleMode) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_handle_mode(anchor_idx, mode);
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn set_path_anchor_smooth(&mut self, id: NodeId, anchor_idx: usize, smooth: bool) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let already = matches!(
            &before.kind,
            NodeKind::Path { path } if path.is_anchor_smooth(anchor_idx) == smooth
        );
        if already {
            return;
        }
        let mut after = before.clone();
        if let NodeKind::Path { path } = &mut after.kind {
            path.set_anchor_smooth(anchor_idx, smooth);
        } else {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchNode { id, before, after },
        );
        self.status_message = if smooth {
            format!("Smooth point {}", anchor_idx + 1)
        } else {
            format!("Sharp point {}", anchor_idx + 1)
        };
    }

    pub fn smooth_selected_path_points(&mut self) {
        let points = self.tools.select.selected_path_points.clone();
        if points.is_empty() {
            return;
        }
        let mut by_path: std::collections::HashMap<NodeId, Vec<usize>> =
            std::collections::HashMap::new();
        for (id, idx) in points {
            by_path.entry(id).or_default().push(idx);
        }
        for (id, indices) in by_path {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            if let NodeKind::Path { path } = &mut after.kind {
                for idx in indices {
                    path.set_anchor_smooth(idx, true);
                }
            } else {
                continue;
            }
            if before != after {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
            }
        }
        self.status_message = "Smooth curve on selected points".into();
    }

    pub fn remove_selected_path_points(&mut self) -> bool {
        let points = self.tools.select.selected_path_points.clone();
        if points.is_empty() {
            return false;
        }
        let mut by_path: std::collections::HashMap<NodeId, Vec<usize>> =
            std::collections::HashMap::new();
        for (id, idx) in points {
            by_path.entry(id).or_default().push(idx);
        }
        let mut removed_any = false;
        for (id, mut indices) in by_path {
            indices.sort_unstable();
            indices.dedup();
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            let mut after = before.clone();
            if let NodeKind::Path { path } = &mut after.kind {
                if path.remove_anchors(&indices) {
                    self.history.push(
                        &mut self.project,
                        ProjectEdit::PatchNode { id, before, after },
                    );
                    removed_any = true;
                }
            }
        }
        if removed_any {
            self.tools.select.clear_path_point_selection();
            self.status_message = "Removed path point(s)".into();
        }
        removed_any
    }

    pub fn selection_path_and_objects(&self) -> Option<(Vec<NodeId>, NodeId)> {
        let mut paths = Vec::new();
        let mut objects = Vec::new();
        for id in &self.selection {
            let Some(node) = self.project.nodes.get(*id) else {
                continue;
            };
            match &node.kind {
                NodeKind::Path { .. } => paths.push(*id),
                NodeKind::Group { .. } => {}
                _ => objects.push(*id),
            }
        }
        if paths.len() == 1 && !objects.is_empty() {
            Some((objects, paths[0]))
        } else {
            None
        }
    }

    pub fn selection_path_and_object(&self) -> Option<(NodeId, NodeId)> {
        self.selection_path_and_objects()
            .and_then(|(objs, path)| objs.first().copied().map(|o| (o, path)))
    }

    pub fn sync_on_path_ui_from_selection(&mut self) {
        // Prefer direct path+object selection
        if let Some((obj, path)) = self.selection_path_and_object() {
            if let Some(effect) =
                find_effect_for_pair(&self.project.document.path_effects, obj, path)
            {
                self.ui_on_path_mode = effect.mode;
                self.ui_on_path_gap = effect.gap;
                self.ui_on_path_count = effect.count;
                self.ui_on_path_cyclic = effect.cyclic;
                self.ui_on_path_rotate = effect.rotate_to_tangent;
                self.ui_on_path_loft_scale = effect.loft_end_scale;
                self.ui_on_path_loft_opacity = effect.loft_end_opacity;
                return;
            }
        }
        // Fallback: path selected that already has effect(s) (panel context)
        if let Some((objs, path)) = self.object_on_path_panel_context() {
            if let Some(&obj) = objs.first() {
                if let Some(effect) =
                    find_effect_for_pair(&self.project.document.path_effects, obj, path)
                {
                    self.ui_on_path_mode = effect.mode;
                    self.ui_on_path_gap = effect.gap;
                    self.ui_on_path_count = effect.count;
                    self.ui_on_path_cyclic = effect.cyclic;
                    self.ui_on_path_rotate = effect.rotate_to_tangent;
                    self.ui_on_path_loft_scale = effect.loft_end_scale;
                    self.ui_on_path_loft_opacity = effect.loft_end_opacity;
                }
            }
        }
    }

    pub fn sync_tiling_ui_from_selection(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        if let Some(&oid) = objs.first() {
            if let Some(effect) = self.project.document.tiling_effects.values().find(|e| e.source_id == oid) {
                self.ui_tiling_rows = effect.count_y;
                self.ui_tiling_cols = effect.count_x;
                self.ui_tiling_offset_x = effect.offset_x;
                self.ui_tiling_offset_y = effect.offset_y;
                self.ui_tiling_row_rot = effect.row_rotation;
                self.ui_tiling_col_rot = effect.col_rotation;
                self.ui_tiling_row_scale = effect.row_scale;
                self.ui_tiling_col_scale = effect.col_scale;
                self.ui_tiling_gap_x = effect.gap_x;
                self.ui_tiling_gap_y = effect.gap_y;
            }
        }
    }

    pub fn sync_circular_ui_from_selection(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        if let Some(&oid) = objs.first() {
            if let Some(effect) = self.project.document.circular_effects.values().find(|e| e.source_id == oid) {
                self.ui_circular_copies = effect.copies;
                self.ui_circular_angle_offset = effect.angle_offset;
                self.ui_circular_origin_x = effect.origin_x;
                self.ui_circular_origin_y = effect.origin_y;
            }
        }
    }

    fn get_tiling_gizmo_points(&self, id: NodeId) -> Option<[(f64, f64); 3]> {
        if let Some(e) = self.project.document.tiling_effects.values().find(|e| e.source_id == id) {
            if let Some(node) = self.project.nodes.get(id) {
                let b = node.bounds();
                let p0 = (b.x0 + e.offset_x, b.y0 + e.offset_y);
                let p1 = (p0.0 + e.gap_x, p0.1);
                let p2 = (p0.0, p0.1 + e.gap_y);
                return Some([p0, p1, p2]);
            }
        }
        None
    }

    fn get_circular_gizmo_points(&self, id: NodeId) -> Option<[(f64, f64); 3]> {
        if let Some(e) = self.project.document.circular_effects.values().find(|e| e.source_id == id) {
            let p0 = (e.base_x, e.base_y);
            let p1 = (e.origin_x, e.origin_y);
            let dx = e.base_x - e.origin_x;
            let dy = e.base_y - e.origin_y;
            let r = dx.hypot(dy).max(1.0);
            let base_ang = dy.atan2(dx);
            let ang1 = base_ang + (std::f64::consts::TAU / e.copies.max(3) as f64) + e.angle_offset.to_radians();
            let p2 = (e.origin_x + r * ang1.cos(), e.origin_y + r * ang1.sin());
            return Some([p0, p1, p2]);
        }
        None
    }

    fn build_on_path_effect(&self, effect_id: uuid::Uuid, source_id: NodeId, path_id: NodeId) -> ObjectOnPathEffect {
        let gap = if self.ui_on_path_mode == OnPathMode::Loft {
            self
                .project
                .nodes
                .get(source_id)
                .map(default_loft_gap_for_node)
                .unwrap_or(2.0)
                .max(0.5)
        } else {
            self.ui_on_path_gap
        };
        ObjectOnPathEffect {
            id: effect_id,
            source_id,
            path_id,
            mode: self.ui_on_path_mode,
            gap,
            count: self.ui_on_path_count.max(2),
            start_offset: 0.0,
            rotate_to_tangent: self.ui_on_path_rotate,
            cyclic: self.ui_on_path_cyclic,
            loft_end_scale: self.ui_on_path_loft_scale,
            loft_end_opacity: self.ui_on_path_loft_opacity,
            hide_source: true,
        }
    }

    pub fn object_on_path_panel_context(&self) -> Option<(Vec<NodeId>, NodeId)> {
        if let Some(ctx) = self.selection_path_and_objects() {
            return Some(ctx);
        }
        if self.selection.len() != 1 {
            return None;
        }
        let path_id = self.selection[0];
        let path_node = self.project.nodes.get(path_id)?;
        if !matches!(path_node.kind, NodeKind::Path { .. }) {
            return None;
        }
        let mut objects = Vec::new();
        for effect_id in &path_node.path_effect_links {
            let Some(effect) = self.project.document.path_effects.get(effect_id) else {
                continue;
            };
            if effect.path_id == path_id && !objects.contains(&effect.source_id) {
                objects.push(effect.source_id);
            }
        }
        if objects.is_empty() {
            None
        } else {
            Some((objects, path_id))
        }
    }

    pub fn selection_has_object_on_path_effect(&self) -> bool {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return false;
        };
        has_effect_for_objects(&self.project.document.path_effects, &objects, path_id)
    }

    pub fn selection_has_tiling_effect(&self) -> bool {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        objs.iter().any(|&oid| self.project.document.tiling_effects.values().any(|e| e.source_id == oid))
    }

    pub fn selection_has_circular_effect(&self) -> bool {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        objs.iter().any(|&oid| self.project.document.circular_effects.values().any(|e| e.source_id == oid))
    }

    fn node_has_tiling_or_circular(&self, id: NodeId) -> bool {
        self.project.document.tiling_effects.values().any(|e| e.source_id == id) ||
        self.project.document.circular_effects.values().any(|e| e.source_id == id)
    }

    /// Commit object-on-path for the current path + object selection.
    pub fn apply_object_on_path_effect(&mut self) {
        let Some((objects, path_id)) = self.selection_path_and_objects() else {
            return;
        };
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created: Vec<(NodeId, uuid::Uuid)> = Vec::new();
        for source_id in &objects {
            if has_effect_for_objects(&after_doc.path_effects, &[*source_id], path_id) {
                continue;
            }
            let effect_id = uuid::Uuid::new_v4();
            let effect = self.build_on_path_effect(effect_id, *source_id, path_id);
            after_doc.path_effects.insert(effect_id, effect);
            created.push((*source_id, effect_id));
        }
        if created.is_empty() {
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        for (source_id, effect_id) in created {
            for id in [source_id, path_id] {
                let Some(before) = self.project.nodes.get(id).cloned() else {
                    continue;
                };
                if before.path_effect_links.contains(&effect_id) {
                    continue;
                }
                let mut after = before.clone();
                after.path_effect_links.push(effect_id);
                self.history.push(
                    &mut self.project,
                    ProjectEdit::PatchNode { id, before, after },
                );
            }
        }
        self.status_message = "Object on path applied".into();
    }

    /// Update parameters on effects that are already applied (live, no undo step).
    pub fn update_object_on_path_effects_live(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        for source_id in objects {
            let Some(existing) =
                find_effect_for_pair(&self.project.document.path_effects, source_id, path_id)
            else {
                continue;
            };
            let effect = self.build_on_path_effect(existing.id, source_id, path_id);
            self.project.document.path_effects.insert(existing.id, effect);
        }
    }

    pub fn update_tiling_effects_live(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        for oid in objs {
            if let Some(existing) = self.project.document.tiling_effects.values().find(|e| e.source_id == oid).cloned() {
                let mut effect = existing;
                effect.count_y = self.ui_tiling_rows;
                effect.count_x = self.ui_tiling_cols;
                effect.offset_x = self.ui_tiling_offset_x;
                effect.offset_y = self.ui_tiling_offset_y;
                effect.row_rotation = self.ui_tiling_row_rot;
                effect.col_rotation = self.ui_tiling_col_rot;
                effect.row_scale = self.ui_tiling_row_scale;
                effect.col_scale = self.ui_tiling_col_scale;
                effect.gap_x = self.ui_tiling_gap_x;
                effect.gap_y = self.ui_tiling_gap_y;
                self.project.document.tiling_effects.insert(effect.id, effect);
            }
        }
    }

    pub fn update_circular_effects_live(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        for oid in objs {
            if let Some(existing) = self.project.document.circular_effects.values().find(|e| e.source_id == oid).cloned() {
                let mut effect = existing;
                effect.copies = self.ui_circular_copies;
                effect.angle_offset = self.ui_circular_angle_offset;
                effect.origin_x = self.ui_circular_origin_x;
                effect.origin_y = self.ui_circular_origin_y;
                self.project.document.circular_effects.insert(effect.id, effect);
            }
        }
    }

    pub fn remove_object_on_path_effect(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        for source_id in objects {
            self.remove_one_object_on_path_effect(source_id, path_id);
        }
    }

    fn remove_one_object_on_path_effect(&mut self, source_id: NodeId, path_id: NodeId) {
        let Some(effect) =
            find_effect_for_pair(&self.project.document.path_effects, source_id, path_id)
        else {
            return;
        };
        let effect_id = effect.id;
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        after_doc.path_effects.swap_remove(&effect_id);
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument {
                before: before_doc,
                after: after_doc,
            },
        );
        for id in [source_id, path_id] {
            let Some(before) = self.project.nodes.get(id).cloned() else {
                continue;
            };
            if !before.path_effect_links.contains(&effect_id) {
                continue;
            }
            let mut after = before.clone();
            after.path_effect_links.retain(|e| *e != effect_id);
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
        self.status_message = "Removed object-on-path effect".into();
    }

    pub fn bake_object_on_path_copies(&mut self) {
        let Some((objects, path_id)) = self.object_on_path_panel_context() else {
            return;
        };
        self.update_object_on_path_effects_live();
        let path_data = self.project.nodes.get(path_id).and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.clone()),
            _ => None,
        });
        let Some(path) = path_data else {
            return;
        };
        let tol = 0.5 / self.viewport.zoom as f64;
        let mut child_ids = Vec::new();
        for source_id in &objects {
            let Some(effect) =
                find_effect_for_pair(&self.project.document.path_effects, *source_id, path_id)
                    .cloned()
            else {
                continue;
            };
            let Some(source) = self.project.nodes.get(*source_id).cloned() else {
                continue;
            };
            if effect.mode == OnPathMode::Loft {
                if let Some(mut node) = loft_sweep_node(&source, &effect, &path, tol) {
                    node.name = format!("{} loft", source.name);
                    let id = node.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node });
                    child_ids.push(id);
                }
            } else {
                let placements = effect_placements(&effect, &path as &dyn PathMagic, tol);
                for (i, placement) in placements.iter().enumerate() {
                    let mut node = node_at_placement(&source as &dyn FaceRenderable, placement);
                    node.name = format!("{} #{}", source.name, i + 1);
                    let id = node.id;
                    self.history
                        .push(&mut self.project, ProjectEdit::InsertNode { node });
                    child_ids.push(id);
                }
            }
        }
        if child_ids.is_empty() {
            self.status_message = "Nothing to bake — adjust path effect settings".into();
            return;
        }
        let group_name = if objects.len() == 1 {
            format!(
                "{} on path",
                self.project
                    .nodes
                    .get(objects[0])
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| "Object".into())
            )
        } else {
            "Objects on path".into()
        };
        let group = Node::group(child_ids.clone(), group_name);
        let group_id = group.id;
        self.history
            .push(&mut self.project, ProjectEdit::InsertNode { node: group });
        self.selection = vec![group_id];
        self.status_message = format!(
            "Baked {} instance(s) into group",
            child_ids.len()
        );
    }

    pub fn apply_tiling_magic(&mut self) {
        let objects: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        if objects.is_empty() {
            self.status_message = "Select object(s) to apply Tiling".into();
            return;
        }
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created = vec![];
        for &source_id in &objects {
            if after_doc.tiling_effects.values().any(|e| e.source_id == source_id) {
                continue;
            }
            let Some(source) = self.project.nodes.get(source_id) else { continue; };
            let b = source.bounds();
            let w = (b.x1 - b.x0).abs().max(1.0);
            let h = (b.y1 - b.y0).abs().max(1.0);
            let effect_id = uuid::Uuid::new_v4();
            let effect = TilingEffect {
                id: effect_id,
                source_id,
                gap_x: w,
                gap_y: h,
                count_x: 3,
                count_y: 3,
                offset_x: 0.0,  // top-left offset for first
                offset_y: 0.0,
                row_rotation: 0.0,
                col_rotation: 0.0,
                row_scale: 0.0,
                col_scale: 0.0,
                hide_source: true,
            };
            after_doc.tiling_effects.insert(effect_id, effect);
            created.push(source_id);
            // sync ui
            self.ui_tiling_gap_x = w;
            self.ui_tiling_gap_y = h;
            self.ui_tiling_rows = 3;
            self.ui_tiling_cols = 3;
            self.ui_tiling_offset_x = 0.0;
            self.ui_tiling_offset_y = 0.0;
            self.ui_tiling_row_rot = 0.0;
            self.ui_tiling_col_rot = 0.0;
            self.ui_tiling_row_scale = 0.0;
            self.ui_tiling_col_scale = 0.0;
        }
        if created.is_empty() {
            self.status_message = "No new Tiling effects".into();
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before: before_doc, after: after_doc },
        );
        self.status_message = format!("Enabled Tiling for {} object(s). Use container to bake.", created.len());
    }

    pub fn apply_circular_clone_magic(&mut self) {
        let objects: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        if objects.is_empty() {
            self.status_message = "Select object(s) to apply CircularClone".into();
            return;
        }
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut created = vec![];
        for &source_id in &objects {
            if after_doc.circular_effects.values().any(|e| e.source_id == source_id) {
                continue;
            }
            let Some(source) = self.project.nodes.get(source_id) else { continue; };
            let b = source.bounds();
            let ref_x = (b.x0 + b.x1) * 0.5;
            let ref_y = (b.y0 + b.y1) * 0.5;
            let r = ((b.x1 - b.x0).abs().max((b.y1 - b.y0).abs()) * 1.5).max(10.0);
            let effect_id = uuid::Uuid::new_v4();
            let effect = CircularCloneEffect {
                id: effect_id,
                source_id,
                origin_x: ref_x,
                origin_y: ref_y,
                radius: r,
                copies: 6,
                angle_offset: 0.0,
                base_x: ref_x,
                base_y: ref_y,
                hide_source: true,
            };
            after_doc.circular_effects.insert(effect_id, effect);
            created.push(source_id);
        }
        if created.is_empty() {
            self.status_message = "No new CircularClone effects".into();
            return;
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::PatchDocument { before: before_doc, after: after_doc },
        );
        self.status_message = format!("Enabled CircularClone for {} object(s). Use container to bake.", created.len());
    }

    pub fn remove_tiling_effect(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut removed = false;
        for oid in &objs {
            let keys: Vec<_> = after_doc.tiling_effects.iter().filter(|(_, e)| e.source_id == *oid).map(|(k, _)| *k).collect();
            for k in keys {
                after_doc.tiling_effects.swap_remove(&k);
                removed = true;
            }
        }
        if !removed { return; }
        self.history.push(&mut self.project, ProjectEdit::PatchDocument { before: before_doc, after: after_doc });
        self.status_message = "Removed Tiling effect(s)".into();
    }

    pub fn remove_circular_effect(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let before_doc = snapshot_document(&self.project.document);
        let mut after_doc = before_doc.clone();
        let mut removed = false;
        for oid in &objs {
            let keys: Vec<_> = after_doc.circular_effects.iter().filter(|(_, e)| e.source_id == *oid).map(|(k, _)| *k).collect();
            for k in keys {
                after_doc.circular_effects.swap_remove(&k);
                removed = true;
            }
        }
        if !removed { return; }
        self.history.push(&mut self.project, ProjectEdit::PatchDocument { before: before_doc, after: after_doc });
        self.status_message = "Removed CircularClone effect(s)".into();
    }

    pub fn bake_tiling(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let mut child_ids = Vec::new();
        for &oid in &objs {
            if let Some(effect) = self.project.document.tiling_effects.values().find(|e| e.source_id == oid).cloned() {
                if let Some(source) = self.project.nodes.get(oid).cloned() {
                    let src_face: &dyn FaceRenderable = &source;
                    let b = source.bounds();
                    let w = b.x1 - b.x0;
                    let h = b.y1 - b.y0;
                    let first_left = b.x0 + effect.offset_x;
                    let first_top = b.y0 + effect.offset_y;
                    for ix in 0..effect.count_x {
                        for iy in 0..effect.count_y {
                            let left = first_left + ix as f64 * effect.gap_x;
                            let top = first_top + iy as f64 * effect.gap_y;
                            let cx = left + w / 2.0;
                            let cy = top + h / 2.0;
                            let rot = (ix as f64 * effect.row_rotation + iy as f64 * effect.col_rotation).to_radians();
                            let sc = 1.0 + (ix as f64 * effect.row_scale + iy as f64 * effect.col_scale);
                            let pl = PathPlacement { x: cx, y: cy, angle_rad: rot, scale: sc as f32, opacity_mul: 1.0 };
                            let mut node = node_at_placement(src_face, &pl);
                            node.name = format!("{} #t{}_{}", source.name, ix, iy);
                            let id = node.id;
                            self.history.push(&mut self.project, ProjectEdit::InsertNode { node });
                            child_ids.push(id);
                        }
                    }
                }
            }
        }
        if !child_ids.is_empty() {
            let group = Node::group(child_ids.clone(), "Tiled group".to_string());
            let gid = group.id;
            self.history.push(&mut self.project, ProjectEdit::InsertNode { node: group });
            self.selection = vec![gid];
            self.status_message = format!("Baked {} tiles", child_ids.len());
        }
    }

    pub fn bake_circular(&mut self) {
        let objs: Vec<NodeId> = self.selection.iter().filter(|&&id| {
            self.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. }))
        }).cloned().collect();
        let mut child_ids = Vec::new();
        for &oid in &objs {
            if let Some(effect) = self.project.document.circular_effects.values().find(|e| e.source_id == oid).cloned() {
                if let Some(source) = self.project.nodes.get(oid).cloned() {
                    let src_face: &dyn FaceRenderable = &source;
                    let dx = effect.base_x - effect.origin_x;
                    let dy = effect.base_y - effect.origin_y;
                    let r = dx.hypot(dy).max(1.0);
                    let base_ang = dy.atan2(dx);
                    let n = effect.copies.max(3);
                    for i in 0..n {
                        let ang = base_ang + (i as f64 / n as f64) * std::f64::consts::TAU + effect.angle_offset.to_radians();
                        let x = effect.origin_x + r * ang.cos();
                        let y = effect.origin_y + r * ang.sin();
                        let pl = PathPlacement { x, y, angle_rad: ang, scale: 1.0, opacity_mul: 1.0 };
                        let mut node = node_at_placement(src_face, &pl);
                        node.name = format!("{} #c{}", source.name, i + 1);
                        let id = node.id;
                        self.history.push(&mut self.project, ProjectEdit::InsertNode { node });
                        child_ids.push(id);
                    }
                }
            }
        }
        if !child_ids.is_empty() {
            let group = Node::group(child_ids.clone(), "Circular group".to_string());
            let gid = group.id;
            self.history.push(&mut self.project, ProjectEdit::InsertNode { node: group });
            self.selection = vec![gid];
            self.status_message = format!("Baked {} circles", child_ids.len());
        }
    }

    pub fn close_open_paths_in_selection(&mut self) {
        let ids: Vec<_> = self
            .selection
            .iter()
            .filter(|id| {
                self.project.nodes.get(**id).is_some_and(|n| {
                    matches!(&n.kind, NodeKind::Path { path } if !path.is_closed())
                })
            })
            .copied()
            .collect();
        let count = ids.len();
        for id in ids {
            self.set_path_closed(id, true);
        }
        if count > 0 {
            self.status_message = format!("Closed {count} path(s)");
        }
    }

    pub fn open_closed_paths_in_selection(&mut self) {
        let ids: Vec<_> = self
            .selection
            .iter()
            .filter(|id| {
                self.project.nodes.get(**id).is_some_and(|n| {
                    matches!(&n.kind, NodeKind::Path { path } if path.is_closed())
                })
            })
            .copied()
            .collect();
        let count = ids.len();
        for id in ids {
            self.set_path_closed(id, false);
        }
        if count > 0 {
            self.status_message = format!("Opened {count} path(s)");
        }
    }

    pub fn begin_on_page_text_edit(&mut self, id: NodeId) {
        if self.on_page_text_edit.is_some() {
            self.finish_on_page_text_edit();
        }
        let Some(node) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let NodeKind::Text { style, .. } = &node.kind else {
            return;
        };
        self.ui_text_content = style.content.clone();
        self.ui_text_font_size = style.font_size;
        self.ui_text_font_family = style.font_family.clone();
        self.ui_text_bold = style.bold;
        self.ui_text_italic = style.italic;
        self.on_page_text_before = Some(node);
        self.on_page_text_edit = Some(id);
        self.on_page_text_focus_pending = true;
        self.selection = vec![id];
        self.sync_inspector_from_selection();
    }

    pub(crate) fn patch_on_page_text_live(&mut self, id: NodeId) {
        let content = self.ui_text_content.clone();
        let Some(node) = self.project.nodes.get_mut(id) else {
            return;
        };
        if let NodeKind::Text { style, .. } = &mut node.kind {
            style.content = content.clone();
            node.name = text_display_name(&content);
        }
    }

    pub fn finish_on_page_text_edit(&mut self) {
        let Some(id) = self.on_page_text_edit.take() else {
            self.on_page_text_newly_created = false;
            return;
        };
        #[cfg(target_os = "android")]
        {
            if let Some(android_app) = crate::ANDROID_APP.get() {
                android_app.hide_soft_input(false);
            }
        }
        self.on_page_text_focus_pending = false;
        self.patch_on_page_text_live(id);

        let newly = self.on_page_text_newly_created;
        self.on_page_text_newly_created = false;

        let Some(after) = self.project.nodes.get(id).cloned() else {
            self.on_page_text_before = None;
            return;
        };
        let content_empty = if let NodeKind::Text { style, .. } = &after.kind {
            style.content.trim().is_empty()
        } else {
            true
        };

        if newly {
            // For brand-new text from the Text tool, do not keep empty nodes at all.
            // Discard with zero history footprint; only record Insert if it has content.
            self.on_page_text_before = None;
            if content_empty {
                self.project.nodes.remove(id);
                self.project.document.remove_from_layers(id);
                self.selection.retain(|&s| s != id);
                return;
            }
            // Commit: the node is live; to record without dup layer entry, re-insert via history.
            self.project.nodes.remove(id);
            self.project.document.remove_from_layers(id);
            self.history.push(
                &mut self.project,
                ProjectEdit::InsertNode { node: after },
            );
            self.selection = vec![id];
            self.sync_inspector_from_selection();
            return;
        }

        // Normal edit of a pre-existing text node: Patch history if changed.
        let Some(before) = self.on_page_text_before.take() else {
            return;
        };
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    pub fn delete_nodes(&mut self, ids: &[NodeId]) {
        if ids.is_empty() || !self.layer_editable() {
            return;
        }
        let layer_index = self.project.document.active_layer_index;
        let layer_nodes_before = self
            .project
            .document
            .active_layer()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let mut removed = Vec::new();
        for id in ids {
            if let Some(node) = self.project.nodes.get(*id).cloned() {
                removed.push((*id, node));
            }
        }
        self.history.push(
            &mut self.project,
            ProjectEdit::RemoveNodes {
                removed,
                layer_index,
                layer_nodes_before,
            },
        );
        self.selection.retain(|id| !ids.contains(id));
    }

    pub fn delete_on_page_text_node(&mut self, id: NodeId) {
        self.on_page_text_edit = None;
        #[cfg(target_os = "android")]
        {
            if let Some(android_app) = crate::ANDROID_APP.get() {
                android_app.hide_soft_input(false);
            }
        }
        self.on_page_text_focus_pending = false;
        
        let newly = self.on_page_text_newly_created;
        self.on_page_text_newly_created = false;
        self.on_page_text_before = None;

        if newly {
            self.project.nodes.remove(id);
            self.project.document.remove_from_layers(id);
            self.selection.retain(|&s| s != id);
        } else {
            let layer_index = self.project.document.active_layer_index;
            let layer_nodes_before = self
                .project
                .document
                .active_layer()
                .map(|l| l.nodes.clone())
                .unwrap_or_default();
            if let Some(node) = self.project.nodes.get(id).cloned() {
                self.history.push(
                    &mut self.project,
                    ProjectEdit::RemoveNodes {
                        removed: vec![(id, node)],
                        layer_index,
                        layer_nodes_before,
                    },
                );
            }
            self.selection.retain(|&s| s != id);
        }
    }

    pub fn set_text_style(&mut self, id: NodeId, style: TextStyle, x: f64, y: f64) {
        let Some(before) = self.project.nodes.get(id).cloned() else {
            return;
        };
        let mut after = before.clone();
        if let NodeKind::Text {
            x: tx,
            y: ty,
            style: ts,
        } = &mut after.kind
        {
            *tx = x;
            *ty = y;
            after.name = text_display_name(&style.content);
            *ts = style;
        } else {
            return;
        }
        if before != after {
            self.history.push(
                &mut self.project,
                ProjectEdit::PatchNode { id, before, after },
            );
        }
    }

    fn tool_select(
        &mut self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
        shift: bool,
        pressed: bool,
        down: bool,
        released: bool,
        double_clicked: bool,
    ) {
        if double_clicked {
            let mut hit: Option<NodeId> = None;
            let mut bbox_only: Option<NodeId> = None;
            for id in self.project.document.ordered_node_ids().into_iter().rev() {
                if let Some(node) = self.project.nodes.get(id) {
                    let does_hit = if self.node_has_tiling_or_circular(id) {
                        let eb = crate::document::get_effective_bounds(node, &self.project.document);
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let slop = 4.0 / self.viewport.zoom as f64;
                        eb.inflate(slop, slop).contains(pt)
                    } else {
                        node.hit_test_with_store(
                            &self.project.nodes,
                            doc.0,
                            doc.1,
                            4.0 / self.viewport.zoom as f64,
                        )
                    };
                    if does_hit {
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let precise = if self.node_has_tiling_or_circular(id) {
                            true
                        } else {
                            node.bez_path().contains(pt)
                                || matches!(node.kind, NodeKind::Text { .. })
                        };
                        if precise {
                            hit = Some(id);
                            break;
                        } else if bbox_only.is_none() {
                            bbox_only = Some(id);
                        }
                    }
                }
            }
            if hit.is_none() {
                hit = bbox_only;
            }
            if let Some(id) = hit {
                self.tools.select.drag_mode = None;
                self.tools.select.marquee = None;
                self.tools.select.drag_snapshot.clear();
                if self
                    .project
                    .nodes
                    .get(id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
                {
                    self.on_page_text_newly_created = false;
                    self.begin_on_page_text_edit(id);
                    return;
                }
                if self
                    .project
                    .nodes
                    .get(id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                {
                    self.selection = vec![id];
                    self.tools.active = ToolKind::Node;
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                    self.sync_inspector_from_selection();
                    return;
                }
                if self.node_has_tiling_or_circular(id) {
                    self.selection = vec![id];
                    self.tools.active = ToolKind::Node;
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                    self.sync_inspector_from_selection();
                    return;
                }
            }
        }

        if pressed && !double_clicked {
            // Resize handles take priority over move (must run on pointer-down, not click-up).
            if self.selection.len() == 1 {
                if let Some(id) = self.selection.first().copied() {
                    if !self.node_has_tiling_or_circular(id) {
                        if let Some(node) = self.project.nodes.get(id) {
                            let sr = render::selection_screen_rect(
                                node,
                                &self.project.nodes,
                                &self.viewport,
                                origin,
                            );
                            if let Some(handle) =
                                render::hit_resize_handle(sr, screen, self.viewport.zoom)
                            {
                                self.tools.select.drag_mode = Some(SelectDrag::Resize(handle));
                                self.tools.select.resize_anchor = node.bounds();
                                self.tools.select.drag_snapshot = vec![(id, node.clone())];
                                self.tools.select.last_doc = doc;
                                self.sync_inspector_from_selection();
                                return;
                            }
                        }
                    }
                }
            }

            // Gizmo for Tiling / CircularClone (edit the 3 points / angle)
            if self.selection.len() == 1 {
                if let Some(id) = self.selection.first().copied() {
                    let slop = 10.0 / (self.viewport.zoom as f64).max(0.1);
                    if let Some(pts) = self.get_tiling_gizmo_points(id) {
                        for (i, &(px, py)) in pts.iter().enumerate() {
                            if (px - doc.0).hypot(py - doc.1) < slop {
                                self.tools.select.drag_mode = Some(SelectDrag::TilingGizmo(i));
                                self.tools.select.last_doc = doc;
                                return;
                            }
                        }
                    }
                    if let Some(pts) = self.get_circular_gizmo_points(id) {
                        for (i, &(px, py)) in pts.iter().enumerate() {
                            if (px - doc.0).hypot(py - doc.1) < slop {
                                self.tools.select.drag_mode = Some(SelectDrag::CircularGizmo(i));
                                self.tools.select.last_doc = doc;
                                return;
                            }
                        }
                    }
                }
            }

            // Clicking a path edge selects both endpoints (switches to node edit).
            if let Some((id, from, to, _, _)) = self.hit_path_segment(screen, origin, doc) {
                self.tools.select.drag_mode = None;
                self.tools.select.marquee = None;
                self.tools.select.drag_snapshot.clear();
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                } else if !shift {
                    self.selection = vec![id];
                }
                self.tools.select.set_path_segment(id, from, to);
                self.tools.active = ToolKind::Node;
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                self.sync_inspector_from_selection();
                return;
            }

            // Hit topmost first (rev order). Prefer nodes where the actual geometry contains the point
            // (precise fill/stroke) so that a small circle inside a large object's bbox is selectable
            // without the large bbox "stealing" via its inflated bounds.
            let mut hit: Option<NodeId> = None;
            let mut bbox_only: Option<NodeId> = None;
            for id in self.project.document.ordered_node_ids().into_iter().rev() {
                if let Some(node) = self.project.nodes.get(id) {
                    let does_hit = if self.node_has_tiling_or_circular(id) {
                        let eb = crate::document::get_effective_bounds(node, &self.project.document);
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let slop = 4.0 / self.viewport.zoom as f64;
                        eb.inflate(slop, slop).contains(pt)
                    } else {
                        node.hit_test_with_store(
                            &self.project.nodes,
                            doc.0,
                            doc.1,
                            4.0 / self.viewport.zoom as f64,
                        )
                    };
                    if does_hit {
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let precise = if self.node_has_tiling_or_circular(id) {
                            true
                        } else {
                            node.bez_path().contains(pt)
                                || matches!(node.kind, NodeKind::Text { .. })
                        };
                        if precise {
                            hit = Some(id);
                            break;
                        } else if bbox_only.is_none() {
                            bbox_only = Some(id);
                        }
                    }
                }
            }
            if hit.is_none() {
                hit = bbox_only;
            }
            if let Some(edit_id) = self.on_page_text_edit {
                let keep_editing = hit == Some(edit_id);
                if !keep_editing {
                    self.finish_on_page_text_edit();
                }
            }
            if let Some(id) = hit {
                self.tools.select.marquee = None;
                self.tools.select.clear_path_point_selection();
                if shift {
                    if self.selection.contains(&id) {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection.push(id);
                    }
                } else if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                if !self.selection.is_empty() {
                    self.tools.select.drag_mode = Some(SelectDrag::Move);
                    self.tools.select.drag_snapshot = self
                        .selection
                        .iter()
                        .filter_map(|sid| {
                            self.project
                                .nodes
                                .get(*sid)
                                .map(|n| (*sid, n.clone()))
                        })
                        .collect();
                }
            } else {
                self.tools.select.drag_mode = None;
                self.tools.select.clear_path_point_selection();
                self.tools.select.marquee = Some(MarqueeSelect {
                    origin_doc: doc,
                    current_doc: doc,
                    shift,
                });
            }
            self.tools.select.last_doc = doc;
            self.sync_inspector_from_selection();
        } else if down {
            if let Some(marquee) = self.tools.select.marquee.as_mut() {
                marquee.current_doc = doc;
            } else if let Some(mode) = self.tools.select.drag_mode {
                match mode {
                    SelectDrag::Move => {
                        let dx = doc.0 - self.tools.select.last_doc.0;
                        let dy = doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = doc;
                        for id in self.selection.clone() {
                            let child_ids = self.project.nodes.get(id).and_then(|n| {
                                if let NodeKind::Group { children } = &n.kind {
                                    Some(children.clone())
                                } else {
                                    None
                                }
                            });
                            if let Some(kids) = child_ids {
                                for cid in kids {
                                    if let Some(child) = self.project.nodes.get_mut(cid) {
                                        child.translate(dx, dy);
                                    }
                                }
                            } else if let Some(node) = self.project.nodes.get_mut(id) {
                                node.translate(dx, dy);
                            }
                        }
                    }
                    SelectDrag::Resize(handle) => {
                        if let Some(id) = self.selection.first().copied() {
                            let new_bounds =
                                tools::resize_bounds(self.tools.select.resize_anchor, handle, doc);
                            if let Some(node) = self.project.nodes.get_mut(id) {
                                node.set_bounds(new_bounds);
                            }
                        }
                    }
                    SelectDrag::TilingGizmo(pt_idx) => {
                        let dx = doc.0 - self.tools.select.last_doc.0;
                        let dy = doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = doc;
                        if let Some(id) = self.selection.first().copied() {
                            if let Some((_, e)) = self.project.document.tiling_effects.iter_mut().find(|(_, e)| e.source_id == id) {
                                match pt_idx {
                                    0 => { e.offset_x += dx; e.offset_y += dy; }
                                    1 => { e.gap_x += dx; }
                                    2 => { e.gap_y += dy; }
                                    _ => {}
                                }
                            }
                        }
                    }
                    SelectDrag::CircularGizmo(pt_idx) => {
                        let dx = doc.0 - self.tools.select.last_doc.0;
                        let dy = doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = doc;
                        if let Some(id) = self.selection.first().copied() {
                            if let Some((_, e)) = self.project.document.circular_effects.iter_mut().find(|(_, e)| e.source_id == id) {
                                match pt_idx {
                                    0 => { e.base_x += dx; e.base_y += dy; }
                                    1 => { e.origin_x += dx; e.origin_y += dy; }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        } else if released {
            if let Some(m) = self.tools.select.marquee.take() {
                if tools::marquee_is_drag(m.origin_doc, m.current_doc) {
                    let rect = tools::marquee_rect(m.origin_doc, m.current_doc);
                    let picked: Vec<NodeId> = self
                        .project
                        .document
                        .ordered_node_ids()
                        .into_iter()
                        .filter(|id| {
                            self.project
                                .nodes
                                .get(*id)
                                .is_some_and(|n| {
                                    if self.node_has_tiling_or_circular(*id) {
                                        let eb = crate::document::get_effective_bounds(n, &self.project.document);
                                        let overlap = eb.intersect(rect);
                                        overlap.width() > 0.0 && overlap.height() > 0.0
                                    } else {
                                        tools::node_bounds_intersects_marquee(n, rect)
                                    }
                                })
                        })
                        .collect();
                    if m.shift {
                        for id in picked {
                            if !self.selection.contains(&id) {
                                self.selection.push(id);
                            }
                        }
                    } else {
                        self.selection = picked;
                    }
                } else if !m.shift {
                    self.selection.clear();
                }
                self.sync_inspector_from_selection();
            } else if let Some(mode) = self.tools.select.drag_mode.take() {
                if !matches!(mode, SelectDrag::TilingGizmo(_) | SelectDrag::CircularGizmo(_)) {
                    self.commit_drag_edits();
                } else {
                    self.tools.select.drag_snapshot.clear();
                }
            }
        }
    }

    fn styled_shape_node(&self, mut node: Node) -> Node {
        node.style.stroke = self.build_ui_stroke();
        node.style.fill = self.build_ui_fill();
        node
    }

    fn tool_drag_shape(&mut self, doc: (f64, f64), down: bool, released: bool) {
        if self.tools.drag_shape.is_none() && down {
            self.tools.drag_shape = Some(DragNewShape {
                origin_doc: doc,
                current_doc: doc,
                kind: Some(self.tools.active),
            });
        } else if let Some(drag) = &mut self.tools.drag_shape {
            drag.current_doc = doc;
            if released {
                let kind = drag.kind;
                let origin = drag.origin_doc;
                let current = drag.current_doc;
                self.tools.drag_shape = None;

                let Some(kind) = kind else {
                    return;
                };

                let node = match kind {
                    ToolKind::Rectangle => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        if w <= 2.0 || h <= 2.0 {
                            return;
                        }
                        self.styled_shape_node(Node::rect(
                            x,
                            y,
                            w,
                            h,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Circle => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        let mut n = Node::ellipse(cx, cy, r, r, self.build_ui_fill());
                        n.name = "Circle".into();
                        self.styled_shape_node(n)
                    }
                    ToolKind::Ellipse => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        if w <= 2.0 || h <= 2.0 {
                            return;
                        }
                        self.styled_shape_node(Node::ellipse(
                            x + w / 2.0,
                            y + h / 2.0,
                            w / 2.0,
                            h / 2.0,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Polygon => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        self.styled_shape_node(Node::polygon(
                            cx,
                            cy,
                            side / 2.0,
                            self.polygon_sides,
                            self.build_ui_fill(),
                        ))
                    }
                    ToolKind::Line => {
                        let dx = current.0 - origin.0;
                        let dy = current.1 - origin.1;
                        if dx.hypot(dy) <= 2.0 {
                            return;
                        }
                        let mut stroke = self.build_ui_stroke();
                        if !self.stroke_enabled {
                            stroke.width = 1.0;
                            stroke.style = Fill::Solid(Paint::from_hex(0x1a1f2e, 1.0));
                        }
                        Node::line(origin.0, origin.1, current.0, current.1, stroke)
                    }
                    ToolKind::Arc => {
                        let (x, y, w, h) = tools::normalize_rect(origin, current);
                        let side = w.min(h);
                        if side <= 2.0 {
                            return;
                        }
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let r = side / 2.0;
                        // Default: 90 degree arc, no join (user edits angle/join in Geometry)
                        let start = -std::f64::consts::FRAC_PI_4;
                        let sweep = std::f64::consts::FRAC_PI_2;
                        self.styled_shape_node(Node::arc(
                            cx,
                            cy,
                            r,
                            start,
                            sweep,
                            crate::document::ArcJoin::NoJoin,
                            self.build_ui_fill(),
                        ))
                    }
                    _ => return,
                };
                self.insert_node(node);
            }
        }
    }

    fn pen_push_anchor(&mut self, doc: (f64, f64), smooth: bool) {
        if self.tools.pen.extend_from_start {
            self.tools.pen.anchors.insert(0, doc);
            let mut smooth_anchors: Vec<usize> = self
                .tools
                .pen
                .smooth_anchors
                .iter()
                .map(|&i| i + 1)
                .collect();
            if smooth {
                smooth_anchors.push(0);
                smooth_anchors.sort_unstable();
                smooth_anchors.dedup();
            }
            let mut out = std::collections::HashMap::new();
            let mut inn = std::collections::HashMap::new();
            for (k, v) in &self.tools.pen.handle_out_offset {
                out.insert(k + 1, *v);
            }
            for (k, v) in &self.tools.pen.handle_in_offset {
                inn.insert(k + 1, *v);
            }
            self.tools.pen.smooth_anchors = smooth_anchors;
            self.tools.pen.handle_out_offset = out;
            self.tools.pen.handle_in_offset = inn;
            if smooth {
                self.tools.pen.curve_adjust = Some(0);
            }
        } else {
            let idx = self.tools.pen.anchors.len();
            self.tools.pen.anchors.push(doc);
            if smooth {
                if !self.tools.pen.smooth_anchors.contains(&idx) {
                    self.tools.pen.smooth_anchors.push(idx);
                    self.tools.pen.smooth_anchors.sort_unstable();
                    self.tools.pen.smooth_anchors.dedup();
                }
                self.tools.pen.curve_adjust = Some(idx);
            }
        }
    }

    fn tool_pen(
        &mut self,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
        ctrl: bool,
    ) {
        let endpoint_thresh = 8.0 / self.viewport.zoom as f64;

        if pressed {
            if let Some(first) = self.tools.pen.anchors.first() {
                if self.tools.pen.anchors.len() >= 2
                    && (first.0 - doc.0).hypot(first.1 - doc.1) < 2.0
                    && (self.tools.pen.continue_node.is_none() || self.tools.pen.was_closed)
                {
                    self.finish_pen_path(true);
                    return;
                }
            }

            if let Some(_) = self.tools.pen.continue_node {
                if let (Some(first), Some(last)) =
                    (self.tools.pen.anchors.first(), self.tools.pen.anchors.last())
                {
                    let near_start = (first.0 - doc.0).hypot(first.1 - doc.1) < endpoint_thresh;
                    let near_end = (last.0 - doc.0).hypot(last.1 - doc.1) < endpoint_thresh;
                    if near_start {
                        self.tools.pen.extend_from_start = true;
                        self.tools.pen.join_anchor = Some(0);
                        if !self.tools.pen.smooth_anchors.contains(&0) {
                            self.tools.pen.smooth_anchors.push(0);
                            self.tools.pen.smooth_anchors.sort_unstable();
                        }
                        return;
                    }
                    if near_end {
                        self.tools.pen.extend_from_start = false;
                        let end_idx = self.tools.pen.anchors.len().saturating_sub(1);
                        self.tools.pen.join_anchor = Some(end_idx);
                        if !self.tools.pen.smooth_anchors.contains(&end_idx) {
                            self.tools.pen.smooth_anchors.push(end_idx);
                            self.tools.pen.smooth_anchors.sort_unstable();
                        }
                        return;
                    }
                }
            }

            self.pen_push_anchor(doc, ctrl);
        }

        if down {
            if let Some(idx) = self.tools.pen.curve_adjust {
                if ctrl {
                    let Some(&(ax, ay)) = self.tools.pen.anchors.get(idx) else {
                        return;
                    };
                    let offset = [doc.0 - ax, doc.1 - ay];
                    self.tools.pen.handle_out_offset.insert(idx, offset);
                    self.tools.pen
                        .handle_in_offset
                        .insert(idx, [-offset[0], -offset[1]]);
                }
            }
        }

        if released {
            self.tools.pen.curve_adjust = None;
        }
    }

    fn handle_gradient_flow_input(
        &mut self,
        origin: Pos2,
        screen: Pos2,
        doc: (f64, f64),
        pressed: bool,
        down: bool,
        released: bool,
    ) -> bool {
        use crate::document::{linear_angle_from_line, translate_linear_line};

        if self.action_tab != ui::ActionTab::ColorStroke || self.selection.len() != 1 {
            self.gradient_flow_drag = None;
            return false;
        }
        let Some(id) = self.selection.first().copied() else {
            return false;
        };
        let Some(node) = self.project.nodes.get(id) else {
            return false;
        };
        let bounds = node.bounds();
        let slop = 12.0;

        let fill_active = self.ui_fill_edit_gradient_line
            && self.fill_enabled
            && matches!(
                self.ui_fill_kind,
                FillKind::LinearGradient | FillKind::RadialGradient
            );
        let stroke_active = self.ui_stroke_edit_gradient_line
            && self.stroke_enabled
            && matches!(
                self.ui_stroke_kind,
                FillKind::LinearGradient | FillKind::RadialGradient
            );
        if !fill_active && !stroke_active {
            self.gradient_flow_drag = None;
            return false;
        }

        if pressed {
            if fill_active {
                if let Some(handle) = render::pick_gradient_flow_handle(
                    &self.viewport,
                    origin,
                    bounds,
                    self.ui_fill_kind,
                    (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    ),
                    self.ui_radial_cx,
                    self.ui_radial_cy,
                    screen,
                    slop,
                ) {
                    let line = (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    );
                    self.gradient_flow_drag = Some(GradientFlowDrag {
                        target: GradientFlowTarget::Fill,
                        handle,
                        line_at_press: line,
                        doc_at_press: doc,
                    });
                }
            } else if stroke_active {
                if let Some(handle) = render::pick_gradient_flow_handle(
                    &self.viewport,
                    origin,
                    bounds,
                    self.ui_stroke_kind,
                    (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    ),
                    self.ui_stroke_radial_cx,
                    self.ui_stroke_radial_cy,
                    screen,
                    slop,
                ) {
                    let line = (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    );
                    self.gradient_flow_drag = Some(GradientFlowDrag {
                        target: GradientFlowTarget::Stroke,
                        handle,
                        line_at_press: line,
                        doc_at_press: doc,
                    });
                }
            }
        }

        if released {
            let was = self.gradient_flow_drag.is_some();
            self.gradient_flow_drag = None;
            return was;
        }

        let Some(drag) = self.gradient_flow_drag else {
            return false;
        };

        if !down {
            return false;
        }

        let w = (bounds.x1 - bounds.x0).max(1e-6);
        let h = (bounds.y1 - bounds.y0).max(1e-6);
        let (nx, ny) = render::linear_norm_from_bounds_drag(bounds, doc);

        match drag.target {
            GradientFlowTarget::Fill => match self.ui_fill_kind {
                FillKind::LinearGradient => {
                    let mut line = (
                        self.ui_fill_line_x0,
                        self.ui_fill_line_y0,
                        self.ui_fill_line_x1,
                        self.ui_fill_line_y1,
                    );
                    match drag.handle {
                        crate::gradient_ui::GradientLineHandle::LinearEnd0 => {
                            line.0 = nx;
                            line.1 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearEnd1 => {
                            line.2 = nx;
                            line.3 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearMid => {
                            let dx = nx
                                - ((drag.doc_at_press.0 - bounds.x0) / w) as f32;
                            let dy = ny
                                - ((drag.doc_at_press.1 - bounds.y0) / h) as f32;
                            line = drag.line_at_press;
                            translate_linear_line(&mut line, dx, dy);
                        }
                        crate::gradient_ui::GradientLineHandle::RadialFocal => {}
                    }
                    self.ui_fill_line_x0 = line.0;
                    self.ui_fill_line_y0 = line.1;
                    self.ui_fill_line_x1 = line.2;
                    self.ui_fill_line_y1 = line.3;
                    self.ui_gradient_angle =
                        linear_angle_from_line(line.0, line.1, line.2, line.3);
                    self.apply_fill_to_selection();
                }
                FillKind::RadialGradient => {
                    if drag.handle == crate::gradient_ui::GradientLineHandle::RadialFocal {
                        let (cx, cy) = render::radial_from_bounds_drag(bounds, doc);
                        self.ui_radial_cx = cx;
                        self.ui_radial_cy = cy;
                        self.apply_fill_to_selection();
                    }
                }
                FillKind::Solid => {}
            },
            GradientFlowTarget::Stroke => match self.ui_stroke_kind {
                FillKind::LinearGradient => {
                    let mut line = (
                        self.ui_stroke_line_x0,
                        self.ui_stroke_line_y0,
                        self.ui_stroke_line_x1,
                        self.ui_stroke_line_y1,
                    );
                    match drag.handle {
                        crate::gradient_ui::GradientLineHandle::LinearEnd0 => {
                            line.0 = nx;
                            line.1 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearEnd1 => {
                            line.2 = nx;
                            line.3 = ny;
                        }
                        crate::gradient_ui::GradientLineHandle::LinearMid => {
                            let dx = nx
                                - ((drag.doc_at_press.0 - bounds.x0) / w) as f32;
                            let dy = ny
                                - ((drag.doc_at_press.1 - bounds.y0) / h) as f32;
                            line = drag.line_at_press;
                            translate_linear_line(&mut line, dx, dy);
                        }
                        crate::gradient_ui::GradientLineHandle::RadialFocal => {}
                    }
                    self.ui_stroke_line_x0 = line.0;
                    self.ui_stroke_line_y0 = line.1;
                    self.ui_stroke_line_x1 = line.2;
                    self.ui_stroke_line_y1 = line.3;
                    self.ui_stroke_angle =
                        linear_angle_from_line(line.0, line.1, line.2, line.3);
                    self.apply_stroke_to_selection();
                }
                FillKind::RadialGradient => {
                    if drag.handle == crate::gradient_ui::GradientLineHandle::RadialFocal {
                        let (cx, cy) = render::radial_from_bounds_drag(bounds, doc);
                        self.ui_stroke_radial_cx = cx;
                        self.ui_stroke_radial_cy = cy;
                        self.apply_stroke_to_selection();
                    }
                }
                FillKind::Solid => {}
            },
        }
        true
    }

    fn canvas_wheel_zoom(&mut self, ctx: &Context) {
        let Some(canvas_rect) = self.canvas_screen_rect else {
            return;
        };
        // Handle multi-touch zoom and pan first
        if let Some(multi_touch) = ctx.input(|i| i.multi_touch()) {
            if canvas_rect.contains(multi_touch.center_pos) {
                if (multi_touch.zoom_delta - 1.0).abs() > 1e-4 {
                    self.viewport.zoom_at(multi_touch.center_pos, self.canvas_origin, multi_touch.zoom_delta);
                }
                self.viewport.pan += multi_touch.translation_delta;
                return;
            }
        }
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let on_canvas = hover.is_some_and(|p| canvas_rect.contains(p));
        if !on_canvas {
            return;
        }
        // egui routes Ctrl+wheel into zoom_delta (not smooth_scroll_delta).
        let factor = ctx.input(|i| i.zoom_delta());
        if (factor - 1.0).abs() <= 1e-4 {
            return;
        }
        let pos = hover.unwrap_or(canvas_rect.center());
        self.viewport.zoom_at(pos, self.canvas_origin, factor);
    }

    fn tool_text(&mut self, doc: (f64, f64), pressed: bool) {
        if !pressed {
            return;
        }
        let style = TextStyle {
            content: String::new(),
            font_size: self.ui_text_font_size,
            font_family: self.ui_text_font_family.clone(),
            bold: self.ui_text_bold,
            italic: self.ui_text_italic,
        };
        let mut node = self.styled_shape_node(Node::text(doc.0, doc.1, style));
        node.name = "Text".into();
        let id = node.id;
        // Add live for preview/typing but do NOT push history yet. Only commit on non-empty finish.
        let _ = self.project.nodes.insert(node.clone());
        self.project.document.append_to_active_layer(id);
        self.ui_text_content.clear();
        self.on_page_text_newly_created = true;
        self.begin_on_page_text_edit(id);
    }

    fn tool_brush(
        &mut self,
        doc: (f64, f64),
        time: f64,
        pressed: bool,
        down: bool,
        released: bool,
    ) {
        if pressed {
            self.tools.brush.points.clear();
            let base_w = self.ui_stroke_width;
            self.tools.brush.points.push(([doc.0, doc.1], time, base_w));
        } else if down {
            if let Some(&(prev_pos, prev_time, prev_w)) = self.tools.brush.points.last() {
                let dist = ((doc.0 - prev_pos[0]).powi(2) + (doc.1 - prev_pos[1]).powi(2)).sqrt();
                if dist > 1.0 {
                    let dt = time - prev_time;
                    let speed = if dt > 0.0001 { dist / dt } else { 0.0 };
                    let target_w = {
                        let min_w = (self.ui_stroke_width * 0.3).max(1.0);
                        let max_w = (self.ui_stroke_width * 2.0).max(4.0);
                        let factor = (speed / 1200.0).min(1.0) as f32;
                        max_w - (max_w - min_w) * factor
                    };
                    let alpha = 0.15;
                    let new_w = prev_w * (1.0 - alpha) + target_w * alpha;
                    self.tools.brush.points.push(([doc.0, doc.1], time, new_w));
                }
            }
        }

        if released {
            let pts = &self.tools.brush.points;
            if pts.len() >= 2 {
                let bez = generate_brush_outline(pts);
                let mut node = Node::path_from_bez(bez, "Brush");
                node.style.fill = self.build_ui_stroke().style;
                node.style.stroke = Stroke {
                    style: Fill::none(),
                    width: 0.0,
                    line_join: crate::document::LineJoin::Miter,
                    line_cap: crate::document::LineCap::Butt,
                };
                self.insert_node(node);
            }
            self.tools.brush.points.clear();
        }
    }

    fn hit_path_segment(
        &self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
    ) -> Option<(NodeId, usize, usize, f64, f64)> {
        let threshold_doc = 16.0 / self.viewport.zoom as f64;
        let ids: Vec<NodeId> = if self.selection.is_empty() {
            self.project.document.ordered_node_ids()
        } else {
            self.selection.clone()
        };
        let mut best: Option<(NodeId, usize, usize, f64, f64, f32)> = None;
        for id in ids {
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            let NodeKind::Path { path } = &node.kind else {
                continue;
            };
            let Some((from, to, px, py)) =
                path.hit_segment(doc.0, doc.1, threshold_doc)
            else {
                continue;
            };
            let hit_screen = self.viewport.doc_to_screen((px, py), origin);
            let d = screen.distance(hit_screen);
            if best.as_ref().map_or(true, |(_, _, _, _, _, bd)| d < *bd) {
                best = Some((id, from, to, px, py, d));
            }
        }
        let screen_thresh = 16.0;
        best.filter(|(.., d)| *d <= screen_thresh)
            .map(|(id, from, to, px, py, _)| (id, from, to, px, py))
    }

    fn hit_node_edit(
        &self,
        screen: Pos2,
        origin: Pos2,
    ) -> Option<(NodeId, PathEditTarget)> {
        let anchor_threshold = 14.0;
        let handle_threshold = 16.0;
        let mut best: Option<(NodeId, PathEditTarget, f32)> = None;
        let ids: Vec<NodeId> = if self.selection.is_empty() {
            self.project.document.ordered_node_ids()
        } else {
            self.selection.clone()
        };
        for id in ids {
            let Some(node) = self.project.nodes.get(id) else {
                continue;
            };
            for (target, p) in node.path_edit_targets() {
                let threshold = match target {
                    PathEditTarget::Anchor(_) => anchor_threshold,
                    PathEditTarget::HandleOut(_) | PathEditTarget::HandleIn(_) => {
                        handle_threshold
                    }
                };
                let ps = self.viewport.doc_to_screen(p, origin);
                let d = screen.distance(ps);
                if d < threshold {
                    let prefer = matches!(
                        target,
                        PathEditTarget::HandleOut(_) | PathEditTarget::HandleIn(_)
                    );
                    let replace = best.as_ref().map_or(true, |(_, bt, bd)| {
                        if prefer && !matches!(bt, PathEditTarget::Anchor(_)) {
                            d < *bd
                        } else if prefer {
                            true
                        } else {
                            d < *bd
                        }
                    });
                    if replace {
                        best = Some((id, target, d));
                    }
                }
            }
        }
        best.map(|(id, target, _)| (id, target))
    }

    fn tool_node(
        &mut self,
        screen: Pos2,
        origin: Pos2,
        doc: (f64, f64),
        shift: bool,
        ctrl: bool,
        pressed: bool,
        down: bool,
        released: bool,
        _released_anywhere: bool,
        double_clicked: bool,
    ) {
        if released && !self.tools.select.drag_snapshot.is_empty() {
            self.commit_drag_edits();
            self.tools.select.node_drag_origin = None;
            self.tools.select.node_drag_active = false;
            return;
        }

        if double_clicked {
            self.tools.select.drag_snapshot.clear();
            self.tools.select.node_edit_target = None;
            self.tools.select.node_drag_origin = None;
            self.tools.select.node_drag_active = false;
            if let Some((id, from, to, px, py)) = self.hit_path_segment(screen, origin, doc) {
                let Some(before) = self.project.nodes.get(id).cloned() else {
                    return;
                };
                let mut after = before.clone();
                if let NodeKind::Path { path } = &mut after.kind {
                    let anchor_count = path.anchor_positions().len();
                    let new_idx = if to > from { to } else { anchor_count };
                    path.insert_anchor_on_segment(from, to, px, py);
                    self.history.push(
                        &mut self.project,
                        ProjectEdit::PatchNode { id, before, after },
                    );
                    self.tools.select.set_path_segment(id, from, new_idx);
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                    self.status_message = "Added point on path".into();
                }
                return;
            }
            if let Some((id, PathEditTarget::Anchor(pi))) = self.hit_node_edit(screen, origin) {
                if self.project.nodes.get(id).is_some_and(|n| matches!(n.kind, NodeKind::Path { .. })) {
                    self.set_path_anchor_smooth(id, pi, {
                        self.project
                            .nodes
                            .get(id)
                            .and_then(|n| match &n.kind {
                                NodeKind::Path { path } => {
                                    Some(!path.is_anchor_smooth(pi))
                                }
                                _ => None,
                            })
                            .unwrap_or(true)
                    });
                    self.tools.select.set_single_path_point(id, pi);
                    ui::promote_action_tab(self, ui::ActionTab::Geometry);
                }
            }
            return;
        }

        if pressed {
            if let Some((id, target)) = self.hit_node_edit(screen, origin) {
                let pi = target.anchor_index();
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                    self.sync_inspector_from_selection();
                }
                if self
                    .project
                    .nodes
                    .get(id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                {
                    if matches!(target, PathEditTarget::Anchor(_)) {
                        self.tools.select.toggle_path_point(id, pi, ctrl);
                        if ctrl {
                            ui::promote_action_tab(self, ui::ActionTab::Geometry);
                            self.status_message =
                                format!("{} point(s) selected", self.tools.select.selected_path_points.len());
                            return;
                        }
                    } else {
                        self.tools.select.set_single_path_point(id, pi);
                    }
                } else {
                    self.tools.select.clear_path_point_selection();
                }
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                let Some(node) = self.project.nodes.get(id) else {
                    return;
                };
                self.tools.select.drag_snapshot = vec![(id, node.clone())];
                self.tools.select.drag_mode = Some(SelectDrag::Move);
                self.tools.select.node_edit_target = Some(target);
                self.tools.select.node_drag_origin = Some(doc);
                self.tools.select.last_doc = doc;
                self.tools.select.node_drag_active = false;
                return;
            }

            if let Some((id, from, to, _, _)) = self.hit_path_segment(screen, origin, doc) {
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                    self.sync_inspector_from_selection();
                }
                self.tools.select.set_path_segment(id, from, to);
                ui::promote_action_tab(self, ui::ActionTab::Geometry);
                return;
            }

            let mut hit: Option<NodeId> = None;
            let mut bbox_only: Option<NodeId> = None;
            for id in self.project.document.ordered_node_ids().into_iter().rev() {
                if let Some(node) = self.project.nodes.get(id) {
                    let does_hit = if self.node_has_tiling_or_circular(id) {
                        let eb = crate::document::get_effective_bounds(node, &self.project.document);
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let slop = 4.0 / self.viewport.zoom as f64;
                        eb.inflate(slop, slop).contains(pt)
                    } else {
                        node.hit_test_with_store(
                            &self.project.nodes,
                            doc.0,
                            doc.1,
                            4.0 / self.viewport.zoom as f64,
                        )
                    };
                    if does_hit {
                        let pt = kurbo::Point::new(doc.0, doc.1);
                        let precise = if self.node_has_tiling_or_circular(id) {
                            true
                        } else {
                            node.bez_path().contains(pt)
                                || matches!(node.kind, NodeKind::Text { .. })
                        };
                        if precise {
                            hit = Some(id);
                            break;
                        } else if bbox_only.is_none() {
                            bbox_only = Some(id);
                        }
                    }
                }
            }
            if hit.is_none() {
                hit = bbox_only;
            }
            if let Some(id) = hit {
                if shift {
                    if self.selection.contains(&id) {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection.push(id);
                    }
                } else if self.selection.len() == 1 && self.selection[0] == id {
                    self.tools.select.clear_path_point_selection();
                    self.tools.select.selected_path_segment = None;
                } else {
                    self.selection = vec![id];
                    self.tools.select.clear_path_point_selection();
                    self.tools.select.selected_path_segment = None;
                }
                self.sync_inspector_from_selection();
            } else if !shift {
                self.tools.select.clear_path_point_selection();
            }
            return;
        }

        if down {
            if let (Some((id, _)), Some(target)) = (
                self.tools.select.drag_snapshot.first(),
                self.tools.select.node_edit_target,
            ) {
                let threshold = 3.0 / self.viewport.zoom as f64;
                if let Some(origin) = self.tools.select.node_drag_origin {
                    if !self.tools.select.node_drag_active {
                        let moved = (doc.0 - origin.0).hypot(doc.1 - origin.1);
                        if moved < threshold {
                            return;
                        }
                        self.tools.select.node_drag_active = true;
                    }
                }
                if self.tools.select.node_drag_active {
                    let indices = self.tools.select.points_on_path(*id);
                    if matches!(target, PathEditTarget::Anchor(_)) && indices.len() > 1 {
                        let dx = doc.0 - self.tools.select.last_doc.0;
                        let dy = doc.1 - self.tools.select.last_doc.1;
                        self.tools.select.last_doc = doc;
                        if let Some(node) = self.project.nodes.get_mut(*id) {
                            if let NodeKind::Path { path } = &mut node.kind {
                                path.move_anchors_by(&indices, dx, dy);
                            }
                        }
                    } else if let Some(node) = self.project.nodes.get_mut(*id) {
                        node.apply_path_edit_target(target, doc.0, doc.1);
                        self.tools.select.last_doc = doc;
                    }
                }
            }
        }
    }
}

impl eframe::App for VadadeeBerryApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "android")]
        {
            if let Some(id) = self.on_page_text_edit {
                if let Some(android_app) = crate::ANDROID_APP.get() {
                    let state = android_app.text_input_state();
                    if state.text != self.last_android_text {
                        self.ui_text_content = state.text.clone();
                        self.last_android_text = state.text.clone();
                        self.patch_on_page_text_live(id);
                        ctx.request_repaint();
                    } else if self.ui_text_content != self.last_android_text {
                        let text = self.ui_text_content.clone();
                        self.last_android_text = text.clone();
                        let len = text.chars().count();
                        let new_state = winit::platform::android::activity::input::TextInputState {
                            text: text.clone(),
                            selection: winit::platform::android::activity::input::TextSpan { start: len, end: len },
                            compose_region: None,
                        };
                        android_app.set_text_input_state(new_state);
                    }
                }
            }
        }

        self.process_file_dialogs();
        if self.paste_progress.is_some() {
            self.advance_paste_operation(ctx);
        }
        let paste_from_events = self.handle_object_clipboard_shortcuts(ctx);
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        self.handle_paste_hotkey_fallback(ctx, paste_from_events);
        if self.ui_anim.needs_repaint() || self.paste_progress.is_some() {
            ctx.request_repaint();
        }
        self.keyboard_shortcuts(ctx);
        self.canvas_wheel_zoom(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        ui::chrome(self, ui);
    }
}

fn generate_brush_outline(points: &[([f64; 2], f64, f32)]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    if points.len() < 2 {
        return path;
    }
    let n = points.len();
    let mut left_pts = Vec::with_capacity(n);
    let mut right_pts = Vec::with_capacity(n);

    for i in 0..n {
        let (pos, _, w) = points[i];
        let half_w = (w / 2.0) as f64;

        let normal = if i == 0 {
            let next_pos = points[1].0;
            let dx = next_pos[0] - pos[0];
            let dy = next_pos[1] - pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else if i == n - 1 {
            let prev_pos = points[n - 2].0;
            let dx = pos[0] - prev_pos[0];
            let dy = pos[1] - prev_pos[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0001 {
                [-dy / len, dx / len]
            } else {
                [0.0, 1.0]
            }
        } else {
            let prev_pos = points[i - 1].0;
            let next_pos = points[i + 1].0;
            let dx1 = pos[0] - prev_pos[0];
            let dy1 = pos[1] - prev_pos[1];
            let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();

            let dx2 = next_pos[0] - pos[0];
            let dy2 = next_pos[1] - pos[1];
            let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();

            let nx1 = if len1 > 0.0001 { -dy1 / len1 } else { 0.0 };
            let ny1 = if len1 > 0.0001 { dx1 / len1 } else { 1.0 };

            let nx2 = if len2 > 0.0001 { -dy2 / len2 } else { 0.0 };
            let ny2 = if len2 > 0.0001 { dx2 / len2 } else { 1.0 };

            let nx = (nx1 + nx2) / 2.0;
            let ny = (ny1 + ny2) / 2.0;
            let nlen = (nx * nx + ny * ny).sqrt();
            if nlen > 0.0001 {
                [nx / nlen, ny / nlen]
            } else {
                [0.0, 1.0]
            }
        };

        left_pts.push([pos[0] + normal[0] * half_w, pos[1] + normal[1] * half_w]);
        right_pts.push([pos[0] - normal[0] * half_w, pos[1] - normal[1] * half_w]);
    }

    path.move_to(kurbo::Point::new(left_pts[0][0], left_pts[0][1]));
    for pt in left_pts.iter().skip(1) {
        path.line_to(kurbo::Point::new(pt[0], pt[1]));
    }
    for pt in right_pts.iter().rev() {
        path.line_to(kurbo::Point::new(pt[0], pt[1]));
    }
    path.close_path();
    path
}