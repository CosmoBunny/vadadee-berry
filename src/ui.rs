use egui::{scroll_area::ScrollBarVisibility, Context, FontFamily, FontId, Rect, RichText, ScrollArea, Ui};

use crate::animation::action_bar_overlay_rect;
use crate::app::{AudioExtractStatus, KeyframeTrack, VadadeeBerryApp};
use crate::document::{
    compute_whole_object_bounds, compute_tiling_whole_bounds, compute_circular_whole_bounds, default_loft_gap_for_node, find_effect_for_pair, ArcJoin, FillKind, GeometryProfile, LineCap,
    LineJoin, NodeKind, OnPathMode, StrokePaintOrder, TextStyle,
};
use crate::gradient_ui::{
    apply_angle_to_flow_line, gradient_flow_line_editor, gradient_strip_editor,
    linear_gradient_angle_dial, paint_kind_selector, solid_color_editor, sync_angle_from_flow_line,
    GradientEditorFocus,
};
use crate::icons::{self, nerd_font_id};
use crate::io;
use crate::theme::{self, colors};
use crate::tools::ToolKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionTab {
    Export,
    #[default]
    Layer,
    ColorStroke,
    Objects,
    Geometry,
    PathMagic,
    Animation,
    /// Graph parameters for Node Editor layers (animatable).
    Parameter,
}

impl ActionTab {
    /// Wire slug for collaboration UI sync.
    pub fn collab_slug(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Layer => "layer",
            Self::ColorStroke => "color_stroke",
            Self::Objects => "objects",
            Self::Geometry => "geometry",
            Self::PathMagic => "path_magic",
            Self::Animation => "animation",
            Self::Parameter => "parameter",
        }
    }

    pub fn from_collab_slug(s: &str) -> Option<Self> {
        match s {
            "export" => Some(Self::Export),
            "layer" => Some(Self::Layer),
            "color_stroke" => Some(Self::ColorStroke),
            "objects" => Some(Self::Objects),
            "geometry" => Some(Self::Geometry),
            "path_magic" => Some(Self::PathMagic),
            "animation" => Some(Self::Animation),
            "parameter" => Some(Self::Parameter),
            _ => None,
        }
    }

    pub fn all_tabs() -> Vec<Self> {
        vec![
            Self::Export,
            Self::Layer,
            Self::ColorStroke,
            Self::Objects,
            Self::Geometry,
            Self::PathMagic,
            Self::Animation,
            Self::Parameter,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Export => "Export",
            Self::Layer => "Layer",
            Self::ColorStroke => "Color & stroke",
            Self::Objects => "Objects",
            Self::Geometry => "Geometry",
            Self::PathMagic => "Path magic",
            Self::Animation => "Animation",
            Self::Parameter => "Parameter",
        }
    }

    /// Tab label in the action bar (video layer → "Color" only; audio hides this tab).
    fn strip_label(self, app: &crate::app::VadadeeBerryApp) -> String {
        if self == Self::ColorStroke {
            if let Some(crate::document::LayerKind::AV) = app.selected_layer_kind() {
                return "Color".into();
            }
        }
        self.label().to_string()
    }

    fn visible_in_strip(self, app: &crate::app::VadadeeBerryApp) -> bool {
        if self == Self::ColorStroke {
            if let Some(crate::document::LayerKind::AV) = app.selected_layer_kind() {
                return true; // AV acts like Video for color (merged from Audio/Video)
            }
        }
        if self == Self::Parameter {
            // Active Node Editor layer is enough (do not require layer id in selection).
            if app
                .project
                .document
                .active_layer()
                .is_some_and(|l| l.kind == crate::document::LayerKind::NodeEditor)
            {
                return true;
            }
            return app
                .selected_layer_kind()
                .is_some_and(|k| k == crate::document::LayerKind::NodeEditor);
        }
        true
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Export => "⤓",
            Self::Layer => icons::LAYER,
            Self::ColorStroke => icons::COLOR,
            Self::Objects => icons::OBJECT,
            Self::Geometry => icons::RECT,
            Self::PathMagic => icons::PATH_MAGIC,
            Self::Animation => "",
            Self::Parameter => icons::PARAMETER,
        }
    }
}

/// Coarse coordinate steps for the status bar so tiny mouse jitter does not restart
/// slide animations and trigger 60 fps repaints.
fn status_coords_text(cursor_doc: Option<(f64, f64)>) -> String {
    match cursor_doc {
        Some((x, y)) => {
            let x = (x * 2.0).round() / 2.0;
            let y = (y * 2.0).round() / 2.0;
            format!("X: {x:.1}  Y: {y:.1}")
        }
        None => "...".into(),
    }
}

/// All chrome must use `show_inside(ui)` on eframe 0.34's root [`Ui`].
/// `Panel::show(ctx)` does not lay out with `run_ui` and bars will not appear.
pub fn chrome(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    menubar(app, ui);

    let action_text = app.derive_action_status(ui.ctx());
    if VadadeeBerryApp::is_ephemeral_status_event(&app.status_message) {
        // Flash the event (Pasted, Pen cancelled, Undo, etc.) then settle to Idle (or live action).
        app.status_message.clear();
    }
    let msg_width = theme::measure_status_label(ui, &action_text);
    let tool_width = theme::measure_status_label(ui, app.tools.active.label());
    let coords_text = status_coords_text(app.cursor_doc);
    let coords_width = if app.cursor_doc.is_some() {
        theme::measure_status_label(ui, &coords_text)
    } else {
        0.0
    };
    app.ui_anim.sync(
        app.action_bar_open,
        app.anim_show_timeline_window,
        app.show_video_editor_window.is_some(),
        app.tools.active,
        app.action_tab,
        &action_text,
        msg_width,
        tool_width,
        &coords_text,
        coords_width,
    );
    app.ui_anim.sync_left_dock(app.left_dock.active);
    app.ui_anim.advance_action_bar_slide(ui.ctx());
    app.ui_anim.advance_timeline_slide(ui.ctx());
    app.ui_anim.advance_video_editor_slide(ui.ctx());
    app.ui_anim.advance_left_dock_slide(ui.ctx());
    app.ui_anim.tick(ui.ctx());
    video_export_progress_window(app, ui.ctx());
    shader_editor_window(app, ui.ctx());
    object_rename_dialog(app, ui.ctx());
    plotter_formula_dialog(app, ui.ctx());
    daw_piano_dialog(app, ui.ctx());
    crate::node_editor_ui::show_node_editor_dialog(app, ui.ctx());
    hit_pick_menu_overlay(app, ui.ctx());
    status_bar_layout_reserve(ui);

    let canvas_alpha = app.ui_anim.canvas_alpha();
    egui::CentralPanel::default()
        .frame(theme::canvas_frame(canvas_alpha))
        .show_inside(ui, |ui| {
            let canvas_work = ui.available_rect_before_wrap();
            let floater_work = theme::floater_work_rect(canvas_work);
            app.canvas_ui(ui);
            app.tick_live_collaboration_after_canvas(ui.ctx());
            app.tools.handle_shortcuts(ui);
            let ctx = ui.ctx().clone();
            floating_toolbar(app, &ctx, canvas_work);
            floating_action_bar(app, &ctx, canvas_work);
            floating_timeline_window(app, &ctx, floater_work);
            floating_video_editor(app, &ctx, floater_work);
            crate::left_dock::show(app, &ctx, canvas_work);
        });

    crate::left_dock::show_chat_toasts(app, ui.ctx());
    status_bar_overlay(app, ui.ctx());

    let ctx = ui.ctx();
    if app.ui_anim.needs_repaint() || app.is_pasting() {
        ctx.request_repaint();
    }
}

fn menubar_action_toggle(ui: &mut Ui, icon: &str, tip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).font(icons::nerd_font_id(16.0)).color(colors::TEXT))
            .fill(colors::BG_ELEVATED)
            .stroke(egui::Stroke::new(1.0, colors::BORDER))
            .min_size(egui::vec2(26.0, 24.0)),
    )
    .on_hover_text(tip)
}

fn menubar(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    let alpha = app.ui_anim.menubar_alpha();
    egui::Panel::top("menubar")
        .frame(theme::bar_frame(alpha))
        .exact_size(32.0)
        .resizable(false)
        .show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New A4 page   Ctrl+N").clicked() {
                        app.new_document();
                        ui.close();
                    }
                    if ui.button("Open project…   Ctrl+O").clicked() {
                        app.request_open_project();
                        ui.close();
                    }
                    if ui.button("Open SVG…").clicked() {
                        app.request_open_svg();
                        ui.close();
                    }
                    if ui.button("Import Image…").clicked() {
                        app.request_import_image();
                        ui.close();
                    }
                    if ui.button("Save project   Ctrl+S").clicked() {
                        app.request_save_project();
                        ui.close();
                    }
                    if ui.button("Live collaboration…").clicked() {
                        app.left_dock.toggle(crate::left_dock::LeftDockPanel::Collab);
                        ui.close();
                    }
                    if ui.button("Export SVG…").clicked() {
                        app.request_export_svg();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(app.history.can_undo(), egui::Button::new("Undo   Ctrl+Z"))
                        .clicked()
                    {
                        app.do_undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            app.history.can_redo(),
                            egui::Button::new("Redo   Ctrl+Shift+Z"),
                        )
                        .clicked()
                    {
                        app.do_redo();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!app.selection.is_empty(), egui::Button::new("Cut   Ctrl+X"))
                        .clicked()
                    {
                        app.cut_selection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(!app.selection.is_empty(), egui::Button::new("Copy   Ctrl+C"))
                        .clicked()
                    {
                        app.copy_selection();
                        ui.close();
                    }
                    if ui.button("Paste   Ctrl+V").clicked() {
                        app.paste_clipboard(false);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Delete   Del").clicked() {
                        app.delete_selection_public();
                        ui.close();
                    }
                });
                ui.menu_button("Object", |ui| {
                    if ui.button("Duplicate   Ctrl+D").clicked() {
                        app.duplicate_selection();
                        ui.close();
                    }
                    if ui
                        .button("Raise")
                        .on_hover_text("Raise vs video/audio layers, or within image layer")
                        .clicked()
                    {
                        app.nudge_z_order(1);
                        ui.close();
                    }
                    if ui
                        .button("Lower")
                        .on_hover_text("Lower vs video/audio layers, or within image layer")
                        .clicked()
                    {
                        app.nudge_z_order(-1);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Flip", |ui| {
                        if ui
                            .button("⟺  Flip Horizontal")
                            .on_hover_text("Ctrl+Shift+H")
                            .clicked()
                        {
                            app.flip_selection(true);
                            ui.close();
                        }
                        if ui
                            .button("⟹  Flip Vertical")
                            .on_hover_text("Ctrl+Shift+V")
                            .clicked()
                        {
                            app.flip_selection(false);
                            ui.close();
                        }
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut app.viewport.show_grid, "Show grid");
                    ui.checkbox(&mut app.viewport.snap_grid, "Snap to grid");
                    ui.checkbox(&mut app.snap_magnet, "Magnetic snap");
                    ui.checkbox(&mut app.pixel_art_mode, "Pixel art mode");
                    if app.pixel_art_mode {
                        ui.add(egui::Slider::new(&mut app.pixel_cell_size, 0.5..=10.0).text("Cell size"));
                    }
                    ui.separator();
                    ui.checkbox(&mut app.gpu_shading, "GPU shading (WGSL)")
                        .on_hover_text(
                            "Compile and run shading layer WGSL on the GPU. \
                             Edits to the WGSL source apply after recompile (toggle pass or restart).",
                        );
                    if ui
                        .checkbox(
                            &mut app.enable_layer_raster_cache,
                            "Layer raster cache",
                        )
                        .on_hover_text(
                            "Caches dense vector layers as textures for smoother pan/zoom. \
                             Best for many rectangles; leave off when text looks shifted or blurry.",
                        )
                        .changed()
                        && !app.enable_layer_raster_cache
                    {
                        app.status_message =
                            "Layer raster cache disabled — drawing vectors directly.".into();
                    }
                    if ui.button("Zoom 100%").clicked() {
                        app.viewport.zoom = 1.0;
                    }
                    if ui.button("Fit A4 page").clicked() {
                        app.viewport.zoom = 0.85;
                        app.viewport.pan = egui::vec2(48.0, 48.0);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, tip) = if app.action_bar_open {
                        (icons::ACTION_HIDE, "Hide action bar")
                    } else {
                        (icons::ACTION_SHOW, "Show action bar")
                    };
                    if menubar_action_toggle(ui, icon, tip).clicked() {
                        app.action_bar_open = !app.action_bar_open;
                        // reset stored sizes so panels expand/contract with available space
                        app.timeline_container_w = 0.0;
                        app.video_editor_container_w = 0.0;
                    }
                    ui.label(
                        RichText::new(format!(
                            "{} · {:.0}×{:.0}",
                            app.project.document.title,
                            app.project.document.width,
                            app.project.document.height
                        ))
                        .small()
                        .color(colors::TEXT_MUTED),
                    );
                });
            });
        });
}

fn floating_toolbar(app: &mut VadadeeBerryApp, ctx: &Context, work: Rect) {
    let alpha = app.ui_anim.toolbar_alpha();
    let inset = theme::overlay_work_rect(work);

    let is_android = cfg!(target_os = "android");
    let btn_size = if is_android { 48.0 } else { 40.0 };
    let spacing = if is_android { 8.0 } else { 6.0 };
    let margin_x = 8.0;
    let margin_y = 10.0;

    // Collapsed = active tool only. Chat/Collab live in a separate rail under the toolbar.
    let collapsed_inner_w = btn_size;
    let collapsed_inner_h = btn_size;

    let is_video_or_audio_layer = app.project.document.active_layer()
        .map_or(false, |l| l.kind == crate::document::LayerKind::AV);
    let is_flowchart_layer = app.project.document.active_layer()
        .map_or(false, |l| l.kind == crate::document::LayerKind::Flowchart);
    let is_node_editor_layer = app.project.document.active_layer()
        .map_or(false, |l| l.kind == crate::document::LayerKind::NodeEditor);
    if is_video_or_audio_layer && app.tools.active != ToolKind::Select && app.tools.active != ToolKind::Eyedropper {
        app.tools.active = ToolKind::Select;
    }
    if is_flowchart_layer && matches!(app.tools.active, ToolKind::Text | ToolKind::Brush | ToolKind::Pen) {
        app.tools.active = ToolKind::Select;
    }
    // Node Editor: no drawing tools (Circle/Rect/Pen/…).
    if is_node_editor_layer && app.tools.active != ToolKind::Select {
        app.tools.active = ToolKind::Select;
    }

    // Tools list
    let tools = if is_video_or_audio_layer {
        vec![
            ToolKind::Select,
            ToolKind::Eyedropper,
        ]
    } else if is_node_editor_layer {
        vec![ToolKind::Select]
    } else if is_flowchart_layer {
        vec![
            ToolKind::Select,
            ToolKind::Node,
            ToolKind::Rectangle,
            ToolKind::Line,
            ToolKind::Eyedropper,
        ]
    } else {
        vec![
            ToolKind::Select,
            ToolKind::Node,
            ToolKind::Pen,
            ToolKind::Rectangle,
            ToolKind::Circle,
            ToolKind::Ellipse,
            ToolKind::Line,
            ToolKind::Polygon,
            ToolKind::Arc,
            ToolKind::Plotter,
            ToolKind::Text,
            ToolKind::Brush,
            ToolKind::Eyedropper,
        ]
    };
    // AV Split + DAW are one-shot actions (not ToolKind) — only when an AV layer is selected.
    let av_action_count = if is_video_or_audio_layer { 2usize } else { 0 };
    let total_slots = tools.len() + av_action_count;
    let expanded_rows = total_slots.div_ceil(3).max(1);
    let expanded_inner_w = 3.0 * btn_size + 2.0 * spacing;
    let expanded_inner_h =
        expanded_rows as f32 * btn_size + (expanded_rows.saturating_sub(1) as f32) * spacing;

    // Use egui's built-in bool animator for smooth transitions
    let expand_t = ctx.animate_bool(egui::Id::new("toolbar_expanded_anim"), app.toolbar_expanded);

    let inner_w = egui::lerp(collapsed_inner_w..=expanded_inner_w, expand_t);
    let inner_h = egui::lerp(collapsed_inner_h..=expanded_inner_h, expand_t);

    let rect = Rect::from_min_size(
        inset.min,
        egui::vec2(inner_w + 2.0 * margin_x, inner_h + 2.0 * margin_y),
    );
    app.toolbar_outer_rect = Some(rect);

    let get_tool_icon = |tool: ToolKind, polygon_sides: u32| -> &'static str {
        match tool {
            ToolKind::Select => icons::SELECT,
            ToolKind::Node => icons::NODE,
            ToolKind::Pen => icons::PEN,
            ToolKind::Rectangle => icons::RECT,
            ToolKind::Circle => icons::CIRCLE,
            ToolKind::Ellipse => icons::ELLIPSE,
            ToolKind::Line => icons::LINE,
            ToolKind::Polygon => icons::polygon_icon(polygon_sides),
            ToolKind::Arc => icons::ARC,
            ToolKind::Plotter => icons::PLOTTER,
            ToolKind::Text => icons::TEXT,
            ToolKind::Brush => icons::BRUSH,
            ToolKind::Eyedropper => icons::EYE_DROPPER,
        }
    };

    let get_tool_tip = |tool: ToolKind| -> &'static str {
        match tool {
            ToolKind::Select => "Select (V)",
            ToolKind::Node => "Edit nodes (N)",
            ToolKind::Pen => "Pen (P)",
            ToolKind::Rectangle => "Rectangle (R)",
            ToolKind::Circle => "Circle (C)",
            ToolKind::Ellipse => "Ellipse (E)",
            ToolKind::Line => "Line (L)",
            ToolKind::Polygon => "Polygon (G)",
            ToolKind::Arc => "Arc / Chord (A)",
            ToolKind::Plotter => "Plotter f(x)/f(y) (M)",
            ToolKind::Text => "Text (T)",
            ToolKind::Brush => "Brush (B)",
            ToolKind::Eyedropper => "Eyedropper (I)",
        }
    };

    let get_grid_pos = |index: usize| -> (f32, f32) {
        let col = index % 3;
        let row = index / 3;
        let x = col as f32 * (btn_size + spacing);
        let y = row as f32 * (btn_size + spacing);
        (x, y)
    };

    // Find active tool index
    let active_index = tools.iter().position(|&t| t == app.tools.active).unwrap_or(0);
    let (ax_grid, ay_grid) = get_grid_pos(active_index);

    // Active button position lerps from (0,0) (collapsed) to its grid position
    let ax = egui::lerp(0.0..=ax_grid, expand_t);
    let ay = egui::lerp(0.0..=ay_grid, expand_t);

    // Pointer events
    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
    let pointer_down = ctx.input(|i| i.pointer.any_down());
    let pointer_released = ctx.input(|i| i.pointer.any_released());

    let collapsed_btn_rect = Rect::from_min_size(
        rect.min + egui::vec2(margin_x, margin_y),
        egui::vec2(btn_size, btn_size),
    );

    // 1. If collapsed, detect press/drag start on the collapsed button
    if !app.toolbar_expanded {
        if pointer_down {
            if let Some(pos) = pointer_pos {
                if collapsed_btn_rect.contains(pos) {
                    app.toolbar_expanded = true;
                    app.toolbar_drag_active = true;
                    ctx.request_repaint();
                }
            }
        }
    }

    // 2. Click outside when toggled open to collapse
    if app.toolbar_expanded && !app.toolbar_drag_active {
        if pointer_down {
            if let Some(pos) = pointer_pos {
                if !rect.contains(pos) && !ctx.memory(|mem| mem.any_popup_open()) {
                    app.toolbar_expanded = false;
                    ctx.request_repaint();
                }
            }
        }
    }

    let mut hovered_tool: Option<ToolKind> = None;
    let mut hovered_av_action_outer: Option<&'static str> = None;

    theme::show_overlay_area(ctx, "float_toolbar", rect, alpha, |ui| {
        // Track the mouse coordinates and find if any button is hovered
        let local_origin = ui.max_rect().min; // Top-left of the inner frame (after margins)

        for (i, &tool) in tools.iter().enumerate() {

            // Get target grid position
            let (gx, gy) = get_grid_pos(i);

            // Interpolate position and size
            let (cx, cy) = if i == active_index {
                (ax, ay)
            } else {
                (gx, gy)
            };

            let scale = if i == active_index {
                1.0
            } else {
                egui::lerp(0.6..=1.0, expand_t)
            };

            let btn_w = btn_size * scale;
            let center = egui::Pos2::new(cx + btn_size / 2.0, cy + btn_size / 2.0);
            let local_rect = Rect::from_center_size(center, egui::vec2(btn_w, btn_w));
            let button_screen_rect = local_rect.translate(local_origin.to_vec2());

            let is_hovered = pointer_pos.map_or(false, |pos| button_screen_rect.contains(pos));

            // Only allow hover interaction when expanded
            let hovered = is_hovered && (app.toolbar_expanded || expand_t > 0.9);
            if hovered {
                hovered_tool = Some(tool);
            }

            // Determine rendering alpha
            let button_alpha = if i == active_index {
                alpha
            } else {
                alpha * expand_t
            };

            if button_alpha > 0.01 {
                let selected = app.tools.active == tool;

                let fill = if selected {
                    if hovered {
                        colors::ACCENT.gamma_multiply(0.7).gamma_multiply(button_alpha)
                    } else {
                        colors::ACCENT.gamma_multiply(0.55).gamma_multiply(button_alpha)
                    }
                } else if hovered {
                    colors::BG_HOVER.gamma_multiply(button_alpha)
                } else {
                    colors::BG_ELEVATED.gamma_multiply(button_alpha)
                };

                let stroke_color = if selected {
                    colors::ACCENT.gamma_multiply(button_alpha)
                } else if hovered {
                    colors::BORDER.gamma_multiply(button_alpha * 1.5)
                } else {
                    colors::BORDER.gamma_multiply(button_alpha)
                };

                let stroke_w = if selected || hovered { 1.5 } else { 1.0 };
                let corner_radius = egui::CornerRadius::same(if is_android { 8 } else { 6 });

                // Draw button rect
                ui.painter().rect(
                    button_screen_rect,
                    corner_radius,
                    fill,
                    egui::Stroke::new(stroke_w, stroke_color),
                    egui::StrokeKind::Inside,
                );

                // Draw icon text
                let icon = get_tool_icon(tool, app.polygon_sides);
                let icon_size = if is_android { 20.0 } else { 18.0 };
                ui.painter().text(
                    button_screen_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    icons::nerd_font_id(icon_size * scale),
                    colors::TEXT.gamma_multiply(button_alpha),
                );

                // Draw simple tooltip on desktop
                if hovered && !is_android {
                    egui::show_tooltip::<()>(ui.ctx(), ui.layer_id(), ui.make_persistent_id("tool_tip"), |ui| {
                        ui.label(get_tool_tip(tool));
                    });
                }
            }
        }

        // AV-only one-shot actions: Split + DAW (only when AV layer is selected).
        if is_video_or_audio_layer && expand_t > 0.01 {
            let av_actions: [(&str, &str, &str); 2] = [
                (icons::SPLIT, "Split", "Split/cut clip at playhead"),
                (icons::MUSIC, "DAW", "Create 1s DAW node on DAW layer (double-click opens piano)"),
            ];
            for (i, (icon, _label, tip)) in av_actions.iter().enumerate() {
                let (gx, gy) = get_grid_pos(tools.len() + i);
                let scale = egui::lerp(0.6..=1.0, expand_t);
                let btn_w = btn_size * scale;
                let center = egui::Pos2::new(gx + btn_size / 2.0, gy + btn_size / 2.0);
                let local_rect = Rect::from_center_size(center, egui::vec2(btn_w, btn_w));
                let button_screen_rect = local_rect.translate(local_origin.to_vec2());
                let is_hovered = pointer_pos.map_or(false, |pos| button_screen_rect.contains(pos));
                let hovered = is_hovered && (app.toolbar_expanded || expand_t > 0.9);
                if hovered {
                    hovered_av_action_outer = Some(if i == 0 { "split" } else { "daw" });
                }
                let button_alpha = alpha * expand_t;
                let fill = if hovered {
                    colors::BG_HOVER.gamma_multiply(button_alpha)
                } else {
                    colors::BG_ELEVATED.gamma_multiply(button_alpha)
                };
                ui.painter().rect(
                    button_screen_rect,
                    egui::CornerRadius::same(if is_android { 8 } else { 6 }),
                    fill,
                    egui::Stroke::new(
                        if hovered { 1.5 } else { 1.0 },
                        colors::BORDER.gamma_multiply(button_alpha),
                    ),
                    egui::StrokeKind::Inside,
                );
                let icon_size = if is_android { 20.0 } else { 18.0 };
                ui.painter().text(
                    button_screen_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    *icon,
                    icons::nerd_font_id(icon_size * scale),
                    colors::TEXT.gamma_multiply(button_alpha),
                );
                if hovered && !is_android {
                    egui::show_tooltip::<()>(
                        ui.ctx(),
                        ui.layer_id(),
                        ui.make_persistent_id(("av_action_tip", i)),
                        |ui| {
                            ui.label(*tip);
                        },
                    );
                }
            }
        }

        // Draw ColorPicker at index 12 (image layers only — after all drawing tools)
        if expand_t > 0.01 && !is_video_or_audio_layer && !is_flowchart_layer {
            let (gx, gy) = get_grid_pos(12);
            let cx = gx;
            let cy = gy;
            let scale = egui::lerp(0.6..=1.0, expand_t);
            let btn_w = btn_size * scale;
            let center = egui::Pos2::new(cx + btn_size / 2.0, cy + btn_size / 2.0);
            let button_screen_rect = Rect::from_center_size(center, egui::vec2(btn_w, btn_w))
                .translate(local_origin.to_vec2());

            let button_alpha = alpha * expand_t;

            if button_alpha > 0.01 {
                let mut c = if app.tools.active == ToolKind::Brush {
                    app.tools.brush.fill_stops.first().map(|s| s.color.to_egui()).unwrap_or(egui::Color32::WHITE)
                } else {
                    app.ui_fill_stops.first().map(|s| s.color.to_egui()).unwrap_or(egui::Color32::WHITE)
                };
                
                // Render the color edit button inside the slot
                ui.allocate_ui_at_rect(button_screen_rect, |ui| {
                    ui.spacing_mut().interact_size = button_screen_rect.size();
                    let resp = ui.color_edit_button_srgba(&mut c);
                    if resp.changed() {
                        let paint = crate::document::Paint {
                            rgba: [
                                c.r() as f32 / 255.0,
                                c.g() as f32 / 255.0,
                                c.b() as f32 / 255.0,
                                c.a() as f32 / 255.0,
                            ],
                        };
                        if app.tools.active == ToolKind::Brush {
                            for s in app.tools.brush.fill_stops.iter_mut() {
                                s.color = paint;
                            }
                        } else {
                            for s in app.ui_fill_stops.iter_mut() {
                                s.color = paint;
                            }
                            for s in app.ui_stroke_stops.iter_mut() {
                                s.color = paint;
                            }
                            app.apply_fill_to_selection();
                            app.apply_stroke_to_selection();
                        }
                    }
                });

                // Draw a sleek color wheel/palette icon on top of it so the user knows it's a picker
                let icon_size = if is_android { 20.0 } else { 18.0 };
                let brightness = c.r() as f32 * 0.299 + c.g() as f32 * 0.587 + c.b() as f32 * 0.114;
                let text_color = if brightness > 150.0 {
                    egui::Color32::BLACK.gamma_multiply(button_alpha)
                } else {
                    egui::Color32::WHITE.gamma_multiply(button_alpha)
                };

                ui.painter().text(
                    button_screen_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::COLOR,
                    icons::nerd_font_id(icon_size * scale),
                    text_color,
                );
            }
        }

    });

    // Live Chat + Collab: separate container, left-aligned under the toolbar (not inside it).
    let collab_gap = 8.0;
    let collab_inner_h = 2.0 * btn_size + spacing;
    let collab_rect = Rect::from_min_size(
        egui::pos2(rect.min.x, rect.max.y + collab_gap),
        egui::vec2(btn_size + 2.0 * margin_x, collab_inner_h + 2.0 * margin_y),
    );
    theme::show_overlay_area(ctx, "float_collab_rail", collab_rect, alpha, |ui| {
        let origin = ui.max_rect().min;
        let collab_buttons: [(&str, crate::left_dock::LeftDockPanel, &str); 2] = [
            (icons::CHAT, crate::left_dock::LeftDockPanel::Chat, "Live chat"),
            (
                icons::COLLAB,
                crate::left_dock::LeftDockPanel::Collab,
                "Collaboration settings",
            ),
        ];
        for (i, (icon, panel, tip)) in collab_buttons.iter().enumerate() {
            let cy = i as f32 * (btn_size + spacing);
            let button_screen_rect = Rect::from_min_size(
                origin + egui::vec2(0.0, cy),
                egui::vec2(btn_size, btn_size),
            );
            let selected = app.left_dock.active == Some(*panel);
            let is_hovered = pointer_pos.map_or(false, |pos| button_screen_rect.contains(pos));
            let fill = if selected {
                colors::ACCENT_DIM.gamma_multiply(alpha)
            } else if is_hovered {
                colors::BG_HOVER.gamma_multiply(alpha)
            } else {
                colors::BG_ELEVATED.gamma_multiply(alpha)
            };
            ui.painter().rect(
                button_screen_rect,
                egui::CornerRadius::same(if is_android { 8 } else { 6 }),
                fill,
                egui::Stroke::new(
                    if selected || is_hovered { 1.5 } else { 1.0 },
                    colors::BORDER.gamma_multiply(alpha),
                ),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                button_screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                *icon,
                icons::nerd_font_id(if is_android { 20.0 } else { 18.0 }),
                colors::TEXT.gamma_multiply(alpha),
            );
            if is_hovered && !is_android {
                egui::show_tooltip::<()>(
                    ui.ctx(),
                    ui.layer_id(),
                    ui.make_persistent_id(("collab_tip", *panel as u8)),
                    |ui| {
                        ui.label(*tip);
                    },
                );
            }
            let collab_resp = ui.interact(
                button_screen_rect,
                ui.make_persistent_id(("collab_rail", *panel as u8)),
                egui::Sense::click(),
            );
            if collab_resp.clicked() {
                app.left_dock.toggle(*panel);
                app.toolbar_drag_active = false;
            }
        }
    });

    let select_tool = |app: &mut VadadeeBerryApp, tool: ToolKind| {
        if app.tools.active != ToolKind::Eyedropper {
            app.tools.last_active_tool = app.tools.active;
        }
        if tool != ToolKind::Brush && app.ui_stroke_width <= 0.01 {
            app.ui_stroke_width = 2.0;
        }
        app.tools.active = tool;
        match tool {
            ToolKind::Node | ToolKind::Polygon | ToolKind::Text | ToolKind::Arc => {
                promote_action_tab(app, ActionTab::Geometry);
            }
            ToolKind::Pen | ToolKind::Brush => {
                promote_action_tab(app, ActionTab::ColorStroke);
            }
            _ => {}
        }
    };

    // 3. Handle release actions
    if app.toolbar_expanded && pointer_released {
        if app.toolbar_drag_active {
            // Drag release
            if let Some(tool) = hovered_tool {
                // If it was just a quick tap inside the active button, don't drag-select, keep open
                if let Some(pos) = pointer_pos {
                    if collapsed_btn_rect.contains(pos) {
                        // Toggled open state
                        app.toolbar_drag_active = false;
                    } else {
                        // Drag-selected a tool!
                        select_tool(app, tool);
                        app.toolbar_expanded = false;
                        app.toolbar_drag_active = false;
                    }
                }
            } else if let Some(action) = hovered_av_action_outer {
                match action {
                    "split" => app.split_active_av_clip_at_playhead(),
                    "daw" => app.create_daw_clip_at_playhead(),
                    _ => {}
                }
                app.toolbar_expanded = false;
                app.toolbar_drag_active = false;
            } else {
                // Released outside -> collapse (unless a popup is open)
                if !ctx.memory(|mem| mem.any_popup_open()) {
                    app.toolbar_expanded = false;
                }
                app.toolbar_drag_active = false;
            }
        } else {
            // Clicked open state click
            if let Some(tool) = hovered_tool {
                select_tool(app, tool);
                app.toolbar_expanded = false;
            } else if let Some(action) = hovered_av_action_outer {
                match action {
                    "split" => app.split_active_av_clip_at_playhead(),
                    "daw" => app.create_daw_clip_at_playhead(),
                    _ => {}
                }
                app.toolbar_expanded = false;
            }
        }
        ctx.request_repaint();
    }
}

/// Programmatic tab focus (tool switch, geometry, etc.).
/// `position` is a zero-based index in the tab strip (clamped to the list length).
pub fn promote_action_tab(app: &mut VadadeeBerryApp, tab: ActionTab) {
    promote_action_tab_at(app, tab, 0);
}

pub fn promote_action_tab_at(app: &mut VadadeeBerryApp, tab: ActionTab, position: usize) {
    if app.action_tab != tab {
        app.ui_anim.on_tab_change();
    }
    app.action_tab_order.retain(|t| *t != tab);
    let pos = position.min(app.action_tab_order.len());
    app.action_tab_order.insert(pos, tab);
    app.action_tab = tab;
    app.action_tab_scroll_home = true;
}

/// User clicked a tab in the strip.
/// - 1st tab (index 0): slide-in animation, order unchanged.
/// - 2nd/3rd tabs (index 1–2): cross-fade only, order unchanged.
/// - 4th+ tabs (index ≥ 3): promote to front with full animation.
fn select_action_tab_from_strip(app: &mut VadadeeBerryApp, tab: ActionTab) {
    if app.action_tab == tab {
        return;
    }
    let idx = app
        .action_tab_order
        .iter()
        .position(|t| *t == tab)
        .unwrap_or(0);
    match idx {
        0 => {
            app.ui_anim.on_tab_change();
            app.action_tab = tab;
        }
        1 | 2 => {
            app.ui_anim.on_tab_change_secondary();
            app.action_tab = tab;
        }
        _ => promote_action_tab(app, tab),
    }
}

/// Single-line tab row: black track, horizontal scroll, distinct tab chips.
fn action_tab_strip(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    const TAB_ROW_H: f32 = 30.0;

    ui.add_space(4.0);
    theme::action_tab_track_frame().show(ui, |ui| {
        ui.set_min_height(TAB_ROW_H);
        ui.set_max_height(TAB_ROW_H);
        ScrollArea::horizontal()
            .id_salt("action_tab_scroll")
            .auto_shrink([false; 2])
            .animated(true)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let mut first_tab: Option<egui::Response> = None;
                    for (i, tab) in app.action_tab_order.clone().into_iter().enumerate() {
                        if !tab.visible_in_strip(app) {
                            continue;
                        }
                        let selected = app.action_tab == tab;
                        let tab_alpha = app.ui_anim.tab_label_alpha(selected);
                        let text = tab.strip_label(app);
                        let label = format!("{} {}", tab.icon(), text);
                        let resp = theme::action_tab_chip(ui, selected, &label, tab_alpha)
                            .on_hover_text(&text);
                        if i == 0 {
                            first_tab = Some(resp.clone());
                        }
                        if resp.clicked() {
                            select_action_tab_from_strip(app, tab);
                        }
                    }
                    if app.action_tab_scroll_home {
                        if let Some(r) = first_tab {
                            r.scroll_to_me(Some(egui::Align::LEFT));
                        }
                        app.action_tab_scroll_home = false;
                    }
                });
            });
    });
    ui.add_space(12.0);
}

fn action_bar_interior(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    if app.action_tab == ActionTab::ColorStroke {
        if let Some(crate::document::LayerKind::AV) = app.selected_layer_kind() {
            app.action_tab = ActionTab::Layer;
        }
    }
    ui.label(RichText::new("Actions").strong().color(colors::TEXT));
    ui.separator();
    action_tab_strip(app, ui);
    let tab_offset = app.ui_anim.tab_content_offset();
    let tab_alpha = app.ui_anim.tab_content_alpha();
    ui.add_space(tab_offset);
    theme::action_content_frame_alpha(tab_alpha).show(ui, |ui| {
        let w = ui.available_width();
        let content_h = ui.available_height().max(64.0);
        ui.set_width(w);
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(content_h)
            .show(ui, |ui| {
                ui.set_width(w);
                match app.action_tab {
                    ActionTab::Export => export_section(app, ui),
                    ActionTab::Layer => layers_section(app, ui),
                    ActionTab::ColorStroke => appearance_section(app, ui),
                    ActionTab::Objects => objects_section(app, ui),
                    ActionTab::Geometry => geometry_section(app, ui),
                    ActionTab::PathMagic => path_magic_section(app, ui),
                    ActionTab::Animation => animation_section(app, ui),
                    ActionTab::Parameter => crate::node_editor_ui::parameter_tab_ui(app, ui),
                }
            });
    });
}

/// Minimum reserved height before the Object on Path panel has been measured.
const ON_PATH_CONTAINER_MIN_H: f32 = 220.0;

fn path_magic_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    app.sync_on_path_ui_from_selection();
    app.sync_tiling_ui_from_selection();
    app.sync_circular_ui_from_selection();
    let on_path_offer = app.selection_path_and_objects().is_some()
        && !app.selection_has_object_on_path_effect();
    let on_path_container = app.selection_has_object_on_path_effect();
    app.ui_anim
        .sync_on_path(on_path_offer, on_path_container);

    let path_ids: Vec<_> = app
        .selection
        .iter()
        .filter(|id| {
            app.project
                .nodes
                .get(**id)
                .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
        })
        .copied()
        .collect();
    let open_path_ids: Vec<_> = path_ids
        .iter()
        .filter(|id| {
            app.project.nodes.get(**id).is_some_and(|n| {
                matches!(&n.kind, NodeKind::Path { path } if !path.is_closed())
            })
        })
        .copied()
        .collect();
    let other_count = app
        .selection
        .iter()
        .filter(|id| {
            app.project
                .nodes
                .get(**id)
                .is_some_and(|n| !matches!(n.kind, NodeKind::Path { .. }))
        })
        .count();

    ui.label(
        RichText::new(format!(
            "{} selected — {} path(s)",
            app.selection.len(),
            path_ids.len()
        ))
        .strong(),
    );
    if other_count > 0 {
        ui.label(
            RichText::new(format!("+ {other_count} other object(s)"))
                .small()
                .color(colors::TEXT_MUTED),
        );
    }
    // Note: Path point editing and Corner curve now live in Geometry tab, not here.
    // This section is for Path Magic effects only.

    if on_path_offer {
        if let Some((objects, path_id)) = app.selection_path_and_objects() {
            let pop = app.ui_anim.on_path_offer_pop();
            let obj_label = object_on_path_object_label(app, &objects);
            let rise = (1.0 - pop) * 14.0;
            let alpha = pop.clamp(0.0, 1.0);
            let scale = 0.86 + 0.14 * pop;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0 + rise),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.add_space(rise);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{obj_label} → path"))
                                .small()
                                .color(colors::TEXT_MUTED.gamma_multiply(alpha)),
                        );
                        if alpha > 0.02 {
                            let btn = egui::Button::new(
                                RichText::new("Object on Path")
                                    .strong()
                                    .color(colors::TEXT.gamma_multiply(alpha)),
                            )
                            .min_size(egui::vec2(100.0 * scale, 28.0 * scale));
                            if ui.add(btn).clicked() {
                                app.apply_object_on_path_effect();
                                ui.ctx().request_repaint();
                            }
                        }
                    });
                },
            );
            ui.add_space(4.0);
            let _ = path_id;
        }
    }

    // Boolean ops (shape+shape) or Clip Mask (image+shape)
    boolean_and_clip_panel(app, ui);

    if on_path_container {
        let expand = app.ui_anim.on_path_container_expand();
        let alpha = app.ui_anim.on_path_container_alpha();
        if expand > 0.004 || alpha > 0.004 {
            let mut settings_changed = false;
            let close = object_on_path_container(ui, app, expand, alpha, |ui, app| {
            if let Some((objects, path_id)) = app.object_on_path_panel_context() {
                let obj_label = object_on_path_object_label(app, &objects);
                ui.label(
                    RichText::new(format!("{obj_label} along path"))
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(4.0);
                settings_changed = object_on_path_controls(ui, app);
                ui.add_space(4.0);
                if ui.button("Bake as group on layer").clicked() {
                    app.bake_object_on_path_copies();
                }
                let _ = path_id;



                // Force the effect to match current UI mode (handles mode switches reliably, even if changed detect misses)
                let needs_update = objects.iter().any(|&sid| {
                    find_effect_for_pair(&app.project.document.path_effects, sid, path_id)
                        .map_or(false, |e| e.mode != app.ui_on_path_mode)
                });
                if needs_update {
                    app.update_object_on_path_effects_live();
                }
            }
            });
            if close {
                app.remove_object_on_path_effect();
                ui.ctx().request_repaint();
            } else if settings_changed {
                app.update_object_on_path_effects_live();
                ui.ctx().request_repaint();
            }
        }
    }

    // Tiling container (live, separate from ObjectOnPath)
    if app.selection_has_tiling_effect() {
        ui.separator();
        ui.label(RichText::new("Tiling (2D)").strong());
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Rows");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_rows).range(1..=20)).changed();
            ui.label("Cols");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_cols).range(1..=20)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Col Gap");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_gap_x).speed(1.0)).changed();
            ui.label("Row Gap");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_gap_y).speed(1.0)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Row Rot °");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_row_rot).speed(1.0)).changed();
            ui.label("Col Rot °");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_col_rot).speed(1.0)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Row Scale");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_row_scale).speed(0.01)).changed();
            ui.label("Col Scale");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_col_scale).speed(0.01)).changed();
        });
        ui.horizontal(|ui| {
            if ui.button("Bake as group").clicked() {
                app.bake_tiling();
            }
            if ui.button("Remove").clicked() {
                app.remove_tiling_effect();
                ui.ctx().request_repaint();
            }
        });
        if changed {
            app.update_tiling_effects_live();
            ui.ctx().request_repaint();
        }
    }

    // CircularClone container
    if app.selection_has_circular_effect() {
        use crate::document::CircularRotateMode;
        ui.separator();
        ui.label(RichText::new("CircularClone").strong());
        let mut changed = false;
        // Keep rows short so the Path Magic panel does not overflow horizontally.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Copies").small());
            changed |= ui
                .add(decimal_drag(&mut app.ui_circular_copies).range(3..=32).speed(1.0))
                .changed();
            ui.label(RichText::new("Off°").small())
                .on_hover_text("Angle offset (degrees)");
            changed |= ui
                .add(
                    decimal_drag(&mut app.ui_circular_angle_offset)
                        .speed(1.0)
                        .range(-360.0..=360.0),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Origin").small());
            changed |= ui
                .add(decimal_drag(&mut app.ui_circular_origin_x).speed(1.0).prefix("X "))
                .changed();
            changed |= ui
                .add(decimal_drag(&mut app.ui_circular_origin_y).speed(1.0).prefix("Y "))
                .changed();
        });
        ui.label(RichText::new("Rotate").small().color(colors::TEXT_MUTED));
        ui.horizontal_wrapped(|ui| {
            for mode in [
                CircularRotateMode::Static,
                CircularRotateMode::ReferenceOrigin,
            ] {
                let selected = app.ui_circular_rotate_mode == mode;
                let tip = match mode {
                    CircularRotateMode::Static => {
                        "Static: every copy keeps the source orientation (translate only)"
                    }
                    CircularRotateMode::ReferenceOrigin => {
                        "Origin: each copy rotates by its step around the origin (fan / chord)"
                    }
                };
                if ui
                    .selectable_label(selected, mode.label())
                    .on_hover_text(tip)
                    .clicked()
                {
                    app.ui_circular_rotate_mode = mode;
                    changed = true;
                }
            }
        });
        // One primary action per row — avoids Path Magic panel horizontal overflow.
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("Bake as group"),
            )
            .on_hover_text("Group owns all copies; delete group removes every copy")
            .clicked()
        {
            app.bake_circular();
        }
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("Bake as path"),
            )
            .on_hover_text("Union all copies into one path (shutter / multi-contour OK)")
            .clicked()
        {
            app.bake_circular_as_path();
        }
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("Split it"),
            )
            .on_hover_text("Turn each copy into its own independent path object")
            .clicked()
        {
            app.split_circular();
        }
        if ui
            .add_sized([ui.available_width(), 24.0], egui::Button::new("Remove"))
            .clicked()
        {
            app.remove_circular_effect();
            ui.ctx().request_repaint();
        }
        if changed {
            app.update_circular_effects_live();
            ui.ctx().request_repaint();
        }
    }

    // Convert non-path shapes → path (always offer when applicable).
    let convertible: Vec<_> = app
        .selection
        .iter()
        .filter(|&&id| {
            app.project.nodes.get(id).is_some_and(|n| {
                !matches!(
                    n.kind,
                    NodeKind::Path { .. }
                        | NodeKind::Group { .. }
                        | NodeKind::Image { .. }
                        | NodeKind::Text { .. }
                        | NodeKind::BrushStroke { .. }
                        | NodeKind::FlowchartNode { .. }
                        | NodeKind::FlowchartPath { .. }
                )
            })
        })
        .copied()
        .collect();
    if !convertible.is_empty() {
        ui.separator();
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("Convert to path"),
            )
            .on_hover_text("Circle / rect / ellipse / chord / polygon / arc → editable path")
            .clicked()
        {
            app.convert_selection_to_path();
        }
    }

    let eligible = app.selection_tiling_circular_sources();
    let has_t_or_c = app.selection_has_tiling_effect() || app.selection_has_circular_effect();
    let has_bool = app.selection_has_boolean_effect() || app.selection_has_clip_mask();

    // Tiling / Circular on any eligible shape including Path.
    if !eligible.is_empty() && !has_t_or_c {
        ui.separator();
        ui.label(RichText::new("Clone effects").strong());
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("Tiling (size gap)"),
            )
            .clicked()
        {
            app.apply_tiling_magic();
        }
        if ui
            .add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new("CircularClone (6 sides)"),
            )
            .clicked()
        {
            app.apply_circular_clone_magic();
        }
        ui.add_space(6.0);
    }

    if path_ids.is_empty() && app.object_on_path_panel_context().is_none() {
        if !has_t_or_c && !has_bool && app.selection.len() < 2 && eligible.is_empty() {
            ui.label(
                RichText::new(
                    "Select shape(s) or path(s) for Tiling/CircularClone, or two shapes for Boolean.",
                )
                .color(colors::TEXT_MUTED),
            );
        }
        // Don't return early if tiling/circular panel already shown above via has_t_or_c containers.
        if !has_t_or_c {
            return;
        }
    }

    if !path_ids.is_empty() {
        path_magic_card(ui, app, "Path tools", |ui, app| {
            let open_count = open_path_ids.len();
            let closed_count = path_ids.len().saturating_sub(open_count);
            if open_count > 0 && ui.button("Close open paths").clicked() {
                app.close_open_paths_in_selection();
            }
            if closed_count > 0 && ui.button("Open closed paths").clicked() {
                app.open_closed_paths_in_selection();
            }
            if ui.button("Smooth all corners").clicked() {
                for id in &path_ids {
                    app.set_all_path_anchors_smooth(*id, true);
                }
            }
            if ui.button("Sharpen all corners").clicked() {
                for id in &path_ids {
                    app.set_all_path_anchors_smooth(*id, false);
                }
            }
            if ui.button("Simplify").clicked() {
                for id in &path_ids {
                    app.simplify_path(*id);
                }
            }
        });

        if path_ids.len() == 1 {
            let id = path_ids[0];
            let closed = app
                .project
                .nodes
                .get(id)
                .and_then(|n| match &n.kind {
                    NodeKind::Path { path } => Some(path.is_closed()),
                    _ => None,
                })
                .unwrap_or(false);
            let name = app
                .project
                .nodes
                .get(id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| "Path".into());
            path_magic_card(ui, app, &name, |ui, app| {
                if closed {
                    if ui.button("Open path").clicked() {
                        app.set_path_closed(id, false);
                    }
                } else if ui.button("Close path").clicked() {
                    app.set_path_closed(id, true);
                }
                if ui.button("Reverse").clicked() {
                    app.reverse_path(id);
                }
            });
        }
    }
}

fn path_magic_card(
    ui: &mut Ui,
    app: &mut VadadeeBerryApp,
    title: &str,
    body: impl FnOnce(&mut Ui, &mut VadadeeBerryApp),
) {
    theme::constraint_block(ui, |ui| {
        ui.label(RichText::new(title).strong());
        ui.add_space(4.0);
        body(ui, app);
    });
}

/// Truncate for narrow Path Magic panel; full name on hover.
fn clamp_object_label(name: &str, max_chars: usize) -> String {
    let n = name.chars().count();
    if n <= max_chars {
        name.to_string()
    } else {
        let take = max_chars.saturating_sub(1);
        format!("{}…", name.chars().take(take).collect::<String>())
    }
}

fn node_display_name(app: &VadadeeBerryApp, id: crate::document::NodeId) -> String {
    app.project
        .nodes
        .get(id)
        .map(|n| {
            if n.name.trim().is_empty() {
                format!("{:?}", n.kind).chars().take(24).collect()
            } else {
                n.name.clone()
            }
        })
        .unwrap_or_else(|| "Object".into())
}

/// Path Magic: Boolean (shape+shape or N-way) or Clip Mask (image+shape solid face).
fn boolean_and_clip_panel(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    use crate::app::BooleanPairMode;
    use crate::document::BooleanOpKind;

    let has_bool = app.selection_has_boolean_effect();
    let has_cm = app.selection_has_clip_mask();
    let pair_mode = app.selection_boolean_mode();
    let multi_shapes = app.selection_booleanable_shapes();
    let multi_count = multi_shapes.len();

    if !has_bool && !has_cm && pair_mode.is_none() {
        return;
    }

    ui.separator();

    // Resolve A/B names (selection order, or active effect operands).
    let (a_id, b_id, mode_kind) = if has_bool {
        let eid = app
            .project
            .document
            .boolean_effects
            .iter()
            .find(|(_, e)| {
                app.selection.contains(&e.a_id)
                    || app.selection.contains(&e.b_id)
                    || e.result_node_id
                        .map(|r| app.selection.contains(&r))
                        .unwrap_or(false)
            })
            .map(|(k, _)| *k);
        if let Some(eid) = eid {
            let e = &app.project.document.boolean_effects[&eid];
            (e.a_id, e.b_id, "boolean")
        } else {
            return;
        }
    } else if has_cm {
        let e = app
            .project
            .document
            .clip_masks
            .values()
            .find(|cm| {
                app.selection.contains(&cm.source_id) || app.selection.contains(&cm.mask_id)
            });
        if let Some(cm) = e {
            (cm.source_id, cm.mask_id, "clip")
        } else {
            return;
        }
    } else if multi_count >= 3 {
        (multi_shapes[0], multi_shapes[1], "boolean_multi")
    } else if let Some(mode) = pair_mode {
        match mode {
            BooleanPairMode::VectorBoolean { a, b } => (a, b, "boolean_offer"),
            BooleanPairMode::ImageClip { source, mask } => (source, mask, "clip_offer"),
        }
    } else {
        return;
    };

    let a_full = node_display_name(app, a_id);
    let b_full = node_display_name(app, b_id);
    let max_c = ((ui.available_width() / 7.5) as usize).clamp(8, 28);

    let title = match mode_kind {
        "boolean" | "boolean_offer" | "boolean_multi" => "Boolean Operation",
        _ => "Clip Mask",
    };
    ui.label(RichText::new(title).strong());

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            if mode_kind == "boolean_multi" {
                ui.label(
                    RichText::new(format!("{multi_count} shapes selected"))
                        .small()
                        .color(colors::ACCENT),
                )
                .on_hover_text(
                    multi_shapes
                        .iter()
                        .map(|id| node_display_name(app, *id))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            } else {
                let a_lab = clamp_object_label(&a_full, max_c);
                let b_lab = clamp_object_label(&b_full, max_c);
                ui.label(RichText::new(format!("A: {a_lab}")).small())
                    .on_hover_text(&a_full);
                ui.label(RichText::new(format!("B: {b_lab}")).small())
                    .on_hover_text(&b_full);
            }
        });
        if mode_kind != "boolean_multi" {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let swap_label =
                    RichText::new(icons::SWAP).font(nerd_font_id(14.0));
                if ui
                    .button(swap_label)
                    .on_hover_text("Reverse A ↔ B")
                    .clicked()
                {
                    if mode_kind == "clip" || mode_kind == "clip_offer" {
                        if mode_kind == "clip" {
                            app.swap_clip_mask_source();
                        } else {
                            app.selection.swap(0, 1);
                        }
                    } else {
                        app.reverse_boolean_operands();
                    }
                    ui.ctx().request_repaint();
                }
            });
        }
    });

    match mode_kind {
        "boolean_multi" => {
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "3+ shapes: Union and Intersection fold all operands.\n\
                     Difference / Exclude need exactly 2 shapes.",
                )
                .small()
                .color(colors::TEXT_MUTED),
            );
            ui.label(RichText::new("Op").small().color(colors::TEXT_MUTED));
            ui.horizontal_wrapped(|ui| {
                for op in [BooleanOpKind::Union, BooleanOpKind::Intersection] {
                    let selected = app.ui_boolean_op == op;
                    if ui.selectable_label(selected, op.label()).clicked() {
                        app.ui_boolean_op = op;
                    }
                }
            });
            // If user had Difference/Exclude selected, snap to Union for multi.
            if !app.ui_boolean_op.supports_multi() {
                app.ui_boolean_op = BooleanOpKind::Union;
            }
            if ui.button("Apply Boolean").clicked() {
                app.apply_boolean_effect();
                ui.ctx().request_repaint();
            }
            ui.label(
                RichText::new("Baked result path (operands hidden). Not a live multi-link.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        "boolean_offer" => {
            ui.add_space(2.0);
            ui.label(RichText::new("Op").small().color(colors::TEXT_MUTED));
            ui.horizontal_wrapped(|ui| {
                for op in [
                    BooleanOpKind::Union,
                    BooleanOpKind::Intersection,
                    BooleanOpKind::Difference,
                    BooleanOpKind::Exclude,
                ] {
                    let selected = app.ui_boolean_op == op;
                    if ui.selectable_label(selected, op.label()).clicked() {
                        app.ui_boolean_op = op;
                    }
                }
            });
            if ui.button("Apply Boolean").clicked() {
                app.apply_boolean_effect();
                ui.ctx().request_repaint();
            }
            ui.label(
                RichText::new("Creates a result path. Use Bake to keep it without live link.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        "boolean" => {
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Ghosts (A/B): Ctrl+Shift+click or Objects tab — move independently.\n\
                     Moving the result moves A+B together.",
                )
                .small()
                .color(colors::TEXT_MUTED),
            );
            ui.label(RichText::new("Op").small().color(colors::TEXT_MUTED));
            ui.horizontal_wrapped(|ui| {
                for op in [
                    BooleanOpKind::Union,
                    BooleanOpKind::Intersection,
                    BooleanOpKind::Difference,
                    BooleanOpKind::Exclude,
                ] {
                    let selected = app.ui_boolean_op == op
                        || app
                            .project
                            .document
                            .boolean_effects
                            .values()
                            .any(|e| {
                                (app.selection.contains(&e.a_id)
                                    || app.selection.contains(&e.b_id)
                                    || e.result_node_id
                                        .map(|r| app.selection.contains(&r))
                                        .unwrap_or(false))
                                    && e.op == op
                            });
                    if ui.selectable_label(selected, op.label()).clicked() {
                        app.set_boolean_op_live(op);
                        ui.ctx().request_repaint();
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Bake to path")
                    .on_hover_text("Keep result path; drop live boolean link")
                    .clicked()
                {
                    app.bake_boolean_effect();
                    ui.ctx().request_repaint();
                }
                if ui.button("✖ Remove").clicked() {
                    app.remove_boolean_effect();
                    ui.ctx().request_repaint();
                }
            });
        }
        "clip_offer" => {
            ui.label(
                RichText::new(
                    "Image clipped to shape solid face (not bounding box).\n\
                     After apply you can Rasterize the clip region only.",
                )
                .small()
                .color(colors::TEXT_MUTED),
            );
            if ui
                .button(
                    RichText::new(format!("{} Apply Clip Mask", icons::IMAGE))
                        .font(nerd_font_id(12.0)),
                )
                .clicked()
            {
                app.apply_clip_mask();
                ui.ctx().request_repaint();
            }
        }
        "clip" => {
            ui.label(
                RichText::new(
                    "Image → solid-face clip. Selection box = mask bounds only.\n\
                     Ghosts: Ctrl+Shift+click (or Objects tab) to edit image/mask alone.\n\
                     Rasterize: bake only the clipped region to a new image.",
                )
                .small()
                .color(colors::TEXT_MUTED),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(
                        RichText::new(format!("{} Rasterize clip", icons::RASTER))
                            .font(nerd_font_id(12.0)),
                    )
                    .on_hover_text("Bake only the clip region (mask solid face) to a new raster image")
                    .clicked()
                {
                    app.bake_clip_mask_to_raster();
                    ui.ctx().request_repaint();
                }
                if ui
                    .button(
                        RichText::new(format!("{} Swap", icons::SWAP)).font(nerd_font_id(12.0)),
                    )
                    .on_hover_text("Swap source / mask")
                    .clicked()
                {
                    app.swap_clip_mask_source();
                    ui.ctx().request_repaint();
                }
                if ui.button(RichText::new(format!("{} Remove", icons::CLOSE)).font(nerd_font_id(12.0))).clicked() {
                    app.remove_clip_mask();
                    ui.ctx().request_repaint();
                }
            });
        }
        _ => {}
    }
    ui.add_space(4.0);
}

fn object_on_path_container(
    ui: &mut Ui,
    app: &mut VadadeeBerryApp,
    expand: f32,
    alpha: f32,
    body: impl FnOnce(&mut Ui, &mut VadadeeBerryApp),
) -> bool {
    let mut close = false;
    let full_h = app.ui_on_path_container_h.max(ON_PATH_CONTAINER_MIN_H);
    let animated_h = full_h * expand;
    let width = ui.available_width();

    let response = ui.allocate_ui_with_layout(
        egui::vec2(width, animated_h),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_clip_rect(ui.max_rect());
            ui.style_mut().visuals.override_text_color =
                Some(colors::TEXT.gamma_multiply(alpha));
            theme::constraint_block(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Object on Path").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(icons::CLOSE)
                                        .font(icons::nerd_font_id(14.0))
                                        .color(colors::TEXT_MUTED.gamma_multiply(alpha)),
                                )
                                .frame(false)
                                .min_size(egui::vec2(20.0, 20.0)),
                            )
                            .on_hover_text("Remove object on path")
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
                body(ui, app);
            });
            if expand >= 0.98 {
                let measured = ui.min_rect().height();
                if measured > ON_PATH_CONTAINER_MIN_H {
                    app.ui_on_path_container_h = measured;
                }
            }
        },
    );
    let _ = response;
    close
}

fn object_on_path_object_label(app: &VadadeeBerryApp, objects: &[crate::document::NodeId]) -> String {
    if objects.len() == 1 {
        app.project
            .nodes
            .get(objects[0])
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "Object".into())
    } else {
        format!("{} objects", objects.len())
    }
}

fn object_on_path_controls(ui: &mut Ui, app: &mut VadadeeBerryApp) -> bool {
    let before = (
        app.ui_on_path_mode,
        app.ui_on_path_gap,
        app.ui_on_path_count,
        app.ui_on_path_cyclic,
        app.ui_on_path_rotate,
        app.ui_on_path_loft_scale,
        app.ui_on_path_loft_opacity,
    );
    ui.label(RichText::new("Mode").small());
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(app.ui_on_path_mode == OnPathMode::GapDuplicate, "Gap")
            .clicked()
        {
            app.ui_on_path_mode = OnPathMode::GapDuplicate;
        }
        if ui
            .selectable_label(app.ui_on_path_mode == OnPathMode::EvenlySpaced, "Even")
            .clicked()
        {
            app.ui_on_path_mode = OnPathMode::EvenlySpaced;
        }
        if ui
            .selectable_label(app.ui_on_path_mode == OnPathMode::Loft, "Loft")
            .on_hover_text("Continuous slices along path — circle × line → cylinder")
            .clicked()
        {
            app.ui_on_path_mode = OnPathMode::Loft;
            if let Some((objects, _)) = app.object_on_path_panel_context() {
                if let Some(id) = objects.first() {
                    if let Some(node) = app.project.nodes.get(*id) {
                        app.ui_on_path_gap = default_loft_gap_for_node(node);
                    }
                }
            }
        }
    });
    match app.ui_on_path_mode {
        OnPathMode::GapDuplicate => {
            ui.add(
                decimal_drag(&mut app.ui_on_path_gap)
                    .range(1.0..=2000.0)
                    .suffix(" px"),
            );
        }
        OnPathMode::EvenlySpaced => {
            ui.add(
                decimal_drag(&mut app.ui_on_path_count)
                    .range(2..=64)
                    .prefix("Count "),
            );
        }
        OnPathMode::Loft => {
            ui.label(
                RichText::new("Continuous integral sweep (solid, single outer stroke)")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.add(
                egui::Slider::new(&mut app.ui_on_path_loft_scale, 0.1..=2.0).text("End scale"),
            );
            ui.add(
                egui::Slider::new(&mut app.ui_on_path_loft_opacity, 0.1..=1.0).text("End shade"),
            );
        }
    }
    if app.ui_on_path_mode != OnPathMode::Loft {
        ui.checkbox(&mut app.ui_on_path_cyclic, "Cyclic wrap");
    }
    ui.checkbox(&mut app.ui_on_path_rotate, "Rotate to tangent");
    let after = (
        app.ui_on_path_mode,
        app.ui_on_path_gap,
        app.ui_on_path_count,
        app.ui_on_path_cyclic,
        app.ui_on_path_rotate,
        app.ui_on_path_loft_scale,
        app.ui_on_path_loft_opacity,
    );
    before != after
}

fn floating_action_bar(app: &mut VadadeeBerryApp, ctx: &Context, work: Rect) {
    let open_amount = app.ui_anim.action_bar_open_t();
    let opacity = app.ui_anim.action_bar_opacity();
    let animating = app.ui_anim.action_bar_slide_running();
    if !app.action_bar_open && !animating && open_amount <= 0.001 {
        return;
    }
    if opacity <= 0.004 && !animating && !app.action_bar_open {
        return;
    }

    let card_w = app.action_bar_width;
    let rect = action_bar_overlay_rect(work, card_w, open_amount);

    theme::show_action_bar_area(ctx, "float_action_bar", rect, opacity, |ui| {
        action_bar_interior(app, ui);
    });
}

fn export_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    page_section(app, ui);
    ui.add_space(8.0);
    ui.separator();

    if !app.selection.is_empty() {
        ui.label(RichText::new("Selection Options").strong());
        if ui.button("Resize as selected").clicked() {
            app.resize_to_selection();
        }
        ui.add_space(8.0);
        ui.separator();
    }

    ui.label(RichText::new("Export").strong());
    ui.horizontal(|ui| {
        ui.label("Image type:");
        egui::ComboBox::from_id_salt("export_image_format")
            .selected_text(app.export_image_format.label())
            .show_ui(ui, |ui| {
                for fmt in [
                    io::ExportImageFormat::Png,
                    io::ExportImageFormat::Jpeg,
                    io::ExportImageFormat::Bmp,
                    io::ExportImageFormat::RawRgba,
                ] {
                    if ui
                        .selectable_label(app.export_image_format == fmt, fmt.label())
                        .clicked()
                    {
                        app.export_image_format = fmt;
                    }
                }
            });
    });
    ui.checkbox(
        &mut app.export_image_selection_only,
        "Export selection only (image)",
    );
    if ui.button("Export image…").clicked() {
        app.request_export_image();
    }
    if ui.button("Export SVG…").clicked() {
        app.request_export_svg();
    }
    if ui.button("Save project…").clicked() {
        app.request_save_project();
    }
    if ui.button("Open SVG…").clicked() {
        app.request_open_svg();
    }
    if ui.button("Import Image…").clicked() {
        app.request_import_image();
    }

    ui.add_space(8.0);
    ui.separator();

    // ── Render to Video ──────────────────────────────────────────────
    theme::constraint_block(ui, |ui| {
        ui.label(
            RichText::new("🎬 Render to Video")
                .font(nerd_font_id(13.0))
                .strong(),
        );
        ui.add_space(4.0);

        // Animated objects note
        ui.label(
            RichText::new("Objects with keyframes will be animated.")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
        ui.add_space(6.0);



        let content_secs = app.animation_content_duration_secs();
        ui.horizontal(|ui| {
            ui.label("Duration");
            let mut dur = if app.video_export.export_duration_secs > 0.05 {
                app.video_export.export_duration_secs
            } else {
                content_secs
            };
            if ui
                .add(
                    egui::DragValue::new(&mut dur)
                        .range(0.1..=3600.0)
                        .speed(0.1)
                        .suffix(" s"),
                )
                .changed()
            {
                app.video_export.export_duration_secs = dur;
            }
            if ui
                .button(RichText::new("Auto").small())
                .on_hover_text(format!("Use timeline content ({content_secs:.2}s)"))
                .clicked()
            {
                app.video_export.export_duration_secs = 0.0;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Cycles");
            let mut cycles = app.video_export.export_cycles.max(1) as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut cycles)
                        .range(1..=100)
                        .speed(0.2)
                        .suffix("×"),
                )
                .on_hover_text(
                    "Repeat the animation this many times in the export (loop / cyclic copy).",
                )
                .changed()
            {
                app.video_export.export_cycles = cycles.clamp(1, 100) as u32;
            }
            let cycle_n = app.video_export.export_cycles.max(1);
            let one = if app.video_export.export_duration_secs > 0.05 {
                app.video_export.export_duration_secs
            } else {
                content_secs
            };
            ui.label(
                RichText::new(format!("→ {:.1}s total", one * cycle_n as f32))
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        });

        // Frame rate (integer)
        ui.horizontal(|ui| {
            ui.label("Frame rate");
            let mut fps = app.video_export.fps as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut fps)
                        .range(1..=240)
                        .suffix(" fps")
                        .speed(1.0),
                )
                .changed()
            {
                app.video_export.fps = fps.clamp(1, 240) as u32;
            }
        });

        // Encode CPU profile
        ui.horizontal(|ui| {
            ui.label("CPU");
            egui::ComboBox::from_id_salt("video_export_power")
                .selected_text(app.video_export.power_level.label())
                .width(100.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut app.video_export.power_level,
                        crate::app::ExportPowerLevel::PowerSaving,
                        crate::app::ExportPowerLevel::PowerSaving.label(),
                    );
                    ui.selectable_value(
                        &mut app.video_export.power_level,
                        crate::app::ExportPowerLevel::FullPower,
                        crate::app::ExportPowerLevel::FullPower.label(),
                    );
                });
        });

        // Resolution
        ui.horizontal(|ui| {
            ui.label("Resolution");
            let mut res = app.video_export.resolution_pct;
            egui::ComboBox::from_id_salt("video_res_combo")
                .selected_text(format!("{}%", res))
                .width(80.0)
                .show_ui(ui, |ui| {
                    for &r in &[25u32, 50, 75, 100, 150, 200] {
                        ui.selectable_value(&mut res, r, format!("{}%", r));
                    }
                });
            app.video_export.resolution_pct = res;
        });

        // Bitrate
        ui.horizontal(|ui| {
            ui.label("Bitrate");
            let mut kb = app.video_export.bitrate_kbps;
            ui.add(egui::DragValue::new(&mut kb).range(500..=80000).suffix(" kbps").speed(100.0));
            app.video_export.bitrate_kbps = kb;
        });

        // Format
        ui.horizontal(|ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("video_fmt_combo")
                .selected_text(app.video_export.format.label())
                .width(130.0)
                .show_ui(ui, |ui| {
                    for &fmt in &[
                        crate::app::VideoFormat::Mp4,
                        crate::app::VideoFormat::Mkv,
                        crate::app::VideoFormat::Webm,
                        crate::app::VideoFormat::Mov,
                    ] {
                        ui.selectable_value(&mut app.video_export.format, fmt, fmt.label());
                    }
                });
        });

        ui.add_space(6.0);

        // Export button
        let btn_text = if app.video_export.rendering {
            "⏳ Rendering…"
        } else {
            "▶ Export Video"
        };
        let export_btn = ui.add_enabled(
            !app.video_export.rendering,
            egui::Button::new(
                RichText::new(btn_text)
                    .color(egui::Color32::from_rgb(80, 200, 120)),
            )
            .fill(colors::BG_DEEP)
            .min_size(egui::vec2(ui.available_width() - 8.0, 28.0)),
        );
        if export_btn.clicked() {
            app.request_video_export(ui.ctx().clone());
        }
    });
}

/// Close any dialog when Escape is pressed (shared helper).
fn dialog_escape_close(ctx: &egui::Context, open: &mut bool) {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }
}

/// Overlay list when multiple objects share the same click hit.
fn hit_pick_menu_overlay(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    let Some((screen, candidates)) = app.hit_pick_menu.clone() else {
        return;
    };
    if candidates.is_empty() {
        app.hit_pick_menu = None;
        return;
    }
    let mut open = true;
    let mut picked: Option<crate::document::NodeId> = None;
    let mut dismiss = false;
    egui::Area::new(egui::Id::new("hit_pick_menu_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen + egui::vec2(8.0, 8.0))
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(colors::BG_PANEL)
                .stroke(egui::Stroke::new(1.0, colors::ACCENT.gamma_multiply(0.6)))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.set_max_width(220.0);
                    ui.label(
                        RichText::new("Select object")
                            .strong()
                            .color(colors::ACCENT)
                            .small(),
                    );
                    ui.separator();
                    for &id in &candidates {
                        let (icon, name) = app
                            .project
                            .nodes
                            .get(id)
                            .map(|n| {
                                (
                                    node_icon(&n.kind),
                                    if n.name.trim().is_empty() {
                                        format!("{:.8}", id)
                                    } else {
                                        n.name.clone()
                                    },
                                )
                            })
                            .unwrap_or_else(|| (icons::OBJECT, id.to_string()));
                        let label = format!("{icon}  {name}");
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label)
                                        .font(nerd_font_id(13.0))
                                        .color(colors::TEXT),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .min_size(egui::vec2(200.0, 22.0)),
                            )
                            .clicked()
                        {
                            picked = Some(id);
                        }
                    }
                    ui.add_space(2.0);
                    if ui
                        .small_button(RichText::new("Cancel").color(colors::TEXT_MUTED))
                        .clicked()
                    {
                        dismiss = true;
                    }
                });
            // Click outside-ish: if pointer released not over this area, dismiss later.
            if ui.input(|i| i.pointer.any_click()) && !ui.rect_contains_pointer(ui.min_rect().expand(4.0)) {
                // Keep menu if still interacting — only dismiss on explicit cancel / pick.
                let _ = open;
            }
        });
    if let Some(id) = picked {
        app.select_from_hit_picker(id);
    } else if dismiss {
        app.hit_pick_menu = None;
    }
}

fn plotter_formula_dialog(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    let Some(id) = app.plotter_formula_dialog else {
        return;
    };
    let Some(node) = app.project.nodes.get(id) else {
        app.plotter_formula_dialog = None;
        return;
    };
    let (axis_label, is_fx) = match &node.kind {
        crate::document::NodeKind::Plotter { ref_axis, .. } => {
            (ref_axis.label(), matches!(ref_axis, crate::document::PlotterRef::Fx))
        }
        _ => {
            app.plotter_formula_dialog = None;
            return;
        }
    };
    let mut open = true;
    let mut close = false;
    let mut apply = false;
    let mut cancel = false;
    let draft_err = {
        let d = app.plotter_formula_draft.trim();
        if d.is_empty() {
            Some("empty expression".into())
        } else {
            let mut v = crate::document::ExprVars::simple(0.5, 0.0, 0.0);
            if is_fx {
                v.x = 0.0;
            } else {
                v.y = 0.0;
            }
            crate::document::eval_expr_vars(d, v).err().map(|e| e.0)
        }
    };
    dialog_escape_close(ctx, &mut open);
    let mut typed = false;
    egui::Window::new(format!("Plotter formula — {axis_label}"))
        .id(egui::Id::new(("plotter_formula_dialog", id)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let (pre, post) = if is_fx {
                    ("f(x) ", " y. Use x (independent), t in [0,1]. e.g. sin(x)")
                } else {
                    ("f(y) ", " x. Use y (independent), t in [0,1]. e.g. sin(y)")
                };
                ui.label(RichText::new(pre).small().color(colors::TEXT_MUTED));
                ui.label(
                    RichText::new(icons::ARROW_RIGHT)
                        .font(nerd_font_id(12.0))
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.label(RichText::new(post).small().color(colors::TEXT_MUTED));
            });
            ui.add_space(4.0);
            let mut te = egui::TextEdit::multiline(&mut app.plotter_formula_draft)
                .id_source(("plotter_formula_edit", id))
                .desired_width(f32::INFINITY)
                .desired_rows(6)
                .font(egui::TextStyle::Monospace);
            if draft_err.is_some() {
                te = te
                    .text_color(egui::Color32::from_rgb(255, 180, 180))
                    .background_color(egui::Color32::from_rgb(60, 16, 16));
            }
            if ui.add(te).changed() {
                typed = true;
            }
            if let Some(ref e) = draft_err {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
            }
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                    close = true;
                }
            });
        });
    if typed {
        app.begin_plotter_expr_edit(id);
        let draft = app.plotter_formula_draft.clone();
        app.set_plotter_expr_live(id, draft.clone());
        app.plotter_inline_expr = Some((id, draft));
    }
    if apply {
        // Live already updated the curve; one undo step for the whole edit session.
        app.commit_plotter_expr_edit(id);
        let draft = app.plotter_formula_draft.clone();
        app.plotter_inline_expr = Some((id, draft));
        close = true;
    } else if cancel || !open {
        // Dismiss without Apply: restore expression from before the dialog edit.
        app.cancel_plotter_expr_edit(id);
        if let Some(node) = app.project.nodes.get(id) {
            if let crate::document::NodeKind::Plotter { expr, .. } = &node.kind {
                app.plotter_inline_expr = Some((id, expr.clone()));
            }
        }
        close = true;
    }
    if close {
        app.plotter_formula_dialog = None;
        app.plotter_formula_draft.clear();
    }
}

fn object_rename_dialog(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    let Some((id_copy, is_layer_copy)) = app
        .object_rename_dialog
        .as_ref()
        .map(|(id, _, layer)| (*id, *layer))
    else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut close = false;
    let title = if is_layer_copy {
        "Rename layer"
    } else {
        "Rename object"
    };
    dialog_escape_close(ctx, &mut open);
    egui::Window::new(title)
        .id(egui::Id::new(("object_rename_dlg", id_copy)))
        .collapsible(false)
        .resizable(false)
        .default_width(320.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new("Enter a new name. Esc or Cancel closes without saving.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.add_space(4.0);
            if let Some((_, draft, _)) = app.object_rename_dialog.as_mut() {
                let te = ui.add(
                    egui::TextEdit::singleline(draft)
                        .id(egui::Id::new(("object_rename_edit", id_copy)))
                        .desired_width(f32::INFINITY)
                        .hint_text("Name"),
                );
                // Focus once when the dialog opens.
                if ctx.data(|d| d.get_temp::<bool>(egui::Id::new(("rename_focus", id_copy))).is_none()) {
                    te.request_focus();
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("rename_focus", id_copy)), true));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Rename").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if apply {
        let name = app
            .object_rename_dialog
            .as_ref()
            .map(|(_, d, _)| d.trim().to_string())
            .unwrap_or_default();
        if !name.is_empty() {
            if is_layer_copy {
                if let Some(layer) = app
                    .project
                    .document
                    .layers
                    .iter_mut()
                    .find(|l| l.id == id_copy)
                {
                    layer.name = name;
                }
            } else if let Some(node) = app.project.nodes.get(id_copy) {
                let before = node.clone();
                let mut after = before.clone();
                after.name = name;
                if before != after {
                    app.history.push(
                        &mut app.project,
                        crate::history::ProjectEdit::PatchNode {
                            id: id_copy,
                            before,
                            after,
                        },
                    );
                }
            }
        }
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new(("rename_focus", id_copy))));
        app.object_rename_dialog = None;
    } else if close || !open {
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new(("rename_focus", id_copy))));
        app.object_rename_dialog = None;
    }
}

/// DAW piano roll as a modal dialog (double-click DAW clip), not a slide-up floater.
fn daw_piano_dialog(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    if app.piano_roll_clip.is_none() {
        return;
    }
    let mut open = true;
    dialog_escape_close(ctx, &mut open);
    if !open {
        app.piano_roll_clip = None;
        return;
    }
    egui::Window::new(format!("{} DAW Piano", icons::MUSIC))
        .id(egui::Id::new("daw_piano_dialog"))
        .collapsible(false)
        .resizable(true)
        .default_size([720.0, 340.0])
        .min_width(420.0)
        .min_height(220.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            crate::av_ui::piano_roll_panel(app, ui, ctx);
        });
    if !open {
        app.piano_roll_clip = None;
    }
}

fn video_export_progress_window(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    if !app.video_export.progress_visible {
        return;
    }
    let Some(prog) = app.video_export.progress else {
        return;
    };
    let mut open = true;
    dialog_escape_close(ctx, &mut open);
    if !open && !app.video_export.rendering {
        app.video_export.progress_visible = false;
        return;
    }
    egui::Window::new("Render to Video")
        .id(egui::Id::new("video_progress_dlg"))
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&app.video_export.status_msg)
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
                // Frame counter from worker (authoritative), not UI receive batches.
                if app.video_export.rendering && app.video_export.total_frames > 0 {
                    ui.label(
                        RichText::new(format!(
                            "Frame {} / {}",
                            app.video_export.worker_frame_done,
                            app.video_export.total_frames
                        ))
                        .small()
                        .color(colors::TEXT_MUTED),
                    );
                }
                ui.add_space(6.0);

                // P7a: smoothed bar (eases toward worker target; no multi-frame jumps).
                let bar_prog = if app.video_export.rendering {
                    app.video_export.progress_smooth.clamp(0.0, 1.0)
                } else {
                    prog
                };
                let pb = egui::ProgressBar::new(bar_prog)
                    .show_percentage()
                    .animate(app.video_export.rendering)
                    .desired_width(ui.available_width());
                ui.add(pb);
                ui.add_space(10.0);

                // --- Funny Dialog/Joke Section ---
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("{} System Status & Dialogue:", icons::ROBOT))
                                .font(nerd_font_id(13.0))
                                .color(colors::TEXT)
                                .strong()
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("\"{}\"", app.video_export.current_joke))
                                .color(colors::ACCENT)
                                .italics()
                        );
                    });
                });
                ui.add_space(10.0);

                // --- Suffering Metrics Panel ---
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("{} System Suffering Monitor:", icons::FIRE))
                                .font(nerd_font_id(13.0))
                                .color(colors::TEXT)
                                .strong()
                        );
                        ui.add_space(6.0);

                        egui::Grid::new("suffering_metrics_grid")
                            .num_columns(2)
                            .spacing([20.0, 6.0])
                            .show(ui, |ui| {
                                // CPU Temperature and Usage
                                ui.label(RichText::new("CPU Suffering:").color(colors::TEXT_MUTED));
                                let cpu_temp_color = if app.video_export.sys_stats.cpu_temp > 80.0 {
                                    egui::Color32::from_rgb(255, 100, 100)
                                } else if app.video_export.sys_stats.cpu_temp > 65.0 {
                                    egui::Color32::from_rgb(255, 180, 100)
                                } else {
                                    colors::TEXT
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{:.1}% ({:.1}°C)",
                                        app.video_export.sys_stats.cpu_usage,
                                        app.video_export.sys_stats.cpu_temp
                                    ))
                                    .color(cpu_temp_color)
                                    .strong(),
                                );
                                ui.end_row();

                                // GPU Usage
                                ui.label(RichText::new("GPU Suffering:").color(colors::TEXT_MUTED));
                                let gpu_color = if app.video_export.sys_stats.gpu_usage > 80.0 {
                                    egui::Color32::from_rgb(255, 100, 100)
                                } else {
                                    colors::TEXT
                                };
                                ui.label(
                                    RichText::new(format!("{:.1}%", app.video_export.sys_stats.gpu_usage))
                                        .color(gpu_color)
                                        .strong(),
                                );
                                ui.end_row();

                                // RAM Consumption (App and System)
                                ui.label(RichText::new("RAM Consumed:").color(colors::TEXT_MUTED));
                                ui.label(
                                    RichText::new(format!(
                                        "{:.1} MB (System: {:.1} / {:.1} GB)",
                                        app.video_export.sys_stats.ram_rss_mb,
                                        app.video_export.sys_stats.ram_sys_used_gb,
                                        app.video_export.sys_stats.ram_sys_total_gb
                                    ))
                                    .color(colors::TEXT)
                                    .strong(),
                                );
                                ui.end_row();

                                // Speed from worker EMA (stable; not UI poll gaps).
                                ui.label(RichText::new("Export Speed:").color(colors::TEXT_MUTED));
                                let speed_text = if app.video_export.sec_per_frame > 1e-6 {
                                    let spf = app.video_export.sec_per_frame;
                                    let fps = 1.0 / spf;
                                    let eta = if app.video_export.worker_frame_done
                                        < app.video_export.total_frames
                                        && app.video_export.total_frames > 0
                                    {
                                        let rem = (app.video_export.total_frames
                                            - app.video_export.worker_frame_done)
                                            as f32
                                            * spf;
                                        if rem < 60.0 {
                                            format!(" · ETA {:.0}s", rem)
                                        } else {
                                            format!(
                                                " · ETA {}:{:02}",
                                                (rem / 60.0) as i32,
                                                (rem % 60.0) as i32
                                            )
                                        }
                                    } else {
                                        String::new()
                                    };
                                    format!("{:.2} s/frame ({:.1} fps){eta}", spf, fps)
                                } else {
                                    "Measuring…".to_string()
                                };
                                ui.label(RichText::new(speed_text).color(colors::TEXT).strong());
                                ui.end_row();
                            });
                    });
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if app.video_export.rendering {
                        if ui.button("Cancel").clicked() {
                            app.cancel_video_export();
                            app.video_export.progress_visible = false;
                        }
                    }
                    if ui.button("Hide").clicked() {
                        app.video_export.progress_visible = false;
                    }
                });
            });
        });
}

fn shader_editor_window(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    let Some(layer_id) = app.show_shader_editor_window else {
        return;
    };
    
    // Find the layer
    let mut open = true;
    let mut title = "Shader Editor".to_string();
    let mut current_pass = None;
    
    if let Some(l) = app.project.document.layers.iter_mut().find(|layer| layer.id == layer_id) {
        if l.kind == crate::document::LayerKind::Shading {
            if l.shading_passes.is_empty() {
                l.shading_passes
                    .push(crate::document::ShadingPass::vignette_preset());
            }
            if l.shading_passes.len() > 1 {
                let keep = l.shading_passes.pop().unwrap();
                l.shading_passes.clear();
                l.shading_passes.push(keep);
            }
            title = format!("Shader Editor - {}", l.name);
            current_pass = Some(&mut l.shading_passes[0]);
        }
    }
    
    if current_pass.is_none() {
        app.show_shader_editor_window = None;
        return;
    }
    
    let pass = current_pass.unwrap();
    dialog_escape_close(ctx, &mut open);
    
    egui::Window::new(title)
        .id(egui::Id::new("shader_editor_window_floating"))
        .open(&mut open)
        .default_size(egui::vec2(500.0, 400.0))
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // Preset & enabled dropdown
                let mut current_preset_name = match pass.name.as_str() {
                    "Vignette" => "Vignette",
                    "CRT" => "CRT",
                    "Blackhole" => "Blackhole",
                    "Starfield" => "Starfield",
                    _ => "Custom",
                };
                
                let preset_options = ["Vignette", "CRT", "Blackhole", "Starfield", "Custom"];
                let mut new_preset = None;

                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("shading_preset_combo_float")
                        .selected_text(current_preset_name)
                        .show_ui(ui, |ui| {
                            for opt in &preset_options {
                                if ui.selectable_value(&mut current_preset_name, *opt, *opt).clicked() {
                                    new_preset = Some(*opt);
                                }
                            }
                        });
                        
                    ui.checkbox(&mut pass.enabled, "Enabled");
                });

                if let Some(opt) = new_preset {
                    match opt {
                        "Vignette" => {
                            *pass = crate::document::ShadingPass::vignette_preset();
                        }
                        "CRT" => {
                            *pass = crate::document::ShadingPass::crt_preset();
                        }
                        "Blackhole" => {
                            *pass = crate::document::ShadingPass::blackhole_preset();
                        }
                        "Starfield" => {
                            *pass = crate::document::ShadingPass::starfield_preset();
                        }
                        _ => {
                            *pass = crate::document::ShadingPass::custom_template();
                        }
                    }
                }

                ui.horizontal(|ui| {
                    ui.label("Reload mode:");
                    let before_hot = pass.hot_reload;
                    ui.radio_value(&mut pass.hot_reload, true, "Hot");
                    ui.radio_value(&mut pass.hot_reload, false, "Press");
                    if pass.hot_reload && !before_hot {
                        pass.compiled_wgsl = Some(pass.wgsl.clone());
                        if let Ok(mut err_lock) = pass.compile_error.lock() {
                            *err_lock = None;
                        }
                    }
                });

                shading_wgsl_file_buttons(ui, pass);

                ui.add_space(4.0);
                ui.label(RichText::new("WGSL source code:").weak());
                ui.label(
                    RichText::new(
                        "Fragment only: @fragment fn main(uv) -> vec4. Load .wgsl or edit below (not multipass compute).",
                    )
                    .small()
                    .weak(),
                );

                let mut text_edit_response = None;
                egui::ScrollArea::both()
                    .id_salt("shader_editor_scroll")
                    .max_height(ui.available_height() - 60.0)
                    .show(ui, |ui| {
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut pass.wgsl)
                                .id(egui::Id::new("shader_editor_text"))
                                .desired_width(f32::INFINITY)
                                .desired_rows(15)
                                .font(egui::TextStyle::Monospace),
                        );
                        text_edit_response = Some(resp);
                    });

                if let Some(resp) = text_edit_response {
                    if resp.changed() {
                        if matches!(
                            pass.name.as_str(),
                            "Vignette" | "CRT" | "Blackhole" | "Starfield"
                        ) {
                            pass.name = "Custom".to_string();
                        }
                        if pass.hot_reload {
                            pass.compiled_wgsl = Some(pass.wgsl.clone());
                            if let Ok(mut err_lock) = pass.compile_error.lock() {
                                *err_lock = None;
                            }
                        }
                    }
                }

                if !pass.hot_reload {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Compile / Reload").clicked() {
                            pass.compiled_wgsl = Some(pass.wgsl.clone());
                            if let Ok(mut err_lock) = pass.compile_error.lock() {
                                *err_lock = None;
                            }
                        }
                    });
                }

                if let Ok(err_lock) = pass.compile_error.lock() {
                    if let Some(ref err) = *err_lock {
                        ui.add_space(4.0);
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), err);
                    }
                }
            });
        });
        
    if !open {
        app.show_shader_editor_window = None;
    }
}

/// Load / save custom WGSL for a shading pass (desktop file dialogs).
/// Uses wrapping so narrow layer panels don't overflow on one row.
fn shading_wgsl_file_buttons(ui: &mut egui::Ui, pass: &mut crate::document::ShadingPass) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().item_spacing.y = 4.0;

        #[cfg(not(target_os = "android"))]
        {
            if ui
                .add(
                    egui::Button::new(RichText::new("Load").font(nerd_font_id(11.0)))
                        .min_size(egui::vec2(0.0, 0.0)),
                )
                .on_hover_text("Load a fragment WGSL module from disk (dynamic, not a preset)")
                .clicked()
            {
                let dlg = rfd::FileDialog::new()
                    .add_filter("WGSL shader", &["wgsl", "txt"])
                    .add_filter("All", &["*"]);
                if let Some(path) = dlg.pick_file() {
                    match crate::shading::load_wgsl_file(&path) {
                        Ok(src) => {
                            let stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Custom");
                            pass.load_wgsl_source(src, Some(stem));
                            if let Err(msg) = crate::shading::validate_shading_wgsl(&pass.wgsl) {
                                if let Ok(mut err) = pass.compile_error.lock() {
                                    *err = Some(msg);
                                }
                            }
                        }
                        Err(e) => {
                            if let Ok(mut err) = pass.compile_error.lock() {
                                *err = Some(e);
                            }
                        }
                    }
                }
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Save").font(nerd_font_id(11.0)))
                        .min_size(egui::vec2(0.0, 0.0)),
                )
                .on_hover_text("Export current WGSL source as .wgsl")
                .clicked()
            {
                let dlg = rfd::FileDialog::new()
                    .add_filter("WGSL shader", &["wgsl"])
                    .set_file_name(format!("{}.wgsl", pass.name.replace(' ', "_")));
                if let Some(path) = dlg.save_file() {
                    if let Err(e) = crate::shading::save_wgsl_file(&path, &pass.wgsl) {
                        if let Ok(mut err) = pass.compile_error.lock() {
                            *err = Some(e);
                        }
                    }
                }
            }
        }
        if ui
            .add(
                egui::Button::new(RichText::new("Reset").font(nerd_font_id(11.0)))
                    .min_size(egui::vec2(0.0, 0.0)),
            )
            .on_hover_text("Replace source with the Custom fragment starter")
            .clicked()
        {
            let id = pass.id;
            let hot = pass.hot_reload;
            let enabled = pass.enabled;
            *pass = crate::document::ShadingPass::custom_template();
            pass.id = id;
            pass.hot_reload = hot;
            pass.enabled = enabled;
        }
    });
}


const FLOATING_PANEL_MIN_W: f32 = 280.0;
const FLOATING_PANEL_MAX_H: f32 = 450.0;

/// Use the last user-sized dimensions when reopening; `stored_w == 0` means full width.
fn restore_floater_width(stored_w: f32, max_w: f32) -> f32 {
    if stored_w <= 0.0 {
        max_w
    } else {
        stored_w.clamp(FLOATING_PANEL_MIN_W.min(max_w), max_w)
    }
}

fn restore_floater_height(stored_h: f32, content_h: f32, max_h: f32) -> f32 {
    stored_h.clamp(content_h, max_h)
}

fn floating_video_editor(app: &mut VadadeeBerryApp, ctx: &Context, work: Rect) {
    let open_t = app.ui_anim.video_editor_t;
    let animating = app.ui_anim.video_editor_running;
    if app.show_video_editor_window.is_none() && !animating && open_t <= 0.001 {
        return;
    }

    let active_video_id_from_index = {
        let active_idx = app.project.document.active_layer_index;
        if active_idx < app.project.document.layers.len() {
            let l = &app.project.document.layers[active_idx];
            if l.kind == crate::document::LayerKind::AV {
                Some(l.id)
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(vid_id) = active_video_id_from_index {
        if app.show_video_editor_window.is_some() {
            app.show_video_editor_window = Some(vid_id);
        }
    }

    let Some(layer_id) = app.show_video_editor_window.or(active_video_id_from_index).or_else(|| {
        app.selection.first().copied().and_then(|sel_id| {
            app.project.document.layers.iter().find(|l| l.id == sel_id && (l.kind == crate::document::LayerKind::AV)).map(|l| l.id)
        })
    }).or_else(|| {
        app.project.document.layers.iter().find(|l| l.kind == crate::document::LayerKind::AV).map(|l| l.id)
    }) else {
        return;
    };
    
    let mut layer_pos = None;
    for (i, l) in app.project.document.layers.iter().enumerate() {
        if l.id == layer_id {
            layer_pos = Some(i);
            break;
        }
    }
    
    let Some(pos) = layer_pos else {
        return;
    };

    let inset = theme::overlay_work_rect(work);
    let gap = theme::chrome_gap() as f32;
    let action_bar_open_amount = app.ui_anim.action_bar_open_t();
    let action_bar_visible_width = app.action_bar_width * action_bar_open_amount;
    let width_reduction = if action_bar_open_amount > 0.001 {
        action_bar_visible_width + gap
    } else {
        0.0
    };
    let max_w = inset.width() - 2.0 * gap - width_reduction;

    let track_count = crate::av_ui::collect_timeline_rows(&app.project.document.layers).len();
    let extracting = video_audio_extracting(app);
    let show_details = app.project.document.active_layer().is_some_and(|l| {
        l.kind == crate::document::LayerKind::AV
    });
    let expected_h = video_editor_panel_height(track_count, extracting, show_details);
    let card_w = max_w;  // always use current available to avoid sticking on resize/ab toggle
    let card_h = restore_floater_height(
        app.video_editor_container_h,
        expected_h,
        FLOATING_PANEL_MAX_H,
    );
    let left = inset.left() + gap;
    let dock_inset = theme::STATUS_BAR_HEIGHT + theme::FLOATING_ABOVE_STATUS_GAP;
    let screen_y = ctx.content_rect().max.y;
    let open_top = screen_y - dock_inset - card_h;
    let travel = card_h + dock_inset + gap;
    let top = open_top + (1.0 - open_t) * travel;
    let rect = Rect::from_min_size(egui::pos2(left, top), egui::vec2(card_w, card_h));
    let opacity = egui::emath::easing::cubic_out(open_t);

    // DAW piano opens as a centered dialog (see daw_piano_dialog), not a slide-up floater.

    if let Some(actual_rect) = theme::show_action_bar_area(ctx, "floating_video_editor", rect, opacity, |ui| {
        video_editor_interior(app, ui, pos);
    }) {
        app.video_editor_container_h = actual_rect.height();
        app.video_editor_container_w = actual_rect.width();
    }
}

fn video_audio_extracting(app: &VadadeeBerryApp) -> bool {
    app.audio_extract_status.lock().ok().is_some_and(|m| {
        m.values()
            .any(|s| matches!(s, AudioExtractStatus::Extracting { .. }))
    })
}

fn video_editor_panel_height(track_count: usize, extracting: bool, show_details: bool) -> f32 {
    let tracks = track_count.max(1).min(5);
    let mut h = 52.0 + 20.0 + tracks as f32 * 36.0 + 10.0;
    if show_details {
        h += 44.0;
    }
    if extracting {
        h += 26.0;
    }
    h.max(130.0)
}

fn best_video_extract_progress(app: &VadadeeBerryApp) -> Option<f32> {
    let map = app.audio_extract_status.lock().ok()?;
    let mut best = 0.0f32;
    let mut any = false;
    for layer in &app.project.document.layers {
        if layer.kind != crate::document::LayerKind::AV || layer.video_path.is_empty() {
            continue;
        }
        if let Some(AudioExtractStatus::Extracting { progress }) = map.get(&layer.video_path) {
            any = true;
            best = best.max(*progress);
        }
    }
    any.then_some(best.clamp(0.0, 1.0))
}

fn video_editor_interior(app: &mut VadadeeBerryApp, ui: &mut egui::Ui, _layer_pos: usize) {
    app.sync_stale_media_layer_durations();
    for layer in &mut app.project.document.layers {
        if layer.kind == crate::document::LayerKind::AV {
            layer.ensure_av_clips();
        }
    }

    let fps = app.anim_fps as f32;
    let max_frames = app.get_max_animation_frame() as f32;
    
    let mut curr_frame = app.anim_current_frame;
    let mut scroll = app.anim_timeline_scroll;

    // Auto-follow playhead: scroll so the playhead stays in the middle 70% of the timeline viewport
    if app.anim_timeline_follow {
        let left_boundary = scroll + 15.0;
        let right_boundary = scroll + 85.0;
        let current = curr_frame as f32;
        if current < left_boundary {
            scroll = (current - 15.0).max(0.0);
        } else if current > right_boundary {
            scroll = (current - 85.0).max(0.0);
        }
    }

    let mut apply_anim_for_frame = None;
    let mut close_editor = false;

    let timeline_rows = crate::av_ui::collect_timeline_rows(&app.project.document.layers);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("🎬 AV / MEDIA TIMELINE EDITOR").strong().color(colors::ACCENT));
            ui.add_space(16.0);
            ui.checkbox(&mut app.anim_timeline_follow, "Follow Playhead");
            ui.add_space(8.0);
            ui.label(RichText::new("Frame width").small().color(colors::TEXT_MUTED));
            let mut vis = app.anim_timeline_visible_frames.max(10.0);
            if ui
                .add(
                    egui::DragValue::new(&mut vis)
                        .range(10.0..=5000.0)
                        .speed(2.0)
                        .suffix(" frames"),
                )
                .on_hover_text("Visible time span of the AV timeline")
                .changed()
            {
                app.anim_timeline_visible_frames = vis;
            }
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new(icons::CLOSE).font(nerd_font_id(12.0))).clicked() {
                    close_editor = true;
                }
            });
        });
        ui.add_space(2.0);
        ui.separator();
        ui.add_space(4.0);

        // Split / DAW live on the main floating toolbar when an AV layer is selected.

        if let Some(progress) = best_video_extract_progress(app) {
            ui.ctx().request_repaint();
            paint_video_editor_extract_banner(ui, progress);
            ui.add_space(4.0);
        }

        let left_col_w = 120.0;
        let track_w = (ui.available_width() - left_col_w - 12.0).max(50.0);
        let ruler_h = 20.0;

        // Draw top playhead ruler aligned with the timeline tracks
        ui.horizontal(|ui| {
            // spacer for left column alignment
            ui.allocate_rect(egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(left_col_w, ruler_h)), egui::Sense::hover());
            let (ruler_resp, ruler_painter) = ui.allocate_painter(egui::vec2(track_w, ruler_h), egui::Sense::click_and_drag());
            let ruler_rect = ruler_resp.rect;

            ruler_painter.rect(
                ruler_rect,
                egui::CornerRadius::same(2),
                egui::Color32::from_rgb(30, 32, 40),
                egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 53, 65)),
                egui::StrokeKind::Inside,
            );

            let start_frame = scroll;
            let visible_frames = app.anim_timeline_visible_frames.max(10.0);
            let end_frame = start_frame + visible_frames;
            let start_sec = start_frame / fps;
            let visible_sec = visible_frames / fps;
            let end_sec = end_frame / fps;

            let start_sec_grid = (start_sec.floor() as i32).max(0);
            let end_sec_grid = end_sec.ceil() as i32;

            for i in start_sec_grid..=end_sec_grid {
                let pct = (i as f32 - start_sec) / visible_sec;
                if pct >= 0.0 && pct <= 1.0 {
                    let x = ruler_rect.left() + pct * ruler_rect.width();
                    ruler_painter.line_segment(
                        [egui::pos2(x, ruler_rect.top()), egui::pos2(x, ruler_rect.bottom())],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 63, 75)),
                    );
                    if i % 2 == 0 || end_sec_grid - start_sec_grid < 10 {
                        ruler_painter.text(
                            egui::pos2(x + 2.0, ruler_rect.top() + 6.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{i}s"),
                            egui::FontId::proportional(8.0),
                            egui::Color32::from_rgb(150, 155, 170),
                        );
                    }
                }
            }

            // Draw orange playhead line on the ruler
            let playhead_frac = (curr_frame as f32 - start_frame) / visible_frames;
            if playhead_frac >= 0.0 && playhead_frac <= 1.0 {
                let playhead_x = ruler_rect.left() + playhead_frac * ruler_rect.width();
                ruler_painter.line_segment(
                    [egui::pos2(playhead_x, ruler_rect.top() - 2.0), egui::pos2(playhead_x, ruler_rect.bottom() + 2.0)],
                    egui::Stroke::new(1.8, egui::Color32::from_rgb(255, 165, 0)),
                );
            }

            if ruler_resp.dragged() || ruler_resp.clicked() {
                if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                    let frac = ((mpos.x - ruler_rect.left()) / ruler_rect.width()).clamp(0.0, 1.0);
                    let target_frame = (start_frame + frac * visible_frames).round() as usize;
                    curr_frame = target_frame; // allow > current max to extend timeline / set high frames
                    apply_anim_for_frame = Some(curr_frame);
                }
            }
        });
        ui.add_space(2.0);

        // Helper to truncate long name (byte-safe; never scan multi-MB strings).
        let truncate_name = |name: &str| -> String { safe_trunc_label(name, 15) };

        let start_frame = scroll;
        let visible_frames = app.anim_timeline_visible_frames.max(10.0);
        let visible_sec = visible_frames / fps;
        let start_sec = start_frame / fps;
        let end_sec = (start_frame + visible_frames) / fps;
        let start_sec_grid = (start_sec.floor() as i32).max(0);
        let end_sec_grid = end_sec.ceil() as i32;

        let mut scroll_area = egui::ScrollArea::vertical();
        if timeline_rows.len() > 5 {
            scroll_area = scroll_area.max_height(5.0 * 36.0);
        }

        let mut scroll_delta_timeline = 0.0;
        let mut scroll_follow_disable = false;
        // Primary press starts a sticky clip/trim drag; release clears it.
        let pointer_primary_down = ui.input(|i| i.pointer.primary_down());
        let pointer_primary_pressed = ui.input(|i| i.pointer.primary_pressed());
        let pointer_primary_released = ui.input(|i| i.pointer.primary_released());
        if pointer_primary_released {
            app.av_timeline_drag = None;
        }
        let pointer_x_now = ui.input(|i| i.pointer.hover_pos().map(|p| p.x));

        // Apply sticky drag with absolute pointer mapping (no mid-drag mode flip).
        if let (Some(drag), Some(px)) = (app.av_timeline_drag, pointer_x_now) {
            if pointer_primary_down {
                let (start, len, offset) = crate::av_ui::apply_sticky_drag(&drag, px);
                if let Some(l) = app.project.document.layers.get_mut(drag.layer_idx) {
                    if drag.is_music {
                        if let Some(clip) = l.music_clips.iter_mut().find(|c| c.id == drag.clip_id) {
                            clip.timeline_start_sec = start;
                            clip.duration_sec = len;
                            clip.track_row = 0;
                        }
                    } else if let Some(clip) = l.av_clips.iter_mut().find(|c| c.id == drag.clip_id) {
                        // Only this clip — never sync length onto other queue items.
                        clip.video_timeline_start = start;
                        clip.video_play_length = len.max(0.1);
                        clip.video_start_offset = offset.max(0.0);
                        clip.track_row = 0;
                        let id = clip.id;
                        l.sync_legacy_from_clip_id(id);
                    }
                }
            }
        }

        scroll_area.show(ui, |ui| {
            for row in &timeline_rows {
                let idx = row.layer_idx;
                ui.horizontal(|ui| {
                    let is_selected_layer = app.project.document.active_layer_index == idx;
                    let display_name = truncate_name(&row.layer_name);
                    let icon = match row.av_role {
                        crate::document::AvRole::Audio => icons::AUDIO,
                        crate::document::AvRole::Daw => icons::MUSIC,
                        crate::document::AvRole::Video => icons::VIDEO,
                    };
                    let label =
                        RichText::new(format!("{} {}", icon, display_name)).font(nerd_font_id(11.0));
                    let label_resp = ui.add_sized(
                        egui::vec2(left_col_w, 32.0),
                        egui::SelectableLabel::new(is_selected_layer, label),
                    );
                    if label_resp.clicked() {
                        app.set_active_layer(idx);
                    }
                    label_resp.on_hover_text(safe_trunc_label(&row.row_label, 96));

                    let (track_resp, track_painter) =
                        ui.allocate_painter(egui::vec2(track_w, 32.0), egui::Sense::click_and_drag());
                    let track_rect = track_resp.rect;

                    track_painter.rect(
                        track_rect,
                        egui::CornerRadius::same(4),
                        if is_selected_layer {
                            egui::Color32::from_rgb(35, 38, 48)
                        } else {
                            egui::Color32::from_rgb(25, 27, 34)
                        },
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 48, 58)),
                        egui::StrokeKind::Inside,
                    );

                    for i in start_sec_grid..=end_sec_grid {
                        let pct = (i as f32 - start_sec) / visible_sec;
                        if pct >= 0.0 && pct <= 1.0 {
                            let x = track_rect.left() + pct * track_rect.width();
                            track_painter.line_segment(
                                [egui::pos2(x, track_rect.top()), egui::pos2(x, track_rect.bottom())],
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 37, 45)),
                            );
                        }
                    }

                    let clip_painter = track_painter.with_clip_rect(track_rect);
                    let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                    let is_down = pointer_primary_down;
                    let l = &app.project.document.layers[idx];
                    let active_drag = app.av_timeline_drag;
                    let mut hit_on_press: Option<crate::av_ui::AvTimelineDrag> = None;

                    // Paint entire media queue on this single layer row.
                    for clip_id in &row.av_clip_ids {
                        let Some(clip) = l.av_clips.iter().find(|c| c.id == *clip_id) else {
                            continue;
                        };
                        let clip_rect =
                            crate::av_ui::av_clip_rect(track_rect, clip, start_frame, visible_frames, fps);
                        if clip_rect.max.x <= track_rect.min.x || clip_rect.min.x >= track_rect.max.x {
                            continue;
                        }
                        // While sticky-dragging this clip, force highlight of locked mode.
                        let av_hit = if let Some(d) = active_drag.filter(|d| d.clip_id == *clip_id && !d.is_music)
                        {
                            match d.mode {
                                crate::av_ui::AvDragMode::Move => crate::av_ui::AvClipHit::Body,
                                crate::av_ui::AvDragMode::TrimStart => crate::av_ui::AvClipHit::TrimStart,
                                crate::av_ui::AvDragMode::TrimEnd => crate::av_ui::AvClipHit::TrimEnd,
                            }
                        } else {
                            mouse_pos
                                .filter(|mp| track_rect.contains(*mp))
                                .map(|mp| crate::av_ui::hit_test_clip(clip_rect, mp, None))
                                .unwrap_or_default()
                        };
                        let is_hovered = !matches!(av_hit, crate::av_ui::AvClipHit::None)
                            || active_drag.is_some_and(|d| d.clip_id == *clip_id);
                        let audio_only = clip.is_audio_only();

                        if audio_only {
                            let fill = if is_hovered && is_down {
                                egui::Color32::from_rgba_unmultiplied(26, 184, 93, 240)
                            } else if is_hovered {
                                egui::Color32::from_rgba_unmultiplied(66, 224, 133, 210)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(46, 204, 113, 180)
                            };
                            clip_painter.rect(
                                clip_rect,
                                egui::CornerRadius::same(3),
                                fill,
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 255, 160)),
                                egui::StrokeKind::Inside,
                            );
                        } else {
                            let v_rect = Rect::from_min_max(
                                egui::pos2(clip_rect.min.x, clip_rect.min.y),
                                egui::pos2(clip_rect.max.x, clip_rect.min.y + 12.0),
                            );
                            let a_rect = Rect::from_min_max(
                                egui::pos2(clip_rect.min.x, clip_rect.min.y + 12.0),
                                egui::pos2(clip_rect.max.x, clip_rect.max.y),
                            );
                            let fill_v = if is_hovered && is_down {
                                egui::Color32::from_rgba_unmultiplied(36, 105, 217, 240)
                            } else if is_hovered {
                                egui::Color32::from_rgba_unmultiplied(66, 135, 247, 210)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(46, 115, 227, 180)
                            };
                            clip_painter.rect(
                                v_rect,
                                egui::CornerRadius::same(2),
                                fill_v,
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 160, 255)),
                                egui::StrokeKind::Inside,
                            );
                            clip_painter.rect(
                                a_rect,
                                egui::CornerRadius::same(2),
                                egui::Color32::from_rgba_unmultiplied(46, 204, 113, 180),
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 255, 160)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        crate::av_ui::paint_trim_caps(
                            &clip_painter,
                            clip_rect,
                            av_hit,
                            egui::Color32::from_rgb(80, 140, 255),
                            egui::Color32::WHITE,
                        );
                        // Always draw handle edges faintly for grab affordance.
                        {
                            let handle = 14.0f32.min(clip_rect.width() * 0.35).max(10.0);
                            let left = Rect::from_min_size(
                                clip_rect.min,
                                egui::vec2(handle, clip_rect.height()),
                            );
                            let right = Rect::from_min_size(
                                egui::pos2(clip_rect.max.x - handle, clip_rect.min.y),
                                egui::vec2(handle, clip_rect.height()),
                            );
                            let cap_col = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50);
                            clip_painter.rect_filled(left, egui::CornerRadius::same(2), cap_col);
                            clip_painter.rect_filled(right, egui::CornerRadius::same(2), cap_col);
                        }
                        let short = safe_trunc_label(&clip.name, 24);
                        let link = if clip.is_object_linked() { "*" } else { "" };
                        clip_painter.text(
                            clip_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!(
                                "{short}{link} ({:.1}-{:.1}s)",
                                clip.video_timeline_start,
                                clip.timeline_end_secs()
                            ),
                            egui::FontId::proportional(9.0),
                            egui::Color32::WHITE,
                        );

                        // Start sticky drag only on primary press (not every drag frame).
                        if pointer_primary_pressed && app.av_timeline_drag.is_none() {
                            if let Some(mp) = mouse_pos {
                                if track_rect.contains(mp) {
                                    let hit = crate::av_ui::hit_test_clip(clip_rect, mp, None);
                                    let mode = match hit {
                                        crate::av_ui::AvClipHit::Body => {
                                            Some(crate::av_ui::AvDragMode::Move)
                                        }
                                        crate::av_ui::AvClipHit::TrimStart => {
                                            Some(crate::av_ui::AvDragMode::TrimStart)
                                        }
                                        crate::av_ui::AvClipHit::TrimEnd => {
                                            Some(crate::av_ui::AvDragMode::TrimEnd)
                                        }
                                        _ => None,
                                    };
                                    if let Some(mode) = mode {
                                        hit_on_press = Some(crate::av_ui::AvTimelineDrag {
                                            layer_idx: idx,
                                            clip_id: *clip_id,
                                            is_music: false,
                                            mode,
                                            origin_start_sec: clip.video_timeline_start,
                                            origin_len_sec: clip
                                                .timeline_play_secs()
                                                .max(clip.video_play_length)
                                                .max(0.1),
                                            origin_offset_sec: clip.video_start_offset,
                                            origin_pointer_x: mp.x,
                                            origin_track_w: track_rect.width(),
                                            origin_visible_sec: visible_sec,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // DAW queue on the same layer row.
                    for mclip_id in &row.music_clip_ids {
                        let Some(mclip) = l.music_clips.iter().find(|c| c.id == *mclip_id) else {
                            continue;
                        };
                        let mrect = crate::av_ui::music_clip_rect(
                            track_rect,
                            mclip,
                            start_frame,
                            visible_frames,
                            fps,
                        );
                        if mrect.max.x <= track_rect.min.x || mrect.min.x >= track_rect.max.x {
                            continue;
                        }
                        let m_hit = if let Some(d) =
                            active_drag.filter(|d| d.clip_id == *mclip_id && d.is_music)
                        {
                            match d.mode {
                                crate::av_ui::AvDragMode::Move => {
                                    crate::av_ui::AvClipHit::MusicBody(*mclip_id)
                                }
                                crate::av_ui::AvDragMode::TrimStart => {
                                    crate::av_ui::AvClipHit::MusicTrimStart(*mclip_id)
                                }
                                crate::av_ui::AvDragMode::TrimEnd => {
                                    crate::av_ui::AvClipHit::MusicTrimEnd(*mclip_id)
                                }
                            }
                        } else {
                            mouse_pos
                                .filter(|mp| track_rect.contains(*mp))
                                .map(|mp| crate::av_ui::hit_test_clip(mrect, mp, Some(mclip.id)))
                                .unwrap_or_default()
                        };
                        clip_painter.rect(
                            mrect,
                            egui::CornerRadius::same(3),
                            egui::Color32::from_rgba_unmultiplied(160, 70, 220, 200),
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 140, 255)),
                            egui::StrokeKind::Inside,
                        );
                        crate::av_ui::paint_trim_caps(
                            &clip_painter,
                            mrect,
                            m_hit,
                            egui::Color32::from_rgb(180, 90, 255),
                            egui::Color32::WHITE,
                        );
                        {
                            let handle = 14.0f32.min(mrect.width() * 0.35).max(10.0);
                            let left =
                                Rect::from_min_size(mrect.min, egui::vec2(handle, mrect.height()));
                            let right = Rect::from_min_size(
                                egui::pos2(mrect.max.x - handle, mrect.min.y),
                                egui::vec2(handle, mrect.height()),
                            );
                            let cap_col = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50);
                            clip_painter.rect_filled(left, egui::CornerRadius::same(2), cap_col);
                            clip_painter.rect_filled(right, egui::CornerRadius::same(2), cap_col);
                        }
                        let short = safe_trunc_label(&mclip.name, 24);
                        clip_painter.text(
                            mrect.center(),
                            egui::Align2::CENTER_CENTER,
                            short,
                            egui::FontId::proportional(9.0),
                            egui::Color32::WHITE,
                        );
                        if track_resp.double_clicked() {
                            if let Some(mp) = mouse_pos {
                                if mrect.contains(mp) {
                                    app.piano_roll_clip = Some(mclip.id);
                                }
                            }
                        }
                        if pointer_primary_pressed && app.av_timeline_drag.is_none() {
                            if let Some(mp) = mouse_pos {
                                if track_rect.contains(mp) {
                                    let hit =
                                        crate::av_ui::hit_test_clip(mrect, mp, Some(mclip.id));
                                    let mode = match hit {
                                        crate::av_ui::AvClipHit::MusicBody(_) => {
                                            Some(crate::av_ui::AvDragMode::Move)
                                        }
                                        crate::av_ui::AvClipHit::MusicTrimStart(_) => {
                                            Some(crate::av_ui::AvDragMode::TrimStart)
                                        }
                                        crate::av_ui::AvClipHit::MusicTrimEnd(_) => {
                                            Some(crate::av_ui::AvDragMode::TrimEnd)
                                        }
                                        _ => None,
                                    };
                                    if let Some(mode) = mode {
                                        hit_on_press = Some(crate::av_ui::AvTimelineDrag {
                                            layer_idx: idx,
                                            clip_id: mclip.id,
                                            is_music: true,
                                            mode,
                                            origin_start_sec: mclip.timeline_start_sec,
                                            origin_len_sec: mclip.duration_sec.max(0.1),
                                            origin_offset_sec: 0.0,
                                            origin_pointer_x: mp.x,
                                            origin_track_w: track_rect.width(),
                                            origin_visible_sec: visible_sec,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    if let Some(drag) = hit_on_press {
                        app.av_timeline_drag = Some(drag);
                        app.anim_timeline_follow = false;
                    }

                    // Empty track drag scrolls timeline — only when not grabbing a clip.
                    if track_resp.dragged() && app.av_timeline_drag.is_none() {
                        if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                            // Only scroll if press was on empty track (not a clip).
                            let pressed_on_clip = {
                                let l = &app.project.document.layers[idx];
                                let mut hit = false;
                                for c in &l.av_clips {
                                    let r = crate::av_ui::av_clip_rect(
                                        track_rect,
                                        c,
                                        start_frame,
                                        visible_frames,
                                        fps,
                                    );
                                    if !matches!(
                                        crate::av_ui::hit_test_clip(r, origin, None),
                                        crate::av_ui::AvClipHit::None
                                    ) {
                                        hit = true;
                                        break;
                                    }
                                }
                                if !hit {
                                    for m in &l.music_clips {
                                        let r = crate::av_ui::music_clip_rect(
                                            track_rect,
                                            m,
                                            start_frame,
                                            visible_frames,
                                            fps,
                                        );
                                        if !matches!(
                                            crate::av_ui::hit_test_clip(r, origin, Some(m.id)),
                                            crate::av_ui::AvClipHit::None
                                        ) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                }
                                hit
                            };
                            if !pressed_on_clip {
                                scroll_delta_timeline = track_resp.drag_delta().x
                                    / track_rect.width()
                                    * visible_frames;
                                scroll_follow_disable = true;
                            }
                        }
                    }

                    let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
                    let wheel_delta = if scroll_delta.x != 0.0 {
                        scroll_delta.x
                    } else {
                        scroll_delta.y
                    };
                    if wheel_delta != 0.0 && track_resp.hovered() && app.av_timeline_drag.is_none()
                    {
                        scroll_delta_timeline = wheel_delta * 0.1;
                        scroll_follow_disable = true;
                    }

                    let playhead_frac = (curr_frame as f32 - start_frame) / visible_frames;
                    if playhead_frac >= 0.0 && playhead_frac <= 1.0 {
                        let playhead_x = track_rect.left() + playhead_frac * track_rect.width();
                        track_painter.line_segment(
                            [
                                egui::pos2(playhead_x, track_rect.top()),
                                egui::pos2(playhead_x, track_rect.bottom()),
                            ],
                            egui::Stroke::new(1.2, egui::Color32::from_rgb(255, 165, 0)),
                        );
                    }

                    // Cursor feedback
                    if app.av_timeline_drag.is_some()
                        || mouse_pos.is_some_and(|mp| {
                            let l = &app.project.document.layers[idx];
                            l.av_clips.iter().any(|c| {
                                let r = crate::av_ui::av_clip_rect(
                                    track_rect,
                                    c,
                                    start_frame,
                                    visible_frames,
                                    fps,
                                );
                                !matches!(
                                    crate::av_ui::hit_test_clip(r, mp, None),
                                    crate::av_ui::AvClipHit::None
                                )
                            }) || l.music_clips.iter().any(|m| {
                                let r = crate::av_ui::music_clip_rect(
                                    track_rect,
                                    m,
                                    start_frame,
                                    visible_frames,
                                    fps,
                                );
                                !matches!(
                                    crate::av_ui::hit_test_clip(r, mp, Some(m.id)),
                                    crate::av_ui::AvClipHit::None
                                )
                            })
                        })
                    {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        if app.av_timeline_drag.is_some() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                    }
                });
            }
        });

        if scroll_delta_timeline != 0.0 && app.av_timeline_drag.is_none() {
            if scroll_follow_disable {
                app.anim_timeline_follow = false;
            }
            scroll = (scroll - scroll_delta_timeline).max(0.0);
        }

        // Active Track Details — edit the **selected** clip only (never other queue items).
        let active_layer_idx = app.project.document.active_layer_index;
        let selected_clip_id = {
            let layer = app.project.document.layers.get(active_layer_idx);
            layer.and_then(|l| {
                if l.kind != crate::document::LayerKind::AV {
                    return None;
                }
                // Prefer selected clip on this layer; else clip under playhead; else first.
                let t = app.anim_current_frame as f32 / app.anim_fps as f32;
                l.av_clips
                    .iter()
                    .find(|c| app.selection.contains(&c.id))
                    .map(|c| c.id)
                    .or_else(|| {
                        l.av_clips
                            .iter()
                            .find(|c| c.contains_timeline_sec(t))
                            .map(|c| c.id)
                    })
                    .or_else(|| l.av_clips.first().map(|c| c.id))
            })
        };

        if let Some(clip_id) = selected_clip_id {
            if let Some(layer) = app.project.document.layers.get_mut(active_layer_idx) {
                if layer.kind == crate::document::LayerKind::AV {
                    layer.ensure_av_clips();
                    // Pull this clip into layer fields for the editors.
                    layer.sync_legacy_from_clip_id(clip_id);

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        let clip_label = layer
                            .av_clips
                            .iter()
                            .find(|c| c.id == clip_id)
                            .map(|c| safe_trunc_label(&c.name, 24))
                            .unwrap_or_else(|| "clip".into());
                        ui.label(
                            RichText::new(format!("Active Clip: {clip_label}"))
                                .strong()
                                .color(colors::ACCENT),
                        );
                        ui.add_space(16.0);

                        ui.label("Volume:");
                        let mut vol_percent = (layer.volume * 100.0) as i32;
                        if ui
                            .add(egui::Slider::new(&mut vol_percent, 0..=100).suffix("%"))
                            .changed()
                        {
                            layer.volume = vol_percent as f32 / 100.0;
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        let source_cap = layer
                            .media_source_duration
                            .filter(|d| *d > 0.05)
                            .unwrap_or(3600.0);
                        let trim_max = source_cap.max(0.1);

                        ui.label("Trim Start:");
                        if layer.video_start_offset > trim_max {
                            layer.video_start_offset = 0.0;
                            layer.sync_clip_from_legacy(clip_id);
                        }
                        let mut trim_start = layer.video_start_offset.clamp(0.0, trim_max);
                        if ui
                            .add(
                                egui::DragValue::new(&mut trim_start)
                                    .speed(0.1)
                                    .range(0.0..=trim_max)
                                    .suffix("s"),
                            )
                            .changed()
                        {
                            layer.video_start_offset = trim_start;
                            let remaining = (source_cap - trim_start).max(0.1);
                            if layer.video_play_length > remaining {
                                layer.video_play_length = remaining;
                            }
                            layer.sync_clip_from_legacy(clip_id);
                        }

                        ui.add_space(8.0);

                        ui.label("Play Duration:");
                        let remaining_after_trim =
                            (source_cap - layer.video_start_offset.max(0.0)).max(0.1);
                        let mut play_len = layer.video_play_length;
                        if play_len >= 3599.0 {
                            play_len = remaining_after_trim;
                        }
                        let play_max = remaining_after_trim;
                        if ui
                            .add(
                                egui::DragValue::new(&mut play_len)
                                    .speed(0.1)
                                    .range(0.1..=play_max)
                                    .suffix("s"),
                            )
                            .changed()
                        {
                            layer.video_play_length = play_len.min(play_max);
                            layer.sync_clip_from_legacy(clip_id);
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label("Bass:");
                        ui.add(
                            egui::DragValue::new(&mut layer.eq_bass)
                                .speed(0.1)
                                .range(-12.0..=12.0)
                                .suffix("dB"),
                        );
                        ui.label("Mid:");
                        ui.add(
                            egui::DragValue::new(&mut layer.eq_mid)
                                .speed(0.1)
                                .range(-12.0..=12.0)
                                .suffix("dB"),
                        );
                        ui.label("Treble:");
                        ui.add(
                            egui::DragValue::new(&mut layer.eq_treble)
                                .speed(0.1)
                                .range(-12.0..=12.0)
                                .suffix("dB"),
                        );
                    });
                }
            }
        }

    });

    if close_editor {
        app.show_video_editor_window = None;
    }
    if curr_frame != app.anim_current_frame {
        app.anim_current_frame = curr_frame;
    }
    if scroll != app.anim_timeline_scroll {
        app.anim_timeline_scroll = scroll;
    }
    if let Some(frame) = apply_anim_for_frame {
        app.apply_animation_for_frame(frame);
    }
}

fn paint_video_editor_extract_banner(ui: &mut Ui, progress: f32) {
    let progress = progress.clamp(0.0, 1.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Audio extraction")
                .small()
                .strong()
                .color(egui::Color32::from_rgb(120, 230, 150)),
        );
        ui.add_space(8.0);
        let w = (ui.available_width() - 48.0).max(80.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(12, 28, 18));
        paint_extract_progress_in_rect(painter, rect, progress);
        ui.label(
            RichText::new(format!("{:.0}%", progress * 100.0))
                .small()
                .color(colors::ACCENT),
        );
    });
}

fn paint_extract_progress_in_rect(painter: &egui::Painter, rect: Rect, progress: f32) {
    let progress = progress.clamp(0.0, 1.0);
    let dark = egui::Color32::from_rgb(8, 92, 38);
    let light = egui::Color32::from_rgb(140, 255, 170);
    let fill_w = rect.width() * progress;
    if fill_w < 0.5 {
        return;
    }
    let strips = 32usize;
    for i in 0..strips {
        let t0 = i as f32 / strips as f32;
        let t1 = (i + 1) as f32 / strips as f32;
        let x0 = rect.min.x + fill_w * t0;
        let x1 = rect.min.x + fill_w * t1;
        if x1 <= x0 + 0.05 {
            continue;
        }
        let t_mid = (t0 + t1) * 0.5;
        let lerp = |a: u8, b: u8| -> u8 {
            (a as f32 + (b as f32 - a as f32) * t_mid).round() as u8
        };
        let color = egui::Color32::from_rgb(
            lerp(dark.r(), light.r()),
            lerp(dark.g(), light.g()),
            lerp(dark.b(), light.b()),
        );
        let strip = egui::Rect::from_min_max(egui::pos2(x0, rect.min.y), egui::pos2(x1, rect.max.y));
        painter.rect_filled(strip, 2.0, color);
    }
}

/// Reserves space at the bottom of the window (layout only).
fn status_bar_layout_reserve(ui: &mut Ui) {
    egui::Panel::bottom("status")
        .frame(egui::Frame::NONE)
        .exact_size(theme::STATUS_BAR_HEIGHT)
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.allocate_exact_size(
                egui::vec2(ui.available_width(), ui.available_height()),
                egui::Sense::hover(),
            );
        });
}

/// Paints the status bar above floating panels (`Order::Foreground`).
fn status_bar_overlay(app: &mut VadadeeBerryApp, ctx: &Context) {
    let alpha = app.ui_anim.status_alpha();
    if alpha <= 0.004 {
        return;
    }
    let vp = ctx.viewport_rect();
    let h = theme::STATUS_BAR_HEIGHT;
    let rect = Rect::from_min_max(egui::pos2(vp.min.x, vp.max.y - h), vp.max);
    egui::Area::new(egui::Id::new("status_bar_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .interactable(true)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_width(rect.width());
            ui.set_height(rect.height());
            theme::bar_frame(alpha).show(ui, |ui| {
                ui.set_opacity(alpha);
                status_bar_body(app, ui);
            });
        });
}

fn status_bar_body(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    let alpha = app.ui_anim.status_alpha();
    let tool_slide_out = app.ui_anim.status_tool_slide_out(120.0);
    let tool_slide_in = app.ui_anim.status_tool_slide_in(120.0);
    let msg_slide_out = app.ui_anim.status_slide_out();
    let msg_slide_in = app.ui_anim.status_slide_in();
    let tool_width = app.ui_anim.status_tool_seg_width();
    let msg_width = app.ui_anim.status_message_seg_width();
    ui.horizontal(|ui| {
                let right_w = 200.0;
                let left_w = ui.available_width() - right_w;
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, ui.available_height()),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let current_coords_text = status_coords_text(app.cursor_doc);
                        let anim_coords_w = app.ui_anim.coords_seg_width();
                        theme::paint_powerline_status(
                            ui,
                            &app.ui_anim.status_tool_outgoing,
                            &app.ui_anim.status_tool_incoming,
                            tool_width,
                            &app.ui_anim.status_msg_outgoing,
                            &app.ui_anim.status_msg_incoming,
                            msg_width,
                            &current_coords_text,
                            &current_coords_text,
                            anim_coords_w,
                            app.viewport.zoom,
                            tool_slide_out,
                            tool_slide_in,
                            msg_slide_out,
                            msg_slide_in,
                            0.0,
                            0.0,
                            alpha,
                        );
                    }
                );
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    if app.video_export.rendering || app.video_export.progress.is_some() {
                        let show_vid = ui.button(
                            RichText::new("󰕧")
                                .font(icons::nerd_font_id(12.0))
                                .color(colors::ACCENT),
                        );
                        if show_vid.clicked() {
                            app.video_export.progress_visible = true;
                        }
                        show_vid.on_hover_text("Show video export progress");
                        ui.add_space(4.0);
                    }
                    // timeline toggle
                    let timeline_btn_icon = if app.anim_show_timeline_window { "" } else { "" };
                    let timeline_btn_tooltip = if app.anim_show_timeline_window { "Hide timeline" } else { "Show timeline" };
                    let mut text = RichText::new(timeline_btn_icon).font(nerd_font_id(12.0));
                    if app.anim_show_timeline_window {
                        text = text.color(colors::ACCENT);
                    }
                    let btn_timeline = ui.button(text);
                    if btn_timeline.clicked() {
                        app.anim_show_timeline_window = !app.anim_show_timeline_window;
                        if app.anim_show_timeline_window {
                            app.refresh_all_media_layer_durations();
                            app.show_video_editor_window = None;
                        }
                    }
                    btn_timeline.on_hover_text(timeline_btn_tooltip);

                    ui.add_space(4.0);

                    // video editor toggle (placed near animation timeline)
                    let active_video_id = app.selection.first().copied().and_then(|sel_id| {
                        app.project.document.layers.iter().find(|l| l.id == sel_id && (l.kind == crate::document::LayerKind::AV)).map(|l| l.id)
                    }).or_else(|| {
                        app.project.document.layers.iter().find(|l| l.kind == crate::document::LayerKind::AV).map(|l| l.id)
                    });

                    if let Some(video_layer_id) = active_video_id {
                        let is_video_editor_open = app.show_video_editor_window.is_some();
                        let video_btn_icon = "🎬";
                        let video_btn_tooltip = if is_video_editor_open { "Hide video editor" } else { "Show video editor" };
                        
                        let mut text = RichText::new(video_btn_icon);
                        if is_video_editor_open {
                            text = text.color(colors::ACCENT);
                        }
                        let btn_video_editor = ui.button(text);
                        if btn_video_editor.clicked() {
                            if is_video_editor_open {
                                app.show_video_editor_window = None;
                            } else {
                                app.refresh_all_media_layer_durations();
                                app.show_video_editor_window = Some(video_layer_id);
                                app.anim_show_timeline_window = false;
                            }
                        }
                        btn_video_editor.on_hover_text(video_btn_tooltip);
                        ui.add_space(4.0);
                    }

                    // playback controls
                    let play_icon = if app.anim_is_playing { "" } else { "" };
                    let play_tooltip = if app.anim_is_playing { "Pause" } else { "Play" };
                    
                    let max_anim_frame = app.get_max_animation_frame();
                    let btn_next = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_next.clicked() {
                        app.anim_current_frame = app.anim_current_frame + 1; // allow beyond to support >100 frames
                    }
                    btn_next.on_hover_text("Forward (1 frame)");

                    let btn_play = ui.button(RichText::new(play_icon).font(nerd_font_id(12.0)));
                    if btn_play.clicked() {
                        app.anim_is_playing = !app.anim_is_playing;
                        if app.anim_is_playing {
                            let now = std::time::Instant::now();
                            app.anim_playback_wall = Some(now);
                            app.anim_play_origin = Some((now, app.anim_current_frame));
                            app.anim_time_accumulator = 0.0;
                        } else {
                            app.anim_playback_wall = None;
                            app.anim_play_origin = None;
                            app.stop_all_video_streams();
                        }
                    }
                    btn_play.on_hover_text(play_tooltip);

                    let btn_prev = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_prev.clicked() {
                        app.anim_current_frame = app.anim_current_frame.saturating_sub(1);
                    }
                    btn_prev.on_hover_text("Backward (1 frame)");

                    let btn_rewind = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_rewind.clicked() {
                        app.anim_current_frame = 0;
                        app.anim_is_playing = false;
                        app.anim_playback_wall = None;
                        app.anim_play_origin = None;
                        app.stop_all_video_streams();
                    }
                    btn_rewind.on_hover_text("Back to start");

                    ui.add_space(4.0);

                    // record toggle
                    let rec_color = if app.anim_keyframing_mode { colors::POWERLINE_C } else { colors::TEXT_MUTED };
                    let btn_rec = ui.button(
                        RichText::new("󰜎")
                            .font(nerd_font_id(12.0))
                            .color(rec_color)
                    );
                    if btn_rec.clicked() {
                        app.toggle_keyframing_mode();
                    }
                    btn_rec.on_hover_text(if app.anim_keyframing_mode { "Stop keyframing" } else { "Start keyframing (Record)" });
                });
    });
}

fn page_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("Title");
        let mut title = app.project.document.title.clone();
        if ui.text_edit_singleline(&mut title).changed() {
            app.set_document_title(title);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Preset:");
        let mut selected_preset_name = "Custom".to_owned();
        let w = app.project.document.width;
        let h = app.project.document.height;
        let presets = [
            ("A0 (P)", 3179.0, 4494.0),
            ("A0 (L)", 4494.0, 3179.0),
            ("A1 (P)", 2245.0, 3179.0),
            ("A1 (L)", 3179.0, 2245.0),
            ("A2 (P)", 1587.0, 2245.0),
            ("A2 (L)", 2245.0, 1587.0),
            ("A3 (P)", 1123.0, 1587.0),
            ("A3 (L)", 1587.0, 1123.0),
            ("A4 (P)", 794.0, 1123.0),
            ("A4 (L)", 1123.0, 794.0),
            ("A5 (P)", 559.0, 794.0),
            ("A5 (L)", 794.0, 559.0),
            ("720p (H)", 1280.0, 720.0),
            ("720p (V)", 720.0, 1280.0),
            ("1080p (H)", 1920.0, 1080.0),
            ("1080p (V)", 1080.0, 1920.0),
            ("4K (H)", 3840.0, 2160.0),
            ("4K (V)", 2160.0, 3840.0),
        ];
        for (name, pw, ph) in &presets {
            if (w - *pw).abs() < 1.0 && (h - *ph).abs() < 1.0 {
                selected_preset_name = name.to_string();
                break;
            }
        }
        egui::ComboBox::from_id_salt("page_preset_combo")
            .selected_text(&selected_preset_name)
            .width(110.0)
            .show_ui(ui, |ui| {
                for (name, pw, ph) in presets {
                    if ui.selectable_label(selected_preset_name == name, name).clicked() {
                        app.set_page_size(pw, ph);
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Unit:");
        let mut unit = app.project.document.page_unit;
        if ui
            .selectable_label(unit == crate::document::PageUnit::Px, "Px")
            .clicked()
        {
            unit = crate::document::PageUnit::Px;
        }
        if ui
            .selectable_label(unit == crate::document::PageUnit::Mm, "Mm")
            .clicked()
        {
            unit = crate::document::PageUnit::Mm;
        }
        if unit != app.project.document.page_unit {
            let before = crate::history::snapshot_document(&app.project.document);
            let mut after = before.clone();
            after.page_unit = unit;
            app.history.push(
                &mut app.project,
                crate::history::ProjectEdit::PatchDocument { before, after },
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label("Size");
        let unit = app.project.document.page_unit;
        let mut w = match unit {
            crate::document::PageUnit::Px => app.project.document.width as f32,
            crate::document::PageUnit::Mm => {
                crate::document::px_to_mm(app.project.document.width) as f32
            }
        };
        let mut h = match unit {
            crate::document::PageUnit::Px => app.project.document.height as f32,
            crate::document::PageUnit::Mm => {
                crate::document::px_to_mm(app.project.document.height) as f32
            }
        };
        let suffix = match unit {
            crate::document::PageUnit::Px => "px",
            crate::document::PageUnit::Mm => "mm",
        };
        let range = match unit {
            crate::document::PageUnit::Px => 64.0..=8192.0,
            crate::document::PageUnit::Mm => 5.0..=600.0,
        };
        let ch = ui.add(decimal_drag(&mut w).range(range.clone()).suffix(suffix));
        let ch2 = ui.add(decimal_drag(&mut h).range(range).suffix(suffix));
        if ch.changed() || ch2.changed() {
            let (pw, ph) = match unit {
                crate::document::PageUnit::Px => (w as f64, h as f64),
                crate::document::PageUnit::Mm => {
                    (crate::document::mm_to_px(w as f64), crate::document::mm_to_px(h as f64))
                }
            };
            app.set_page_size(pw, ph);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Page Color");
        let mut col = app.project.document.page_color;
        if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
            app.project.document.page_color = col;
        }
    });
}

fn truncate_path_display(path: &str, max_chars: usize) -> String {
    if path.len() <= max_chars {
        return path.to_owned();
    }
    let file = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    safe_trunc_label(file, max_chars)
}

/// Track row with a hover-to-show delete control that stays hittable.
/// Always reserves delete-button space so the pointer can move onto it without the icon vanishing.
/// Returns true if the label was clicked (select).
fn track_row_with_hover_delete(
    ui: &mut Ui,
    selected: bool,
    label: RichText,
    hover_text: &str,
    delete_tip: &str,
    clip_id: uuid::Uuid,
    double_opens_daw: bool,
    delete_clip: &mut Option<uuid::Uuid>,
    piano_roll_clip: &mut Option<uuid::Uuid>,
) -> bool {
    let mut select = false;
    let hover_id = ui.make_persistent_id(("track_row_hover", clip_id));
    let show_del = ui.ctx().data(|d| d.get_temp::<bool>(hover_id)).unwrap_or(false);

    ui.horizontal(|ui| {
        let resp = ui.selectable_label(selected, label);
        if resp.clicked() {
            select = true;
        }
        if double_opens_daw && resp.double_clicked() {
            select = true;
            *piano_roll_clip = Some(clip_id);
        }
        let label_rect = resp.rect;

        // Fixed-size hit target always allocated — no layout gap when icon is hidden.
        let del_size = egui::vec2(22.0, 18.0);
        let (del_rect, del_resp) = ui.allocate_exact_size(del_size, egui::Sense::click());
        let row_rect = label_rect.union(del_rect);
        let row_hot = ui.rect_contains_pointer(row_rect);
        ui.ctx().data_mut(|d| d.insert_temp(hover_id, row_hot));

        // Paint on same frame as hover so the icon is reachable immediately.
        if row_hot || show_del {
            ui.painter().text(
                del_rect.center(),
                egui::Align2::CENTER_CENTER,
                icons::DELETE,
                nerd_font_id(12.0),
                egui::Color32::from_rgb(255, 95, 110),
            );
            if del_resp.clicked() {
                *delete_clip = Some(clip_id);
            }
            if del_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                del_resp.on_hover_text(delete_tip);
            }
        }
        resp.on_hover_text(hover_text);
    });
    select
}

/// Truncate UI labels without full Unicode scans on multi-megabyte strings.
/// Caps at `max_chars` chars using a byte pre-limit (O(max_chars), not O(len)).
fn safe_trunc_label(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if s.len() <= max_chars {
        return s.to_owned();
    }
    // Fast path: ASCII-ish short prefix
    let byte_cap = max_chars.saturating_mul(4).min(s.len());
    let prefix = &s[..byte_cap];
    let mut out = String::with_capacity(max_chars + 1);
    for (i, ch) in prefix.chars().enumerate() {
        if i >= max_chars.saturating_sub(1) {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    // If prefix exhausted under max, return as-is (entire string was short in chars).
    if s.len() <= byte_cap {
        out
    } else {
        if out.chars().count() >= max_chars {
            // already ended with …
            out
        } else {
            out.push('…');
            out
        }
    }
}

fn layers_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.menu_button("+ Add Layer ▾", |ui| {
        if ui.button("Image Layer").clicked() {
            app.add_layer("Layer");
            ui.close();
        }
        if ui.button(format!("{} Video Layer (Empty)", icons::VIDEO)).clicked() {
            let n = app
                .project
                .document
                .layers
                .iter()
                .filter(|l| l.av_role == crate::document::AvRole::Video)
                .count()
                + 1;
            app.add_empty_av_layer_with_role(&format!("Video {n}"), crate::document::AvRole::Video);
            ui.close();
        }
        if ui.button(format!("{} Audio Layer (Empty)", icons::AUDIO)).clicked() {
            let n = app
                .project
                .document
                .layers
                .iter()
                .filter(|l| l.av_role == crate::document::AvRole::Audio)
                .count()
                + 1;
            app.add_empty_av_layer_with_role(&format!("Audio {n}"), crate::document::AvRole::Audio);
            ui.close();
        }
        if ui.button(format!("{} DAW Layer (Empty)", icons::MUSIC)).clicked() {
            let n = app
                .project
                .document
                .layers
                .iter()
                .filter(|l| l.av_role == crate::document::AvRole::Daw)
                .count()
                + 1;
            app.add_empty_av_layer_with_role(&format!("DAW {n}"), crate::document::AvRole::Daw);
            ui.close();
        }
        if ui.button(format!("{} Shading Layer", icons::SHADING)).clicked() {
            let n = app.project.document.layers.len() + 1;
            app.add_shading_layer(&format!("Shading {n}"));
            ui.close();
        }
        if ui.button(RichText::new(format!("{} Flowchart Layer", icons::FLOWCHART)).font(nerd_font_id(12.0))).clicked() {
            let n = app.project.document.layers.len() + 1;
            app.add_flowchart_layer(&format!("Flowchart {n}"));
            ui.close();
        }
        if ui
            .button(
                RichText::new(format!("{} Node Editor", icons::NODE_EDITOR))
                    .font(nerd_font_id(12.0)),
            )
            .clicked()
        {
            let n = app
                .project
                .document
                .layers
                .iter()
                .filter(|l| l.kind == crate::document::LayerKind::NodeEditor)
                .count()
                + 1;
            app.add_node_editor_layer(&format!("Node Editor {n}"));
            ui.close();
        }
        #[cfg(not(target_os = "android"))]
        {
            if ui.button("Media from file… (auto Video/Audio layer)").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "Media",
                        &[
                            "mp4", "mkv", "avi", "mov", "webm", "png", "jpg", "jpeg", "webp",
                            "gif", "mp3", "wav", "aac", "m4a", "flac", "ogg",
                        ],
                    )
                    .pick_file()
                {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    app.add_av_layer(&name, path.to_string_lossy().into_owned());
                }
                ui.close();
            }
        }
    });
    ui.add_space(4.0);
    
    let _layer_count = app.project.document.layers.len();
    let mut i = 0usize;
    while i < app.project.document.layers.len() {
        let active = app.project.document.active_layer_index == i;
        let (name, visible, locked, kind) = {
            let l = &app.project.document.layers[i];
            (l.name.clone(), l.visible, l.locked, l.kind)
        };
        let mut delete_layer = false;
        ui.horizontal(|ui| {
            if ui.selectable_label(active, "●").clicked() {
                app.set_active_layer(i);
            }
            let mut vis = visible;
            if ui.checkbox(&mut vis, "V").changed() {
                app.set_layer_visible(i, vis);
            }
            let mut lck = locked;
            if ui.checkbox(&mut lck, "L").changed() {
                app.set_layer_locked(i, lck);
            }
            // Cap layer name for TextEdit — multi-MB names freeze the ActionBar.
            let mut edit_name = safe_trunc_label(&name, 128);
            let icon = match kind {
                crate::document::LayerKind::AV => {
                    match app.project.document.layers.get(i).map(|l| l.av_role) {
                        Some(crate::document::AvRole::Audio) => icons::AUDIO,
                        Some(crate::document::AvRole::Daw) => icons::MUSIC,
                        _ => icons::VIDEO,
                    }
                }
                crate::document::LayerKind::Image => icons::IMAGE,
                crate::document::LayerKind::Shading => icons::SHADING,
                crate::document::LayerKind::Flowchart => icons::FLOWCHART,
                crate::document::LayerKind::NodeEditor => icons::NODE_EDITOR,
            };
            ui.label(RichText::new(icon).font(nerd_font_id(13.0)));
            let name_w = (ui.available_width() - 28.0).max(48.0);
            if ui
                .add(egui::TextEdit::singleline(&mut edit_name).desired_width(name_w))
                .changed()
            {
                app.rename_layer(i, edit_name);
            }
            if app.project.document.layers.len() > 1 {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(icons::DELETE)
                                .font(nerd_font_id(13.0))
                                .color(egui::Color32::from_rgb(255, 95, 110)),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Delete this layer")
                    .clicked()
                {
                    delete_layer = true;
                }
            }
        });
        if delete_layer {
            app.delete_layer(i);
            continue;
        }
        if kind == crate::document::LayerKind::NodeEditor {
            let layer_id = app.project.document.layers[i].id;
            app.project.document.layers[i].ensure_node_graph();
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} New node", icons::ADD))
                    .on_hover_text("Add a Value node to the graph")
                    .clicked()
                {
                    app.set_active_layer(i);
                    app.add_graph_node_to_active(crate::document::GraphNodeKind::Value {
                        value: 0.0,
                    });
                }
                let open = app.node_editor_ui.open_layer_id == Some(layer_id);
                if ui
                    .button(
                        RichText::new(if open {
                            icons::NODE_EDITOR_HIDE
                        } else {
                            icons::NODE_EDITOR_OPEN
                        })
                        .font(nerd_font_id(14.0)),
                    )
                    .on_hover_text(if open {
                        "Hide node editor"
                    } else {
                        "Open node editor"
                    })
                    .clicked()
                {
                    if open {
                        app.node_editor_ui.close();
                    } else {
                        app.set_active_layer(i);
                        app.selection = vec![layer_id];
                        app.node_editor_ui.open(layer_id);
                        promote_action_tab(app, ActionTab::Parameter);
                    }
                }
            });
            if let Some(g) = app.project.document.layers[i].node_graph.as_ref() {
                ui.label(
                    RichText::new(format!(
                        "     {} nodes · {} links",
                        g.nodes.len(),
                        g.links.len()
                    ))
                    .small()
                    .weak(),
                );
                if let Some(err) = &g.root_error {
                    ui.colored_label(egui::Color32::from_rgb(255, 120, 120), err);
                }
            }
        }
        if kind == crate::document::LayerKind::AV {
            let mut l = app.project.document.layers[i].clone();
            l.ensure_av_clips();
            let mut delete_clip: Option<uuid::Uuid> = None;
            for clip in l.av_clips.iter().rev() {
                let selected = app.selection.contains(&clip.id);
                let cicon = if clip.is_audio_only() {
                    icons::AUDIO
                } else if clip.is_still_image() {
                    icons::IMAGE
                } else {
                    icons::VIDEO
                };
                let label = RichText::new(format!(
                    "     {} {}",
                    cicon,
                    safe_trunc_label(&clip.name, 16)
                ))
                .font(nerd_font_id(12.0))
                .weak();
                if track_row_with_hover_delete(
                    ui,
                    selected,
                    label,
                    &format!(
                        "{}\n{}",
                        safe_trunc_label(&clip.name, 48),
                        safe_trunc_label(&clip.media_path, 64)
                    ),
                    "Delete track",
                    clip.id,
                    false,
                    &mut delete_clip,
                    &mut app.piano_roll_clip,
                ) {
                    app.set_selection(vec![clip.id]);
                }
            }
            for mclip in l.music_clips.iter().rev() {
                let selected = app.selection.contains(&mclip.id);
                let label = RichText::new(format!(
                    "     {} {}",
                    icons::MUSIC,
                    safe_trunc_label(&mclip.name, 16)
                ))
                .font(nerd_font_id(12.0))
                .weak();
                if track_row_with_hover_delete(
                    ui,
                    selected,
                    label,
                    &format!(
                        "{}\nDouble-click to open DAW piano",
                        safe_trunc_label(&mclip.name, 48)
                    ),
                    "Delete DAW track",
                    mclip.id,
                    true,
                    &mut delete_clip,
                    &mut app.piano_roll_clip,
                ) {
                    app.set_selection(vec![mclip.id]);
                }
            }
            if let Some(cid) = delete_clip {
                app.delete_av_clip(i, cid);
            }
        }
        i += 1;
    }

    // Active Layer settings (Renderer/Non-renderer and Video/Audio details)
    let active_idx = app.project.document.active_layer_index;
    let mut probe_media_at: Option<usize> = None;
    if let Some(l) = app.project.document.layers.get_mut(active_idx) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        theme::constraint_block(ui, |ui| {
            ui.label(RichText::new("Layer Properties").strong());
            ui.add_space(4.0);

            ui.checkbox(&mut l.is_renderer, "Export Renderer Layer").on_hover_text("If unchecked, this layer will not render/play during export");

            ui.horizontal(|ui| {
                ui.label("Type:");
                let current_label = match l.kind {
                    crate::document::LayerKind::Image => format!("{} Image", icons::IMAGE),
                    crate::document::LayerKind::AV => format!("{} AV", icons::VIDEO),
                    crate::document::LayerKind::Shading => format!("{} Shading", icons::SHADING),
                    crate::document::LayerKind::Flowchart => format!("{} Flowchart", icons::FLOWCHART),
                    crate::document::LayerKind::NodeEditor => {
                        format!("{} Node Editor", icons::NODE_EDITOR)
                    }
                };
                egui::ComboBox::from_id_salt("layer_kind_combo")
                    .selected_text(RichText::new(current_label).font(nerd_font_id(12.0)))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut l.kind, crate::document::LayerKind::Image, RichText::new(format!("{} Image", icons::IMAGE)).font(nerd_font_id(12.0)));
                        ui.selectable_value(&mut l.kind, crate::document::LayerKind::AV, RichText::new(format!("{} AV Layer", icons::VIDEO)).font(nerd_font_id(12.0)));
                        ui.selectable_value(&mut l.kind, crate::document::LayerKind::Shading, RichText::new(format!("{} Shading", icons::SHADING)).font(nerd_font_id(12.0)));
                        ui.selectable_value(&mut l.kind, crate::document::LayerKind::Flowchart, RichText::new(format!("{} Flowchart", icons::FLOWCHART)).font(nerd_font_id(12.0)));
                        ui.selectable_value(
                            &mut l.kind,
                            crate::document::LayerKind::NodeEditor,
                            RichText::new(format!("{} Node Editor", icons::NODE_EDITOR))
                                .font(nerd_font_id(12.0)),
                        );
                    });
                if l.kind == crate::document::LayerKind::NodeEditor {
                    l.ensure_node_graph();
                }
            });

            if l.kind == crate::document::LayerKind::Shading {
                if l.shading_passes.is_empty() {
                    l.shading_passes
                        .push(crate::document::ShadingPass::vignette_preset());
                }

                // One pass per layer. Prefer the last entry so MCP/custom sources
                // that were previously appended after a default vignette survive.
                if l.shading_passes.len() > 1 {
                    let keep = l.shading_passes.pop().unwrap();
                    l.shading_passes.clear();
                    l.shading_passes.push(keep);
                }

                let pass = &mut l.shading_passes[0];

                let mut current_preset_name = match pass.name.as_str() {
                    "Vignette" => "Vignette",
                    "CRT" => "CRT",
                    "Blackhole" => "Blackhole",
                    "Starfield" => "Starfield",
                    _ => "Custom",
                };

                let preset_options = ["Vignette", "CRT", "Blackhole", "Starfield", "Custom"];
                let mut new_preset = None;

                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("shading_preset_combo")
                        .selected_text(current_preset_name)
                        .width(ui.available_width().min(200.0).max(100.0))
                        .show_ui(ui, |ui| {
                            for opt in &preset_options {
                                if ui.selectable_value(&mut current_preset_name, *opt, *opt).clicked() {
                                    new_preset = Some(*opt);
                                }
                            }
                        });
                });

                if let Some(opt) = new_preset {
                    match opt {
                        "Vignette" => {
                            *pass = crate::document::ShadingPass::vignette_preset();
                        }
                        "CRT" => {
                            *pass = crate::document::ShadingPass::crt_preset();
                        }
                        "Blackhole" => {
                            *pass = crate::document::ShadingPass::blackhole_preset();
                        }
                        "Starfield" => {
                            *pass = crate::document::ShadingPass::starfield_preset();
                        }
                        _ => {
                            *pass = crate::document::ShadingPass::custom_template();
                        }
                    }
                }

                ui.horizontal(|ui| {
                    ui.checkbox(&mut pass.enabled, "Enabled");
                });

                ui.horizontal(|ui| {
                    ui.label("Reload mode:");
                    let before_hot = pass.hot_reload;
                    ui.radio_value(&mut pass.hot_reload, true, "Hot");
                    ui.radio_value(&mut pass.hot_reload, false, "Press");
                    if pass.hot_reload && !before_hot {
                        pass.compiled_wgsl = Some(pass.wgsl.clone());
                        if let Ok(mut err_lock) = pass.compile_error.lock() {
                            *err_lock = None;
                        }
                    }
                });

                shading_wgsl_file_buttons(ui, pass);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("WGSL source").small().weak());
                    if ui.button(RichText::new(format!("{} Edit in window", icons::EDIT)).font(nerd_font_id(12.0))).clicked() {
                        app.show_shader_editor_window = Some(l.id);
                    }
                });
                let mut text_edit_response = None;
                egui::ScrollArea::vertical()
                    .id_salt("sidebar_shader_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut pass.wgsl)
                                .id(egui::Id::new("sidebar_shader_editor_text"))
                                .desired_width(f32::INFINITY)
                                .desired_rows(8)
                                .font(egui::TextStyle::Monospace),
                        );
                        text_edit_response = Some(resp);
                    });

                if let Some(resp) = text_edit_response {
                    if resp.changed() {
                        if matches!(
                            pass.name.as_str(),
                            "Vignette" | "CRT" | "Blackhole" | "Starfield"
                        ) {
                            pass.name = "Custom".to_string();
                        }
                        if pass.hot_reload {
                            pass.compiled_wgsl = Some(pass.wgsl.clone());
                            if let Ok(mut err_lock) = pass.compile_error.lock() {
                                *err_lock = None;
                            }
                        }
                    }
                }

                if !pass.hot_reload {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Compile / Reload").clicked() {
                            pass.compiled_wgsl = Some(pass.wgsl.clone());
                            if let Ok(mut err_lock) = pass.compile_error.lock() {
                                *err_lock = None;
                            }
                        }
                    });
                }

                if let Ok(err_lock) = pass.compile_error.lock() {
                    if let Some(ref err) = *err_lock {
                        ui.add_space(4.0);
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), err);
                    }
                }
            }

            if l.kind == crate::document::LayerKind::AV {
                let role = l.av_role;
                ui.label(
                    RichText::new(match role {
                        crate::document::AvRole::Video => "Video track queue",
                        crate::document::AvRole::Audio => "Audio track queue",
                        crate::document::AvRole::Daw => "DAW track queue",
                    })
                    .small()
                    .weak(),
                );
                ui.add_space(4.0);
                // Capture role before nested mut borrows end; actions after block.
                let _ = role;
            }
        });
    }

    // AV "Add track" actions (outside mut borrow of layer).
    if let Some(l) = app.project.document.layers.get(active_idx) {
        if l.kind == crate::document::LayerKind::AV {
            let role = l.av_role;
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Add track").strong());
                ui.add_space(4.0);
                match role {
                    crate::document::AvRole::Video => {
                        #[cfg(not(target_os = "android"))]
                        {
                            if ui
                                .button(format!("{} From path… (video/image)", icons::VIDEO))
                                .on_hover_text(
                                    "Queue a video or image file on this Video layer only",
                                )
                                .clicked()
                            {
                                let dlg = rfd::FileDialog::new().add_filter(
                                    "Video / Image",
                                    &[
                                        "mp4", "mkv", "avi", "mov", "webm", "m4v", "png", "jpg",
                                        "jpeg", "webp", "gif", "bmp",
                                    ],
                                );
                                if let Some(path) = dlg.pick_file() {
                                    let name = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned();
                                    match app.push_media_clip(
                                        &name,
                                        path.to_string_lossy().into_owned(),
                                        true,
                                    ) {
                                        Ok(m) => app.status_message = m,
                                        Err(e) => app.status_message = e,
                                    }
                                }
                            }
                        }
                        if ui
                            .button(format!("{} From object…", icons::IMAGE))
                            .on_hover_text(
                                "Rasterize selection / Image object onto this Video layer",
                            )
                            .clicked()
                        {
                            match app.push_selection_as_av_image_clip() {
                                Ok(m) => app.status_message = m,
                                Err(e) => app.status_message = e,
                            }
                        }
                    }
                    crate::document::AvRole::Audio => {
                        #[cfg(not(target_os = "android"))]
                        {
                            if ui
                                .button(format!("{} From path… (audio)", icons::AUDIO))
                                .on_hover_text("Queue an audio file on this Audio layer only")
                                .clicked()
                            {
                                let dlg = rfd::FileDialog::new().add_filter(
                                    "Audio",
                                    &["mp3", "wav", "aac", "m4a", "flac", "ogg", "opus"],
                                );
                                if let Some(path) = dlg.pick_file() {
                                    let name = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned();
                                    match app.push_media_clip(
                                        &name,
                                        path.to_string_lossy().into_owned(),
                                        true,
                                    ) {
                                        Ok(m) => app.status_message = m,
                                        Err(e) => app.status_message = e,
                                    }
                                }
                            }
                        }
                    }
                    crate::document::AvRole::Daw => {
                        if ui
                            .button(format!("{} DAW node (1s)", icons::MUSIC))
                            .on_hover_text("Create a 1s DAW clip on this layer")
                            .clicked()
                        {
                            app.create_daw_clip_at_playhead();
                        }
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Volume:");
                    if let Some(layer) = app.project.document.layers.get_mut(active_idx) {
                        ui.add(egui::Slider::new(&mut layer.volume, 0.0..=1.0));
                    }
                })
                .response
                .on_hover_text("Layer mix volume (also in Active Track Details)");
            });
        }
    }

    // Selected Clip Properties Panel
    let mut selected_clip_info = None;
    for (layer_idx, layer) in app.project.document.layers.iter().enumerate() {
        if layer.kind == crate::document::LayerKind::AV {
            let mut l_clips = layer.clone();
            l_clips.ensure_av_clips();
            if let Some(clip) = l_clips.av_clips.iter().find(|c| app.selection.contains(&c.id)) {
                selected_clip_info = Some((layer_idx, clip.id));
                break;
            }
        }
    }
    let mut sync_needed = false;
    if let Some((layer_idx, clip_id)) = selected_clip_info {
        if let Some(l) = app.project.document.layers.get_mut(layer_idx) {
            l.ensure_av_clips();
            if let Some(clip) = l.av_clips.iter_mut().find(|c| c.id == clip_id) {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                theme::constraint_block(ui, |ui| {
                    ui.label(RichText::new("Clip Properties").strong());
                    ui.add_space(4.0);

                    let mut changed = false;
                    let mut path_changed = false;

                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        if ui.text_edit_singleline(&mut clip.name).changed() {
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Path:");
                        #[cfg(not(target_os = "android"))]
                        if ui.button("Browse...").clicked() {
                            let dlg = rfd::FileDialog::new()
                                .add_filter("Media (AV)", &["mp4", "mkv", "avi", "mov", "webm", "mp3", "wav", "aac", "m4a", "flac", "ogg"]);
                            if let Some(path) = dlg.pick_file() {
                                let clean_name = std::path::Path::new(&path)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("");
                                clip.name = clean_name.to_string();
                                clip.media_path = path.to_string_lossy().into_owned();
                                path_changed = true;
                                changed = true;
                            }
                        }
                    });
                    let path_w = ui.available_width().min(220.0).max(80.0);
                    let path_resp = ui.add(
                        egui::TextEdit::singleline(&mut clip.media_path)
                            .desired_width(path_w)
                            .hint_text("media file path"),
                    );
                    if path_resp.changed() {
                        changed = true;
                    }
                    if path_resp.lost_focus() {
                        path_changed = true;
                    }

                    ui.horizontal(|ui| {
                        ui.label("Start (Timeline):");
                        if ui.add(egui::DragValue::new(&mut clip.video_timeline_start).speed(0.1).suffix("s")).changed() {
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Start Offset (Source):");
                        if ui.add(egui::DragValue::new(&mut clip.video_start_offset).speed(0.1).suffix("s")).changed() {
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Play Length:");
                        if ui.add(egui::DragValue::new(&mut clip.video_play_length).speed(0.1).suffix("s")).changed() {
                            changed = true;
                        }
                    });

                    if path_changed && !clip.media_path.is_empty() {
                        if let Some(dur) = crate::video_decode::probe_media_duration_secs(&clip.media_path) {
                            clip.media_source_duration = Some(dur);
                            clip.video_play_length = dur;
                        }
                    }

                    if changed {
                        sync_needed = true;
                    }
                });
            }
            if sync_needed {
                // Only refresh layer fields from the clip we edited.
                l.sync_legacy_from_clip_id(clip_id);
            }
        }
    }

    if let Some(idx) = probe_media_at {
        if let Some(path) = app.project.document.layers.get(idx).map(|l| l.video_path.clone()) {
            if !path.is_empty() {
                app.apply_media_duration_from_path(idx, &path);
            }
        }
    }
}


/// Snapshot row for a graph node in the Objects tab (P6a).
struct NeGraphRow {
    id: uuid::Uuid,
    name: String,
    kind_title: String,
    category: String,
    is_output: bool,
    /// Short status: path basename, param, or empty.
    detail: String,
    has_image: bool,
    has_sound: bool,
    icon: &'static str,
}

fn objects_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(format!("{} selected", app.selection.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("▲")
                .on_hover_text("Raise — move vs video/audio layers or within layer")
                .clicked()
            {
                app.nudge_z_order(1);
            }
            if ui
                .small_button("▼")
                .on_hover_text("Lower — move vs video/audio layers or within layer")
                .clicked()
            {
                app.nudge_z_order(-1);
            }
            if ui.small_button("⧉").on_hover_text("Duplicate").clicked() {
                app.duplicate_selection();
            }
        });
    });
    // Snapshot only cheap fields — never clone full Text content via nodes for labels.
    // NE graph rows: (node_id, name, kind title, category, is_output, detail, has_img, has_snd)
    // + output eval summary for the layer.
    let layers_meta: Vec<_> = app
        .project
        .document
        .layers
        .iter()
        .map(|l| {
            let mut ne_nodes: Vec<NeGraphRow> = Vec::new();
            let mut out_img = String::new();
            let mut out_snd = String::new();
            if l.kind == crate::document::LayerKind::NodeEditor {
                if let Some(g) = l.node_graph.as_ref() {
                    let eval = g.resolve_output_image();
                    out_img = match &eval.image {
                        crate::document::GraphImageSource::Empty => "image: —".into(),
                        crate::document::GraphImageSource::FilePath(p) => {
                            let name = std::path::Path::new(p)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file");
                            format!("image: {name}")
                        }
                        crate::document::GraphImageSource::AppObjects(ids) => {
                            format!("image: {} app obj", ids.len())
                        }
                    };
                    if eval.blur_px > 0.01 {
                        out_img = format!("{out_img} · blur {:.1}", eval.blur_px);
                    }
                    let snd = g.resolve_output_sound();
                    out_snd = match snd.path() {
                        Some(p) => {
                            let name = std::path::Path::new(p)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("audio");
                            format!("sound: {name}")
                        }
                        None => "sound: —".into(),
                    };
                    // Objects tab: only Output Object(s) — internal graph nodes stay in NE.
                    for n in g.nodes.values() {
                        if !matches!(n.kind, crate::document::GraphNodeKind::OutputObject) {
                            continue;
                        }
                        let img_ok = !matches!(
                            eval.image,
                            crate::document::GraphImageSource::Empty
                        );
                        let snd_ok = snd.path().is_some();
                        // Keep detail short — long "image: …" paths overflow the Objects panel.
                        let detail = String::new();
                        ne_nodes.push(NeGraphRow {
                            id: n.id,
                            name: safe_trunc_label(&n.name, 24),
                            kind_title: "Output Object".into(),
                            category: "Object".into(),
                            is_output: true,
                            detail,
                            has_image: img_ok,
                            has_sound: snd_ok,
                            icon: icons::NODE_EDITOR,
                        });
                    }
                }
            }
            (
                l.id,
                safe_trunc_label(&l.name, 32),
                l.kind,
                l.av_role,
                l.nodes.clone(),
                l.av_clips
                    .iter()
                    .map(|c| (c.id, safe_trunc_label(&c.name, 32), c.is_audio_only()))
                    .collect::<Vec<_>>(),
                l.music_clips
                    .iter()
                    .map(|c| (c.id, safe_trunc_label(&c.name, 32)))
                    .collect::<Vec<_>>(),
                ne_nodes,
                out_img,
                out_snd,
            )
        })
        .collect();

    // List all layers and their objects in rendering order (top-most first)
    for (
        layer_id,
        layer_name,
        layer_kind,
        av_role,
        layer_nodes,
        av_clips,
        music_clips,
        ne_nodes,
        out_img,
        out_snd,
    ) in layers_meta.into_iter().rev()
    {
        match layer_kind {
            crate::document::LayerKind::Shading => {
                ui.label(RichText::new(format!("{} Shading passes (WGSL)", icons::SHADING)).font(nerd_font_id(12.0)));
            }
            crate::document::LayerKind::Flowchart => {
                ui.label(RichText::new(format!("{} Flowchart", icons::FLOWCHART)).font(nerd_font_id(12.0)));
            }
            crate::document::LayerKind::NodeEditor => {
                // P6a: layer + Output Object only (not internal graph nodes).
                let layer_sel = app.selection.contains(&layer_id)
                    || app.node_editor_ui.open_layer_id == Some(layer_id);
                let header = format!("{} {}", icons::NODE_EDITOR, layer_name);
                ui.horizontal(|ui| {
                    let resp = ui.selectable_label(
                        layer_sel,
                        RichText::new(header).font(nerd_font_id(12.0)),
                    );
                    if resp.clicked() {
                        app.selection = vec![layer_id];
                        if let Some(idx) = app
                            .project
                            .document
                            .layers
                            .iter()
                            .position(|l| l.id == layer_id)
                        {
                            app.set_active_layer(idx);
                        }
                    }
                    if resp.double_clicked() {
                        app.selection = vec![layer_id];
                        if let Some(idx) = app
                            .project
                            .document
                            .layers
                            .iter()
                            .position(|l| l.id == layer_id)
                        {
                            app.set_active_layer(idx);
                        }
                        app.node_editor_ui.open(layer_id);
                        promote_action_tab(app, ActionTab::Parameter);
                    }
                    resp.on_hover_text(
                        "Click: select layer · Double-click: open Node Editor",
                    );
                    if ui
                        .small_button(if app.node_editor_ui.open_layer_id == Some(layer_id) {
                            "Hide"
                        } else {
                            "Open"
                        })
                        .on_hover_text("Toggle Node Editor window")
                        .clicked()
                    {
                        if app.node_editor_ui.open_layer_id == Some(layer_id) {
                            app.node_editor_ui.close();
                        } else {
                            if let Some(idx) = app
                                .project
                                .document
                                .layers
                                .iter()
                                .position(|l| l.id == layer_id)
                            {
                                app.set_active_layer(idx);
                            }
                            app.selection = vec![layer_id];
                            app.node_editor_ui.open(layer_id);
                            promote_action_tab(app, ActionTab::Parameter);
                        }
                    }
                });
                if ne_nodes.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new("no Output Object").small().weak());
                    });
                }
                for row in ne_nodes {
                    // P6b: canvas selection targets the Output proxy Image when present.
                    let proxy_id = app
                        .project
                        .document
                        .layers
                        .iter()
                        .find(|l| l.id == layer_id)
                        .and_then(|l| l.ne_output_proxy);
                    let g_sel = proxy_id
                        .map(|pid| app.selection.contains(&pid))
                        .unwrap_or_else(|| app.selection.contains(&layer_id))
                        || (app.node_editor_ui.selected == Some(row.id)
                            && app.node_editor_ui.open_layer_id == Some(layer_id));
                    let label_txt = format!("{} Output Object", row.icon);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let resp = ui.selectable_label(
                            g_sel,
                            RichText::new(label_txt).font(nerd_font_id(13.0)),
                        );
                        if resp.clicked() {
                            if let Some(idx) = app
                                .project
                                .document
                                .layers
                                .iter()
                                .position(|l| l.id == layer_id)
                            {
                                app.set_active_layer(idx);
                            }
                            // Ensure proxy exists, then select it for canvas transform tools.
                            if let Some(layer) = app
                                .project
                                .document
                                .layers
                                .iter_mut()
                                .find(|l| l.id == layer_id)
                            {
                                let _ = layer.ensure_ne_output_proxy(&mut app.project.nodes);
                            }
                            let pid = app
                                .project
                                .document
                                .layers
                                .iter()
                                .find(|l| l.id == layer_id)
                                .and_then(|l| l.ne_output_proxy);
                            app.selection = vec![pid.unwrap_or(layer_id)];
                            if app.node_editor_ui.open_layer_id != Some(layer_id) {
                                app.node_editor_ui.open(layer_id);
                            }
                            app.node_editor_ui.selected = Some(row.id);
                            app.node_editor_ui.selected_link = None;
                            promote_action_tab(app, ActionTab::Parameter);
                        }
                        if resp.double_clicked() {
                            if let Some(idx) = app
                                .project
                                .document
                                .layers
                                .iter()
                                .position(|l| l.id == layer_id)
                            {
                                app.set_active_layer(idx);
                            }
                            app.node_editor_ui.open(layer_id);
                            app.node_editor_ui.selected = Some(row.id);
                        }
                        // Hover shows short image/sound summary (not inline — avoids panel overflow).
                        let hover = {
                            let img = safe_trunc_label(&out_img, 48);
                            let snd = safe_trunc_label(&out_snd, 40);
                            format!(
                                "Output Object\n{img}\n{snd}\nClick: select · Double-click: open NE"
                            )
                        };
                        resp.on_hover_text(hover);
                    });
                }
            }
            crate::document::LayerKind::AV => {
                let icon = match av_role {
                    crate::document::AvRole::Audio => icons::AUDIO,
                    crate::document::AvRole::Daw => icons::MUSIC,
                    crate::document::AvRole::Video => icons::VIDEO,
                };
                ui.label(
                    RichText::new(format!("{} {}", icon, layer_name))
                        .small()
                        .weak(),
                );
                for (clip_id, display_name, audio_only) in av_clips.into_iter().rev() {
                    let selected = app.selection.contains(&clip_id);
                    let cicon = if audio_only { icons::AUDIO } else { icons::VIDEO };
                    let label =
                        RichText::new(format!("{} {}", cicon, display_name)).font(nerd_font_id(13.0));
                    ui.horizontal(|ui| {
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            app.set_selection(vec![clip_id]);
                        }
                        resp.on_hover_text(&display_name);
                    });
                }
                for (mclip_id, display_name) in music_clips.into_iter().rev() {
                    let selected = app.selection.contains(&mclip_id);
                    let label = RichText::new(format!("{} {}", icons::MUSIC, display_name))
                        .font(nerd_font_id(13.0));
                    ui.horizontal(|ui| {
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            app.set_selection(vec![mclip_id]);
                        }
                        if resp.double_clicked() {
                            app.set_selection(vec![mclip_id]);
                            app.piano_roll_clip = Some(mclip_id);
                        }
                        resp.on_hover_text(format!("{display_name}\nDouble-click to open DAW piano"));
                    });
                }
            }
            crate::document::LayerKind::Image => {
                for id in layer_nodes.iter().rev() {
                        let Some(node) = app.project.nodes.get(*id) else {
                            continue;
                        };
                        let selected = app.selection.contains(id);
                        let icon = node_icon(&node.kind);
                        // Never materialize full multi-MB name/content for the list row.
                        let display_name = match &node.kind {
                            crate::document::NodeKind::Text { style, .. } => {
                                // Prefer short content preview; name is often empty/generic.
                                let preview = crate::document::text_display_name(&style.content);
                                if node.name.is_empty() || node.name == "Text" {
                                    preview
                                } else {
                                    safe_trunc_label(&node.name, 18)
                                }
                            }
                            _ => safe_trunc_label(&node.name, 18),
                        };
                        let rename_draft = safe_trunc_label(&node.name, 256);
                        let id_copy = *id;
                        let label = RichText::new(format!("{icon} {}", display_name)).font(nerd_font_id(13.0));
                        ui.horizontal(|ui| {
                            let resp = ui.selectable_label(selected, label);
                            if resp.clicked() {
                                app.set_selection(vec![id_copy]);
                            }
                            if resp.double_clicked() {
                                app.set_selection(vec![id_copy]);
                                app.object_rename_dialog =
                                    Some((id_copy, rename_draft.clone(), false));
                            }
                            resp.on_hover_text(format!("{display_name}\nDouble-click to rename"));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let delete_btn = ui.add(
                                    egui::Button::new(
                                        RichText::new("✖")
                                            .color(egui::Color32::from_rgb(255, 23, 68))
                                            .strong()
                                            .size(11.0)
                                    )
                                    .frame(false)
                                );
                                if delete_btn.clicked() {
                                    app.delete_nodes(&[id_copy]);
                                }
                                delete_btn.on_hover_text("Delete object");
                            });
                        });
                    }
            }
        }
    }
}

/// P7e: Color/Stroke panel content when the Output Object proxy Image is selected.
fn ne_output_proxy_inspector(
    app: &mut VadadeeBerryApp,
    ui: &mut Ui,
    layer_idx: usize,
    proxy_id: uuid::Uuid,
) {
    let (layer_name, layer_id, img_line, snd_line, output_graph_id, (px, py, pw, ph, rot_deg)) = {
        let Some(layer) = app.project.document.layers.get(layer_idx) else {
            return;
        };
        let g = layer.node_graph.as_ref();
        let eval = g.map(|g| g.resolve_output_image());
        let img_line = match eval.as_ref().map(|e| &e.image) {
            Some(crate::document::GraphImageSource::FilePath(p)) => {
                let name = std::path::Path::new(p)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file");
                format!("Image: {name}")
            }
            Some(crate::document::GraphImageSource::AppObjects(ids)) => {
                format!("Image: {} app object(s)", ids.len())
            }
            Some(crate::document::GraphImageSource::Empty) | None => "Image: —".into(),
        };
        let snd = g.map(|g| g.resolve_output_sound());
        let snd_line = match snd.as_ref().and_then(|s| s.path()) {
            Some(p) => {
                let name = std::path::Path::new(p)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("audio");
                format!("Sound: {name}")
            }
            None => "Sound: —".into(),
        };
        let output_graph_id = g.and_then(|g| {
            g.nodes
                .values()
                .find(|n| matches!(n.kind, crate::document::GraphNodeKind::OutputObject))
                .map(|n| n.id)
        });
        let geom = if let Some(n) = app.project.nodes.get(proxy_id) {
            if let NodeKind::Image {
                x,
                y,
                width,
                height,
                ..
            } = &n.kind
            {
                (
                    *x,
                    *y,
                    *width,
                    *height,
                    n.get_rotation().to_degrees(),
                )
            } else {
                (0.0, 0.0, 0.0, 0.0, 0.0)
            }
        } else {
            (0.0, 0.0, 0.0, 0.0, 0.0)
        };
        (
            layer.name.clone(),
            layer.id,
            img_line,
            snd_line,
            output_graph_id,
            geom,
        )
    };

    ui.label(
        RichText::new(format!("{} Output Object", icons::NODE_EDITOR))
            .strong()
            .color(colors::ACCENT)
            .font(nerd_font_id(14.0)),
    );
    ui.label(
        RichText::new(format!("Layer: {layer_name}"))
            .small()
            .color(colors::TEXT_MUTED),
    );
    ui.add_space(6.0);
    ui.label(RichText::new(&img_line).color(colors::TEXT));
    ui.label(RichText::new(&snd_line).color(colors::TEXT));
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);
    ui.label(RichText::new("Canvas transform").strong());
    ui.label(
        RichText::new(format!(
            "x {:.0}  y {:.0}  ·  {:.0}×{:.0}  ·  rot {:.1}°",
            px, py, pw, ph, rot_deg
        ))
        .small()
        .color(colors::TEXT_MUTED),
    );
    ui.label(
        RichText::new("Move / scale / rotate with Select on the canvas.")
            .small()
            .color(colors::TEXT_MUTED),
    );
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        if ui
            .button("Open Node Editor")
            .on_hover_text("Edit the graph that drives this Output")
            .clicked()
        {
            if let Some(idx) = app
                .project
                .document
                .layers
                .iter()
                .position(|l| l.id == layer_id)
            {
                app.set_active_layer(idx);
            }
            app.node_editor_ui.open(layer_id);
            if let Some(oid) = output_graph_id {
                app.node_editor_ui.selected = Some(oid);
                app.node_editor_ui.selected_link = None;
            }
            promote_action_tab(app, ActionTab::Parameter);
        }
        if ui
            .small_button("Select layer")
            .on_hover_text("Select the Node Editor layer")
            .clicked()
        {
            app.selection = vec![layer_id];
            if let Some(idx) = app
                .project
                .document
                .layers
                .iter()
                .position(|l| l.id == layer_id)
            {
                app.set_active_layer(idx);
            }
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new("Graph parameters: Parameter tab · Keyframes: Animation tab")
            .small()
            .color(colors::TEXT_MUTED),
    );
}

fn appearance_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    // P7e: NE Output proxy — dedicated inspector (not fill/stroke of empty Image).
    if app.selection.len() == 1 {
        let id = app.selection[0];
        if let Some(layer_idx) = app.project.document.ne_output_proxy_layer_index(id) {
            ne_output_proxy_inspector(app, ui, layer_idx, id);
            return;
        }
    }
    if app.selection.len() == 1 {
        let id = app.selection[0];
        if let Some(pos) = app.project.document.layers.iter().position(|l| l.id == id) {
            let kind = app.project.document.layers[pos].kind;
            if kind == crate::document::LayerKind::AV {
                // AV media settings (video color + audio) are in Layer tab or Video Editor
                // fallthrough to show video color controls if applicable
            }
            if kind == crate::document::LayerKind::AV {
                let layer = &mut app.project.document.layers[pos];
                theme::constraint_block(ui, |ui| {
                    ui.label(RichText::new("🎥 Color").strong().color(colors::ACCENT));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Hue:");
                        ui.add(egui::Slider::new(&mut layer.hue, -180.0..=180.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Saturation:");
                        ui.add(egui::Slider::new(&mut layer.saturation, 0.0..=2.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(egui::Slider::new(&mut layer.brightness, 0.0..=2.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Contrast:");
                        ui.add(egui::Slider::new(&mut layer.contrast, 0.0..=2.0));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut layer.name);
                });
                return;
            }
        }
    }

    if app.tools.active == ToolKind::Brush {
        theme::constraint_block(ui, |ui| {
            ui.label(RichText::new("Brush Fill").strong());
            ui.label("Type");
            let mut changed = paint_kind_selector(ui, &mut app.tools.brush.fill_kind);
            if app.tools.brush.fill_kind == FillKind::Solid {
                changed |= solid_color_editor(ui, &mut app.tools.brush.fill_stops);
            } else {
                let strip = gradient_strip_editor(
                    ui,
                    ui.id().with("brush_gradient_strip"),
                    GradientEditorFocus::Fill,
                    &mut app.tools.brush.fill_stops,
                    &mut app.tools.brush.fill_stop_sel,
                );
                changed |= strip.changed;
                if strip.focus == GradientEditorFocus::Fill {
                    app.gradient_editor_focus = GradientEditorFocus::Fill;
                }
                if app.tools.brush.fill_kind == FillKind::LinearGradient {
                    if linear_gradient_angle_dial(
                        ui,
                        ui.id().with("brush_angle_dial"),
                        &mut app.tools.brush.gradient_angle,
                    ) {
                        let mut line = (
                            app.tools.brush.fill_line_x0,
                            app.tools.brush.fill_line_y0,
                            app.tools.brush.fill_line_x1,
                            app.tools.brush.fill_line_y1,
                        );
                        apply_angle_to_flow_line(app.tools.brush.gradient_angle, &mut line);
                        app.tools.brush.fill_line_x0 = line.0;
                        app.tools.brush.fill_line_y0 = line.1;
                        app.tools.brush.fill_line_x1 = line.2;
                        app.tools.brush.fill_line_y1 = line.3;
                        changed = true;
                    }
                }
                if matches!(
                    app.tools.brush.fill_kind,
                    FillKind::LinearGradient | FillKind::RadialGradient
                ) {
                    if ui
                        .checkbox(&mut app.tools.brush.fill_edit_gradient_line, "Edit gradient line")
                        .changed()
                    {
                        changed = true;
                    }
                    if app.tools.brush.fill_edit_gradient_line {
                        if gradient_flow_line_editor(
                            ui,
                            ui.id().with("brush_flow_line"),
                            app.tools.brush.fill_kind,
                            &mut app.tools.brush.fill_line_x0,
                            &mut app.tools.brush.fill_line_y0,
                            &mut app.tools.brush.fill_line_x1,
                            &mut app.tools.brush.fill_line_y1,
                            &mut app.tools.brush.radial_cx,
                            &mut app.tools.brush.radial_cy,
                            1.0,
                            &app.tools.brush.fill_stops,
                        ) {
                            if app.tools.brush.fill_kind == FillKind::LinearGradient {
                                app.tools.brush.gradient_angle = sync_angle_from_flow_line((
                                    app.tools.brush.fill_line_x0,
                                    app.tools.brush.fill_line_y0,
                                    app.tools.brush.fill_line_x1,
                                    app.tools.brush.fill_line_y1,
                                ));
                            }
                            changed = true;
                        }
                    }
                }
            }
        });
        return;
    }
    if app.selection.is_empty() {
        ui.label(RichText::new("Select an object").color(colors::TEXT_MUTED));
        return;
    }
    if app.tools.active == ToolKind::Node {
        ui.label(
            RichText::new("Edit points mode — fill applies to closed paths only")
                .small()
                .color(colors::ACCENT),
        );
        ui.separator();
    }
    if app.selection.len() == 1 {
        if let Some(id) = app.selection.first() {
            if let Some(NodeKind::Path { path }) = app.project.nodes.get(*id).map(|n| &n.kind) {
                let mut closed = path.is_closed();
                if ui
                    .checkbox(&mut closed, "Closed path (cyclic)")
                    .on_hover_text("Fill color applies only when the path is closed")
                    .changed()
                {
                    app.set_path_closed(*id, closed);
                }
                if !closed {
                    ui.label(
                        RichText::new("Open paths use stroke only; enable closed for fill")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                }
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label("Opacity");
        let mut op = app.inspector_opacity();
        if ui
            .add(egui::Slider::new(&mut op, 0.0..=1.0).show_value(true))
            .changed()
        {
            app.set_selection_opacity(op);
        }
    });

    // ── Blend Mode ────────────────────────────────────────────────────
    {
        let current_blend = app.selection.first()
            .and_then(|&id| app.project.nodes.get(id))
            .map(|n| n.style.blend_mode)
            .unwrap_or_default();
        let mut selected = current_blend;
        ui.horizontal(|ui| {
            ui.label("Blend Mode");
            egui::ComboBox::from_id_salt("blend_mode_combo")
                .selected_text(selected.label())
                .width(140.0)
                .show_ui(ui, |ui| {
                    for &mode in crate::document::BlendMode::all() {
                        ui.selectable_value(&mut selected, mode, mode.label());
                    }
                });
        });
        if selected != current_blend {
            for &id in &app.selection.clone() {
                let Some(before) = app.project.nodes.get(id).cloned() else {
                    continue;
                };
                let mut after = before.clone();
                after.style.blend_mode = selected;
                app.history.push(
                    &mut app.project,
                    crate::history::ProjectEdit::PatchNode {
                        id,
                        before,
                        after,
                    },
                );
            }
        }
    }

    theme::constraint_block(ui, |ui| {
        ui.label(RichText::new("Fill").strong());
        let mut fill_changed = false;
        fill_changed |= ui.checkbox(&mut app.fill_enabled, "Enabled").changed();
        ui.label("Type");
        fill_changed |= paint_kind_selector(ui, &mut app.ui_fill_kind);
        if app.fill_enabled {
            if app.ui_fill_kind == FillKind::Solid {
                fill_changed |= solid_color_editor(ui, &mut app.ui_fill_stops);
            } else {
                let strip = gradient_strip_editor(
                    ui,
                    ui.id().with("fill_gradient_strip"),
                    GradientEditorFocus::Fill,
                    &mut app.ui_fill_stops,
                    &mut app.ui_fill_stop_sel,
                );
                fill_changed |= strip.changed;
                if strip.focus == GradientEditorFocus::Fill {
                    app.gradient_editor_focus = GradientEditorFocus::Fill;
                }
                if app.ui_fill_kind == FillKind::LinearGradient {
                    if linear_gradient_angle_dial(
                        ui,
                        ui.id().with("fill_angle_dial"),
                        &mut app.ui_gradient_angle,
                    ) {
                        let mut line = (
                            app.ui_fill_line_x0,
                            app.ui_fill_line_y0,
                            app.ui_fill_line_x1,
                            app.ui_fill_line_y1,
                        );
                        apply_angle_to_flow_line(app.ui_gradient_angle, &mut line);
                        app.ui_fill_line_x0 = line.0;
                        app.ui_fill_line_y0 = line.1;
                        app.ui_fill_line_x1 = line.2;
                        app.ui_fill_line_y1 = line.3;
                        fill_changed = true;
                    }
                }
                if matches!(
                    app.ui_fill_kind,
                    FillKind::LinearGradient | FillKind::RadialGradient
                ) {
                    if ui
                        .checkbox(&mut app.ui_fill_edit_gradient_line, "Edit gradient line")
                        .changed()
                    {
                        fill_changed = true;
                    }
                    if app.ui_fill_edit_gradient_line {
                        let aspect = app
                            .selection
                            .first()
                            .and_then(|id| app.project.nodes.get(*id))
                            .map(|n| {
                                let b = n.bounds();
                                let w = (b.x1 - b.x0).max(1.0);
                                let h = (b.y1 - b.y0).max(1.0);
                                (w / h) as f32
                            })
                            .unwrap_or(1.0);
                        if gradient_flow_line_editor(
                            ui,
                            ui.id().with("fill_flow_line"),
                            app.ui_fill_kind,
                            &mut app.ui_fill_line_x0,
                            &mut app.ui_fill_line_y0,
                            &mut app.ui_fill_line_x1,
                            &mut app.ui_fill_line_y1,
                            &mut app.ui_radial_cx,
                            &mut app.ui_radial_cy,
                            aspect,
                            &app.ui_fill_stops,
                        ) {
                            if app.ui_fill_kind == FillKind::LinearGradient {
                                app.ui_gradient_angle = sync_angle_from_flow_line((
                                    app.ui_fill_line_x0,
                                    app.ui_fill_line_y0,
                                    app.ui_fill_line_x1,
                                    app.ui_fill_line_y1,
                                ));
                            }
                            fill_changed = true;
                        }
                    }
                }
            }
        }
        if fill_changed {
            app.apply_fill_to_selection();
        }
    });
    theme::constraint_block(ui, |ui| {
        ui.label(RichText::new("Stroke").strong());
        let mut stroke_changed = false;
        if ui.checkbox(&mut app.stroke_enabled, "Enabled").changed() {
            stroke_changed = true;
            if !app.stroke_enabled {
                app.apply_no_stroke_to_selection();
            }
        }
        if app.stroke_enabled {
            ui.label("Type");
            stroke_changed |= paint_kind_selector(ui, &mut app.ui_stroke_kind);
            if app.ui_stroke_kind == FillKind::Solid {
                stroke_changed |= solid_color_editor(ui, &mut app.ui_stroke_stops);
            } else {
                let strip = gradient_strip_editor(
                    ui,
                    ui.id().with("stroke_gradient_strip"),
                    GradientEditorFocus::Stroke,
                    &mut app.ui_stroke_stops,
                    &mut app.ui_stroke_stop_sel,
                );
                stroke_changed |= strip.changed;
                if strip.focus == GradientEditorFocus::Stroke {
                    app.gradient_editor_focus = GradientEditorFocus::Stroke;
                }
                if app.ui_stroke_kind == FillKind::LinearGradient {
                    if linear_gradient_angle_dial(
                        ui,
                        ui.id().with("stroke_angle_dial"),
                        &mut app.ui_stroke_angle,
                    ) {
                        let mut line = (
                            app.ui_stroke_line_x0,
                            app.ui_stroke_line_y0,
                            app.ui_stroke_line_x1,
                            app.ui_stroke_line_y1,
                        );
                        apply_angle_to_flow_line(app.ui_stroke_angle, &mut line);
                        app.ui_stroke_line_x0 = line.0;
                        app.ui_stroke_line_y0 = line.1;
                        app.ui_stroke_line_x1 = line.2;
                        app.ui_stroke_line_y1 = line.3;
                        stroke_changed = true;
                    }
                }
                if matches!(
                    app.ui_stroke_kind,
                    FillKind::LinearGradient | FillKind::RadialGradient
                ) {
                    if ui
                        .checkbox(&mut app.ui_stroke_edit_gradient_line, "Edit gradient line")
                        .changed()
                    {
                        stroke_changed = true;
                    }
                    if app.ui_stroke_edit_gradient_line {
                        let aspect = app
                            .selection
                            .first()
                            .and_then(|id| app.project.nodes.get(*id))
                            .map(|n| {
                                let b = n.bounds();
                                let w = (b.x1 - b.x0).max(1.0);
                                let h = (b.y1 - b.y0).max(1.0);
                                (w / h) as f32
                            })
                            .unwrap_or(1.0);
                        if gradient_flow_line_editor(
                            ui,
                            ui.id().with("stroke_flow_line"),
                            app.ui_stroke_kind,
                            &mut app.ui_stroke_line_x0,
                            &mut app.ui_stroke_line_y0,
                            &mut app.ui_stroke_line_x1,
                            &mut app.ui_stroke_line_y1,
                            &mut app.ui_stroke_radial_cx,
                            &mut app.ui_stroke_radial_cy,
                            aspect,
                            &app.ui_stroke_stops,
                        ) {
                            if app.ui_stroke_kind == FillKind::LinearGradient {
                                app.ui_stroke_angle = sync_angle_from_flow_line((
                                    app.ui_stroke_line_x0,
                                    app.ui_stroke_line_y0,
                                    app.ui_stroke_line_x1,
                                    app.ui_stroke_line_y1,
                                ));
                            }
                            stroke_changed = true;
                        }
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label("Join");
                let nf = nerd_font_id(18.0);
                if ui
                    .selectable_label(
                        app.ui_stroke_line_join == LineJoin::Miter,
                        RichText::new(icons::JOIN_SHARP).font(nf.clone()),
                    )
                    .on_hover_text("Sharp (miter)")
                    .clicked()
                {
                    app.ui_stroke_line_join = LineJoin::Miter;
                    stroke_changed = true;
                }
                if ui
                    .selectable_label(
                        app.ui_stroke_line_join == LineJoin::Round,
                        RichText::new(icons::JOIN_SMOOTH).font(nf),
                    )
                    .on_hover_text("Smooth (round)")
                    .clicked()
                {
                    app.ui_stroke_line_join = LineJoin::Round;
                    stroke_changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("End");
                let nf = nerd_font_id(18.0);
                if ui
                    .selectable_label(
                        app.ui_stroke_line_cap == LineCap::Butt,
                        RichText::new(icons::CAP_BUTT).font(nf.clone()),
                    )
                    .on_hover_text("Flat end")
                    .clicked()
                {
                    app.ui_stroke_line_cap = LineCap::Butt;
                    stroke_changed = true;
                }
                if ui
                    .selectable_label(
                        app.ui_stroke_line_cap == LineCap::Round,
                        RichText::new(icons::CAP_ROUND).font(nf),
                    )
                    .on_hover_text("Round end")
                    .clicked()
                {
                    app.ui_stroke_line_cap = LineCap::Round;
                    stroke_changed = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Order");
                if ui
                    .selectable_label(
                        app.ui_stroke_paint_order == StrokePaintOrder::BehindFill,
                        StrokePaintOrder::BehindFill.label(),
                    )
                    .on_hover_text("Stroke behind fill (inner half covered)")
                    .clicked()
                {
                    app.ui_stroke_paint_order = StrokePaintOrder::BehindFill;
                    stroke_changed = true;
                }
                if ui
                    .selectable_label(
                        app.ui_stroke_paint_order == StrokePaintOrder::AboveFill,
                        StrokePaintOrder::AboveFill.label(),
                    )
                    .on_hover_text("Stroke above fill (full stroke on top)")
                    .clicked()
                {
                    app.ui_stroke_paint_order = StrokePaintOrder::AboveFill;
                    stroke_changed = true;
                }
            });
            stroke_changed |= ui
                .add(egui::Slider::new(&mut app.ui_stroke_width, 0.0..=48.0).text("Width"))
                .changed();
            if stroke_changed {
                app.apply_stroke_to_selection();
            }
        }
    });
    if app.selection.len() == 1 {
        let id = app.selection[0];
        let mut name = app
            .project
            .nodes
            .get(id)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Name");
            if ui.text_edit_singleline(&mut name).changed() {
                app.rename_node(id, name);
            }
        });
    }
}

/// Path marker (arrow/point icon) UI moved to Geometry tab.
/// Includes icon type selector, 2D offset, color, size, common size option, and live previews.
fn path_markers_geometry_ui(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    let is_path = app.selection.len() == 1 && app.project.nodes.get(app.selection[0])
        .map_or(false, |n| matches!(&n.kind, NodeKind::Path { .. }));

    if !is_path {
        return;
    }

    let stroke_width = app.project.nodes.get(app.selection[0])
        .map(|n| n.style.stroke.width.max(1.0))
        .unwrap_or(2.0);

    theme::constraint_block(ui, |ui| {
        ui.label(
            RichText::new("➤ Path Markers (Start / Mid / End)").strong()
        );
        ui.add_space(4.0);

        // Common size option
        let mut common_changed = false;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut app.ui_marker_use_common_size, "Equal size for all").changed() {
                common_changed = true;
                if app.ui_marker_use_common_size {
                    let s = app.ui_marker_common_size;
                    app.ui_marker_start.size = s;
                    app.ui_marker_mid.size = s;
                    app.ui_marker_end.size = s;
                }
            }
            if app.ui_marker_use_common_size {
                let prev = app.ui_marker_common_size;
                if ui.add(egui::Slider::new(&mut app.ui_marker_common_size, 2.0..=60.0).text("Size")).changed() {
                    let s = app.ui_marker_common_size;
                    app.ui_marker_start.size = s;
                    app.ui_marker_mid.size = s;
                    app.ui_marker_end.size = s;
                    common_changed = true;
                }
            }
        });
        if common_changed {
            app.apply_path_markers_to_selection();
        }

        ui.add_space(4.0);

        // Row by row (vertical) for Start, Mid, End to prevent width overflow from long labels + controls on same row
        let mut any_changed = false;
        any_changed |= marker_column(ui, "Start", &mut app.ui_marker_start, stroke_width);
        any_changed |= marker_column(ui, "Mid", &mut app.ui_marker_mid, stroke_width);
        any_changed |= marker_column(ui, "End", &mut app.ui_marker_end, stroke_width);

        if any_changed {
            app.apply_path_markers_to_selection();
        }
        if ui.button("Apply to Path").clicked() {
            app.apply_path_markers_to_selection();
        }
    });
}

fn marker_column(
    ui: &mut Ui,
    label: &str,
    m: &mut crate::document::PathMarker,
    line_width: f32,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.label(RichText::new(label).strong().small());

        // Icon type selector (combo)
        egui::ComboBox::from_id_source(format!("marker_kind_{}", label))
            .selected_text(m.kind.label())
            .show_ui(ui, |ui| {
                for kind in crate::document::MarkerKind::all() {
                    let val = *kind;
                    if ui.selectable_value(&mut m.kind, val, val.label()).clicked() {
                        changed = true;
                    }
                }
            });

        if m.kind == crate::document::MarkerKind::None {
            ui.add_space(4.0);
            return;
        }

        // Color
        let mut rgb = [m.color.rgba[0], m.color.rgba[1], m.color.rgba[2]];
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            m.color.rgba[0] = rgb[0].clamp(0.,1.);
            m.color.rgba[1] = rgb[1].clamp(0.,1.);
            m.color.rgba[2] = rgb[2].clamp(0.,1.);
            changed = true;
        }
        ui.label("Color");

        // Size (individual, unless common is on) -- caller handles common
        let mut sz = m.size;
        if ui.add(egui::Slider::new(&mut sz, 2.0..=60.0).text("Size")).changed() {
            m.size = sz;
            changed = true;
        }

        // 2D Offset
        let mut ox = m.offset[0] as f32;
        let mut oy = m.offset[1] as f32;
        if ui.add(egui::Slider::new(&mut ox, -30.0..=30.0).text("Off X")).changed() {
            m.offset[0] = ox as f64;
            changed = true;
        }
        if ui.add(egui::Slider::new(&mut oy, -30.0..=30.0).text("Off Y")).changed() {
            m.offset[1] = oy as f64;
            changed = true;
        }

        // Rotation
        let mut rot = m.rotation as f32;
        if ui.add(egui::Slider::new(&mut rot, -180.0..=180.0).text("Rotate °")).changed() {
            m.rotation = rot as f64;
            changed = true;
        }
        if ui.checkbox(&mut m.auto_rotate, "Auto opposite").changed() {
            changed = true;
        }

        ui.add_space(4.0);

        // Live preview
        draw_marker_preview(ui, label, m, line_width);
    });
    changed
}

fn draw_marker_preview(
    ui: &mut Ui,
    which: &str,
    m: &crate::document::PathMarker,
    line_width: f32,
) {
    let size = egui::vec2(130.0, 46.0);
    let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);

    let center = rect.center();
    let w = rect.width();
    let h = rect.height();

    let line_color = egui::Color32::from_gray(160);
    let preview_line_w = line_width.max(1.0).min(4.0);  // match path stroke, clamped for preview

    let icon_size = m.size.max(4.0);
    let col = m.color.to_egui();

    let base = if m.auto_rotate {
        if which == "Start" {
            std::f32::consts::PI  // opposite direction for start marker
        } else {
            0.0_f32
        }
    } else {
        0.0_f32
    };
    let rot_rad = base + (m.rotation as f32).to_radians();

    // Compute local offset in preview space
    let off_x = m.offset[0] as f32;
    let off_y = m.offset[1] as f32;
    let c = rot_rad.cos();
    let s = rot_rad.sin();

    // Determine attach point on the line first, then place icon origin (local 0,0) there + offset
    // Use f32 for preview coords
    let (attach_x, attach_y) = if which == "Mid" {
        (center.x, center.y)
    } else if which == "Start" {
        (center.x, center.y)  // center; line comes from right to this attach point
    } else {
        (center.x, center.y)
    };

    let icon_cx = attach_x + off_x * c - off_y * s;
    let icon_cy = attach_y + off_x * s + off_y * c;

    if which == "Mid" {
        // full width straight line, marker in middle
        painter.line_segment(
            [egui::pos2(rect.left() + 4.0, center.y), egui::pos2(rect.right() - 4.0, center.y)],
            egui::Stroke::new(preview_line_w, line_color),
        );
    } else {
        // start or end: line ends at the attach point (icon origin)
        let attach_pt = egui::pos2(attach_x, attach_y);
        if which == "Start" {
            // line coming from right to attach
            painter.line_segment(
                [attach_pt, egui::pos2(rect.right() - 5.0, center.y)],
                egui::Stroke::new(preview_line_w, line_color),
            );
        } else {
            // end: line from left to attach
            painter.line_segment(
                [egui::pos2(rect.left() + 5.0, center.y), attach_pt],
                egui::Stroke::new(preview_line_w, line_color),
            );
        }
    }

    // Draw the icon itself at (icon_cx, icon_cy) with rotation, size, color
    // Simple shape drawing for preview (triangle, square, etc)
    let px = icon_cx;
    let py = icon_cy;
    let r = icon_size * 0.5;

    let c = rot_rad.cos();
    let s = rot_rad.sin();

    match m.kind {
        crate::document::MarkerKind::Triangle => {
            // local: tip (h,0), base at (0, \pm)  -- attach at (0,0) base
            let p1 = egui::pos2(px + r * c, py + r * s); // tip
            let p2 = egui::pos2(px + 0.65 * r * s, py - 0.65 * r * c);
            let p3 = egui::pos2(px - 0.65 * r * s, py + 0.65 * r * c);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], col, egui::Stroke::NONE));
        }
        crate::document::MarkerKind::Square => {
            let pts = vec![
                egui::pos2(px - r*c - r*s, py - r*s + r*c),
                egui::pos2(px + r*c - r*s, py - r*s - r*c),
                egui::pos2(px + r*c + r*s, py + r*s - r*c),
                egui::pos2(px - r*c + r*s, py + r*s + r*c),
            ];
            painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
        }
        crate::document::MarkerKind::HollowSquare => {
            let pts = vec![
                egui::pos2(px - r*c - r*s, py - r*s + r*c),
                egui::pos2(px + r*c - r*s, py - r*s - r*c),
                egui::pos2(px + r*c + r*s, py + r*s - r*c),
                egui::pos2(px - r*c + r*s, py + r*s + r*c),
            ];
            painter.add(egui::Shape::convex_polygon(pts, egui::Color32::TRANSPARENT, egui::Stroke::new(1.5, col)));
        }
        crate::document::MarkerKind::Ring => {
            painter.circle_stroke(egui::pos2(px, py), r * 0.85, egui::Stroke::new(1.8, col));
        }
        crate::document::MarkerKind::Line => {
            let dx = r * c;
            let dy = r * s;
            painter.line_segment([egui::pos2(px-dx, py-dy), egui::pos2(px+dx, py+dy)], egui::Stroke::new(2.0, col));
        }
        crate::document::MarkerKind::Arrow => {
            // attach at (0,0) local, tip forward
            let tip = egui::pos2(px + r * c, py + r * s);
            let w1 = egui::pos2(px + 0.0 * c - (-0.48 * r) * s, py + 0.0 * s + (-0.48 * r) * c );
            let b1 = egui::pos2(px + (-0.6*r) * c - (-0.48*r) * s, py + (-0.6*r)*s + (-0.48*r)*c );
            let b2 = egui::pos2(px + (-0.6*r) * c - (0.48*r) * s, py + (-0.6*r)*s + (0.48*r)*c );
            let w2 = egui::pos2(px + 0.0 * c - (0.48 * r) * s, py + 0.0 * s + (0.48 * r) * c );
            painter.add(egui::Shape::convex_polygon(vec![tip, w1, b1, b2, w2], col, egui::Stroke::NONE));
        }
        _ => {}
    }

    // small center cross for attach point
    painter.line_segment([egui::pos2(px-2.0, py), egui::pos2(px+2.0, py)], egui::Stroke::new(0.5, egui::Color32::from_gray(100)));
    painter.line_segment([egui::pos2(px, py-2.0), egui::pos2(px, py+2.0)], egui::Stroke::new(0.5, egui::Color32::from_gray(100)));
}

fn brush_numeric_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui
            .add(egui::DragValue::new(value).range(range).speed(speed))
            .changed();
    });
    changed
}

fn geometry_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    // Path markers (arrows / point icons) belong in Geometry, not Color & Stroke
    if app.selection.len() == 1 {
        let id = app.selection[0];
        if let Some(node) = app.project.nodes.get(id) {
            if matches!(&node.kind, NodeKind::Path { .. }) {
                path_markers_geometry_ui(app, ui);
                ui.add_space(8.0);
            }
            // Use tight scopes so immutable node borrows end before any &mut app.set calls
            {
                let id = app.selection[0];
                if let Some(node) = app.project.nodes.get(id) {
                    if matches!(&node.kind, NodeKind::FlowchartPath { .. }) {
                        if let NodeKind::FlowchartPath { path } = &node.kind {
                            let mut cr = path.corner_radius;
                            let mut ms = path.endpoint_marker_size;
                            let mut changed = false;
                            ui.separator();
                            ui.label(RichText::new("Flowchart Connector").strong().small());
                            theme::constraint_block(ui, |ui| {
                                changed |= ui.add(egui::Slider::new(&mut cr, 0.0..=48.0).text("Corner radius (curviness)")).changed();
                                changed |= ui.add(egui::Slider::new(&mut ms, 0.0..=24.0).text("Endpoint marker size")).changed();
                            });
                            if changed {
                                app.set_flowchart_path_props(id, cr, ms);
                            }
                        }
                    }
                }
            }
            ui.add_space(4.0);
            {
                let id = app.selection[0];
                if let Some(node) = app.project.nodes.get(id) {
                    if matches!(&node.kind, NodeKind::FlowchartNode { .. }) {
                        if let NodeKind::FlowchartNode { label, label_font_size, label_align, label_font_family, label_bold, label_italic, .. } = &node.kind {
                            let mut new_label = label.clone();
                            let mut new_fs = *label_font_size;
                            let mut new_al = *label_align;
                            let mut new_fam = label_font_family.clone();
                            let mut new_b = *label_bold;
                            let mut new_i = *label_italic;
                            let mut changed = false;

                            ui.separator();
                            ui.label(RichText::new("Label").strong().small());
                            changed |= ui.text_edit_singleline(&mut new_label).changed();
                            theme::constraint_block(ui, |ui| {
                                changed |= ui.add(egui::Slider::new(&mut new_fs, 6.0..=64.0).text("Font size")).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Align:");
                                if ui.selectable_label(matches!(new_al, crate::document::TextAlign::Left), "Left").clicked() { new_al = crate::document::TextAlign::Left; changed=true; }
                                if ui.selectable_label(matches!(new_al, crate::document::TextAlign::Center), "Center").clicked() { new_al = crate::document::TextAlign::Center; changed=true; }
                                if ui.selectable_label(matches!(new_al, crate::document::TextAlign::Right), "Right").clicked() { new_al = crate::document::TextAlign::Right; changed=true; }
                            });
                            changed |= ui.text_edit_singleline(&mut new_fam).changed();
                            ui.horizontal(|ui| {
                                changed |= ui.checkbox(&mut new_b, "Bold").changed();
                                changed |= ui.checkbox(&mut new_i, "Italic").changed();
                            });
                            if changed {
                                app.set_flowchart_node_label(id, new_label, new_fs, new_al, new_fam, new_b, new_i);
                            }
                        }
                    }
                }
            }
        }
    }

    if app.tools.active == ToolKind::Brush {
        // ── Main Brush Settings ─────────────────────────────────────────
        theme::constraint_block(ui, |ui| {
            ui.label(
                RichText::new(format!("{} Brush settings", icons::BRUSH))
                    .font(nerd_font_id(14.0))
                    .strong(),
            );
            ui.add_space(4.0);

            egui::ComboBox::from_label("Type")
                .selected_text(match app.tools.brush.brush_type {
                    crate::tools::BrushType::Standard => "Standard",
                    crate::tools::BrushType::Pen => "Pen",
                    crate::tools::BrushType::Calligraphy => "Calligraphy",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.tools.brush.brush_type, crate::tools::BrushType::Standard, "Standard");
                    ui.selectable_value(&mut app.tools.brush.brush_type, crate::tools::BrushType::Pen, "Pen");
                    ui.selectable_value(&mut app.tools.brush.brush_type, crate::tools::BrushType::Calligraphy, "Calligraphy");
                });

            ui.add_space(4.0);
            brush_numeric_row(ui, "Size", &mut app.tools.brush.size, 1.0..=100.0, 0.4);
            brush_numeric_row(ui, "Smoothness", &mut app.tools.brush.smoothness, 0.0..=1.0, 0.01);
            brush_numeric_row(ui, "Heavybrush", &mut app.tools.brush.heavy, 0.0..=1.0, 0.01);
        });

        ui.add_space(6.0);

        // ── Input Mode Panel ────────────────────────────────────────────
        theme::constraint_block(ui, |ui| {
            ui.label(
                RichText::new("🖱 Input Mode")
                    .font(nerd_font_id(13.0))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let is_mouse = app.tools.brush.input_mode == crate::tools::BrushInputMode::Mouse;
                let is_stylus = app.tools.brush.input_mode == crate::tools::BrushInputMode::Stylus;
                if ui.selectable_label(is_mouse, "🖱 Mouse").clicked() {
                    app.tools.brush.input_mode = crate::tools::BrushInputMode::Mouse;
                }
                ui.add_space(4.0);
                if ui.selectable_label(is_stylus, "✏ Stylus").clicked() {
                    app.tools.brush.input_mode = crate::tools::BrushInputMode::Stylus;
                }
            });

            ui.add_space(6.0);

            match app.tools.brush.input_mode {
                crate::tools::BrushInputMode::Mouse => {
                    ui.label(RichText::new("Mouse sensitivity").color(colors::TEXT_MUTED));
                    ui.add_space(2.0);
                    brush_numeric_row(
                        ui,
                        "Pressure sensitivity",
                        &mut app.tools.brush.mouse_pressure_sensitivity,
                        0.0..=2.0,
                        0.02,
                    );
                    brush_numeric_row(
                        ui,
                        "Speed sensitivity",
                        &mut app.tools.brush.mouse_speed_sensitivity,
                        0.0..=2.0,
                        0.02,
                    );
                    ui.add_space(4.0);
                    ui.checkbox(&mut app.tools.brush.mouse_rotate_by_direction, "Rotate tip by direction");
                }
                crate::tools::BrushInputMode::Stylus => {
                    ui.label(RichText::new("Stylus options").color(colors::TEXT_MUTED));
                    ui.add_space(2.0);
                    brush_numeric_row(
                        ui,
                        "Tilt angle (°)",
                        &mut app.tools.brush.stylus_tilt_angle,
                        0.0..=90.0,
                        0.5,
                    );
                    let pen_angle_changed = brush_numeric_row(
                        ui,
                        "Pen angle (°)",
                        &mut app.tools.brush.stylus_pen_angle,
                        0.0..=360.0,
                        1.0,
                    );
                    brush_numeric_row(
                        ui,
                        "Pressure",
                        &mut app.tools.brush.stylus_pressure,
                        0.0..=1.0,
                        0.01,
                    );
                    ui.add_space(6.0);
                    // 3D pen-angle preview
                    ui.label(RichText::new("Pen angle 3D preview").strong());
                    ui.add_space(2.0);
                    draw_stylus_3d_preview(ui,
                        app.tools.brush.stylus_pen_angle,
                        app.tools.brush.stylus_tilt_angle,
                        app.tools.brush.stylus_pressure);
                    let _ = pen_angle_changed; // used to trigger repaint in future
                }
            }
        });

        ui.add_space(6.0);

        // ── Brush-type Configure Container ──────────────────────────────
        match app.tools.brush.brush_type {
            crate::tools::BrushType::Standard => {}  // No extra container for Standard

            crate::tools::BrushType::Pen => {
                theme::constraint_block(ui, |ui| {
                    ui.label(
                        RichText::new("🖊 Pen Configuration")
                            .font(nerd_font_id(13.0))
                            .strong(),
                    );
                    ui.add_space(4.0);
                    brush_numeric_row(ui, "Tip roundness", &mut app.tools.brush.pen_roundness, 0.0..=1.0, 0.01);
                    brush_numeric_row(
                        ui,
                        "Press on paper",
                        &mut app.tools.brush.pen_press_on_paper,
                        0.0..=1.0,
                        0.01,
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("Pen Tip 3D Preview").strong());
                    ui.add_space(2.0);
                    let is_drawing = !app.tools.brush.points.is_empty();
                    let active_width = if is_drawing {
                        app.tools.brush.points.last().map(|&(_, _, w)| w).unwrap_or(app.tools.brush.size)
                    } else {
                        app.tools.brush.size
                    };
                    draw_3d_pen_tip(ui, active_width, is_drawing);
                });
                ui.add_space(6.0);
            }

            crate::tools::BrushType::Calligraphy => {
                theme::constraint_block(ui, |ui| {
                    ui.label(
                        RichText::new("🖋 Calligraphy Configuration")
                            .font(nerd_font_id(13.0))
                            .strong(),
                    );
                    ui.add_space(4.0);
                    ui.checkbox(&mut app.tools.brush.calli_rotate_tip, "Rotate tip by stroke direction");
                    ui.add_space(4.0);
                    ui.label(RichText::new("Nib geometry").color(colors::TEXT_MUTED));
                    brush_numeric_row(
                        ui,
                        "Fountain nib size",
                        &mut app.tools.brush.calli_fountain_size,
                        0.1..=3.0,
                        0.02,
                    );
                    ui.add_space(4.0);
                    ui.checkbox(&mut app.tools.brush.calli_dynamic, "Dynamic nib (speed-reactive)");
                    ui.add_space(6.0);
                    ui.label(RichText::new("Calligraphy Nib Preview").strong());
                    ui.add_space(2.0);
                    let is_drawing = !app.tools.brush.points.is_empty();
                    let active_width = if is_drawing {
                        app.tools.brush.points.last().map(|&(_, _, w)| w).unwrap_or(app.tools.brush.size)
                    } else {
                        app.tools.brush.size
                    };
                    draw_3d_calligraphy_nib(ui, active_width, is_drawing);
                });
                ui.add_space(6.0);
            }
        }

        return;
    }

    if app.selection.is_empty() {
        if app.tools.active == ToolKind::Polygon {
            theme::constraint_block(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} Polygon tool",
                        icons::polygon_icon(app.polygon_sides)
                    ))
                    .font(nerd_font_id(14.0))
                    .strong(),
                );
                let mut sides = app.polygon_sides;
                if ui
                    .add(egui::Slider::new(&mut sides, 3..=24).text("Sides"))
                    .changed()
                {
                    app.polygon_sides = sides;
                }
            });
            return;
        }
        ui.label(
            RichText::new("Select one object to edit geometry")
                .color(colors::TEXT_MUTED),
        );
        return;
    }
    let point_edit_id = app
        .tools
        .select
        .primary_path_point()
        .filter(|(pid, _)| app.selection.contains(pid))
        .map(|(pid, _)| pid);

    if app.selection.len() != 1 && point_edit_id.is_none() {
        ui.label(
            RichText::new("Select one object to edit geometry")
                .color(colors::TEXT_MUTED),
        );
        return;
    }
    let id = point_edit_id.unwrap_or(app.selection[0]);

    // Check if the selected ID is a Layer (specifically Video layer, or any layer)
    let is_layer = app.project.document.layers.iter().any(|l| l.id == id);
    if is_layer {
        if let Some(pos) = app.project.document.layers.iter().position(|l| l.id == id) {
            let layer = &mut app.project.document.layers[pos];
            let display_name = if layer.name.chars().count() > 16 {
                format!("{}...", layer.name.chars().take(14).collect::<String>())
            } else {
                layer.name.clone()
            };
            theme::constraint_block(ui, |ui| {
                let lbl = ui.label(RichText::new(format!("🎥 Video: {}", display_name)).strong().color(colors::ACCENT));
                lbl.on_hover_text(&layer.name);
                ui.add_space(4.0);
                
                // Position X and Y
                ui.horizontal(|ui| {
                    ui.label("Position X");
                    ui.add(egui::DragValue::new(&mut layer.x).speed(1.0));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut layer.y).speed(1.0));
                });
                
                // Rotation
                ui.horizontal(|ui| {
                    ui.label("Rotation (°)");
                    ui.add(egui::DragValue::new(&mut layer.rotation).speed(1.0).range(-360.0..=360.0));
                });
                
                // Scale (Width / Height)
                ui.horizontal(|ui| {
                    ui.label("Width");
                    let prev_w = layer.width;
                    if ui.add(egui::DragValue::new(&mut layer.width).speed(1.0).range(1.0..=8192.0)).changed() {
                        if layer.aspect_ratio_locked && prev_w > 0.0 {
                            let ratio = layer.height / prev_w;
                            layer.height = layer.width * ratio;
                        }
                    }
                    ui.label("Height");
                    let prev_h = layer.height;
                    if ui.add(egui::DragValue::new(&mut layer.height).speed(1.0).range(1.0..=8192.0)).changed() {
                        if layer.aspect_ratio_locked && prev_h > 0.0 {
                            let ratio = layer.width / prev_h;
                            layer.width = layer.height * ratio;
                        }
                    }
                });
                
                // Aspect ratio lock toggle
                ui.checkbox(&mut layer.aspect_ratio_locked, "Keep Aspect Ratio (No Squeeze)");
            });
            return;
        }
    }

    let Some(node) = app.project.nodes.get(id).cloned() else {
        return;
    };

    if app.tools.active == ToolKind::Node {
        ui.label(
            RichText::new(format!("Editing: {}", node.name))
                .strong()
                .color(colors::ACCENT),
        );
        ui.separator();
    }

    if matches!(&node.kind, NodeKind::Path { .. }) {
        let points_on: Vec<_> = app.tools.select.points_on_path(id);
        if points_on.len() > 1 {
            ui.label(
                RichText::new(format!("{} points selected", points_on.len())).strong(),
            );
            // Wrap so long labels don't overflow the narrow Actions panel.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.spacing_mut().item_spacing.y = 4.0;
                if ui.button("Smooth").on_hover_text("Smooth selected points").clicked() {
                    app.smooth_selected_path_points();
                }
                if ui
                    .button(RichText::new("Delete").color(colors::ALERT))
                    .on_hover_text("Delete selected points")
                    .clicked()
                {
                    app.remove_selected_path_points();
                }
            });
            ui.separator();
        } else if let Some(point_idx) = points_on.first().copied() {
            let smooth = app
                .project
                .nodes
                .get(id)
                .and_then(|n| match &n.kind {
                    NodeKind::Path { path } => Some(path.is_anchor_smooth(point_idx)),
                    _ => None,
                })
                .unwrap_or(false);
            path_point_bezier_panel(app, ui, id, point_idx, smooth);
        }
    }

    if app.tools.active == ToolKind::Text && app.selection.is_empty() {
        text_style_panel(app, ui, true);
        return;
    }

    // When an ObjectOnPath path is selected, show the *whole* resulting object size
    // (union of all placed source instances), not just the spine path.
    if matches!(&node.kind, NodeKind::Path { .. }) {
        if let Some((objects, path_id)) = app.object_on_path_panel_context() {
            if path_id == id && !objects.is_empty() {
                if let Some(first_src_id) = objects.first() {
                    if let (Some(src), Some(eff)) = (
                        app.project.nodes.get(*first_src_id),
                        find_effect_for_pair(&app.project.document.path_effects, *first_src_id, path_id),
                    ) {
                        if let NodeKind::Path { path } = &node.kind {
                            let whole = compute_whole_object_bounds(src, eff, path, 0.5);
                            let w = (whole.x1 - whole.x0).abs();
                            let h = (whole.y1 - whole.y0).abs();
                            ui.label(
                                RichText::new("Whole Object (on-path placements)")
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                            ui.horizontal(|ui| {
                                ui.label(format!("W: {w:.1}"));
                                ui.label(format!("H: {h:.1}"));
                            });
                            ui.separator();
                        }
                    }
                }
            }
        }
    }

    // Show whole bounds for Tiling or CircularClone on this selected object
    if let Some(e) = app.project.document.tiling_effects.values().find(|e| e.source_id == id) {
        if let Some(src) = app.project.nodes.get(id) {
            let whole = compute_tiling_whole_bounds(src, e);
            let w = (whole.x1 - whole.x0).abs();
            let h = (whole.y1 - whole.y0).abs();
            ui.label(
                RichText::new("Whole Tiling")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.horizontal(|ui| {
                ui.label(format!("W: {w:.1}"));
                ui.label(format!("H: {h:.1}"));
            });
            ui.separator();
        }
    }
    if let Some(e) = app.project.document.circular_effects.values().find(|e| e.source_id == id) {
        if let Some(src) = app.project.nodes.get(id) {
            let whole = compute_circular_whole_bounds(src, e);
            let w = (whole.x1 - whole.x0).abs();
            let h = (whole.y1 - whole.y0).abs();
            ui.label(
                RichText::new("Whole CircularClone")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.horizontal(|ui| {
                ui.label(format!("W: {w:.1}"));
                ui.label(format!("H: {h:.1}"));
            });
            ui.separator();
        }
    }

    match node.geometry_profile() {
        GeometryProfile::Rect {
            origin_x,
            origin_y,
            width,
            height,
            corner_radius,
        } => {
            ui.label(RichText::new("Rectangle").strong());
            let mut x = origin_x;
            let mut y = origin_y;
            let mut w = width;
            let mut h = height;
            let mut rx = corner_radius;
            let mut changed = false;
            constraint_origin(ui, &mut x, &mut y, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Size").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut w).prefix("W:")).changed();
                changed |= ui.add(decimal_drag(&mut h).prefix("H:")).changed();
            });
            theme::constraint_block(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icons::BORDER_RADIUS)
                            .font(nerd_font_id(14.0))
                            .color(colors::TEXT_MUTED),
                    );
                    ui.label("Border radius");
                });
                changed |= ui.add(decimal_drag(&mut rx)).changed();
            });
            if changed {
                app.set_rect_geometry(id, x, y, w, h, rx);
            }
        }
        GeometryProfile::Circle {
            origin_x,
            origin_y,
            radius,
        } => {
            ui.label(RichText::new("Circle").strong());
            let mut cx = origin_x;
            let mut cy = origin_y;
            let mut r = radius;
            let mut changed = false;
            constraint_origin(ui, &mut cx, &mut cy, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Radius").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut r)).changed();
            });
            if changed {
                app.set_circle_geometry(id, cx, cy, r);
            }
        }
        GeometryProfile::Ellipse {
            origin_x,
            origin_y,
            radius_x,
            radius_y,
        } => {
            ui.label(RichText::new("Ellipse").strong());
            let mut cx = origin_x;
            let mut cy = origin_y;
            let mut rx = radius_x;
            let mut ry = radius_y;
            let mut changed = false;
            constraint_origin(ui, &mut cx, &mut cy, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Radii").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut rx).prefix("RX:")).changed();
                changed |= ui.add(decimal_drag(&mut ry).prefix("RY:")).changed();
            });
            if changed {
                app.set_ellipse_geometry(id, cx, cy, rx, ry);
            }
        }
        GeometryProfile::Polygon {
            origin_x,
            origin_y,
            radius,
            sides,
            rotation_deg,
        } => {
            ui.label(
                RichText::new(format!("{} Polygon", icons::polygon_icon(sides)))
                    .font(nerd_font_id(14.0))
                    .strong(),
            );
            let mut cx = origin_x;
            let mut cy = origin_y;
            let mut r = radius;
            let mut n_sides = sides;
            let mut rot = rotation_deg;
            let mut changed = false;
            constraint_origin(ui, &mut cx, &mut cy, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Shape").small().color(colors::TEXT_MUTED));
                changed |= ui
                    .add(egui::Slider::new(&mut n_sides, 3..=24).text("Sides"))
                    .changed();
                changed |= ui
                    .add(decimal_drag(&mut r).prefix("Radius:"))
                    .changed();
                changed |= ui
                    .add(decimal_drag(&mut rot).prefix("Rotation °:"))
                    .changed();
            });
            if changed {
                app.polygon_sides = n_sides;
                app.set_polygon_geometry(id, cx, cy, r, n_sides, rot);
            }
        }
        GeometryProfile::Line {
            origin_x,
            origin_y,
            end_x,
            end_y,
            length,
            angle_deg,
        } => {
            ui.label(RichText::new("Line").strong());
            let mut x0 = origin_x;
            let mut y0 = origin_y;
            let mut x1 = end_x;
            let mut y1 = end_y;
            let mut changed = false;
            constraint_origin(ui, &mut x0, &mut y0, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("End").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut x1).prefix("X:")).changed();
                changed |= ui.add(decimal_drag(&mut y1).prefix("Y:")).changed();
            });
            ui.horizontal(|ui| {
                ui.label(format!("Length: {length:.2}"));
                ui.label(format!("Angle: {angle_deg:.1}°"));
            });
            if changed {
                app.set_line_geometry(id, x0, y0, x1, y1);
            }
        }
        GeometryProfile::ClosedPath {
            origin_x,
            origin_y,
            vertices,
            cyclic,
        } => {
            ui.label(RichText::new("Closed path").strong());
            let mut ox = origin_x;
            let mut oy = origin_y;
            let mut changed = false;
            constraint_origin(ui, &mut ox, &mut oy, &mut changed);
            if changed {
                app.set_path_origin(id, ox, oy);
            }
            ui.label(format!("Vertices: {vertices}"));
            ui.label(format!("Cyclic: {cyclic}"));
            ui.label(
                RichText::new("Fill enabled — drag points with the node tool (N)")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        GeometryProfile::OpenPath {
            origin_x,
            origin_y,
            vertices,
            cyclic,
        } => {
            ui.label(RichText::new("Open path").strong());
            let mut ox = origin_x;
            let mut oy = origin_y;
            let mut changed = false;
            constraint_origin(ui, &mut ox, &mut oy, &mut changed);
            if changed {
                app.set_path_origin(id, ox, oy);
            }
            ui.label(format!("Vertices: {vertices}"));
            ui.label(format!("Cyclic: {cyclic}"));
            ui.label(
                RichText::new("Not cyclic — close the path in Color & stroke to apply fill")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        GeometryProfile::Plotter {
            origin_x,
            origin_y,
            width,
            height,
            expr,
            ref_axis,
            domain_min,
            domain_max,
            range_min,
            range_max,
            auto_range,
            margin_pct,
            plot_stroke_width,
            plot_stroke_rgba,
        } => {
            ui.label(
                RichText::new(format!("{} Plotter", icons::PLOTTER))
                    .font(nerd_font_id(14.0))
                    .strong(),
            );
            let mut x = origin_x;
            let mut y = origin_y;
            let mut w = width;
            let mut h = height;
            let mut axis = ref_axis;
            let mut d0 = domain_min;
            let mut d1 = domain_max;
            let mut r0 = range_min;
            let mut r1 = range_max;
            let mut auto_r = auto_range;
            let mut margin = margin_pct;
            let mut psw = plot_stroke_width;
            let mut pcol = plot_stroke_rgba;
            // Keep a stable draft buffer while typing so Geometry doesn't reset the field every frame.
            let need_seed = !matches!(
                app.plotter_inline_expr.as_ref(),
                Some((nid, _)) if *nid == id
            );
            if need_seed {
                // Leaving another plotter's edit: commit its expr history first.
                if let Some((prev_id, _)) = app.plotter_expr_edit_before {
                    if prev_id != id {
                        app.commit_plotter_expr_edit(prev_id);
                    }
                }
                app.plotter_inline_expr = Some((id, expr.clone()));
            }
            let mut changed = false;
            let mut commit_expr = false;
            let mut expr_typed = false;

            constraint_origin(ui, &mut x, &mut y, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Size").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut w).prefix("W:")).changed();
                changed |= ui.add(decimal_drag(&mut h).prefix("H:")).changed();
            });

            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Plot line").small().color(colors::TEXT_MUTED));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Width").small().color(colors::TEXT_MUTED));
                    changed |= ui
                        .add(
                            decimal_drag(&mut psw)
                                .speed(0.1)
                                .range(0.5..=48.0)
                                .suffix(" px"),
                        )
                        .changed();
                });
                let mut c = egui::Color32::from_rgba_unmultiplied(
                    (pcol[0] * 255.0) as u8,
                    (pcol[1] * 255.0) as u8,
                    (pcol[2] * 255.0) as u8,
                    (pcol[3] * 255.0) as u8,
                );
                if ui.color_edit_button_srgba(&mut c).changed() {
                    let [r, g, b, a] = c.to_array();
                    pcol = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0];
                    changed = true;
                }
            });

            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Reference").small().color(colors::TEXT_MUTED));
                ui.horizontal(|ui| {
                    let fx_lbl = RichText::new(format!("f(x) {} y", icons::ARROW_RIGHT))
                        .font(nerd_font_id(13.0));
                    let fy_lbl = RichText::new(format!("f(y) {} x", icons::ARROW_RIGHT))
                        .font(nerd_font_id(13.0));
                    if ui
                        .selectable_label(matches!(axis, crate::document::PlotterRef::Fx), fx_lbl)
                        .clicked()
                    {
                        axis = crate::document::PlotterRef::Fx;
                        changed = true;
                    }
                    if ui
                        .selectable_label(matches!(axis, crate::document::PlotterRef::Fy), fy_lbl)
                        .clicked()
                    {
                        axis = crate::document::PlotterRef::Fy;
                        changed = true;
                    }
                });
                ui.label(
                    RichText::new(if matches!(axis, crate::document::PlotterRef::Fx) {
                        "expr uses x (and t 0..1)"
                    } else {
                        "expr uses y (and t 0..1)"
                    })
                    .small()
                    .color(colors::TEXT_MUTED),
                );
                let draft = app
                    .plotter_inline_expr
                    .as_mut()
                    .map(|(_, s)| s)
                    .expect("plotter_inline_expr seeded");
                let te = ui.add(
                    egui::TextEdit::multiline(draft)
                        .id(egui::Id::new(("plotter_inline_expr", id)))
                        .desired_rows(2)
                        .desired_width(ui.available_width().min(220.0))
                        .font(egui::TextStyle::Monospace)
                        .hint_text(if matches!(axis, crate::document::PlotterRef::Fx) {
                            "sin(x)"
                        } else {
                            "sin(y)"
                        }),
                );
                if te.changed() {
                    expr_typed = true;
                }
                if te.double_clicked() {
                    let cur = app
                        .plotter_inline_expr
                        .as_ref()
                        .map(|(_, s)| s.clone())
                        .unwrap_or_else(|| expr.clone());
                    app.begin_plotter_expr_edit(id);
                    app.plotter_formula_dialog = Some(id);
                    app.plotter_formula_draft = cur;
                }
                if te.lost_focus() {
                    commit_expr = true;
                }
                let draft_ref = app
                    .plotter_inline_expr
                    .as_ref()
                    .map(|(_, s)| s.as_str())
                    .unwrap_or("");
                let probe = if matches!(axis, crate::document::PlotterRef::Fx) {
                    let mut v = crate::document::ExprVars::simple(0.5, 0.0, 0.0);
                    v.x = 0.0;
                    crate::document::eval_expr_vars(draft_ref, v)
                } else {
                    let mut v = crate::document::ExprVars::simple(0.5, 0.0, 0.0);
                    v.y = 0.0;
                    crate::document::eval_expr_vars(draft_ref, v)
                };
                if let Err(e) = probe {
                    ui.label(RichText::new(e.0).small().color(egui::Color32::from_rgb(220, 90, 90)));
                }
            });

            // Live preview: push draft expr onto the node every keystroke (no undo spam).
            if expr_typed {
                let draft_now = app
                    .plotter_inline_expr
                    .as_ref()
                    .map(|(_, s)| s.clone())
                    .unwrap_or_default();
                app.begin_plotter_expr_edit(id);
                app.set_plotter_expr_live(id, draft_now);
            }

            theme::constraint_block(ui, |ui| {
                let (dom_label, rng_label) = if matches!(axis, crate::document::PlotterRef::Fx) {
                    ("X domain", "Y range")
                } else {
                    ("Y domain", "X range")
                };
                ui.label(RichText::new(dom_label).small().color(colors::TEXT_MUTED));
                ui.horizontal(|ui| {
                    changed |= ui.add(decimal_drag(&mut d0).prefix("min ")).changed();
                    changed |= ui.add(decimal_drag(&mut d1).prefix("max ")).changed();
                });
                ui.label(RichText::new(rng_label).small().color(colors::TEXT_MUTED));
                changed |= ui.checkbox(&mut auto_r, "Auto range").changed();
                ui.add_enabled_ui(!auto_r, |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.add(decimal_drag(&mut r0).prefix("min ")).changed();
                        changed |= ui.add(decimal_drag(&mut r1).prefix("max ")).changed();
                    });
                });
                if auto_r {
                    if let Some(node) = app.project.nodes.get(id) {
                        if let Some((_, cr0, cr1)) = node.plotter_polyline() {
                            ui.label(
                                RichText::new(format!("Auto view: [{cr0:.3}, {cr1:.3}]"))
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Margin").small().color(colors::TEXT_MUTED));
                        changed |= ui
                            .add(
                                decimal_drag(&mut margin)
                                    .speed(0.5)
                                    .range(0.0..=50.0)
                                    .suffix(" %"),
                            )
                            .changed();
                    });
                }
            });

            let expr_now = app
                .plotter_inline_expr
                .as_ref()
                .map(|(_, s)| s.clone())
                .unwrap_or_else(|| expr.clone());
            if changed {
                // Geometry/range/etc. — include current draft expr so undo stays coherent.
                app.set_plotter_geometry(
                    id,
                    x,
                    y,
                    w,
                    h,
                    expr_now.clone(),
                    axis,
                    d0,
                    d1,
                    r0,
                    r1,
                    auto_r,
                    margin,
                    psw,
                    pcol,
                );
                // Geometry push already recorded full node; drop pending expr-only snapshot.
                app.plotter_expr_edit_before = None;
            }
            if commit_expr {
                app.commit_plotter_expr_edit(id);
                app.plotter_inline_expr = Some((id, expr_now));
            }
        }
        GeometryProfile::Arc {
            origin_x,
            origin_y,
            radius,
            start_angle_deg,
            sweep_angle_deg,
            join,
        } => {
            ui.label(RichText::new("Arc").strong());
            let mut cx = origin_x;
            let mut cy = origin_y;
            let mut r = radius;
            let mut start = start_angle_deg;
            let mut sweep = sweep_angle_deg;
            let mut current_join = join;
            let mut changed = false;

            constraint_origin(ui, &mut cx, &mut cy, &mut changed);

            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Radius").small().color(colors::TEXT_MUTED));
                changed |= ui.add(decimal_drag(&mut r).range(0.5..=10000.0).speed(0.5).fixed_decimals(1)).changed();

                ui.label(RichText::new("Angles (degrees)").small().color(colors::TEXT_MUTED));
                let mut end = start + sweep;
                changed |= ui.add(decimal_drag(&mut start).prefix("Start: ").suffix("°").speed(1.0).fixed_decimals(1)).changed();
                changed |= ui.add(decimal_drag(&mut end).prefix("End: ").suffix("°").speed(1.0).fixed_decimals(1)).changed();
                sweep = end - start;
            });

            ui.label(RichText::new("Fill type").small().color(colors::TEXT_MUTED));
            ui.vertical(|ui| {
                for mode in [ArcJoin::NoJoin, ArcJoin::Chord, ArcJoin::ToOrigin] {
                    let label = match mode {
                        ArcJoin::NoJoin => "None",
                        ArcJoin::Chord => "Chord",
                        ArcJoin::ToOrigin => "Sector",
                    };
                    if ui.selectable_label(current_join == mode, label).clicked() {
                        current_join = mode;
                        changed = true;
                    }
                }
            });

            if changed {
                app.set_arc_geometry(id, cx, cy, r, start, sweep, current_join);
            }
        }
        GeometryProfile::Text {
            origin_x,
            origin_y,
            width,
            height,
            content,
            font_size,
            font_family,
            bold,
            italic,
        } => {
            ui.label(RichText::new("Text").strong());
            let mut x = origin_x;
            let mut y = origin_y;
            let mut changed = false;
            constraint_origin(ui, &mut x, &mut y, &mut changed);
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("Bounds").small().color(colors::TEXT_MUTED));
                ui.horizontal(|ui| {
                    ui.label(format!("W: {width:.1}"));
                    ui.label(format!("H: {height:.1}"));
                });
            });
            let mut style = TextStyle {
                content,
                font_size,
                font_family,
                bold,
                italic,
            };
            if text_style_panel(app, ui, false) {
                style.content = app.ui_text_content.clone();
                style.font_size = app.ui_text_font_size;
                style.font_family = app.ui_text_font_family.clone();
                style.bold = app.ui_text_bold;
                style.italic = app.ui_text_italic;
                changed = true;
            }
            if changed {
                app.set_text_style(id, style, x, y);
            }
        }
        GeometryProfile::Unsupported => {
            ui.label(
                RichText::new("No geometric constraints for this object")
                    .color(colors::TEXT_MUTED),
            );
        }
    }
}

fn path_point_bezier_panel(
    app: &mut VadadeeBerryApp,
    ui: &mut Ui,
    id: crate::document::NodeId,
    point_idx: usize,
    smooth: bool,
) {
    use crate::document::BezierHandleMode;

    let handle_mode = app
        .project
        .nodes
        .get(id)
        .and_then(|n| match &n.kind {
            crate::document::NodeKind::Path { path } => Some(path.handle_mode(point_idx)),
            _ => None,
        })
        .unwrap_or(BezierHandleMode::Symmetric);

    theme::constraint_block(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::BEZIER)
                    .font(nerd_font_id(16.0))
                    .color(colors::ACCENT),
            );
            ui.label(RichText::new(format!("Point {}", point_idx + 1)).strong());
        });
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(format!("{} Corner", icons::JOIN_SHARP)).font(nerd_font_id(14.0)))
                .on_hover_text("Sharp corner at this point")
                .clicked()
            {
                app.set_path_anchor_smooth(id, point_idx, false);
            }
            if ui
                .button(RichText::new(format!("{} Bezier", icons::BEZIER)).font(nerd_font_id(14.0)))
                .on_hover_text("Smooth (round) bezier handles at this point")
                .clicked()
            {
                app.set_path_anchor_smooth(id, point_idx, true);
            }
        });
        ui.add_space(4.0);
        if ui
            .button(RichText::new(format!("{} Delete Point", icons::DELETE)).font(nerd_font_id(14.0)).color(colors::ALERT))
            .on_hover_text("Delete this point")
            .clicked()
        {
            app.remove_selected_path_points();
        }
        if smooth {
            ui.label(
                RichText::new("Handle mode")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            ui.vertical(|ui| {
                for mode in [
                    BezierHandleMode::Symmetric,
                    BezierHandleMode::Asymmetric,
                    BezierHandleMode::EqualLength,
                    BezierHandleMode::LeftOnly,
                    BezierHandleMode::RightOnly,
                    BezierHandleMode::Both,
                ] {
                    if ui
                        .selectable_label(handle_mode == mode, mode.label())
                        .on_hover_text(match mode {
                            BezierHandleMode::Symmetric => {
                                "Opposite direction; each handle keeps its own length"
                            }
                            BezierHandleMode::Asymmetric => {
                                "Move each handle independently"
                            }
                            BezierHandleMode::EqualLength => {
                                "Opposite direction with equal handle lengths"
                            }
                            BezierHandleMode::LeftOnly => {
                                "Single incoming handle (left)"
                            }
                            BezierHandleMode::RightOnly => {
                                "Single outgoing handle (right)"
                            }
                            BezierHandleMode::Both => {
                                "Both handles independent"
                            }
                        })
                        .clicked()
                        && handle_mode != mode
                    {
                        app.set_path_handle_mode(id, point_idx, mode);
                    }
                }
            });
            ui.label(
                RichText::new("Drag orange handles on canvas to shape the curve")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        } else {
            ui.label(
                RichText::new("Double-click point or choose Bezier — works best on paths with 3+ points")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        if !smooth {
            ui.add_space(4.0);
            if ui.button(
                RichText::new(format!("{} Corner curve", icons::BEZIER))
                    .font(nerd_font_id(11.0))
            ).clicked() {
                app.make_corner_curve(id, point_idx);
            }
            ui.label(
                RichText::new("Fillet at sharp corner: yellow T1/T2 kept equidistant (D = R / tan(θ/2)). Drag to adjust radius. Non-destructive.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
    });
}

/// Multiline editor positioned on the canvas at the text object (WYSIWYG typing).
pub fn show_on_page_text_editor(
    app: &mut VadadeeBerryApp,
    ui: &mut Ui,
    canvas_response: &egui::Response,
    origin: egui::Pos2,
) {
    let Some(id) = app.on_page_text_edit else {
        app.text_editor_rect = None;
        return;
    };
    let (doc_x, doc_y, font_size, font_family) = {
        let Some(node) = app.project.nodes.get(id) else {
            app.on_page_text_edit = None;
            return;
        };
        let NodeKind::Text { x, y, style } = &node.kind else {
            app.on_page_text_edit = None;
            return;
        };
        (*x, *y, style.font_size, style.font_family.clone())
    };

    let screen_pos = app.viewport.doc_to_screen((doc_x, doc_y), origin);
    let bounds = app
        .project
        .nodes
        .get(id)
        .map(|n| n.bounds())
        .unwrap_or_default();
    let min_w = ((bounds.x1 - bounds.x0) as f32 * app.viewport.zoom).max(160.0);

    let ctx = ui.ctx().clone();
    app.fonts.ensure_loaded(&ctx, &font_family);
    let font = FontId::new(
        (font_size * app.viewport.zoom).max(8.0),
        FontFamily::Name(font_family.as_str().into()),
    );

    let mut focus_resp = None;
    // On-page edit: direct canvas typing with no external framed widget or "On page" label.
    // The TextEdit is transparent and positioned at the text origin so input/caret appear in-place.
    // The normal node text draw is suppressed while editing (see canvas_ui) so only one set of glyphs.
    egui::Area::new(egui::Id::new("on_page_text_edit"))
        .fixed_pos(screen_pos)
        .order(egui::Order::Foreground)
        .constrain(false)
        .interactable(true)
        .show(&ctx, |ui| {
            ui.set_min_width(min_w);
            let frame = egui::Frame::NONE;
            frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    let tick_resp = ui.horizontal(|ui| {
                        let btn_frame = egui::Frame::NONE
                            .fill(egui::Color32::from_black_alpha(200))
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(10, 6));
                        btn_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let resp = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✔")
                                            .color(egui::Color32::from_rgb(0, 230, 118))
                                            .strong()
                                            .size(16.0)
                                    )
                                    .frame(false)
                                );
                                if resp.clicked() {
                                    app.finish_on_page_text_edit();
                                }
                                
                                ui.add_space(8.0);
                                
                                let cross_resp = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("✖")
                                            .color(egui::Color32::from_rgb(255, 23, 68))
                                            .strong()
                                            .size(16.0)
                                    )
                                    .frame(false)
                                );
                                if cross_resp.clicked() {
                                    app.delete_on_page_text_node(id);
                                }
                                
                                resp
                            })
                        })
                    });
                    ui.add_space(6.0); // margin between checkmark and text box

                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut app.ui_text_content)
                            .font(font)
                            .desired_rows(4)
                            .desired_width(min_w)
                            .hint_text("Type here…"),
                    );
                    
                    let union_rect = resp.rect.union(tick_resp.response.rect);
                    app.text_editor_rect = Some(union_rect);

                    if resp.changed() {
                        app.patch_on_page_text_live(id);
                    }
                    focus_resp = Some(resp);
                });
            });
        });

    if app.on_page_text_focus_pending {
        if let Some(r) = focus_resp {
            r.request_focus();
        }
        #[cfg(target_os = "android")]
        {
            if let Some(android_app) = crate::ANDROID_APP.get() {
                let text = app.ui_text_content.clone();
                app.last_android_text = text.clone();
                let len = text.chars().count();
                let state = winit::platform::android::activity::input::TextInputState {
                    text: text.clone(),
                    selection: winit::platform::android::activity::input::TextSpan { start: len, end: len },
                    compose_region: None,
                };
                android_app.set_text_input_state(state);
                android_app.show_soft_input(true);
            }
        }
        app.on_page_text_focus_pending = false;
    }

    let _ = canvas_response;
}

/// Text content + typography editor. Returns true if any field changed.
fn text_style_panel(app: &mut VadadeeBerryApp, ui: &mut Ui, for_new_text: bool) -> bool {
    let mut changed = false;
    theme::constraint_block(ui, |ui| {
        if for_new_text {
            ui.label(RichText::new("New text").strong());
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Content").small().color(colors::TEXT_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_frame = egui::Frame::NONE
                    .fill(egui::Color32::from_black_alpha(200))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 3));
                btn_frame.show(ui, |ui| {
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new("✔")
                                .color(egui::Color32::from_rgb(0, 230, 118))
                                .strong()
                                .size(12.0)
                        )
                        .frame(false)
                    );
                    if resp.clicked() {
                        ui.ctx().memory_mut(|mem| mem.stop_text_input());
                        #[cfg(target_os = "android")]
                        {
                            if let Some(android_app) = crate::ANDROID_APP.get() {
                                android_app.hide_soft_input(false);
                            }
                        }
                    }
                });
            });
        });
        ui.add_space(4.0);
        changed |= ui
            .add(
                egui::TextEdit::multiline(&mut app.ui_text_content)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("Type here…"),
            )
            .changed();
        if for_new_text {
            ui.label(
                RichText::new("Click on the page — type in the on-page editor")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
    });
    theme::constraint_block(ui, |ui| {
        ui.label(RichText::new("Style").small().color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut app.ui_text_font_size, 8.0..=128.0).text("Size"))
            .changed();
        ui.horizontal(|ui| {
            ui.label("Font");
            let selected = app.ui_text_font_family.clone();
            let families: Vec<String> = app.fonts.families().to_vec();
            egui::ComboBox::from_id_salt("text_font_family")
                .selected_text(&selected)
                .width(180.0)
                .show_ui(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for family in &families {
                                if ui
                                    .selectable_label(&app.ui_text_font_family == family, family)
                                    .clicked()
                                {
                                    app.ui_text_font_family = family.clone();
                                    changed = true;
                                }
                            }
                        });
                });
            if changed {
                app.fonts
                    .ensure_loaded(ui.ctx(), &app.ui_text_font_family);
            }
        });
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut app.ui_text_bold, "Bold").changed();
            changed |= ui.checkbox(&mut app.ui_text_italic, "Italic").changed();
        });
    });
    changed
}

fn node_icon(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Rect { .. } => icons::RECT,
        NodeKind::Polygon { sides, .. } => icons::polygon_icon(*sides),
        NodeKind::Ellipse { rx, ry, .. } => {
            if (rx - ry).abs() < 0.01 {
                icons::CIRCLE
            } else {
                icons::ELLIPSE
            }
        }
        NodeKind::Path { path } => {
            if path.points.len() == 2 && path.verbs == [0, 1] {
                icons::LINE
            } else {
                icons::POLY
            }
        }
        NodeKind::Text { .. } => icons::TEXT,
        NodeKind::Group { .. } => icons::OBJECT,
        NodeKind::Image { .. } => icons::OBJECT,
        NodeKind::Plotter { .. } => icons::PLOTTER,
        NodeKind::Arc { .. } => icons::ARC,
        NodeKind::BrushStroke { .. } => icons::BRUSH,
        NodeKind::FlowchartNode { .. } => icons::RECT,
        NodeKind::FlowchartPath { .. } => icons::LINE,
    }
}

fn constraint_origin(ui: &mut Ui, x: &mut f64, y: &mut f64, changed: &mut bool) {
    theme::constraint_block(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::ORIGIN)
                    .font(nerd_font_id(14.0))
                    .color(colors::TEXT_MUTED),
            );
            ui.label(RichText::new("Origin").strong());
        });
        ui.horizontal(|ui| {
            *changed |= ui.add(decimal_drag(x).prefix("X:")).changed();
            *changed |= ui.add(decimal_drag(y).prefix("Y:")).changed();
        });
    });
}

fn decimal_drag<'a, Num: egui::emath::Numeric>(value: &'a mut Num) -> egui::DragValue<'a> {
    egui::DragValue::new(value).custom_parser(|text| {
        let normalized = text.trim().replace(',', ".");
        normalized.parse::<f64>().ok()
    })
}

/// Render a real-time 3D stylus preview that reacts to pen_angle, tilt_angle, and pressure.
///
/// Mathematical model:
///   - The pen is a cylinder of fixed length, held at `tilt_angle` from the paper surface (0° = flat, 90° = vertical).
///   - `pen_angle` rotates the pen around the vertical axis (azimuth), so the shadow/contact point orbits.
///   - Projection: orthographic with a 30° elevation view angle.
///   - The tip contact point is offset from center using:
///       dx = cos(azimuth) * cos(tilt_rad) * shaft_len
///       dy = sin(azimuth) * cos(tilt_rad) * shaft_len   (in paper-plane)
///       height at base = sin(tilt_rad) * shaft_len       (perspective foreshortening)
///   - `pressure` squashes the footprint ellipse and shifts the tip down.
fn draw_stylus_3d_preview(ui: &mut egui::Ui, pen_angle_deg: f32, tilt_angle_deg: f32, pressure: f32) {
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 150.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        colors::BG_PANEL,
        egui::Stroke::new(1.0, colors::BORDER),
        egui::StrokeKind::Inside,
    );

    let cx = rect.center().x;
    let paper_y = rect.bottom() - 28.0;

    // --- parameters
    let azimuth = pen_angle_deg.to_radians();
    let tilt_rad = tilt_angle_deg.to_radians(); // 0 = flat on paper, π/2 = vertical
    let shaft_len: f32 = 70.0; // visual shaft length in pixels

    // tip sits on paper; base of pen is above-and-behind it
    let tip_x = cx;
    let tip_y = paper_y - pressure * 4.0; // slight sinking with pressure

    // 3D displacement of pen body: elevation view at 30°
    let proj_y_scale: f32 = 0.5_f32.to_radians().cos(); // foreshorten y
    let dx = azimuth.cos() * tilt_rad.cos() * shaft_len;
    let dy = azimuth.sin() * tilt_rad.cos() * shaft_len * proj_y_scale;
    let dz = tilt_rad.sin() * shaft_len; // screen-up component

    let base_x = tip_x - dx;
    let base_y = tip_y - dz + dy * 0.3; // slight backward offset

    // --- paper grid lines (subtle perspective)
    let paper_color = colors::BORDER.gamma_multiply(0.25);
    for i in 0..5i32 {
        let ox = (i - 2) as f32 * 28.0;
        painter.line_segment(
            [egui::pos2(cx + ox - 60.0, paper_y + 12.0), egui::pos2(cx + ox + 60.0, paper_y + 12.0)],
            egui::Stroke::new(1.0, paper_color),
        );
    }
    painter.line_segment(
        [egui::pos2(cx - 80.0, paper_y + 2.0), egui::pos2(cx + 80.0, paper_y + 2.0)],
        egui::Stroke::new(1.5, colors::BORDER.gamma_multiply(0.5)),
    );

    // --- shadow ellipse under tip (contact footprint)
    let shadow_rx = 4.0 + pressure * 3.0;
    let shadow_ry = (2.0 + pressure * 1.5) * (1.0 - tilt_rad.sin() * 0.5);
    let shadow_col = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 60);
    // Draw shadow as a flattened circle using multiple concentric arcs approximation
    for r_frac in [1.0f32, 0.7, 0.4] {
        painter.circle_filled(
            egui::pos2(tip_x, paper_y + 4.0 + shadow_ry * (1.0 - r_frac)),
            shadow_rx * r_frac,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, (shadow_col.a() as f32 * r_frac * 0.5) as u8),
        );
    }

    // --- pen shaft (thick line with gradient approximation via two lines)
    let pen_col = egui::Color32::from_rgb(70, 130, 200);
    let pen_col_hi = egui::Color32::from_rgb(130, 185, 240);
    painter.line_segment(
        [egui::pos2(base_x - 1.0, base_y), egui::pos2(tip_x - 1.0, tip_y)],
        egui::Stroke::new(5.0, pen_col),
    );
    painter.line_segment(
        [egui::pos2(base_x + 1.0, base_y), egui::pos2(tip_x + 1.0, tip_y)],
        egui::Stroke::new(2.0, pen_col_hi),
    );
    // pen tip cone
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(tip_x, tip_y),
            egui::pos2(tip_x + 4.0, tip_y + 6.0),
            egui::pos2(tip_x - 4.0, tip_y + 6.0),
        ],
        egui::Color32::from_rgb(200, 170, 90),
        egui::Stroke::NONE,
    ));
    // pen cap (eraser end)
    painter.circle_filled(egui::pos2(base_x, base_y), 5.0, egui::Color32::from_rgb(220, 60, 60));

    // --- angle info label
    let font = egui::FontId::new(9.0, egui::FontFamily::Proportional);
    painter.text(
        egui::pos2(rect.left() + 6.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        format!("Angle: {:.0}° | Tilt: {:.0}° | Pressure: {:.2}", pen_angle_deg, tilt_angle_deg, pressure),
        font,
        colors::TEXT_MUTED,
    );
}

fn draw_3d_pen_tip(ui: &mut egui::Ui, active_width: f32, is_drawing: bool) {

    let (rect, _response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 130.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background card styling
    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        colors::BG_PANEL,
        egui::Stroke::new(1.0, colors::BORDER),
        egui::StrokeKind::Inside,
    );

    let cx = rect.center().x;
    let paper_y = rect.top() + 90.0;
    
    // Lively pressure parameter
    let pressure = if is_drawing {
        (active_width / 100.0).clamp(0.05, 1.0)
    } else {
        0.15
    };

    // Calculate tip position and footprint size
    let base_y = rect.top() + 15.0;
    let tip_y = paper_y - 12.0 + (pressure * 16.0); // Tip sinks down with pressure
    
    // Draw background grid lines on the "paper" (vanishing point perspective)
    let paper_color = colors::BORDER.gamma_multiply(0.3);
    for offset in [-60.0, -30.0, 0.0, 30.0, 60.0] {
        let vp_y = rect.top() + 35.0;
        let x0 = cx + offset * 0.2;
        let x1 = cx + offset * 1.5;
        painter.line_segment(
            [egui::pos2(x0, vp_y + 10.0), egui::pos2(x1, rect.bottom() - 5.0)],
            egui::Stroke::new(1.0, paper_color),
        );
    }
    // Horizontal perspective lines
    for py in [90.0, 100.0, 110.0, 120.0] {
        let y_coord = rect.top() + py;
        let width_factor = (py - 35.0) / 55.0;
        let x_span = 80.0 * width_factor;
        painter.line_segment(
            [egui::pos2(cx - x_span, y_coord), egui::pos2(cx + x_span, y_coord)],
            egui::Stroke::new(1.0, paper_color),
        );
    }

    // Draw the pen tip cone
    let pen_color = colors::ACCENT;
    let pen_shaft_w = 14.0;
    let pen_tip_w = 3.0 + pressure * 6.0;

    // Draw pen shaft
    let points = vec![
        egui::pos2(cx - pen_shaft_w, base_y),
        egui::pos2(cx + pen_shaft_w, base_y),
        egui::pos2(cx + pen_tip_w, tip_y),
        egui::pos2(cx - pen_tip_w, tip_y),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        colors::BG_ELEVATED,
        egui::Stroke::new(1.5, colors::BORDER),
    ));

    // Highlight on pen shaft for 3D look
    painter.line_segment(
        [egui::pos2(cx - pen_shaft_w * 0.3, base_y), egui::pos2(cx - pen_tip_w * 0.3, tip_y)],
        egui::Stroke::new(2.5, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
    );

    // Draw the dome tip
    let tip_radius = 2.0 + pressure * 5.0;
    painter.circle(
        egui::pos2(cx, tip_y),
        tip_radius,
        pen_color,
        egui::Stroke::new(1.0, colors::BORDER),
    );

    // Translucent depth illusion: overlay a semi-transparent paper layer below paper_y
    let paper_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, paper_y),
        egui::pos2(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(
        paper_rect,
        egui::CornerRadius::ZERO,
        colors::BG_PANEL.gamma_multiply(0.85),
    );

    // Draw the paper horizon line
    painter.line_segment(
        [egui::pos2(rect.left() + 5.0, paper_y), egui::pos2(rect.right() - 5.0, paper_y)],
        egui::Stroke::new(1.5, colors::BORDER),
    );

    // Draw footprint/shadow ellipse on the paper (at paper_y)
    let footprint_rx = 2.0 + pressure * 14.0;
    let footprint_ry = 1.0 + pressure * 7.0;
    painter.add(egui::Shape::ellipse_filled(
        egui::pos2(cx, paper_y),
        egui::vec2(footprint_rx, footprint_ry),
        colors::ACCENT.gamma_multiply(0.5),
    ));
    painter.add(egui::Shape::ellipse_stroke(
        egui::pos2(cx, paper_y),
        egui::vec2(footprint_rx, footprint_ry),
        egui::Stroke::new(1.5, colors::ACCENT),
    ));
}

fn draw_3d_calligraphy_nib(ui: &mut egui::Ui, active_width: f32, is_drawing: bool) {
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 130.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background card styling
    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        colors::BG_PANEL,
        egui::Stroke::new(1.0, colors::BORDER),
        egui::StrokeKind::Inside,
    );

    let cx = rect.center().x;
    let paper_y = rect.top() + 90.0;
    
    // Lively pressure parameter
    let pressure = if is_drawing {
        (active_width / 100.0).clamp(0.05, 1.0)
    } else {
        0.15
    };

    // Calculate tip position and footprint size
    let base_y = rect.top() + 15.0;
    let tip_y = paper_y - 12.0 + (pressure * 16.0); // Tip sinks down with pressure
    
    // Draw background grid lines on the "paper" (vanishing point perspective)
    let paper_color = colors::BORDER.gamma_multiply(0.3);
    for offset in [-60.0, -30.0, 0.0, 30.0, 60.0] {
        let vp_y = rect.top() + 35.0;
        let x0 = cx + offset * 0.2;
        let x1 = cx + offset * 1.5;
        painter.line_segment(
            [egui::pos2(x0, vp_y + 10.0), egui::pos2(x1, rect.bottom() - 5.0)],
            egui::Stroke::new(1.0, paper_color),
        );
    }
    // Horizontal perspective lines
    for py in [90.0, 100.0, 110.0, 120.0] {
        let y_coord = rect.top() + py;
        let width_factor = (py - 35.0) / 55.0;
        let x_span = 80.0 * width_factor;
        painter.line_segment(
            [egui::pos2(cx - x_span, y_coord), egui::pos2(cx + x_span, y_coord)],
            egui::Stroke::new(1.0, paper_color),
        );
    }

    // Draw the pen shaft tapering down to a chisel tip holder
    let pen_shaft_w = 14.0;
    let shaft_points = vec![
        egui::pos2(cx - pen_shaft_w, base_y),
        egui::pos2(cx + pen_shaft_w, base_y),
        egui::pos2(cx + 8.0, tip_y - 15.0),
        egui::pos2(cx - 8.0, tip_y - 15.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        shaft_points,
        colors::BG_ELEVATED,
        egui::Stroke::new(1.5, colors::BORDER),
    ));

    // Highlight on pen shaft for 3D look
    painter.line_segment(
        [egui::pos2(cx - pen_shaft_w * 0.3, base_y), egui::pos2(cx - 2.4, tip_y - 15.0)],
        egui::Stroke::new(2.5, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
    );

    // Calligraphy metal nib: thin flat angled prism slanted at 45 degrees
    let metal_color = egui::Color32::from_rgb(220, 200, 150); // Gold-like metallic
    let slant_dx = 8.0 + pressure * 4.0;
    let slant_dy = 4.0 + pressure * 2.0;
    let tip_left = egui::pos2(cx - slant_dx, tip_y + slant_dy);
    let tip_right = egui::pos2(cx + slant_dx, tip_y - slant_dy);
    
    // Top of the metal nib:
    let top_left = egui::pos2(cx - 6.0, tip_y - 15.0);
    let top_right = egui::pos2(cx + 6.0, tip_y - 15.0);

    // Front face of the flat nib:
    painter.add(egui::Shape::convex_polygon(
        vec![top_left, top_right, tip_right, tip_left],
        metal_color,
        egui::Stroke::new(1.0, colors::BORDER),
    ));

    // Slit down the center of the nib
    painter.line_segment(
        [egui::pos2(cx, tip_y - 12.0), egui::pos2(cx, tip_y)],
        egui::Stroke::new(1.0, colors::BORDER),
    );
    // Breather hole
    painter.circle_filled(
        egui::pos2(cx, tip_y - 12.0),
        1.5,
        colors::BG_PANEL,
    );

    // Translucent depth illusion: overlay a semi-transparent paper layer below paper_y
    let paper_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, paper_y),
        egui::pos2(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(
        paper_rect,
        egui::CornerRadius::ZERO,
        colors::BG_PANEL.gamma_multiply(0.85),
    );

    // Draw the paper horizon line
    painter.line_segment(
        [egui::pos2(rect.left() + 5.0, paper_y), egui::pos2(rect.right() - 5.0, paper_y)],
        egui::Stroke::new(1.5, colors::BORDER),
    );

    // Draw footprint/shadow of the slanted calligraphy nib on the paper:
    let footprint_l = egui::pos2(cx - slant_dx, paper_y + slant_dy * 0.5);
    let footprint_r = egui::pos2(cx + slant_dx, paper_y - slant_dy * 0.5);
    painter.line_segment(
        [footprint_l, footprint_r],
        egui::Stroke::new(2.5 + pressure * 4.0, colors::ACCENT.gamma_multiply(0.7)),
    );
}

struct TrackPlotInfo<'a> {
    label: &'static str,
    track: &'a mut crate::app::KeyframeTrack,
    color: egui::Color32,
    default_val: f64,
}

fn draw_timeline_track(
    ui: &mut egui::Ui,
    track_label: &str,
    node_id: Option<crate::document::NodeId>,
    plots: &mut [TrackPlotInfo<'_>],
    current_frame: &mut usize,
    timeline_scroll: &mut f32,
    timeline_follow: &mut bool,
    content_max_frame: usize,
    edit_mode: bool,
    dragged_keyframe: &mut Option<(crate::document::NodeId, String, usize)>,
    selected_keyframe: &mut Option<(crate::document::NodeId, String, usize)>,
    graph_editor_track: &mut Option<(crate::document::NodeId, String)>,
    graph_editor_target_track: &mut Option<(crate::document::NodeId, String)>,
    visible_frames: f32,
) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.allocate_ui(egui::vec2(60.0, 32.0), |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(track_label).strong().color(colors::TEXT_MUTED));
            });
        });
        
        let track_width = ui.available_width() - 8.0;
        let track_height = 32.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(track_width, track_height),
            egui::Sense::click_and_drag()
        );
        
        let painter = ui.painter_at(rect);
        
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(4),
            colors::BG_DEEP,
        );
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0, colors::BORDER),
            egui::StrokeKind::Inside,
        );
        
        let start_frame = *timeline_scroll;
        let visible_frames = visible_frames.max(10.0);
        let end_frame = start_frame + visible_frames;
        
        // Draw vertical grid lines every 10 frames in the visible range
        let grid_start = ((start_frame / 10.0).floor() * 10.0) as i32;
        let grid_end = (end_frame / 10.0).ceil() as i32 * 10;
        
        for f in (grid_start..=grid_end).step_by(10) {
            if f >= 0 {
                let frac = (f as f32 - start_frame) / visible_frames;
                if frac >= 0.0 && frac <= 1.0 {
                    let x = rect.left() + frac * rect.width();
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.0, colors::BORDER.gamma_multiply(0.3)),
                    );
                    
                    if f % 20 == 0 {
                        let font = egui::FontId::new(9.0, egui::FontFamily::Proportional);
                        painter.text(
                            egui::pos2(x, rect.top() + 2.0),
                            egui::Align2::CENTER_TOP,
                            f.to_string(),
                            font,
                            colors::TEXT_MUTED.gamma_multiply(0.5),
                        );
                    }
                }
            }
        }
        
        let padding = 6.0;
        // Compute min and max values across all plots to scale y-axis
        let mut val_min = f64::MAX;
        let mut val_max = f64::MIN;
        let mut has_any_kf = false;
        for plot in plots.iter() {
            if !plot.track.keyframes.is_empty() {
                has_any_kf = true;
                for kf in &plot.track.keyframes {
                    val_min = val_min.min(kf.value);
                    val_max = val_max.max(kf.value);
                    if kf.interpolation == crate::app::InterpolationMode::Bezier {
                        val_min = val_min.min(kf.value + kf.handle_right.1);
                        val_max = val_max.max(kf.value + kf.handle_right.1);
                        val_min = val_min.min(kf.value + kf.handle_left.1);
                        val_max = val_max.max(kf.value + kf.handle_left.1);
                    }
                }
            }
        }
        if !has_any_kf || val_min >= val_max {
            if has_any_kf {
                val_min = val_min - 50.0;
                val_max = val_max + 50.0;
            } else {
                val_min = 0.0;
                val_max = 100.0;
            }
        } else {
            let span = val_max - val_min;
            val_min -= span * 0.25;
            val_max += span * 0.25;
        }
        
        // Keyframe dragging/shifting in edit mode
        if edit_mode {
            let mut drag_to_apply = None; // (plot_label, orig_frame, target_frame)
            
            if let (Some(n_id), Some((drag_n_id, drag_lbl, drag_orig_frame))) = (node_id, dragged_keyframe.clone()) {
                if n_id == drag_n_id {
                    if ui.input(|i| i.pointer.any_down()) {
                        if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                            let relative_x = mpos.x - rect.left();
                            let raw_frame = start_frame + (relative_x / rect.width() * visible_frames);
                            let target_frame = raw_frame.round().max(0.0) as usize;
                            
                            if target_frame != drag_orig_frame {
                                drag_to_apply = Some((drag_lbl.clone(), drag_orig_frame, target_frame));
                            }
                        }
                    } else {
                        *dragged_keyframe = None;
                    }
                }
            }
            
            // Check if we need to start a new drag
            if dragged_keyframe.is_none() {
                if let Some(n_id) = node_id {
                    for plot in plots.iter() {
                        for kf in &plot.track.keyframes {
                            let kf_frame = kf.frame as f32;
                            if kf_frame >= start_frame && kf_frame <= end_frame {
                                let frac_x = (kf_frame - start_frame) / visible_frames;
                                let kf_x = rect.left() + frac_x * rect.width();
                                let frac_y = (kf.value - val_min) / (val_max - val_min);
                                let kf_y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
                                let center = egui::pos2(kf_x, kf_y);
                                
                                let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                                let is_hovered = if let Some(mpos) = mouse_pos {
                                    mpos.distance(center) < 8.0
                                } else {
                                    false
                                };
                                
                                if is_hovered && ui.input(|i| i.pointer.any_pressed()) {
                                    *dragged_keyframe = Some((n_id, plot.label.to_string(), kf.frame));
                                    *selected_keyframe = Some((n_id, plot.label.to_string(), kf.frame));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            
            // Apply frame shift
            if let Some((lbl, orig_f, target_f)) = drag_to_apply {
                for plot in plots.iter_mut() {
                    if plot.label == lbl {
                        plot.track.keyframes.retain(|kf| kf.frame != target_f || kf.frame == orig_f);
                        if let Some(pos) = plot.track.keyframes.iter().position(|kf| kf.frame == orig_f) {
                            plot.track.keyframes[pos].frame = target_f;
                            plot.track.keyframes.sort_by_key(|kf| kf.frame);
                            if let Some((_, _, drag_f)) = dragged_keyframe.as_mut() {
                                *drag_f = target_f;
                            }
                        }
                    }
                }
            }
        }
        
        // Draw linear lines between keyframes
        for plot in plots.iter() {
            let mut pts = Vec::new();
            for f in grid_start..=grid_end {
                if f >= 0 {
                    let val = plot.track.interpolate(f as usize).unwrap_or(plot.default_val);
                    let frac_x = (f as f32 - start_frame) / visible_frames;
                    let x = rect.left() + frac_x * rect.width();
                    let frac_y = (val - val_min) / (val_max - val_min);
                    let y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
                    pts.push(egui::pos2(x, y));
                }
            }
            if pts.len() > 1 {
                for window in pts.windows(2) {
                    painter.line_segment([window[0], window[1]], egui::Stroke::new(1.5, plot.color));
                }
            }
        }
        
        // Draw keyframe points (circles)
        for plot in plots.iter() {
            for kf in &plot.track.keyframes {
                let kf_frame = kf.frame as f32;
                if kf_frame >= start_frame && kf_frame <= end_frame {
                    let frac_x = (kf_frame - start_frame) / visible_frames;
                    let kf_x = rect.left() + frac_x * rect.width();
                    let frac_y = (kf.value - val_min) / (val_max - val_min);
                    let kf_y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
                    let center = egui::pos2(kf_x, kf_y);
                    
                    let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                    let is_hovered = if let Some(mpos) = mouse_pos {
                        mpos.distance(center) < 8.0
                    } else {
                        false
                    };
                    
                    let is_being_dragged = if let (Some(n_id), Some((drag_n_id, drag_lbl, drag_orig_frame))) = (node_id, &dragged_keyframe) {
                        n_id == *drag_n_id && plot.label == drag_lbl && kf.frame == *drag_orig_frame
                    } else {
                        false
                    };

                    let is_selected = if let (Some(n_id), Some(&(ref sel_n_id, ref sel_lbl, ref sel_frame))) = (node_id, selected_keyframe.as_ref()) {
                        n_id == *sel_n_id && plot.label == sel_lbl && kf.frame == *sel_frame
                    } else {
                        false
                    };
                    
                    let kf_color = if is_hovered || is_being_dragged {
                        colors::ACCENT
                    } else {
                        plot.color
                    };

                    let stroke_color = if is_selected {
                        colors::ACCENT
                    } else {
                        colors::BG_PANEL
                    };
                    let stroke_w = if is_selected { 2.0 } else { 1.0 };
                    let radius = if is_selected { 6.0 } else { 4.5 };
                    
                    if kf.interpolation == crate::app::InterpolationMode::Bezier {
                        let pts = [
                            egui::pos2(center.x, center.y - radius),
                            egui::pos2(center.x + radius, center.y),
                            egui::pos2(center.x, center.y + radius),
                            egui::pos2(center.x - radius, center.y),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            pts.to_vec(),
                            kf_color,
                            egui::Stroke::new(stroke_w, stroke_color),
                        ));
                    } else {
                        painter.circle(
                            center,
                            radius,
                            kf_color,
                            egui::Stroke::new(stroke_w, stroke_color),
                        );
                    }
                    
                    if edit_mode && is_hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                }
            }
        }
        
        // Draw active frame line (playhead)
        let active_frame_f = *current_frame as f32;
        if active_frame_f >= start_frame && active_frame_f <= end_frame {
            let active_frac = (active_frame_f - start_frame) / visible_frames;
            let playhead_x = rect.left() + active_frac * rect.width();
            painter.line_segment(
                [egui::pos2(playhead_x, rect.top()), egui::pos2(playhead_x, rect.bottom())],
                egui::Stroke::new(1.5, colors::ACCENT),
            );
        }
        
        // Mouse wheel scroll to pan timeline (maps horizontal and vertical wheel scrolling to timeline scroll)
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        let wheel_delta = if scroll_delta.x != 0.0 { scroll_delta.x } else { scroll_delta.y };
        if wheel_delta != 0.0 && response.hovered() {
            *timeline_scroll = (*timeline_scroll - wheel_delta * 0.1).max(0.0);
            *timeline_follow = false;
        }
        
        // Find if a specific plot's keyframe is hovered
        let mut hovered_plot_lbl = None;
        if let Some(_n_id) = node_id {
            for plot in plots.iter() {
                for kf in &plot.track.keyframes {
                    let kf_frame = kf.frame as f32;
                    if kf_frame >= start_frame && kf_frame <= end_frame {
                        let frac_x = (kf_frame - start_frame) / visible_frames;
                        let kf_x = rect.left() + frac_x * rect.width();
                        let frac_y = (kf.value - val_min) / (val_max - val_min);
                        let kf_y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
                        let center = egui::pos2(kf_x, kf_y);
                        
                        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                        if let Some(mpos) = mouse_pos {
                            if mpos.distance(center) < 8.0 {
                                hovered_plot_lbl = Some(plot.label.to_string());
                                break;
                            }
                        }
                    }
                }
                if hovered_plot_lbl.is_some() {
                    break;
                }
            }
        }
        
        // Double-click track to open/toggle graph editor
        let double_clicked_track = response.double_clicked();
        if double_clicked_track {
            if let Some(n_id) = node_id {
                let label = hovered_plot_lbl.unwrap_or_else(|| plots[0].label.to_string());
                let new_track = (n_id, label);
                if let Some(current) = graph_editor_track.as_ref() {
                    if current == &new_track {
                        // Toggle close
                        *graph_editor_track = None;
                    } else {
                        *graph_editor_target_track = Some(new_track);
                    }
                } else {
                    *graph_editor_track = Some(new_track);
                }
            }
        }

        // Drag interaction
        if dragged_keyframe.is_some() {
            // Dragging keyframe: do not scrub playhead or pan
        } else if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
        {
            let delta_x = ui.input(|i| i.pointer.delta().x);
            let frames_pan = delta_x / rect.width() * visible_frames;
            *timeline_scroll = (*timeline_scroll - frames_pan).max(0.0);
            *timeline_follow = false;
        } else if response.dragged_by(egui::PointerButton::Primary) || response.clicked_by(egui::PointerButton::Primary) {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let relative_x = mouse_pos.x - rect.left();
                let raw_frame = start_frame + (relative_x / rect.width() * visible_frames);
                *current_frame = raw_frame.round().max(0.0) as usize;
                // intentionally no .min(content_max) here: allows user to scrub/set frame > current max (e.g. >100) to extend animation
            }
        }
    });
}

fn timeline_interior(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    app.sync_stale_media_layer_durations();
    // Ghost End frames come from keyframes on deleted objects.
    let _ = app.prune_orphan_animation_tracks();
    let content_max_frame = app.get_max_animation_frame();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("ANIMATION TIMELINE").strong().color(colors::ACCENT));
            
            // Align "Edit mode" button to top-center
            let width_left = ui.available_width();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space((width_left * 0.5 - 75.0).max(0.0));
                
                let edit_color = if app.anim_edit_mode { colors::POWERLINE_C } else { colors::TEXT_MUTED };
                let btn_edit = ui.toggle_value(&mut app.anim_edit_mode, RichText::new("Edit mode").strong().color(edit_color));
                btn_edit.on_hover_text("Edit properties of the keyframe at the current frame in the sidebar");
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.anim_keyframing_mode {
                    let time = ui.input(|i| i.time);
                    let rec_color = if (time * 2.0).sin() > 0.0 {
                        egui::Color32::from_rgb(255, 80, 80)
                    } else {
                        colors::TEXT_MUTED
                    };
                    ui.label(RichText::new("● REC").strong().color(rec_color));
                } else {
                    ui.label(RichText::new("Idle").color(colors::TEXT_MUTED));
                }
                
                ui.label(RichText::new(format!("Frame {}", app.anim_current_frame)).color(colors::TEXT));

                ui.add_space(8.0);
                let mut fps = app.anim_fps;
                if ui.add(egui::DragValue::new(&mut fps).range(1..=120).suffix(" fps")).changed() {
                    app.anim_fps = fps;
                }
                ui.label(RichText::new("Speed:").color(colors::TEXT_MUTED));
                ui.add_space(8.0);

                let mut apply_anim_after = false;
                let mut before_timeline = None;
                if let Some((n_id, track_lbl, frame)) = app.anim_selected_keyframe.clone() {
                    before_timeline = Some(app.project.anim_timeline.clone());
                    let mut interp_changed = None;
                    if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&n_id) {
                        if let Some(track) = anim.get_track_mut(&track_lbl) {
                            if let Some(idx) = track.keyframes.iter().position(|k| k.frame == frame) {
                                let next_kf_val = track.keyframes.iter()
                                    .find(|k| k.frame > frame)
                                    .map(|k| (k.frame, k.value));
                                
                                let kf = &mut track.keyframes[idx];
                                let mut selected_mode = kf.interpolation;
                                ui.add_space(8.0);
                                egui::ComboBox::from_id_salt("kf_interp_combo")
                                    .selected_text(match selected_mode {
                                        crate::app::InterpolationMode::Linear => "Linear",
                                        crate::app::InterpolationMode::Bezier => "Bezier/Smooth",
                                    })
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(&mut selected_mode, crate::app::InterpolationMode::Linear, "Linear").clicked() {
                                            interp_changed = Some(crate::app::InterpolationMode::Linear);
                                        }
                                        if ui.selectable_value(&mut selected_mode, crate::app::InterpolationMode::Bezier, "Bezier/Smooth").clicked() {
                                            interp_changed = Some(crate::app::InterpolationMode::Bezier);
                                        }
                                    });
                                if let Some(new_mode) = interp_changed {
                                    kf.interpolation = new_mode;
                                    if new_mode == crate::app::InterpolationMode::Bezier {
                                        if let Some((next_frame, next_value)) = next_kf_val {
                                            kf.handle_right = (
                                                (next_frame - kf.frame) as f64 * 0.5,
                                                (next_value - kf.value) * 0.5
                                            );
                                        } else {
                                            kf.handle_right = (5.0, 0.0);
                                        }
                                    }
                                    apply_anim_after = true;
                                }
                                ui.label(RichText::new(format!("Keyframe (Frame {}):", frame)).color(colors::TEXT_MUTED));
                            }
                        }
                    }
                }
                if apply_anim_after {
                    if let Some(before) = before_timeline {
                        let after_timeline = app.project.anim_timeline.clone();
                        app.history.push(
                            &mut app.project,
                            crate::history::ProjectEdit::PatchTimeline {
                                before,
                                after: after_timeline,
                            },
                        );
                        app.apply_animation_for_frame(app.anim_current_frame);
                    }
                }
            });
        });
        
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let mut curr_frame = app.anim_current_frame; // no min(content); support high frames >100
        let mut scroll = app.anim_timeline_scroll;

        // Auto-follow playhead: scroll so the playhead stays in the middle 70% of the timeline viewport
        if app.anim_timeline_follow {
            let left_boundary = scroll + 15.0;
            let right_boundary = scroll + 85.0;
            let current = curr_frame as f32;
            if current < left_boundary {
                scroll = (current - 15.0).max(0.0);
            } else if current > right_boundary {
                scroll = (current - 85.0).max(0.0);
            }
        }

        // --- HORIZONTAL TIMELINE SCROLL & RULER ---
        ui.horizontal(|ui| {
            // Frame number indicator (slider removed as scroll is done via drag/grab and wheel)
            ui.label(RichText::new(format!("Current Frame: {}", curr_frame)).strong().color(colors::TEXT));
            let content_secs = (content_max_frame + 1) as f32 / app.anim_fps.max(1) as f32;
            ui.label(
                RichText::new(format!(
                    "End: frame {content_max_frame} ({content_secs:.2}s)"
                ))
                .color(colors::TEXT_MUTED)
                .small(),
            );
            ui.add_space(8.0);
            ui.label(RichText::new("Frame width").small().color(colors::TEXT_MUTED));
            let mut vis = app.anim_timeline_visible_frames.max(10.0);
            if ui
                .add(
                    egui::DragValue::new(&mut vis)
                        .range(10.0..=5000.0)
                        .speed(2.0)
                        .suffix(" frames"),
                )
                .on_hover_text("How many frames the animation timeline shows (time axis zoom)")
                .changed()
            {
                app.anim_timeline_visible_frames = vis;
            }
            ui.add_space(8.0);
            ui.checkbox(&mut app.anim_timeline_follow, "Follow Playhead");
        });
        ui.add_space(4.0);
        
        // Draw progress ruler bar (aligned with tracks)
        let ruler_width = ui.available_width() - 8.0;
        let ruler_height = 24.0;
        ui.horizontal(|ui| {
            ui.add_space(64.0); // Perfect alignment with tracks
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2((ruler_width - 64.0).max(10.0), ruler_height),
                egui::Sense::click_and_drag()
            );
            
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, colors::BG_DEEP.gamma_multiply(0.5));
            painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, colors::BORDER.gamma_multiply(0.5)), egui::StrokeKind::Inside);
            
            let start_frame = scroll;
            let visible_frames = app.anim_timeline_visible_frames.max(10.0);
            let end_frame = start_frame + visible_frames;
            
            let grid_start = ((start_frame / 10.0).floor() * 10.0) as i32;
            let grid_end = (end_frame / 10.0).ceil() as i32 * 10;
            
            // Draw ticks & numbers
            for f in grid_start..=grid_end {
                if f >= 0 {
                    let frac = (f as f32 - start_frame) / visible_frames;
                    if frac >= 0.0 && frac <= 1.0 {
                        let x = rect.left() + frac * rect.width();
                        let is_major = f % 10 == 0;
                        let tick_h = if is_major { 10.0 } else { 5.0 };
                        painter.line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + tick_h)],
                            egui::Stroke::new(1.0, colors::TEXT_MUTED.gamma_multiply(0.7)),
                        );
                        if is_major {
                            painter.text(
                                egui::pos2(x, rect.top() + 10.0),
                                egui::Align2::CENTER_TOP,
                                f.to_string(),
                                egui::FontId::new(9.0, egui::FontFamily::Proportional),
                                colors::TEXT_MUTED,
                            );
                        }
                    }
                }
            }
            
            // Mouse wheel scroll to pan timeline (maps horizontal and vertical wheel scrolling to timeline scroll)
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
            let wheel_delta = if scroll_delta.x != 0.0 { scroll_delta.x } else { scroll_delta.y };
            if wheel_delta != 0.0 && response.hovered() {
                scroll = (scroll - wheel_delta * 0.1).max(0.0);
            }

            // Handle scrubbing/clicking/dragging to change frame or pan
            if response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
            {
                let delta_x = ui.input(|i| i.pointer.delta().x);
                scroll = (scroll - (delta_x / rect.width() * visible_frames)).max(0.0);
            } else if response.clicked_by(egui::PointerButton::Primary) || response.dragged_by(egui::PointerButton::Primary) {
                if let Some(mpos) = response.interact_pointer_pos() {
                    let frac = ((mpos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let target_frame = (start_frame + frac * visible_frames).round() as usize;
                    curr_frame = target_frame; // allow beyond content max to set frames >100 etc.
                    app.apply_animation_for_frame(curr_frame);
                }
            }
            
            // Draw current frame playhead indicator
            let current_frac = (curr_frame as f32 - start_frame) / visible_frames;
            if current_frac >= 0.0 && current_frac <= 1.0 {
                let px = rect.left() + current_frac * rect.width();
                let size = 6.0;
                let pts = vec![
                    egui::pos2(px - size, rect.top()),
                    egui::pos2(px + size, rect.top()),
                    egui::pos2(px, rect.top() + size * 1.5),
                ];
                painter.add(egui::Shape::convex_polygon(pts, colors::ACCENT, egui::Stroke::NONE));
                painter.line_segment(
                    [egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())],
                    egui::Stroke::new(1.5, colors::ACCENT),
                );
            }
        });
        ui.add_space(6.0);

        let mut dragged = app.anim_dragged_keyframe.clone();
        let mut post_selected_kf = app.anim_selected_keyframe.clone();
        let mut post_graph_track = app.anim_graph_editor_track.clone();
        let mut post_target_track = app.anim_graph_editor_target_track.clone();

        egui::ScrollArea::vertical()
            .max_height(90.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let edit_mode = app.anim_edit_mode;
                if let Some(node_id) = app.selection.first().copied() {
                    let mut temp_selected_kf = post_selected_kf.clone();
                    let mut temp_graph_track = post_graph_track.clone();
                    let mut temp_target_track = post_target_track.clone();
                    let geom_floats = app.get_node_geom_floats(node_id);

                    if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) {
                        let selected_point_indices: Vec<usize> = if app.tools.active == ToolKind::Node {
                            app.tools.select.selected_path_points
                                .iter()
                                .filter(|(pid, _)| *pid == node_id)
                                .map(|(_, pi)| *pi)
                                .collect()
                        } else {
                            vec![]
                        };

                        // Determine which tracks have keyframes
                        let has_pos = !anim.pos_x.keyframes.is_empty() || !anim.pos_y.keyframes.is_empty();
                        let has_rot = !anim.rotation.keyframes.is_empty();
                        let has_op = !anim.opacity.keyframes.is_empty();
                        let has_col = !anim.color_r.keyframes.is_empty() 
                            || !anim.color_g.keyframes.is_empty() 
                            || !anim.color_b.keyframes.is_empty() 
                            || !anim.color_a.keyframes.is_empty();
                        let has_stroke_w = !anim.stroke_width.keyframes.is_empty();
                        let has_stroke_col = !anim.stroke_r.keyframes.is_empty()
                            || !anim.stroke_g.keyframes.is_empty()
                            || !anim.stroke_b.keyframes.is_empty()
                            || !anim.stroke_a.keyframes.is_empty();
                        
                        ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                        
                        let is_output_proxy = app
                            .project
                            .document
                            .ne_output_proxy_layer_index(node_id)
                            .is_some();
                        let pos_label = if is_output_proxy {
                            "Output Position"
                        } else {
                            "Position"
                        };
                        let rot_label = if is_output_proxy {
                            "Output Rotation"
                        } else {
                            "Rotation"
                        };

                        if has_pos {
                            let mut plots = vec![
                                TrackPlotInfo {
                                    label: "pos_x",
                                    track: &mut anim.pos_x,
                                    color: egui::Color32::from_rgb(0, 200, 0), // green
                                    default_val: 0.0,
                                },
                                TrackPlotInfo {
                                    label: "pos_y",
                                    track: &mut anim.pos_y,
                                    color: egui::Color32::from_rgb(200, 0, 0), // red
                                    default_val: 0.0,
                                },
                            ];
                            draw_timeline_track(
                                ui,
                                pos_label,
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }
                        
                        if has_rot {
                            let mut plots = vec![
                                TrackPlotInfo {
                                    label: "rotation",
                                    track: &mut anim.rotation,
                                    color: colors::ACCENT,
                                    default_val: 0.0,
                                },
                            ];
                            draw_timeline_track(
                                ui,
                                rot_label,
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }
                        
                        if has_op {
                            let mut plots = vec![
                                TrackPlotInfo {
                                    label: "opacity",
                                    track: &mut anim.opacity,
                                    color: egui::Color32::from_rgb(150, 150, 150),
                                    default_val: 1.0,
                                },
                            ];
                            draw_timeline_track(
                                ui,
                                "Opacity",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }
                        
                        if has_col {
                            let mut plots = vec![
                                TrackPlotInfo {
                                    label: "color_r",
                                    track: &mut anim.color_r,
                                    color: egui::Color32::from_rgb(255, 100, 100),
                                    default_val: 1.0,
                                },
                                TrackPlotInfo {
                                    label: "color_g",
                                    track: &mut anim.color_g,
                                    color: egui::Color32::from_rgb(100, 255, 100),
                                    default_val: 1.0,
                                },
                                TrackPlotInfo {
                                    label: "color_b",
                                    track: &mut anim.color_b,
                                    color: egui::Color32::from_rgb(100, 100, 255),
                                    default_val: 1.0,
                                },
                            ];
                            draw_timeline_track(
                                ui,
                                "Fill Color",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }

                        if has_stroke_w {
                            let mut plots = vec![TrackPlotInfo {
                                label: "stroke_width",
                                track: &mut anim.stroke_width,
                                color: egui::Color32::from_rgb(200, 160, 80),
                                default_val: 2.0,
                            }];
                            draw_timeline_track(
                                ui,
                                "Stroke Width",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }

                        if has_stroke_col {
                            let mut plots = vec![
                                TrackPlotInfo {
                                    label: "stroke_r",
                                    track: &mut anim.stroke_r,
                                    color: egui::Color32::from_rgb(220, 80, 80),
                                    default_val: 0.1,
                                },
                                TrackPlotInfo {
                                    label: "stroke_g",
                                    track: &mut anim.stroke_g,
                                    color: egui::Color32::from_rgb(80, 220, 80),
                                    default_val: 0.1,
                                },
                                TrackPlotInfo {
                                    label: "stroke_b",
                                    track: &mut anim.stroke_b,
                                    color: egui::Color32::from_rgb(80, 80, 220),
                                    default_val: 0.18,
                                },
                            ];
                            draw_timeline_track(
                                ui,
                                "Stroke Color",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                &mut app.anim_timeline_follow,
                                content_max_frame,
                                edit_mode,
                                &mut dragged,
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                                app.anim_timeline_visible_frames,
                            );
                        }

                        // Grouped geom tracks (Path X/Y merges + filter to selected pts when using Node tool; cap via container ScrollArea)
                        static GEOM_LABELS: &[&str] = &[
                            "geom_0", "geom_1", "geom_2", "geom_3", "geom_4", "geom_5", "geom_6", "geom_7", "geom_8", "geom_9",
                            "geom_10", "geom_11", "geom_12", "geom_13", "geom_14", "geom_15", "geom_16", "geom_17", "geom_18", "geom_19",
                            "geom_20", "geom_21", "geom_22", "geom_23", "geom_24", "geom_25", "geom_26", "geom_27", "geom_28", "geom_29",
                            "geom_30", "geom_31", "geom_32", "geom_33", "geom_34", "geom_35", "geom_36", "geom_37", "geom_38", "geom_39",
                            "geom_40", "geom_41", "geom_42", "geom_43", "geom_44", "geom_45", "geom_46", "geom_47", "geom_48", "geom_49",
                            "geom_50", "geom_51", "geom_52", "geom_53", "geom_54", "geom_55", "geom_56", "geom_57", "geom_58", "geom_59",
                            "geom_60", "geom_61", "geom_62", "geom_63", "geom_64", "geom_65", "geom_66", "geom_67", "geom_68", "geom_69",
                            "geom_70", "geom_71", "geom_72", "geom_73", "geom_74", "geom_75", "geom_76", "geom_77", "geom_78", "geom_79",
                            "geom_80", "geom_81", "geom_82", "geom_83", "geom_84", "geom_85", "geom_86", "geom_87", "geom_88", "geom_89",
                            "geom_90", "geom_91", "geom_92", "geom_93", "geom_94", "geom_95", "geom_96", "geom_97", "geom_98", "geom_99",
                        ];
                        let has_any_geom_kf = anim.geom_tracks.iter().any(|t| !t.keyframes.is_empty());
                        if has_any_geom_kf {
                            if let Some(node) = app.project.nodes.get(node_id) {
                                match &node.kind {
                                    NodeKind::Path { path } => {
                                        let num_anchors = path.anchor_positions().len();
                                        for pt_idx in 0..num_anchors {
                                            if !selected_point_indices.is_empty() && !selected_point_indices.contains(&pt_idx) {
                                                continue;
                                            }
                                            let pairs: [(usize, &str, egui::Color32, egui::Color32); 3] = [
                                                (0, "Pt {}", egui::Color32::from_rgb(0, 200, 0), egui::Color32::from_rgb(200, 0, 0)),
                                                (2, "Out {}", egui::Color32::from_rgb(0, 200, 200), egui::Color32::from_rgb(200, 0, 200)),
                                                (4, "In {}", egui::Color32::from_rgb(100, 200, 100), egui::Color32::from_rgb(200, 100, 200)),
                                            ];
                                            for (off, label_tmpl, c1, c2) in pairs {
                                                let i1 = pt_idx * 6 + off;
                                                let i2 = i1 + 1;
                                                let len_g = anim.geom_tracks.len();
                                                let has1 = i1 < len_g && !anim.geom_tracks[i1].keyframes.is_empty();
                                                let has2 = i2 < len_g && !anim.geom_tracks[i2].keyframes.is_empty();
                                                if !has1 && !has2 { continue; }
                                                let mut plots = vec![];
                                                if has1 || has2 {
                                                    let (left, right) = anim.geom_tracks.split_at_mut(i2);
                                                    if has1 {
                                                        let lbl = if i1 < GEOM_LABELS.len() { GEOM_LABELS[i1] } else { "geom_unknown" };
                                                        plots.push(TrackPlotInfo {
                                                            label: lbl,
                                                            track: &mut left[i1],
                                                            color: c1,
                                                            default_val: if i1 < geom_floats.len() { geom_floats[i1] } else { 0.0 },
                                                        });
                                                    }
                                                    if has2 {
                                                        let lbl = if i2 < GEOM_LABELS.len() { GEOM_LABELS[i2] } else { "geom_unknown" };
                                                        plots.push(TrackPlotInfo {
                                                            label: lbl,
                                                            track: &mut right[0],
                                                            color: c2,
                                                            default_val: if i2 < geom_floats.len() { geom_floats[i2] } else { 0.0 },
                                                        });
                                                    }
                                                }
                                                if !plots.is_empty() {
                                                    let tname = label_tmpl.replace("{}", &pt_idx.to_string());
                                                    draw_timeline_track(
                                                        ui,
                                                        &tname,
                                                        Some(node_id),
                                                        &mut plots,
                                                        &mut curr_frame,
                                                        &mut scroll,
                                                        &mut app.anim_timeline_follow,
                                                        content_max_frame,
                                                        edit_mode,
                                                        &mut dragged,
                                                        &mut temp_selected_kf,
                                                        &mut temp_graph_track,
                                                        &mut temp_target_track,
                                                    app.anim_timeline_visible_frames,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    NodeKind::BrushStroke { points } => {
                                        let num_pts = points.len();
                                        for pt_idx in 0..num_pts {
                                            // X/Y as one row
                                            let i1 = pt_idx * 3;
                                            let i2 = i1 + 1;
                                            let len_g = anim.geom_tracks.len();
                                            let has1 = i1 < len_g && !anim.geom_tracks[i1].keyframes.is_empty();
                                            let has2 = i2 < len_g && !anim.geom_tracks[i2].keyframes.is_empty();
                                            if has1 || has2 {
                                                let mut plots = vec![];
                                                let (left, right) = anim.geom_tracks.split_at_mut(i2);
                                                if has1 {
                                                    let lbl = if i1 < GEOM_LABELS.len() { GEOM_LABELS[i1] } else { "geom_unknown" };
                                                    plots.push(TrackPlotInfo {
                                                        label: lbl,
                                                        track: &mut left[i1],
                                                        color: egui::Color32::from_rgb(0, 200, 0),
                                                        default_val: if i1 < geom_floats.len() { geom_floats[i1] } else { 0.0 },
                                                    });
                                                }
                                                if has2 {
                                                    let lbl = if i2 < GEOM_LABELS.len() { GEOM_LABELS[i2] } else { "geom_unknown" };
                                                    plots.push(TrackPlotInfo {
                                                        label: lbl,
                                                        track: &mut right[0],
                                                        color: egui::Color32::from_rgb(200, 0, 0),
                                                        default_val: if i2 < geom_floats.len() { geom_floats[i2] } else { 0.0 },
                                                    });
                                                }
                                                let tname = format!("Stroke {} (X/Y)", pt_idx);
                                                draw_timeline_track(
                                                    ui,
                                                    &tname,
                                                    Some(node_id),
                                                    &mut plots,
                                                    &mut curr_frame,
                                                    &mut scroll,
                                                    &mut app.anim_timeline_follow,
                                                    content_max_frame,
                                                    edit_mode,
                                                    &mut dragged,
                                                    &mut temp_selected_kf,
                                                    &mut temp_graph_track,
                                                    &mut temp_target_track,
                                                app.anim_timeline_visible_frames,
                                                );
                                            }
                                            let iw = pt_idx * 3 + 2;
                                            if iw < anim.geom_tracks.len() && !anim.geom_tracks[iw].keyframes.is_empty() {
                                                let mut plots = vec![TrackPlotInfo {
                                                    label: if iw < GEOM_LABELS.len() { GEOM_LABELS[iw] } else { "geom_unknown" },
                                                    track: &mut anim.geom_tracks[iw],
                                                    color: colors::POWERLINE_C,
                                                    default_val: if iw < geom_floats.len() { geom_floats[iw] } else { 0.0 },
                                                }];
                                                let tname = format!("Stroke {} W", pt_idx);
                                                draw_timeline_track(
                                                    ui,
                                                    &tname,
                                                    Some(node_id),
                                                    &mut plots,
                                                    &mut curr_frame,
                                                    &mut scroll,
                                                    &mut app.anim_timeline_follow,
                                                    content_max_frame,
                                                    edit_mode,
                                                    &mut dragged,
                                                    &mut temp_selected_kf,
                                                    &mut temp_graph_track,
                                                    &mut temp_target_track,
                                                app.anim_timeline_visible_frames,
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        for i in 0..anim.geom_tracks.len() {
                                            if anim.geom_tracks[i].keyframes.is_empty() {
                                                continue;
                                            }
                                            let label = if i < GEOM_LABELS.len() { GEOM_LABELS[i] } else { "geom_unknown" };
                                            let default_val = if i < geom_floats.len() { geom_floats[i] } else { 0.0 };
                                            let is_out = app
                                                .project
                                                .document
                                                .ne_output_proxy_layer_index(node_id)
                                                .is_some();
                                            let track_name = match &node.kind {
                                                NodeKind::Rect { .. } => match i {
                                                    0 => "Width".to_string(),
                                                    1 => "Height".to_string(),
                                                    2 => "Corner Rad".to_string(),
                                                    _ => format!("Geom {}", i),
                                                },
                                                NodeKind::Image { .. } => {
                                                    let base = match i {
                                                        0 => "Width",
                                                        1 => "Height",
                                                        _ => "Geom",
                                                    };
                                                    if is_out {
                                                        format!("Output {base}")
                                                    } else if i <= 1 {
                                                        base.to_string()
                                                    } else {
                                                        format!("Geom {}", i)
                                                    }
                                                }
                                                NodeKind::Ellipse { .. } => match i {
                                                    0 => "Radius X".to_string(),
                                                    1 => "Radius Y".to_string(),
                                                    _ => format!("Geom {}", i),
                                                },
                                                NodeKind::Polygon { .. } => match i {
                                                    0 => "Radius".to_string(),
                                                    1 => "Sides".to_string(),
                                                    _ => format!("Geom {}", i),
                                                },
                                                NodeKind::Arc { .. } => match i {
                                                    0 => "Radius".to_string(),
                                                    1 => "Start Ang".to_string(),
                                                    2 => "Sweep Ang".to_string(),
                                                    _ => format!("Geom {}", i),
                                                },
                                                _ => format!("Geom {}", i),
                                            };
                                            let mut plots = vec![TrackPlotInfo {
                                                label,
                                                track: &mut anim.geom_tracks[i],
                                                color: colors::POWERLINE_C,
                                                default_val,
                                            }];
                                            draw_timeline_track(
                                                ui,
                                                &track_name,
                                                Some(node_id),
                                                &mut plots,
                                                &mut curr_frame,
                                                &mut scroll,
                                                &mut app.anim_timeline_follow,
                                                content_max_frame,
                                                edit_mode,
                                                &mut dragged,
                                                &mut temp_selected_kf,
                                                &mut temp_graph_track,
                                                &mut temp_target_track,
                                            app.anim_timeline_visible_frames,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
                    post_selected_kf = temp_selected_kf;
                    post_graph_track = temp_graph_track;
                    post_target_track = temp_target_track;
                }
            });

        app.anim_selected_keyframe = post_selected_kf;
        app.anim_graph_editor_track = post_graph_track;
        app.anim_graph_editor_target_track = post_target_track;
        app.anim_dragged_keyframe = dragged;

        // no hard min(content_max_frame) to support setting frames > prior max (e.g. 100+)
        if curr_frame != app.anim_current_frame {
            app.anim_current_frame = curr_frame;
        }
        if scroll != app.anim_timeline_scroll {
            app.anim_timeline_scroll = scroll;
        }
    });
}

fn floating_timeline_window(app: &mut VadadeeBerryApp, ctx: &Context, work: Rect) {
    let open_t = app.ui_anim.timeline_t;
    let animating = app.ui_anim.timeline_running;
    if !app.anim_show_timeline_window && !animating && open_t <= 0.001 {
        return;
    }

    let inset = theme::overlay_work_rect(work);
    let gap = theme::chrome_gap() as f32;
    let action_bar_open_amount = app.ui_anim.action_bar_open_t();
    let action_bar_visible_width = app.action_bar_width * action_bar_open_amount;
    let width_reduction = if action_bar_open_amount > 0.001 {
        action_bar_visible_width + gap
    } else {
        0.0
    };
    let max_w = inset.width() - 2.0 * gap - width_reduction;
    let track_count = if let Some(node_id) = app.selection.first().copied() {
        if let Some(anim) = app.project.anim_timeline.nodes.get(&node_id) {
            let has_pos = !anim.pos_x.keyframes.is_empty() || !anim.pos_y.keyframes.is_empty();
            let has_rot = !anim.rotation.keyframes.is_empty();
            let has_op = !anim.opacity.keyframes.is_empty();
            let has_col = !anim.color_r.keyframes.is_empty() 
                || !anim.color_g.keyframes.is_empty() 
                || !anim.color_b.keyframes.is_empty() 
                || !anim.color_a.keyframes.is_empty();
            let has_stroke_w = !anim.stroke_width.keyframes.is_empty();
            let has_stroke_col = !anim.stroke_r.keyframes.is_empty()
                || !anim.stroke_g.keyframes.is_empty()
                || !anim.stroke_b.keyframes.is_empty()
                || !anim.stroke_a.keyframes.is_empty();
            let geom_row_count = if let Some(node) = app.project.nodes.get(node_id) {
                let selected_point_indices: Vec<usize> = if app.tools.active == ToolKind::Node {
                    app.tools.select.selected_path_points
                        .iter()
                        .filter(|(pid, _)| *pid == node_id)
                        .map(|(_, pi)| *pi)
                        .collect()
                } else {
                    vec![]
                };
                match &node.kind {
                    NodeKind::Path { path } => {
                        let n = path.anchor_positions().len();
                        let mut c = 0usize;
                        for pti in 0..n {
                            if !selected_point_indices.is_empty() && !selected_point_indices.contains(&pti) {
                                continue;
                            }
                            for off in [0usize, 2, 4] {
                                let ii = pti * 6 + off;
                                let has = (ii < anim.geom_tracks.len() && !anim.geom_tracks[ii].keyframes.is_empty())
                                    || (ii + 1 < anim.geom_tracks.len() && !anim.geom_tracks[ii + 1].keyframes.is_empty());
                                if has { c += 1; }
                            }
                        }
                        c
                    }
                    NodeKind::BrushStroke { points } => {
                        let n = points.len();
                        let mut c = 0usize;
                        for pti in 0..n {
                            let i1 = pti * 3;
                            let i2 = i1 + 1;
                            if (i1 < anim.geom_tracks.len() && !anim.geom_tracks[i1].keyframes.is_empty())
                                || (i2 < anim.geom_tracks.len() && !anim.geom_tracks[i2].keyframes.is_empty()) { c += 1; }
                            let iw = pti * 3 + 2;
                            if iw < anim.geom_tracks.len() && !anim.geom_tracks[iw].keyframes.is_empty() { c += 1; }
                        }
                        c
                    }
                    _ => anim.geom_tracks.iter().filter(|t| !t.keyframes.is_empty()).count(),
                }
            } else {
                0
            };
            
            (if has_pos { 1 } else { 0 })
                + (if has_rot { 1 } else { 0 })
                + (if has_op { 1 } else { 0 })
                + (if has_col { 1 } else { 0 })
                + (if has_stroke_w { 1 } else { 0 })
                + (if has_stroke_col { 1 } else { 0 })
                + geom_row_count
        } else {
            0
        }
    } else {
        0
    };

    let display_rows = track_count.min(3);
    let expected_h = if track_count == 0 {
        56.0
    } else {
        56.0 + (display_rows as f32 * 36.0)
    };
    let max_h = (inset.height() * 0.85).max(expected_h);
    let card_w = max_w;  // always use current available to avoid sticking on resize/ab toggle
    let card_h = restore_floater_height(app.timeline_container_h, expected_h, max_h);

    let left = inset.left() + gap;
    let dock_inset = theme::STATUS_BAR_HEIGHT + theme::FLOATING_ABOVE_STATUS_GAP;
    let screen_y = ctx.content_rect().max.y;
    let open_top = screen_y - dock_inset - card_h;
    let travel = card_h + dock_inset + gap;
    let top = open_top + (1.0 - open_t) * travel;

    let rect = Rect::from_min_size(egui::pos2(left, top), egui::vec2(card_w, card_h));
    let opacity = egui::emath::easing::cubic_out(open_t);

    // Render Graph Editor if open/opening
    if app.anim_graph_editor_t > 0.001 {
        let graph_h = 180.0;
        let graph_top = top - gap - graph_h * app.anim_graph_editor_t;
        let graph_rect = Rect::from_min_size(egui::pos2(left, graph_top), egui::vec2(card_w, graph_h));
        let graph_opacity = egui::emath::easing::cubic_out(app.anim_graph_editor_t) * opacity;

        theme::show_action_bar_area(ctx, "graph_editor", graph_rect, graph_opacity, |ui| {
            graph_editor_interior(app, ui);
        });
    }

    if let Some(actual_rect) = theme::show_action_bar_area(ctx, "floating_timeline", rect, opacity, |ui| {
        timeline_interior(app, ui);
    }) {
        app.timeline_container_h = actual_rect.height();
        app.timeline_container_w = actual_rect.width();
    }
}

fn draw_dotted_line(painter: &egui::Painter, p1: egui::Pos2, p2: egui::Pos2, stroke: egui::Stroke) {
    let dist = p1.distance(p2);
    if dist < 1e-3 {
        return;
    }
    let step = 6.0; // gap + dot length
    let dir = (p2 - p1) / dist;
    let mut current = 0.0;
    while current < dist {
        let start = p1 + dir * current;
        let end = p1 + dir * (current + 3.0).min(dist);
        painter.line_segment([start, end], stroke);
        current += step;
    }
}

/// Navy shade for stack-function regions (independent of track line colors).
fn stack_region_fill() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(20, 40, 100, 72)
}
fn stack_region_border() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(80, 120, 200, 180)
}
fn stack_resize_hi() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(160, 200, 255, 220)
}

fn graph_editor_interior(app: &mut VadadeeBerryApp, ui: &mut egui::Ui) {
    let Some((node_id, track_lbl)) = app.anim_graph_editor_track.clone() else {
        return;
    };
    
    // Resolve current node state for default values
    let (node_pos, node_rot, node_op, node_col, geom_floats) = if let Some(node) = app.project.nodes.get(node_id) {
        (
            node.get_pos(),
            node.get_rotation(),
            node.get_opacity() as f64,
            node.get_color(),
            node.get_geom_floats(),
        )
    } else {
        ((0.0, 0.0), 0.0, 1.0, [1.0, 1.0, 1.0, 1.0], Vec::new())
    };
    
    // Resolve human-readable track name
    let track_name = match track_lbl.as_str() {
        "pos_x" | "pos_y" => "Position".to_string(),
        "rotation" => "Rotation".to_string(),
        "opacity" => "Opacity".to_string(),
        "color_r" | "color_g" | "color_b" | "color_a" => "Fill Color".to_string(),
        "stroke_width" => "Stroke Width".to_string(),
        "stroke_r" | "stroke_g" | "stroke_b" | "stroke_a" => "Stroke Color".to_string(),
        _ if track_lbl.starts_with("geom_") => {
            if let Ok(idx) = track_lbl["geom_".len()..].parse::<usize>() {
                app.get_node_geom_track_name(node_id, idx)
            } else {
                track_lbl.clone()
            }
        }
        _ if track_lbl.starts_with("param:") => {
            // param:{uuid} or param:{uuid}:N
            let rest = &track_lbl["param:".len()..];
            let (id_str, comp) = rest
                .split_once(':')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((rest, None));
            let pname = uuid::Uuid::parse_str(id_str)
                .ok()
                .and_then(|pid| {
                    app.project
                        .document
                        .layers
                        .iter()
                        .find(|l| l.id == node_id)
                        .and_then(|l| l.node_graph.as_ref())
                        .and_then(|g| g.parameters.iter().find(|p| p.id == pid))
                        .map(|p| p.name.clone())
                })
                .unwrap_or_else(|| "Param".into());
            match comp {
                None => format!("Param · {pname}"),
                Some("0") => format!("Param · {pname} · 0/X/R"),
                Some("1") => format!("Param · {pname} · 1/Y/G"),
                Some("2") => format!("Param · {pname} · B"),
                Some("3") => format!("Param · {pname} · A"),
                Some(c) => format!("Param · {pname} · {c}"),
            }
        }
        _ => track_lbl.clone(),
    };
    
    ui.vertical(|ui| {
        // Title row + stack function inspector (aligned)
        ui.horizontal_wrapped(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("GRAPH EDITOR: {}", track_name)).strong().color(colors::ACCENT));

            // Stack function controls (when one is selected)
            if let Some(stack_id) = app.anim_graph_selected_stack {
                graph_stack_header_controls(app, ui, node_id, stack_id);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new(icons::CLOSE).font(icons::nerd_font_id(12.0))).clicked() {
                    app.anim_graph_editor_track = None;
                    app.anim_graph_selected_stack = None;
                    app.anim_graph_region_select = None;
                }
                
                // Show interpolation mode selector inside graph editor header
                if let Some((n_id, ref s_lbl, frame)) = app.anim_selected_keyframe.clone() {
                    if n_id == node_id {
                        if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) {
                            if let Some(track) = anim.get_track_mut(&s_lbl) {
                                if let Some(idx) = track.keyframes.iter().position(|k| k.frame == frame) {
                                    let next_kf_val = track.keyframes.iter()
                                        .find(|k| k.frame > frame)
                                        .map(|k| (k.frame, k.value));
                                    
                                    let kf = &mut track.keyframes[idx];
                                    let mut selected_mode = kf.interpolation;
                                    ui.add_space(8.0);
                                    let combo = egui::ComboBox::from_id_salt("graph_kf_interp_combo")
                                        .selected_text(match selected_mode {
                                            crate::app::InterpolationMode::Linear => "Linear",
                                            crate::app::InterpolationMode::Bezier => "Bezier/Smooth",
                                        })
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_value(&mut selected_mode, crate::app::InterpolationMode::Linear, "Linear").clicked() {
                                                kf.interpolation = crate::app::InterpolationMode::Linear;
                                            }
                                            if ui.selectable_value(&mut selected_mode, crate::app::InterpolationMode::Bezier, "Bezier/Smooth").clicked() {
                                                kf.interpolation = crate::app::InterpolationMode::Bezier;
                                                if let Some((next_frame, next_value)) = next_kf_val {
                                                    kf.handle_right = (
                                                        (next_frame - kf.frame) as f64 * 0.5,
                                                        (next_value - kf.value) * 0.5
                                                    );
                                                } else {
                                                    kf.handle_right = (5.0, 0.0);
                                                }
                                            }
                                        });
                                    if combo.response.changed() {
                                        app.apply_animation_for_frame(app.anim_current_frame);
                                    }
                                    ui.label(RichText::new(format!("Keyframe (Frame {}):", frame)).color(colors::TEXT_MUTED));
                                }
                            }
                        }
                    }
                }
            });
        });
        
        ui.add_space(4.0);
        
        let default_val = if track_lbl.starts_with("geom_") {
            if let Ok(idx) = track_lbl["geom_".len()..].parse::<usize>() {
                geom_floats.get(idx).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        } else if track_lbl == "opacity" {
            node_op
        } else if track_lbl == "rotation" {
            node_rot
        } else if track_lbl == "pos_x" {
            node_pos.0
        } else if track_lbl == "pos_y" {
            node_pos.1
        } else if track_lbl == "color_r" {
            node_col[0] as f64
        } else if track_lbl == "color_g" {
            node_col[1] as f64
        } else if track_lbl == "color_b" {
            node_col[2] as f64
        } else if track_lbl == "color_a" {
            node_col[3] as f64
        } else if track_lbl == "stroke_width" {
            app.project
                .nodes
                .get(node_id)
                .map(|n| n.get_stroke_width() as f64)
                .unwrap_or(2.0)
        } else if track_lbl.starts_with("stroke_") {
            let sc = app
                .project
                .nodes
                .get(node_id)
                .map(|n| n.get_stroke_color())
                .unwrap_or([0.1, 0.1, 0.18, 1.0]);
            match track_lbl.as_str() {
                "stroke_r" => sc[0] as f64,
                "stroke_g" => sc[1] as f64,
                "stroke_b" => sc[2] as f64,
                "stroke_a" => sc[3] as f64,
                _ => 0.0,
            }
        } else if track_lbl.starts_with("param:") {
            let rest = &track_lbl["param:".len()..];
            let (id_str, comp) = rest
                .split_once(':')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((rest, None));
            uuid::Uuid::parse_str(id_str)
                .ok()
                .and_then(|pid| {
                    app.project
                        .document
                        .layers
                        .iter()
                        .find(|l| l.id == node_id)
                        .and_then(|l| l.node_graph.as_ref())
                        .and_then(|g| g.parameters.iter().find(|p| p.id == pid))
                        .map(|p| match comp {
                            None | Some("0") => p.v0,
                            Some("1") => p.v1,
                            Some("2") => p.v2,
                            Some("3") => p.v3,
                            _ => p.v0,
                        })
                })
                .unwrap_or(0.0)
        } else {
            0.0
        };

        // Owned track snapshots (release timeline borrow for stack UI mutations).
        let (tracks_to_draw, stack_fns) = {
            let Some(anim) = app.project.anim_timeline.nodes.get(&node_id) else {
                return;
            };
            let mut tracks_to_draw: Vec<(String, egui::Color32, crate::document::KeyframeTrack, f64)> =
                Vec::new();
            if track_lbl == "pos_x" || track_lbl == "pos_y" {
                tracks_to_draw.push(("pos_x".to_string(), egui::Color32::from_rgb(0, 200, 0), anim.pos_x.clone(), node_pos.0));
                tracks_to_draw.push(("pos_y".to_string(), egui::Color32::from_rgb(200, 0, 0), anim.pos_y.clone(), node_pos.1));
            } else if track_lbl.starts_with("color_") {
                tracks_to_draw.push(("color_r".to_string(), egui::Color32::from_rgb(255, 100, 100), anim.color_r.clone(), node_col[0] as f64));
                tracks_to_draw.push(("color_g".to_string(), egui::Color32::from_rgb(100, 255, 100), anim.color_g.clone(), node_col[1] as f64));
                tracks_to_draw.push(("color_b".to_string(), egui::Color32::from_rgb(100, 100, 255), anim.color_b.clone(), node_col[2] as f64));
            } else if track_lbl == "stroke_width" {
                let sw = app
                    .project
                    .nodes
                    .get(node_id)
                    .map(|n| n.get_stroke_width() as f64)
                    .unwrap_or(2.0);
                tracks_to_draw.push((
                    "stroke_width".to_string(),
                    egui::Color32::from_rgb(200, 160, 80),
                    anim.stroke_width.clone(),
                    sw,
                ));
            } else if track_lbl.starts_with("stroke_") {
                let sc = app
                    .project
                    .nodes
                    .get(node_id)
                    .map(|n| n.get_stroke_color())
                    .unwrap_or([0.1, 0.1, 0.18, 1.0]);
                tracks_to_draw.push((
                    "stroke_r".to_string(),
                    egui::Color32::from_rgb(220, 80, 80),
                    anim.stroke_r.clone(),
                    sc[0] as f64,
                ));
                tracks_to_draw.push((
                    "stroke_g".to_string(),
                    egui::Color32::from_rgb(80, 220, 80),
                    anim.stroke_g.clone(),
                    sc[1] as f64,
                ));
                tracks_to_draw.push((
                    "stroke_b".to_string(),
                    egui::Color32::from_rgb(80, 80, 220),
                    anim.stroke_b.clone(),
                    sc[2] as f64,
                ));
            } else if track_lbl.starts_with("param:") {
                if let Some(t) = anim.get_track(&track_lbl) {
                    tracks_to_draw.push((
                        track_lbl.clone(),
                        egui::Color32::from_rgb(200, 160, 60),
                        t.clone(),
                        default_val,
                    ));
                }
            } else if track_lbl.starts_with("geom_") {
                if let Ok(idx) = track_lbl["geom_".len()..].parse::<usize>() {
                    let mut grouped = false;
                    if let Some(node) = app.project.nodes.get(node_id) {
                        let base_len = node.get_geom_floats().len();
                        if idx < base_len {
                            match &node.kind {
                                NodeKind::Path { .. } => {
                                    let anchor_idx = idx / 6;
                                    let sub_idx = idx % 6;
                                    let pairs = match sub_idx {
                                        0 | 1 => Some((anchor_idx * 6, anchor_idx * 6 + 1, egui::Color32::from_rgb(0, 200, 0), egui::Color32::from_rgb(200, 0, 0))),
                                        2 | 3 => Some((anchor_idx * 6 + 2, anchor_idx * 6 + 3, egui::Color32::from_rgb(0, 200, 200), egui::Color32::from_rgb(200, 0, 200))),
                                        4 | 5 => Some((anchor_idx * 6 + 4, anchor_idx * 6 + 5, egui::Color32::from_rgb(100, 200, 100), egui::Color32::from_rgb(200, 100, 200))),
                                        _ => None,
                                    };
                                    if let Some((ix, iy, cx, cy)) = pairs {
                                        let lbl_x = format!("geom_{ix}");
                                        let lbl_y = format!("geom_{iy}");
                                        let def_x = geom_floats.get(ix).copied().unwrap_or(0.0);
                                        let def_y = geom_floats.get(iy).copied().unwrap_or(0.0);
                                        if let Some(t_x) = anim.get_track(&lbl_x) {
                                            tracks_to_draw.push((lbl_x, cx, t_x.clone(), def_x));
                                        }
                                        if let Some(t_y) = anim.get_track(&lbl_y) {
                                            tracks_to_draw.push((lbl_y, cy, t_y.clone(), def_y));
                                        }
                                        grouped = true;
                                    }
                                }
                                NodeKind::BrushStroke { .. } => {
                                    let pt_idx = idx / 3;
                                    let sub_idx = idx % 3;
                                    if sub_idx == 0 || sub_idx == 1 {
                                        let idx_x = pt_idx * 3;
                                        let idx_y = pt_idx * 3 + 1;
                                        let lbl_x = format!("geom_{idx_x}");
                                        let lbl_y = format!("geom_{idx_y}");
                                        let def_x = geom_floats.get(idx_x).copied().unwrap_or(0.0);
                                        let def_y = geom_floats.get(idx_y).copied().unwrap_or(0.0);
                                        if let Some(t_x) = anim.get_track(&lbl_x) {
                                            tracks_to_draw.push((lbl_x, egui::Color32::from_rgb(0, 200, 0), t_x.clone(), def_x));
                                        }
                                        if let Some(t_y) = anim.get_track(&lbl_y) {
                                            tracks_to_draw.push((lbl_y, egui::Color32::from_rgb(200, 0, 0), t_y.clone(), def_y));
                                        }
                                        grouped = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if !grouped {
                        if let Some(t) = anim.get_track(&track_lbl) {
                            tracks_to_draw.push((track_lbl.clone(), colors::ACCENT, t.clone(), default_val));
                        }
                    }
                }
            } else if let Some(t) = anim.get_track(&track_lbl) {
                tracks_to_draw.push((track_lbl.clone(), colors::ACCENT, t.clone(), default_val));
            }

            let track_labels: Vec<String> = tracks_to_draw.iter().map(|(l, _, _, _)| l.clone()).collect();
            let stack_fns: Vec<crate::document::StackAnimationFunction> = anim
                .stack_functions
                .iter()
                .filter(|sf| sf.channels.iter().any(|c| track_labels.iter().any(|l| l == &c.track)))
                .cloned()
                .collect();
            (tracks_to_draw, stack_fns)
        };

        if tracks_to_draw.is_empty() {
            return;
        }
        let graph_track_labels: Vec<String> =
            tracks_to_draw.iter().map(|(l, _, _, _)| l.clone()).collect();
        let graph_track_defaults: Vec<f64> =
            tracks_to_draw.iter().map(|(_, _, _, d)| *d).collect();

        let mut graph_scroll = app.anim_graph_scroll;
        let mut graph_visible = app.anim_graph_visible_frames.max(10.0);
        let graph_frame_max = tracks_to_draw
            .iter()
            .flat_map(|(_, _, track, _)| track.keyframes.iter().map(|k| k.frame))
            .chain(stack_fns.iter().map(|sf| sf.end_frame()))
            .max()
            .unwrap_or(0)
            .max(app.get_max_animation_frame())
            .max(100);

        // Frame-width control (how many frames the graph plot shows).
        ui.horizontal(|ui| {
            ui.label(RichText::new("Frame width").small().color(colors::TEXT_MUTED));
            let mut vis = graph_visible;
            if ui
                .add(
                    egui::DragValue::new(&mut vis)
                        .range(10.0..=5000.0)
                        .speed(2.0)
                        .suffix(" frames"),
                )
                .on_hover_text("Visible span of the graph plot (time axis zoom)")
                .changed()
            {
                graph_visible = vis;
                app.anim_graph_visible_frames = vis;
            }
            ui.label(RichText::new("Scroll").small().color(colors::TEXT_MUTED));
            let mut scr = graph_scroll;
            if ui
                .add(
                    egui::DragValue::new(&mut scr)
                        .range(0.0..=(graph_frame_max as f32 + 100.0))
                        .speed(1.0)
                        .suffix(" start"),
                )
                .changed()
            {
                graph_scroll = scr.max(0.0);
                app.anim_graph_scroll = graph_scroll;
            }
        });

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width() - 8.0, 136.0),
            egui::Sense::click_and_drag()
        );
        let painter = ui.painter_at(rect);
        
        painter.rect_filled(rect, egui::CornerRadius::same(4), colors::BG_DEEP);
        painter.rect_stroke(rect, egui::CornerRadius::same(4), egui::Stroke::new(1.0, colors::BORDER), egui::StrokeKind::Inside);
        
        let padding = 12.0;
        
        // Content exists?
        let mut has_keyframes = tracks_to_draw.iter().any(|(_, _, t, _)| !t.keyframes.is_empty())
            || !stack_fns.is_empty();
        if !has_keyframes {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No keyframes on this track.").color(colors::TEXT_MUTED));
            });
            return;
        }

        // Scroll / grab pan (wheel + middle/right/shift-drag), like the main timeline
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        let wheel_delta = if scroll_delta.x != 0.0 {
            scroll_delta.x
        } else {
            scroll_delta.y
        };
        if wheel_delta != 0.0 && response.hovered() {
            // Ctrl+wheel: zoom frame width; plain wheel: pan
            if ui.input(|i| i.modifiers.ctrl) {
                let factor = if wheel_delta > 0.0 { 0.9 } else { 1.1 };
                graph_visible = (graph_visible * factor).clamp(10.0, 5000.0);
            } else {
                graph_scroll = (graph_scroll - wheel_delta * 0.15).max(0.0);
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary)
                && ui.input(|i| i.modifiers.shift)
                && app.anim_graph_editor_dragged_kf.is_none()
                && app.anim_graph_editor_dragged_handle.is_none())
        {
            let delta_x = ui.input(|i| i.pointer.delta().x);
            graph_scroll =
                (graph_scroll - (delta_x / rect.width()) * graph_visible).max(0.0);
        }
        let scroll_max = (graph_frame_max as f32 + 20.0 - graph_visible).max(0.0);
        graph_scroll = graph_scroll.min(scroll_max);

        // Auto Y-range from samples in the *visible* frame window (incl. stack exprs).
        let vis_lo = graph_scroll.floor().max(0.0) as usize;
        let vis_hi = (graph_scroll + graph_visible).ceil() as usize;
        let sample_step = ((vis_hi.saturating_sub(vis_lo) / 80).max(1)) as usize;
        let mut target_min = f64::MAX;
        let mut target_max = f64::MIN;
        for (lbl, _, track, default_val) in &tracks_to_draw {
            for kf in &track.keyframes {
                if kf.frame >= vis_lo && kf.frame <= vis_hi {
                    target_min = target_min.min(kf.value);
                    target_max = target_max.max(kf.value);
                }
            }
            let mut f = vis_lo;
            while f <= vis_hi {
                let mut val = track.interpolate(f).unwrap_or(*default_val);
                for sf in &stack_fns {
                    if let Ok(Some(v)) = sf.sample_channel_ref(lbl, f) {
                        val = v;
                        break;
                    }
                }
                target_min = target_min.min(val);
                target_max = target_max.max(val);
                f += sample_step;
            }
        }
        if !target_min.is_finite() || !target_max.is_finite() || target_min > target_max {
            target_min = 0.0;
            target_max = 1.0;
        }
        if (target_max - target_min).abs() < 1e-6 {
            target_min -= 1.0;
            target_max += 1.0;
        } else {
            let pad = (target_max - target_min) * 0.18;
            target_min -= pad;
            target_max += pad;
        }
        // Smoothly approach target so range doesn't jump every frame.
        let smooth = 0.22_f64;
        if (app.anim_graph_view_val_max - app.anim_graph_view_val_min).abs() < 1e-9 {
            app.anim_graph_view_val_min = target_min;
            app.anim_graph_view_val_max = target_max;
        } else {
            app.anim_graph_view_val_min +=
                (target_min - app.anim_graph_view_val_min) * smooth;
            app.anim_graph_view_val_max +=
                (target_max - app.anim_graph_view_val_max) * smooth;
        }
        let val_min = app.anim_graph_view_val_min;
        let val_max = app.anim_graph_view_val_max;
        ui.ctx().request_repaint(); // keep smooth Y settle

        // Draw grid (frame ticks in visible range)
        let grid_step = if graph_visible > 200.0 {
            20
        } else if graph_visible > 80.0 {
            10
        } else {
            5
        };
        let grid_start = ((graph_scroll / grid_step as f32).floor() * grid_step as f32) as i32;
        let grid_end =
            ((graph_scroll + graph_visible) / grid_step as f32).ceil() as i32 * grid_step;
        for f in (grid_start..=grid_end).step_by(grid_step as usize) {
            if f < 0 {
                continue;
            }
            let frac = (f as f32 - graph_scroll) / graph_visible;
            if !(0.0..=1.0).contains(&frac) {
                continue;
            }
            let x = rect.left() + frac * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, colors::BORDER.gamma_multiply(0.2)),
            );
            if f % (grid_step * 2) == 0 {
                painter.text(
                    egui::pos2(x, rect.bottom() - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    f.to_string(),
                    egui::FontId::new(8.0, egui::FontFamily::Proportional),
                    colors::TEXT_MUTED.gamma_multiply(0.55),
                );
            }
        }
        for i in 0..=4 {
            let frac = i as f32 / 4.0;
            let y = rect.bottom() - padding - frac * (rect.height() - 2.0 * padding);
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0, colors::BORDER.gamma_multiply(0.2)),
            );
            
            let val = val_min + (frac as f64) * (val_max - val_min);
            let font = egui::FontId::new(9.0, egui::FontFamily::Proportional);
            painter.text(
                egui::pos2(rect.left() + 4.0, y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.1}", val),
                font,
                colors::TEXT_MUTED.gamma_multiply(0.6),
            );
        }
        
        // Setup screen/graph space mapping closures (panned frame axis)
        let scroll_snap = graph_scroll;
        let visible_snap = graph_visible;
        let to_screen = |f: f64, v: f64| -> egui::Pos2 {
            let frac_x = (f as f32 - scroll_snap) / visible_snap;
            let x = rect.left() + frac_x * rect.width();
            let frac_y = (v - val_min) / (val_max - val_min);
            let y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
            egui::pos2(x, y)
        };

        let to_graph = |pos: egui::Pos2| -> (f64, f64) {
            let frac_x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let frame = scroll_snap as f64 + frac_x as f64 * visible_snap as f64;
            let target_frac_y = (rect.bottom() - padding - pos.y) / (rect.height() - 2.0 * padding);
            let value = val_min + (target_frac_y as f64) * (val_max - val_min);
            (frame, value)
        };
        
        // Navy stack-function regions (under curves)
        let mut hover_stack_resize: Option<uuid::Uuid> = None;
        let mut hover_stack_body: Option<uuid::Uuid> = None;
        for sf in &stack_fns {
            let x0 = to_screen(sf.start_frame as f64, val_min).x;
            let x1 = to_screen(sf.end_frame() as f64, val_min).x;
            let mut region = egui::Rect::from_min_max(
                egui::pos2(x0.min(x1), rect.top() + 2.0),
                egui::pos2(x0.max(x1), rect.bottom() - 2.0),
            );
            region = region.intersect(rect);
            if region.width() < 1.0 {
                continue;
            }
            let selected = app.anim_graph_selected_stack == Some(sf.id);
            painter.rect_filled(region, 0.0, stack_region_fill());
            painter.rect_stroke(
                region,
                0.0,
                egui::Stroke::new(if selected { 1.5 } else { 1.0 }, stack_region_border()),
                egui::StrokeKind::Inside,
            );
            // Relative time endpoints: f=0 at stack start, f=duration at stack end (not global timeline).
            let start_x = x0.min(x1);
            let end_x = x0.max(x1);
            let rel_end = sf.duration_frames.max(1);
            // Start point (relative 0)
            painter.line_segment(
                [
                    egui::pos2(start_x, region.top()),
                    egui::pos2(start_x, region.bottom()),
                ],
                egui::Stroke::new(2.0, stack_region_border()),
            );
            painter.circle_filled(
                egui::pos2(start_x, region.center().y),
                4.0,
                stack_region_border(),
            );
            painter.text(
                egui::pos2(start_x + 3.0, region.bottom() - 2.0),
                egui::Align2::LEFT_BOTTOM,
                "0",
                egui::FontId::new(9.0, egui::FontFamily::Proportional),
                stack_region_border(),
            );
            // End point (relative duration)
            painter.line_segment(
                [
                    egui::pos2(end_x, region.top()),
                    egui::pos2(end_x, region.bottom()),
                ],
                egui::Stroke::new(2.0, stack_region_border()),
            );
            painter.circle_filled(
                egui::pos2(end_x, region.center().y),
                4.0,
                stack_region_border(),
            );
            painter.text(
                egui::pos2(end_x - 3.0, region.bottom() - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{rel_end}"),
                egui::FontId::new(9.0, egui::FontFamily::Proportional),
                stack_region_border(),
            );
            // Formula label (relative time)
            let parts: Vec<String> = sf
                .channels
                .iter()
                .map(|c| {
                    if c.expr.trim().is_empty() {
                        format!("{:.3}", c.start_value)
                    } else {
                        c.expr.trim().to_string()
                    }
                })
                .collect();
            // Use nerd-font arrow (); Unicode → often renders as □ in default UI fonts.
            let arrow = icons::ARROW_RIGHT;
            let ch_bits: Vec<String> = sf
                .channels
                .iter()
                .zip(parts.iter())
                .map(|(c, p)| {
                    let short = match c.track.as_str() {
                        "pos_x" => "x".to_string(),
                        "pos_y" => "y".to_string(),
                        "rotation" => "r".to_string(),
                        "opacity" => "o".to_string(),
                        "color_r" => "R".to_string(),
                        "color_g" => "G".to_string(),
                        "color_b" => "B".to_string(),
                        "color_a" => "A".to_string(),
                        "stroke_width" => "sw".to_string(),
                        "stroke_r" => "sR".to_string(),
                        "stroke_g" => "sG".to_string(),
                        "stroke_b" => "sB".to_string(),
                        "stroke_a" => "sA".to_string(),
                        other => other.to_string(),
                    };
                    if c.expr.trim().is_empty() {
                        // Constant hold: show start binding e.g. "x  54"
                        format!("{short} {arrow} {p}")
                    } else {
                        format!("{short}={p}")
                    }
                })
                .collect();
            let body = if ch_bits.len() == 1 {
                ch_bits[0].clone()
            } else {
                format!("({})", ch_bits.join(", "))
            };
            let label = format!("f(t) = {body}  [t:0{arrow}1]");
            painter.text(
                egui::pos2(region.left() + 6.0, region.top() + 2.0),
                egui::Align2::LEFT_TOP,
                label,
                icons::nerd_font_id(10.0),
                colors::TEXT_MUTED.gamma_multiply(1.2),
            );
            // End resize hover zone
            let end_zone = egui::Rect::from_center_size(
                egui::pos2(end_x, region.center().y),
                egui::vec2(10.0, region.height()),
            );
            if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                if end_zone.contains(mpos) {
                    hover_stack_resize = Some(sf.id);
                    painter.line_segment(
                        [
                            egui::pos2(end_x, region.top()),
                            egui::pos2(end_x, region.bottom()),
                        ],
                        egui::Stroke::new(3.5, stack_resize_hi()),
                    );
                } else if region.contains(mpos) {
                    hover_stack_body = Some(sf.id);
                }
            }
        }

        // Draw graph curves (stack functions override sample) and detect segment clicks
        let mut clicked_segment: Option<(String, usize, usize, egui::Pos2)> = None; // (track_lbl, left_frame, right_frame, click_pos)
        for (lbl, color, track, default_val) in &tracks_to_draw {
            let track_lbl_str = lbl.to_string();
            let mut curve_pts: Vec<(usize, egui::Pos2)> = Vec::new(); // (frame, screen_pos)
            let sample_start = graph_scroll.floor().max(0.0) as usize;
            let sample_end = (graph_scroll + graph_visible).ceil() as usize;
            for f in sample_start..=sample_end {
                let mut val = track.interpolate(f).unwrap_or(*default_val);
                for sf in &stack_fns {
                    if let Ok(Some(v)) = sf.sample_channel_ref(&track_lbl_str, f) {
                        val = v;
                        break;
                    }
                }
                curve_pts.push((f, to_screen(f as f64, val)));
            }

            // Check if pointer is near a segment (but not near a keyframe node)
            let near_any_kf = ui.input(|i| i.pointer.hover_pos()).map_or(false, |mpos| {
                track.keyframes.iter().any(|kf| to_screen(kf.frame as f64, kf.value).distance(mpos) < 10.0)
            });

            if !near_any_kf {
                if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                    for w in curve_pts.windows(2) {
                        let (fa, pa) = w[0];
                        let (fb, pb) = w[1];
                        let seg_len = pa.distance(pb);
                        if seg_len < 0.5 { continue; }
                        // Project mpos onto segment
                        let t = ((mpos - pa).dot(pb - pa)) / (seg_len * seg_len);
                        let t = t.clamp(0.0, 1.0);
                        let proj = pa + (pb - pa) * t;
                        if mpos.distance(proj) < 6.0 {
                            // Segment hovered — highlight it
                            painter.line_segment([pa, pb], egui::Stroke::new(3.5, color.linear_multiply(1.6)));
                            if ui.input(|i| i.pointer.any_pressed()) {
                                // Find left/right actual keyframes bracketing this segment
                                let lf = track.keyframes.iter().filter(|k| k.frame <= fa).map(|k| k.frame).last();
                                let rf = track.keyframes.iter().filter(|k| k.frame >= fb).map(|k| k.frame).next();
                                if let (Some(lf), Some(rf)) = (lf, rf) {
                                    clicked_segment = Some((track_lbl_str.clone(), lf, rf, mpos));
                                }
                            }
                            break;
                        }
                    }
                }
            }

            // Redraw curves normally (the extra stroke above will layer on top)
            for window in curve_pts.windows(2) {
                painter.line_segment([window[0].1, window[1].1], egui::Stroke::new(1.5, *color));
            }
        }

        // Handle segment click → select segment for bezier-add
        if let Some((ref seg_lbl, lf, rf, _)) = clicked_segment {
            app.anim_graph_selected_segment = Some((seg_lbl.clone(), lf, rf));
            app.anim_selected_keyframe = None;
        }

        // Draw selected-segment highlight
        if let Some((ref seg_lbl, lf, rf)) = app.anim_graph_selected_segment.clone() {
            for (lbl, _color, track, default_val) in &tracks_to_draw {
                if lbl.to_string() == *seg_lbl {
                    let left_val = track.interpolate(lf).unwrap_or(*default_val);
                    let right_val = track.interpolate(rf).unwrap_or(*default_val);
                    let ps = to_screen(lf as f64, left_val);
                    let pe = to_screen(rf as f64, right_val);
                    painter.line_segment([ps, pe], egui::Stroke::new(3.0, colors::ACCENT.gamma_multiply(0.7)));
                    // Midpoint indicator
                    let mid_frame = (lf + rf) / 2;
                    let mid_val = track.interpolate(mid_frame).unwrap_or((left_val + right_val) * 0.5);
                    let pm = to_screen(mid_frame as f64, mid_val);
                    painter.circle_filled(pm, 5.0, colors::ACCENT);
                    painter.circle(pm, 5.0, egui::Color32::TRANSPARENT, egui::Stroke::new(1.5, egui::Color32::WHITE));
                }
            }
        }

        // Draw keyframe nodes and Bezier handles
        let mut _clicked_any = false;
        let mut next_dragged_kf = app.anim_graph_editor_dragged_kf.clone();
        
        for (lbl, color, track, _) in &tracks_to_draw {
            let track_lbl_str = lbl.to_string();
            let keyframes_len = track.keyframes.len();
            for (_i, kf) in track.keyframes.iter().enumerate() {
                let center = to_screen(kf.frame as f64, kf.value);
                
                let mpos = ui.input(|i| i.pointer.hover_pos());
                let is_hovered = mpos.map_or(false, |mp| mp.distance(center) < 8.0);
                
                let is_selected = app.anim_selected_keyframe.as_ref().map_or(false, |&(s_id, ref s_lbl, s_f)| {
                    s_id == node_id && s_lbl == &track_lbl_str && s_f == kf.frame
                });
                
                let is_dragged = app.anim_graph_editor_dragged_kf.as_ref().map_or(false, |(d_lbl, d_frame)| {
                    d_lbl == &track_lbl_str && *d_frame == kf.frame
                });
                
                let kf_color = if is_hovered || is_dragged {
                    colors::ACCENT
                } else {
                    *color
                };
                
                let stroke_color = if is_selected {
                    colors::ACCENT
                } else {
                    colors::BG_PANEL
                };
                let stroke_w = if is_selected { 2.0 } else { 1.0 };
                let radius = if is_selected { 6.0 } else { 4.5 };
                
                // Draw Bezier handle if interpolation is Bezier and we have a next keyframe
                if kf.interpolation == crate::app::InterpolationMode::Bezier && _i + 1 < keyframes_len {
                    let kf_next = &track.keyframes[_i + 1];
                    let right_pt = to_screen(kf.frame as f64 + kf.handle_right.0, kf.value + kf.handle_right.1);
                    let next_center = to_screen(kf_next.frame as f64, kf_next.value);
                    
                    let dotted_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE.gamma_multiply(0.6));
                    draw_dotted_line(&painter, center, right_pt, dotted_stroke);
                    draw_dotted_line(&painter, right_pt, next_center, dotted_stroke);
                    
                    let handle_color = egui::Color32::from_rgb(250, 200, 50);
                    let is_h = mpos.map_or(false, |mp| mp.distance(right_pt) < 6.0);
                    let is_d = app.anim_graph_editor_dragged_handle.as_ref().map_or(false, |(t, f, is_l)| {
                        t == &track_lbl_str && *f == kf.frame && !*is_l
                    });
                    
                    let pt_color = if is_h || is_d { colors::ACCENT } else { handle_color };
                    painter.circle_filled(right_pt, 4.0, pt_color);
                    
                    if is_h && ui.input(|i| i.pointer.any_pressed()) {
                        app.anim_graph_editor_dragged_handle = Some((track_lbl_str.clone(), kf.frame, false));
                        _clicked_any = true;
                    }
                }
                
                if kf.interpolation == crate::app::InterpolationMode::Bezier {
                    let pts = [
                        egui::pos2(center.x, center.y - radius),
                        egui::pos2(center.x + radius, center.y),
                        egui::pos2(center.x, center.y + radius),
                        egui::pos2(center.x - radius, center.y),
                    ];
                    painter.add(egui::Shape::convex_polygon(pts.to_vec(), kf_color, egui::Stroke::new(stroke_w, stroke_color)));
                } else {
                    painter.circle(center, radius, kf_color, egui::Stroke::new(stroke_w, stroke_color));
                }
                
                if is_hovered && ui.input(|i| i.pointer.any_pressed()) {
                    // Record drag-start position to avoid creating duplicates on pure clicks
                    if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                        app.anim_graph_kf_drag_start = Some((track_lbl_str.clone(), kf.frame, mpos));
                    }
                    app.anim_selected_keyframe = Some((node_id, track_lbl_str.clone(), kf.frame));
                    app.anim_graph_selected_segment = None;
                    _clicked_any = true;
                }
            }
        }

        // Only commit a drag if pointer has moved >3px from where we first pressed
        if let Some((ref drag_lbl, drag_frame, start_pos)) = app.anim_graph_kf_drag_start.clone() {
            if ui.input(|i| i.pointer.any_down()) {
                let moved = ui.input(|i| i.pointer.hover_pos())
                    .map_or(false, |mpos| mpos.distance(start_pos) > 3.0);
                if moved {
                    next_dragged_kf = Some((drag_lbl.clone(), drag_frame));
                }
            } else {
                app.anim_graph_kf_drag_start = None;
                if next_dragged_kf.is_none() {
                    // Was just a click, not a drag — don't touch positions
                }
            }
        }
        
        app.anim_graph_editor_dragged_kf = next_dragged_kf;
        
        // Handle drag value updates
        if let Some((drag_lbl, drag_frame)) = app.anim_graph_editor_dragged_kf.clone() {
            if ui.input(|i| i.pointer.any_down()) {
                if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                    let (frame_f, _) = to_graph(mpos);
                    let target_frame = frame_f.round().max(0.0) as usize;
                    let target_frac_y = (rect.bottom() - padding - mpos.y) / (rect.height() - 2.0 * padding);
                    let target_val = val_min + (target_frac_y as f64) * (val_max - val_min);
                    
                    if let Some(anim_mut) = app.project.anim_timeline.nodes.get_mut(&node_id) {
                        if let Some(track) = anim_mut.get_track_mut(&drag_lbl) {
                            if let Some(idx) = track.keyframes.iter().position(|k| k.frame == drag_frame) {
                                let old_frame = track.keyframes[idx].frame;
                                
                                let has_other_at_frame = track.keyframes.iter().enumerate()
                                    .any(|(k_idx, k)| k_idx != idx && k.frame == target_frame);
                                
                                let final_frame = if has_other_at_frame { old_frame } else { target_frame };
                                
                                track.keyframes[idx].value = target_val;
                                track.keyframes[idx].frame = final_frame;
                                track.keyframes.sort_by_key(|k| k.frame);
                                
                                app.anim_graph_editor_dragged_kf = Some((drag_lbl.clone(), final_frame));
                                app.anim_selected_keyframe = Some((node_id, drag_lbl.clone(), final_frame));
                            }
                        }
                        // Start keyframes of stacks drive start_value / x,y,r,g,b constants.
                        anim_mut.sync_stack_starts_from_keyframes();
                        anim_mut.ensure_stack_start_keyframes();
                        anim_mut.ensure_stack_end_keyframes();
                    }
                    app.apply_animation_for_frame(app.anim_current_frame);
                }
            } else {
                // Drag ended — commit to history
                let snap = app.project.anim_timeline.clone();
                app.history.push(
                    &mut app.project,
                    crate::history::ProjectEdit::PatchTimeline { before: snap.clone(), after: snap },
                );
                app.anim_graph_editor_dragged_kf = None;
            }
        }
        
        // Handle drag handle updates
        if let Some((drag_lbl, drag_frame, _is_left)) = app.anim_graph_editor_dragged_handle.clone() {
            if ui.input(|i| i.pointer.any_down()) {
                if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                    if let Some(anim_mut) = app.project.anim_timeline.nodes.get_mut(&node_id) {
                        if let Some(track) = anim_mut.get_track_mut(&drag_lbl) {
                            if let Some(idx) = track.keyframes.iter().position(|k| k.frame == drag_frame) {
                                if idx + 1 < track.keyframes.len() {
                                    let next_frame = track.keyframes[idx + 1].frame;
                                    let kf = &mut track.keyframes[idx];
                                    let (m_frame, m_val) = to_graph(mpos);
                                    
                                    let delta_frame = m_frame - kf.frame as f64;
                                    let delta_value = m_val - kf.value;
                                    
                                    let range = (next_frame - kf.frame) as f64;
                                    let df = delta_frame.clamp(0.0, range);
                                    kf.handle_right = (df, delta_value);
                                }
                            }
                        }
                    }
                    app.apply_animation_for_frame(app.anim_current_frame);
                }
            } else {
                // Drag ended — commit to history
                let snap = app.project.anim_timeline.clone();
                app.history.push(
                    &mut app.project,
                    crate::history::ProjectEdit::PatchTimeline { before: snap.clone(), after: snap },
                );
                app.anim_graph_editor_dragged_handle = None;
            }
        }
        
        // Draw playhead line
        let play_frac = (app.anim_current_frame as f32 - graph_scroll) / graph_visible;
        if (0.0..=1.0).contains(&play_frac) {
            let playhead_x = rect.left() + play_frac * rect.width();
            painter.line_segment(
                [egui::pos2(playhead_x, rect.top()), egui::pos2(playhead_x, rect.bottom())],
                egui::Stroke::new(1.0, colors::ACCENT.gamma_multiply(0.4)),
            );
        }

        // Stack region pointer: select / move / resize-end
        if response.hovered() {
            if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                if ui.input(|i| i.pointer.primary_pressed())
                    && app.anim_graph_editor_dragged_kf.is_none()
                    && app.anim_graph_editor_dragged_handle.is_none()
                    && !ui.input(|i| i.modifiers.shift)
                {
                    if let Some(sid) = hover_stack_resize {
                        if let Some(sf) = stack_fns.iter().find(|s| s.id == sid) {
                            app.anim_graph_selected_stack = Some(sid);
                            app.anim_graph_stack_drag =
                                Some(crate::app::AnimGraphStackDrag::ResizeEnd {
                                    id: sid,
                                    orig_duration: sf.duration_frames,
                                });
                            app.anim_graph_selected_segment = None;
                            app.anim_graph_region_select = None;
                        }
                    } else if let Some(sid) = hover_stack_body {
                        if let Some(sf) = stack_fns.iter().find(|s| s.id == sid) {
                            let (gf, _) = to_graph(mpos);
                            app.anim_graph_selected_stack = Some(sid);
                            app.anim_graph_stack_drag =
                                Some(crate::app::AnimGraphStackDrag::Move {
                                    id: sid,
                                    grab_frame: gf,
                                    orig_start: sf.start_frame,
                                });
                            app.anim_graph_selected_segment = None;
                            app.anim_graph_region_select = None;
                        }
                    } else if clicked_segment.is_none()
                        && app.anim_graph_editor_dragged_kf.is_none()
                    {
                        // Start marquee region select on empty graph area
                        let (gf, _) = to_graph(mpos);
                        let f0 = gf.round().max(0.0) as usize;
                        app.anim_graph_region_select = Some((f0, f0));
                        app.anim_graph_selected_stack = None;
                    }
                }
            }
        }

        // Update marquee while dragging primary
        if response.dragged_by(egui::PointerButton::Primary)
            && app.anim_graph_stack_drag.is_none()
            && app.anim_graph_editor_dragged_kf.is_none()
            && !ui.input(|i| i.modifiers.shift)
        {
            if let (Some(mpos), Some((a, _))) = (
                ui.input(|i| i.pointer.hover_pos()),
                app.anim_graph_region_select,
            ) {
                let (gf, _) = to_graph(mpos);
                let f1 = gf.round().max(0.0) as usize;
                app.anim_graph_region_select = Some((a, f1));
            }
        }

        // Draw marquee region (navy)
        if let Some((a, b)) = app.anim_graph_region_select {
            let f0 = a.min(b) as f64;
            let f1 = a.max(b) as f64;
            let x0 = to_screen(f0, val_min).x;
            let x1 = to_screen(f1, val_min).x;
            let region = egui::Rect::from_min_max(
                egui::pos2(x0.min(x1), rect.top() + 2.0),
                egui::pos2(x0.max(x1), rect.bottom() - 2.0),
            )
            .intersect(rect);
            if region.width() > 1.0 {
                painter.rect_filled(region, 0.0, stack_region_fill());
                painter.rect_stroke(
                    region,
                    0.0,
                    egui::Stroke::new(1.0, stack_region_border()),
                    egui::StrokeKind::Inside,
                );
            }
        }

        // Live stack move / resize
        if let Some(drag) = app.anim_graph_stack_drag.clone() {
            if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(mpos) = ui.input(|i| i.pointer.hover_pos()) {
                    let (gf, _) = to_graph(mpos);
                    match drag {
                        crate::app::AnimGraphStackDrag::Move {
                            id,
                            grab_frame,
                            orig_start,
                        } => {
                            let delta = (gf - grab_frame).round() as isize;
                            let new_start = (orig_start as isize + delta).max(0) as usize;
                            let mut moved = false;
                            if let Some(anim) =
                                app.project.anim_timeline.nodes.get_mut(&node_id)
                            {
                                if let Some(sf) =
                                    anim.stack_functions.iter_mut().find(|s| s.id == id)
                                {
                                    if sf.start_frame != new_start {
                                        let labels: Vec<String> = sf
                                            .channels
                                            .iter()
                                            .map(|c| c.track.clone())
                                            .collect();
                                        let old_start = sf.start_frame;
                                        let old_end = sf.end_frame();
                                        let start_vals: Vec<(String, f64)> = sf
                                            .channels
                                            .iter()
                                            .map(|c| (c.track.clone(), c.start_value))
                                            .collect();
                                        sf.start_frame = new_start;
                                        let new_end = sf.end_frame();
                                        let lo = old_start.min(new_start);
                                        let hi = old_end.max(new_end);
                                        let refs: Vec<&str> =
                                            labels.iter().map(|s| s.as_str()).collect();
                                        // Drop keyframes that now fall under the stack span.
                                        anim.clear_keyframes_under_stack(
                                            &refs, new_start, new_end,
                                        );
                                        // Also clear the old span interior (points left behind).
                                        anim.clear_keyframes_under_stack(
                                            &refs, old_start, old_end,
                                        );
                                        // Remove any KF still sitting in [lo, hi] except new start.
                                        for label in &labels {
                                            if let Some(tr) = anim.get_track_mut(label) {
                                                tr.keyframes.retain(|kf| {
                                                    kf.frame == new_start
                                                        || kf.frame < lo
                                                        || kf.frame > hi
                                                });
                                            }
                                        }
                                        for (tr, v) in &start_vals {
                                            if let Some(track) = anim.get_track_mut(tr) {
                                                track.insert(new_start, *v);
                                            }
                                        }
                                        anim.ensure_stack_start_keyframes();
                                        anim.ensure_stack_end_keyframes();
                                        moved = true;
                                    }
                                }
                            }
                            if moved {
                                app.apply_animation_for_frame(app.anim_current_frame);
                            }
                        }
                        crate::app::AnimGraphStackDrag::ResizeEnd { id, orig_duration } => {
                            let _ = orig_duration;
                            let end = gf.round().max(0.0) as usize;
                            let mut apply = false;
                            let mut labels = Vec::new();
                            let mut start = 0usize;
                            let mut lo = 0usize;
                            let mut hi = 0usize;
                            let mut start_vals = Vec::new();
                            if let Some(anim) =
                                app.project.anim_timeline.nodes.get_mut(&node_id)
                            {
                                if let Some(sf) =
                                    anim.stack_functions.iter_mut().find(|s| s.id == id)
                                {
                                    let new_dur =
                                        end.saturating_sub(sf.start_frame).max(1);
                                    if sf.duration_frames != new_dur {
                                        labels = sf
                                            .channels
                                            .iter()
                                            .map(|c| c.track.clone())
                                            .collect();
                                        start = sf.start_frame;
                                        let old_end = sf.end_frame();
                                        sf.duration_frames = new_dur;
                                        let new_end = sf.end_frame();
                                        lo = start;
                                        hi = old_end.max(new_end);
                                        start_vals = sf
                                            .channels
                                            .iter()
                                            .map(|c| (c.track.clone(), c.start_value))
                                            .collect();
                                        apply = true;
                                    }
                                }
                                if apply {
                                    let refs: Vec<&str> =
                                        labels.iter().map(|s| s.as_str()).collect();
                                    anim.clear_keyframes_under_stack(&refs, start, hi.max(lo));
                                    for (tr, v) in &start_vals {
                                        if let Some(track) = anim.get_track_mut(tr) {
                                            track.insert(start, *v);
                                        }
                                    }
                                    anim.ensure_stack_start_keyframes();
                                    anim.ensure_stack_end_keyframes();
                                }
                            }
                            if apply {
                                app.apply_animation_for_frame(app.anim_current_frame);
                            }
                        }
                    }
                }
            } else if ui.input(|i| i.pointer.any_released()) {
                let before = app.project.anim_timeline.clone();
                app.history.push(
                    &mut app.project,
                    crate::history::ProjectEdit::PatchTimeline {
                        before: before.clone(),
                        after: before,
                    },
                );
                app.anim_graph_stack_drag = None;
            }
        }

        app.anim_graph_scroll = graph_scroll;
        app.anim_graph_visible_frames = graph_visible;

        // Region or segment selected: stack function + bezier actions
        let region_opt = app.anim_graph_region_select.map(|(a, b)| (a.min(b), a.max(b)));
        let segment_opt = app
            .anim_graph_selected_segment
            .as_ref()
            .map(|(_, lf, rf)| (*lf, *rf));
        let apply_span = region_opt.or(segment_opt).filter(|(a, b)| b > a);

        if let Some((lf, rf)) = apply_span {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Region [{} – {}]", lf, rf))
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("+ Stack animation function")
                                .color(egui::Color32::from_rgb(120, 180, 255)),
                        )
                        .fill(colors::BG_DEEP),
                    )
                    .on_hover_text(
                        "Replace keyframes in this span with f(t) expressions (t=0..1). Deletes interior keyframes.",
                    )
                    .clicked()
                {
                    apply_stack_animation_function(
                        app,
                        node_id,
                        &graph_track_labels,
                        &graph_track_defaults,
                        lf,
                        rf,
                    );
                    app.anim_graph_selected_segment = None;
                    app.anim_graph_region_select = None;
                }
                // Bezier only for pure segment selection (not after stack apply / stack selected).
                if app.anim_graph_selected_segment.is_some()
                    && app.anim_graph_selected_stack.is_none()
                    && app.anim_graph_region_select.is_none()
                {
                    ui.add_space(4.0);
                    if ui
                        .button(
                            RichText::new("+ Apply Bezier")
                                .color(egui::Color32::from_rgb(80, 200, 120)),
                        )
                        .clicked()
                    {
                        if let Some((ref seg_lbl, lf, rf)) = app.anim_graph_selected_segment.clone()
                        {
                            let before_timeline = app.project.anim_timeline.clone();
                            if let Some(anim_mut) =
                                app.project.anim_timeline.nodes.get_mut(&node_id)
                            {
                                if let Some(track) = anim_mut.get_track_mut(&seg_lbl) {
                                    let left_val = track
                                        .keyframes
                                        .iter()
                                        .find(|k| k.frame == lf)
                                        .map(|k| k.value)
                                        .unwrap_or(0.0);
                                    let right_val = track
                                        .keyframes
                                        .iter()
                                        .find(|k| k.frame == rf)
                                        .map(|k| k.value)
                                        .unwrap_or(left_val);
                                    let range = (rf - lf) as f64;
                                    if let Some(lk) =
                                        track.keyframes.iter_mut().find(|k| k.frame == lf)
                                    {
                                        lk.interpolation =
                                            crate::app::InterpolationMode::Bezier;
                                        lk.handle_right = (
                                            (range * 0.33).clamp(1.0, range.max(1.0)),
                                            (right_val - left_val) * 0.33,
                                        );
                                    }
                                }
                            }
                            let after_timeline = app.project.anim_timeline.clone();
                            app.history.push(
                                &mut app.project,
                                crate::history::ProjectEdit::PatchTimeline {
                                    before: before_timeline,
                                    after: after_timeline,
                                },
                            );
                            app.anim_graph_selected_segment = None;
                            app.apply_animation_for_frame(app.anim_current_frame);
                        }
                    }
                }
                ui.add_space(4.0);
                if ui
                    .button(RichText::new("x Deselect").color(colors::TEXT_MUTED))
                    .clicked()
                {
                    app.anim_graph_selected_segment = None;
                    app.anim_graph_region_select = None;
                }
            });
        }
    });

    // Formula dialog window
    graph_stack_formula_dialog(app, ui.ctx());
}

fn graph_stack_header_controls(
    app: &mut VadadeeBerryApp,
    ui: &mut egui::Ui,
    node_id: crate::document::NodeId,
    stack_id: uuid::Uuid,
) {
    // Snapshot for UI, then write back.
    let Some(sf_snap) = app
        .project
        .anim_timeline
        .nodes
        .get(&node_id)
        .and_then(|a| a.stack_functions.iter().find(|s| s.id == stack_id))
        .cloned()
    else {
        return;
    };

    ui.separator();
    // Placement on the global timeline (where the stack sits).
    ui.label(
        RichText::new("On timeline")
            .small()
            .color(colors::TEXT_MUTED),
    );
    let mut start = sf_snap.start_frame as i32;
    let start_changed = ui
        .add(
            egui::DragValue::new(&mut start)
                .range(0..=100_000)
                .speed(1.0)
                .prefix("frame "),
        )
        .on_hover_text("Global timeline frame where this stack begins (f=0 inside the formula)")
        .changed();

    ui.label(RichText::new("Length").small().color(colors::TEXT_MUTED));
    let mut dur = sf_snap.duration_frames as i32;
    let dur_changed = ui
        .add(
            egui::DragValue::new(&mut dur)
                .range(1..=100_000)
                .speed(1.0)
                .suffix(" f"),
        )
        .on_hover_text("Relative length: formulas use t=0..1 and f=0..length (not global frame)")
        .changed();
    ui.label(
        RichText::new(format!(
            "rel 0{}{}",
            icons::ARROW_RIGHT,
            sf_snap.duration_frames.max(1)
        ))
        .font(nerd_font_id(11.0))
        .small()
        .color(colors::TEXT_MUTED),
    );

    ui.label(
        RichText::new("f(t)=")
            .small()
            .color(colors::TEXT_MUTED),
    )
    .on_hover_text(
        "t=0 at stack start, t=1 at stack end. f=0..length (local). \
x,y,r,g,b,a,s = start constants. abs(x) for positive; mod(a,m) or a%m (0..|m|). Empty = hold start.",
    );
    ui.label(RichText::new("(").color(colors::TEXT_MUTED));

    let mut exprs: Vec<String> = sf_snap.channels.iter().map(|c| c.expr.clone()).collect();
    let errs: Vec<Option<String>> = sf_snap.channels.iter().map(|c| c.last_error.clone()).collect();
    let mut expr_changed = false;
    let mut open_dialog: Option<usize> = None;

    for ci in 0..exprs.len() {
        if ci > 0 {
            ui.label(RichText::new(",").color(colors::TEXT_MUTED));
        }
        let has_err = errs.get(ci).and_then(|e| e.as_ref()).is_some();
        let mut te = egui::TextEdit::singleline(&mut exprs[ci])
            .desired_width(96.0)
            .hint_text("t,f,x,y…");
        if has_err {
            te = te
                .text_color(egui::Color32::from_rgb(255, 180, 180))
                .background_color(egui::Color32::from_rgb(60, 16, 16));
        }
        let r = ui.add(te);
        if let Some(Some(e)) = errs.get(ci) {
            r.clone().on_hover_text(e);
            ui.painter().rect_stroke(
                r.rect.expand(1.0),
                2.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 60, 60)),
                egui::StrokeKind::Outside,
            );
        }
        if r.changed() {
            expr_changed = true;
        }
        if r.double_clicked() {
            open_dialog = Some(ci);
        }
    }
    ui.label(RichText::new(")").color(colors::TEXT_MUTED));

    if ui
        .button(RichText::new("Delete stack").color(egui::Color32::from_rgb(255, 120, 120)))
        .on_hover_text("Remove this stack animation function")
        .clicked()
    {
        delete_stack_animation_function(app, node_id, stack_id);
        return;
    }

    if let Some(ci) = open_dialog {
        // Seed draft once on open; dialog edits this buffer until Apply/Cancel.
        app.anim_stack_formula_draft = exprs.get(ci).cloned().unwrap_or_default();
        app.anim_stack_formula_dialog = Some((node_id, stack_id, ci));
    }

    if start_changed || dur_changed || expr_changed {
        let before = app.project.anim_timeline.clone();
        if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) {
            if let Some(sf) = anim.stack_functions.iter_mut().find(|s| s.id == stack_id) {
                let old_start = sf.start_frame;
                let old_end = sf.end_frame();
                if start_changed {
                    sf.start_frame = start.max(0) as usize;
                }
                if dur_changed {
                    sf.duration_frames = dur.max(1) as usize;
                }
                if expr_changed {
                    for (ci, ch) in sf.channels.iter_mut().enumerate() {
                        if let Some(e) = exprs.get(ci) {
                            ch.expr = e.clone();
                            if ch.expr.trim().is_empty() {
                                ch.last_error = None;
                            } else if let Err(err) =
                                crate::document::eval_expr(&ch.expr, 0.5, 0.0)
                            {
                                // Silent UI-only error (no terminal spam while typing).
                                ch.last_error = Some(err.0);
                            } else {
                                ch.last_error = None;
                            }
                        }
                    }
                }
                let labels: Vec<String> = sf.channels.iter().map(|c| c.track.clone()).collect();
                let start_f = sf.start_frame;
                let end_f = sf.end_frame();
                let start_vals: Vec<(String, f64)> = sf
                    .channels
                    .iter()
                    .map(|c| (c.track.clone(), c.start_value))
                    .collect();
                let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                let lo = old_start.min(start_f);
                let hi = old_end.max(end_f);
                anim.clear_keyframes_under_stack(&refs, start_f, end_f);
                // Clear leftover keys from previous span when moving/resizing via UI.
                for label in &labels {
                    if let Some(tr) = anim.get_track_mut(label) {
                        tr.keyframes.retain(|kf| {
                            kf.frame == start_f || kf.frame < lo || kf.frame > hi
                        });
                    }
                }
                for (tr, v) in start_vals {
                    if let Some(track) = anim.get_track_mut(&tr) {
                        track.insert(start_f, v);
                    }
                }
                anim.ensure_stack_start_keyframes();
                anim.ensure_stack_end_keyframes();
            }
        }
        let after = app.project.anim_timeline.clone();
        app.history.push(
            &mut app.project,
            crate::history::ProjectEdit::PatchTimeline { before, after },
        );
        app.apply_animation_for_frame(app.anim_current_frame);
    }
}

fn apply_stack_animation_function(
    app: &mut VadadeeBerryApp,
    node_id: crate::document::NodeId,
    track_labels: &[String],
    defaults: &[f64],
    start: usize,
    end: usize,
) {
    let duration = end.saturating_sub(start).max(1);
    // Prefer live geometry (Pt X/Y etc.) over stale interpolated keys.
    let live_geom = app.get_node_geom_floats(node_id);
    let before = app.project.anim_timeline.clone();
    let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) else {
        return;
    };
    // Ensure geom_N slots exist before insert.
    for label in track_labels {
        anim.ensure_track(label);
    }
    let mut channels = Vec::new();
    for (i, label) in track_labels.iter().enumerate() {
        let def = if let Some(idx) = label
            .strip_prefix("geom_")
            .and_then(|s| s.parse::<usize>().ok())
        {
            live_geom
                .get(idx)
                .copied()
                .or_else(|| defaults.get(i).copied())
                .unwrap_or(0.0)
        } else {
            defaults.get(i).copied().unwrap_or(0.0)
        };
        // Exact key at stack start only — do not hold/interpolate distant keys (wrong for Pt stacks).
        let start_value = anim
            .get_track(label)
            .and_then(|t| {
                t.keyframes
                    .iter()
                    .find(|k| k.frame == start)
                    .map(|k| k.value)
            })
            .unwrap_or(def);
        channels.push(crate::document::StackAnimChannel {
            track: label.clone(),
            expr: String::new(),
            start_value,
            last_error: None,
        });
    }
    let labels_ref: Vec<&str> = track_labels.iter().map(|s| s.as_str()).collect();
    let end_f = start.saturating_add(duration);
    anim.clear_keyframes_under_stack(&labels_ref, start, end_f);
    // Editable initial keyframes at the start of the stack.
    for ch in &channels {
        if let Some(tr) = anim.get_track_mut(&ch.track) {
            tr.insert(start, ch.start_value);
        }
    }
    let id = uuid::Uuid::new_v4();
    anim.stack_functions.push(crate::document::StackAnimationFunction {
        id,
        start_frame: start,
        duration_frames: duration,
        channels,
    });
    anim.ensure_stack_start_keyframes();
    anim.ensure_stack_end_keyframes();
    let after = app.project.anim_timeline.clone();
    app.history.push(
        &mut app.project,
        crate::history::ProjectEdit::PatchTimeline { before, after },
    );
    app.anim_graph_selected_stack = Some(id);
    app.anim_graph_selected_segment = None;
    app.anim_graph_region_select = None;
    app.apply_animation_for_frame(app.anim_current_frame);
}

fn delete_stack_animation_function(
    app: &mut VadadeeBerryApp,
    node_id: crate::document::NodeId,
    stack_id: uuid::Uuid,
) {
    let before = app.project.anim_timeline.clone();
    let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) else {
        return;
    };
    if !anim.remove_stack_function_with_keyframes(stack_id) {
        return;
    }
    let after = app.project.anim_timeline.clone();
    app.history.push(
        &mut app.project,
        crate::history::ProjectEdit::PatchTimeline { before, after },
    );
    if app.anim_graph_selected_stack == Some(stack_id) {
        app.anim_graph_selected_stack = None;
    }
    app.apply_animation_for_frame(app.anim_current_frame);
}

fn graph_stack_formula_dialog(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    let Some((node_id, stack_id, ch_idx)) = app.anim_stack_formula_dialog else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let mut apply = false;
    let track_name = app
        .project
        .anim_timeline
        .nodes
        .get(&node_id)
        .and_then(|a| a.stack_functions.iter().find(|s| s.id == stack_id))
        .and_then(|s| s.channels.get(ch_idx))
        .map(|c| c.track.clone())
        .unwrap_or_default();
    // Live-validate the draft buffer (do not overwrite document until Apply).
    let draft_err = {
        let d = app.anim_stack_formula_draft.trim();
        if d.is_empty() {
            None
        } else {
            crate::document::eval_expr(d, 0.5, 0.0)
                .err()
                .map(|e| e.0)
        }
    };
    dialog_escape_close(ctx, &mut open);
    egui::Window::new(format!("Stack formula — {track_name}"))
        .id(egui::Id::new(("stack_formula_dialog", stack_id, ch_idx)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("Relative time: t=0 at start ")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.label(
                    RichText::new(icons::ARROW_RIGHT)
                        .font(nerd_font_id(11.0))
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.label(
                    RichText::new(
                        " t=1 at end; f=0..length. x,y,r,g,b,a,s = starts. Empty expr = hold start.",
                    )
                    .small()
                    .color(colors::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);
            let mut te = egui::TextEdit::multiline(&mut app.anim_stack_formula_draft)
                .id_source(("stack_formula_edit", stack_id, ch_idx))
                .desired_width(f32::INFINITY)
                .desired_rows(6);
            if draft_err.is_some() {
                te = te
                    .text_color(egui::Color32::from_rgb(255, 180, 180))
                    .background_color(egui::Color32::from_rgb(60, 16, 16));
            }
            ui.add(te);
            if let Some(ref e) = draft_err {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
            }
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if apply {
        let draft = app.anim_stack_formula_draft.clone();
        let before = app.project.anim_timeline.clone();
        if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) {
            if let Some(sf) = anim.stack_functions.iter_mut().find(|s| s.id == stack_id) {
                if let Some(ch) = sf.channels.get_mut(ch_idx) {
                    ch.expr = draft;
                    if ch.expr.trim().is_empty() {
                        ch.last_error = None;
                    } else if let Err(e) = crate::document::eval_expr(&ch.expr, 0.5, 0.0) {
                        // UI-only; no terminal log while editing.
                        ch.last_error = Some(e.0);
                    } else {
                        ch.last_error = None;
                    }
                }
            }
            anim.ensure_stack_end_keyframes();
        }
        let after = app.project.anim_timeline.clone();
        app.history.push(
            &mut app.project,
            crate::history::ProjectEdit::PatchTimeline { before, after },
        );
        app.apply_animation_for_frame(app.anim_current_frame);
        close = true;
    }
    if close || !open {
        app.anim_stack_formula_dialog = None;
        app.anim_stack_formula_draft.clear();
    }
}

/// Animation tab for Node Editor graph parameters (keyed on the layer id).
fn animation_node_editor_params(app: &mut VadadeeBerryApp, ui: &mut Ui, layer_id: uuid::Uuid) {
    let layer_name = app
        .project
        .document
        .layers
        .iter()
        .find(|l| l.id == layer_id)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| "Node Editor".into());
    ui.label(
        RichText::new(format!("Animation · Node Editor · {layer_name}"))
            .strong()
            .color(colors::ACCENT),
    );
    ui.label(
        RichText::new(format!("Current Frame: {}", app.anim_current_frame))
            .strong(),
    );
    ui.separator();

    let params: Vec<(uuid::Uuid, String, crate::document::GraphParamKind, f64, f64, f64, f64)> = {
        let Some(layer) = app
            .project
            .document
            .layers
            .iter()
            .find(|l| l.id == layer_id)
        else {
            return;
        };
        let Some(g) = layer.node_graph.as_ref() else {
            ui.label(
                RichText::new("No graph on this layer.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            return;
        };
        if g.parameters.is_empty() {
            ui.label(
                RichText::new("Add parameters on the Parameter tab, then keyframe them here.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            return;
        }
        g.parameters
            .iter()
            .map(|p| (p.id, p.name.clone(), p.kind, p.v0, p.v1, p.v2, p.v3))
            .collect()
    };

    let frame = app.anim_current_frame;
    let before_timeline = app.project.anim_timeline.clone();
    let mut entry = app
        .project
        .anim_timeline
        .nodes
        .entry(layer_id)
        .or_default()
        .clone();
    let mut entry_changed = false;
    let mut open_graph: Option<String> = None;

    for (pid, name, kind, v0, v1, v2, v3) in params {
        ui.group(|ui| {
            ui.label(RichText::new(&name).strong());
            let rows: Vec<(String, &str, f64)> = match kind {
                crate::document::GraphParamKind::Real => {
                    vec![(format!("param:{pid}"), "value", v0)]
                }
                crate::document::GraphParamKind::Color => vec![
                    (format!("param:{pid}:0"), "R", v0),
                    (format!("param:{pid}:1"), "G", v1),
                    (format!("param:{pid}:2"), "B", v2),
                    (format!("param:{pid}:3"), "A", v3),
                ],
                crate::document::GraphParamKind::Position => vec![
                    (format!("param:{pid}:0"), "X", v0),
                    (format!("param:{pid}:1"), "Y", v1),
                ],
            };
            for (lbl, comp_name, def) in rows {
                entry.ensure_track(&lbl);
                let track = entry
                    .param_tracks
                    .entry(lbl.clone())
                    .or_default();
                let has_kf = track.keyframes.iter().any(|kf| kf.frame == frame);
                let val = track.interpolate(frame).unwrap_or(def);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(comp_name).small());
                    if has_kf {
                        let mut v = val;
                        if ui
                            .add(egui::DragValue::new(&mut v).speed(0.05))
                            .changed()
                        {
                            track.insert(frame, v);
                            entry_changed = true;
                        }
                        if ui.button("🗑").on_hover_text("Delete keyframe").clicked() {
                            track.keyframes.retain(|kf| kf.frame != frame);
                            entry_changed = true;
                        }
                    } else {
                        ui.label(
                            RichText::new(format!("{val:.3} (interp)"))
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                        if ui.button("+").on_hover_text("Add keyframe").clicked() {
                            track.insert(frame, val);
                            entry_changed = true;
                        }
                    }
                    if ui
                        .small_button("Graph")
                        .on_hover_text("Open graph editor for this track")
                        .clicked()
                    {
                        open_graph = Some(lbl.clone());
                    }
                });
            }
        });
        ui.add_space(4.0);
    }

    if entry_changed {
        app.project.anim_timeline.nodes.insert(layer_id, entry);
        let after_timeline = app.project.anim_timeline.clone();
        app.history.push(
            &mut app.project,
            crate::history::ProjectEdit::PatchTimeline {
                before: before_timeline,
                after: after_timeline,
            },
        );
        app.apply_animation_for_frame(app.anim_current_frame);
    } else if open_graph.is_some() {
        // Ensure timeline slot exists so the graph editor can sample the track.
        app.project.anim_timeline.nodes.insert(layer_id, entry);
    }
    if let Some(lbl) = open_graph {
        app.anim_graph_editor_track = Some((layer_id, lbl));
        app.anim_graph_editor_target_track = None;
    }
}

fn animation_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    // Node Editor layer parameters (P3): animate when layer is active, with or without selection.
    let ne_layer_id = app
        .project
        .document
        .active_layer()
        .filter(|l| l.kind == crate::document::LayerKind::NodeEditor)
        .map(|l| l.id);
    if let Some(layer_id) = ne_layer_id {
        let sel_is_layer = app.selection.first().copied() == Some(layer_id);
        let sel_is_doc_node = app
            .selection
            .first()
            .is_some_and(|id| app.project.nodes.get(*id).is_some());
        if app.selection.is_empty() || sel_is_layer || !sel_is_doc_node {
            animation_node_editor_params(app, ui, layer_id);
            return;
        }
    }

    if app.selection.is_empty() {
        ui.label(RichText::new("Select one object to edit animation properties").color(colors::TEXT_MUTED));
        return;
    }
    let id = app.selection[0];

    let selected_point_indices: Vec<usize> = if app.tools.active == ToolKind::Node {
        app.tools.select.selected_path_points
            .iter()
            .filter(|(pid, _)| *pid == id)
            .map(|(_, pi)| *pi)
            .collect()
    } else {
        vec![]
    };
    
    let (name, curr_pos, curr_rot, curr_op, curr_color) = {
        let Some(node) = app.project.nodes.get(id) else {
            return;
        };
        (
            node.name.clone(),
            node.get_pos(),
            node.get_rotation(),
            node.get_opacity() as f64,
            node.get_color(),
        )
    };
    
    let is_output_proxy = app
        .project
        .document
        .ne_output_proxy_layer_index(id)
        .is_some();
    let title = if is_output_proxy {
        format!("Animation · Output Object ({name})")
    } else {
        format!("Animation for {name}")
    };
    ui.label(RichText::new(title).strong().color(colors::ACCENT));
    ui.add_space(4.0);
    ui.label(RichText::new(format!("Current Frame: {}", app.anim_current_frame)).strong());
    ui.separator();
    ui.add_space(4.0);

    if app.tools.active == ToolKind::Node {
        let multi: Vec<_> = app
            .tools
            .select
            .selected_path_points
            .iter()
            .filter(|(pid, _)| app.selection.contains(pid))
            .copied()
            .collect();
        if !multi.is_empty() {
            if multi.len() > 1 {
                ui.label(RichText::new(format!("{} points selected", multi.len())).strong());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().item_spacing.y = 4.0;
                    if ui.button("Smooth").on_hover_text("Smooth selected points").clicked() {
                        app.smooth_selected_path_points();
                    }
                    if ui
                        .button(RichText::new("Delete").color(colors::ALERT))
                        .on_hover_text("Delete selected points")
                        .clicked()
                    {
                        app.remove_selected_path_points();
                    }
                });
            } else if let Some((pid, point_idx)) = multi.first().copied() {
                let smooth = app
                    .project
                    .nodes
                    .get(pid)
                    .and_then(|n| match &n.kind {
                        NodeKind::Path { path } => Some(path.is_anchor_smooth(point_idx)),
                        _ => None,
                    })
                    .unwrap_or(false);
                path_point_bezier_panel(app, ui, pid, point_idx, smooth);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
            }
        }
    }

    let before_timeline = app.project.anim_timeline.clone();
    let mut entry = app.project.anim_timeline.nodes.entry(id).or_default().clone();
    let frame = app.anim_current_frame;

    let render_prop_row = |ui: &mut Ui, label: &str, track: &mut KeyframeTrack, default_val: f64, min: f64, max: f64, speed: f64| -> (bool, Option<f64>) {
        let has_kf = track.keyframes.iter().any(|kf| kf.frame == frame);
        let val = track.interpolate(frame).unwrap_or(default_val);
        
        let mut ret = (false, None);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong());
            ui.add_space(10.0);
            
            if has_kf {
                let mut v = val;
                let drag = ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(speed));
                if drag.changed() {
                    track.insert(frame, v);
                    ret = (true, Some(v));
                }
                
                if ui.button("🗑").on_hover_text("Delete keyframe").clicked() {
                    track.keyframes.retain(|kf| kf.frame != frame);
                    ret = (true, None);
                }
            } else {
                ui.label(RichText::new(format!("{:.2} (interp)", val)).color(colors::TEXT_MUTED));
                if ui.button("+").on_hover_text("Add keyframe").clicked() {
                    track.insert(frame, val);
                    ret = (true, Some(val));
                }
            }
        });
        ui.add_space(4.0);
        ret
    };

    let mut entry_changed = false;

    let mut track_x = entry.pos_x.clone();
    let (changed_x, val_x) = render_prop_row(ui, "Position X", &mut track_x, curr_pos.0, -10000.0, 10000.0, 1.0);
    if changed_x {
        entry.pos_x = track_x;
        entry_changed = true;
        if let Some(vx) = val_x {
            if let Some(n) = app.project.nodes.get_mut(id) {
                let p = n.get_pos();
                n.translate(vx - p.0, 0.0);
            }
        }
    }

    let mut track_y = entry.pos_y.clone();
    let (changed_y, val_y) = render_prop_row(ui, "Position Y", &mut track_y, curr_pos.1, -10000.0, 10000.0, 1.0);
    if changed_y {
        entry.pos_y = track_y;
        entry_changed = true;
        if let Some(vy) = val_y {
            if let Some(n) = app.project.nodes.get_mut(id) {
                let p = n.get_pos();
                n.translate(0.0, vy - p.1);
            }
        }
    }

    let mut track_rot = entry.rotation.clone();
    let (changed_rot, val_rot) = render_prop_row(ui, "Rotation", &mut track_rot, curr_rot.to_degrees(), -360.0, 360.0, 1.0);
    if changed_rot {
        entry.rotation = track_rot.clone();
        entry_changed = true;
        if let Some(vrot) = val_rot {
            app.convert_rect_to_path(id);
            if let Some(new_entry) = app.project.anim_timeline.nodes.get(&id) {
                entry = new_entry.clone();
                entry.rotation = track_rot;
            }
            if let Some(n) = app.project.nodes.get_mut(id) {
                n.set_rotation(vrot.to_radians());
            }
        }
    }

    let mut track_op = entry.opacity.clone();
    let (changed_op, val_op) = render_prop_row(ui, "Opacity", &mut track_op, curr_op, 0.0, 1.0, 0.01);
    if changed_op {
        entry.opacity = track_op;
        entry_changed = true;
        if let Some(vop) = val_op {
            if let Some(n) = app.project.nodes.get_mut(id) {
                n.set_opacity(vop as f32);
            }
        }
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("Fill Color").strong());
        ui.add_space(10.0);
        
        let has_r = entry.color_r.keyframes.iter().any(|kf| kf.frame == frame);
        
        let r = entry.color_r.interpolate(frame).unwrap_or(curr_color[0] as f64) as f32;
        let g = entry.color_g.interpolate(frame).unwrap_or(curr_color[1] as f64) as f32;
        let b = entry.color_b.interpolate(frame).unwrap_or(curr_color[2] as f64) as f32;
        let a = entry.color_a.interpolate(frame).unwrap_or(curr_color[3] as f64) as f32;
        
        let mut color_color32 = egui::Color32::from_rgba_unmultiplied(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        );
        
        if has_r {
            if ui.color_edit_button_srgba(&mut color_color32).changed() {
                let rgba = color_color32.to_array();
                let rf = rgba[0] as f64 / 255.0;
                let gf = rgba[1] as f64 / 255.0;
                let bf = rgba[2] as f64 / 255.0;
                let af = rgba[3] as f64 / 255.0;
                
                entry.color_r.insert(frame, rf);
                entry.color_g.insert(frame, gf);
                entry.color_b.insert(frame, bf);
                entry.color_a.insert(frame, af);
                entry_changed = true;
                
                if let Some(n) = app.project.nodes.get_mut(id) {
                    n.set_color([rf as f32, gf as f32, bf as f32, af as f32]);
                }
            }
            
            if ui.button("🗑").on_hover_text("Delete color keyframe").clicked() {
                entry.color_r.keyframes.retain(|kf| kf.frame != frame);
                entry.color_g.keyframes.retain(|kf| kf.frame != frame);
                entry.color_b.keyframes.retain(|kf| kf.frame != frame);
                entry.color_a.keyframes.retain(|kf| kf.frame != frame);
                entry_changed = true;
            }
        } else {
            let mut display_color = egui::Color32::from_rgba_unmultiplied(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                (a * 255.0) as u8,
            );
            ui.color_edit_button_srgba(&mut display_color);
            ui.label(RichText::new(" (interp)").color(colors::TEXT_MUTED));
            if ui.button("+").on_hover_text("Add color keyframe").clicked() {
                entry.color_r.insert(frame, r as f64);
                entry.color_g.insert(frame, g as f64);
                entry.color_b.insert(frame, b as f64);
                entry.color_a.insert(frame, a as f64);
                entry_changed = true;
            }
        }
    });

    // Stroke width
    let curr_sw = app
        .project
        .nodes
        .get(id)
        .map(|n| n.get_stroke_width() as f64)
        .unwrap_or(2.0);
    let mut track_sw = entry.stroke_width.clone();
    let (changed_sw, val_sw) =
        render_prop_row(ui, "Stroke Width", &mut track_sw, curr_sw, 0.0, 64.0, 0.1);
    if changed_sw {
        entry.stroke_width = track_sw;
        entry_changed = true;
        if let Some(v) = val_sw {
            if let Some(n) = app.project.nodes.get_mut(id) {
                n.set_stroke_width(v as f32);
            }
        }
    }

    // Stroke color
    let curr_sc = app
        .project
        .nodes
        .get(id)
        .map(|n| n.get_stroke_color())
        .unwrap_or([0.1, 0.1, 0.18, 1.0]);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Stroke Color").strong());
        ui.add_space(10.0);
        let has_sr = entry.stroke_r.keyframes.iter().any(|kf| kf.frame == frame);
        let r = entry
            .stroke_r
            .interpolate(frame)
            .unwrap_or(curr_sc[0] as f64) as f32;
        let g = entry
            .stroke_g
            .interpolate(frame)
            .unwrap_or(curr_sc[1] as f64) as f32;
        let b = entry
            .stroke_b
            .interpolate(frame)
            .unwrap_or(curr_sc[2] as f64) as f32;
        let a = entry
            .stroke_a
            .interpolate(frame)
            .unwrap_or(curr_sc[3] as f64) as f32;
        let mut c32 = egui::Color32::from_rgba_unmultiplied(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        );
        if has_sr {
            if ui.color_edit_button_srgba(&mut c32).changed() {
                let rgba = c32.to_array();
                let rf = rgba[0] as f64 / 255.0;
                let gf = rgba[1] as f64 / 255.0;
                let bf = rgba[2] as f64 / 255.0;
                let af = rgba[3] as f64 / 255.0;
                entry.stroke_r.insert(frame, rf);
                entry.stroke_g.insert(frame, gf);
                entry.stroke_b.insert(frame, bf);
                entry.stroke_a.insert(frame, af);
                entry_changed = true;
                if let Some(n) = app.project.nodes.get_mut(id) {
                    n.set_stroke_color([rf as f32, gf as f32, bf as f32, af as f32]);
                }
            }
            if ui.button("🗑").on_hover_text("Delete stroke color keyframe").clicked() {
                entry.stroke_r.keyframes.retain(|kf| kf.frame != frame);
                entry.stroke_g.keyframes.retain(|kf| kf.frame != frame);
                entry.stroke_b.keyframes.retain(|kf| kf.frame != frame);
                entry.stroke_a.keyframes.retain(|kf| kf.frame != frame);
                entry_changed = true;
            }
        } else {
            let mut display = c32;
            ui.color_edit_button_srgba(&mut display);
            ui.label(RichText::new(" (interp)").color(colors::TEXT_MUTED));
            if ui.button("+").on_hover_text("Add stroke color keyframe").clicked() {
                entry.stroke_r.insert(frame, r as f64);
                entry.stroke_g.insert(frame, g as f64);
                entry.stroke_b.insert(frame, b as f64);
                entry.stroke_a.insert(frame, a as f64);
                entry_changed = true;
            }
        }
    });

    // Handle geometry tracks
    let mut geom_floats = {
        let Some(_) = app.project.nodes.get(id) else {
            return;
        };
        app.get_node_geom_floats(id)
    };

    // Paths with many anchors (e.g. boolean results) must not expand thousands of
    // geom track rows / empty KeyframeTrack slots — that freezes the Animation tab.
    let is_path = app
        .project
        .nodes
        .get(id)
        .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }));
    let path_anchor_count = app
        .project
        .nodes
        .get(id)
        .and_then(|n| match &n.kind {
            NodeKind::Path { path } => Some(path.anchor_positions().len()),
            _ => None,
        })
        .unwrap_or(0);
    let path_geom_lazy = is_path && selected_point_indices.is_empty() && path_anchor_count > 8;
    
    if !geom_floats.is_empty() {
        ui.add_space(4.0);
        // Collapsible to keep the Animation tab compact (long path point lists overflow).
        let geom_default_open = !is_path || path_anchor_count <= 8 || !selected_point_indices.is_empty();
        let geom_header = egui::CollapsingHeader::new(
            RichText::new("Geometry Properties")
                .strong()
                .color(colors::POWERLINE_C),
        )
        .default_open(geom_default_open)
        .id_salt(("anim_geom_props", id));
        let geom_body = geom_header.show(ui, |ui| {
        if path_geom_lazy {
            ui.label(
                RichText::new(format!(
                    "Path has {path_anchor_count} points — per-point keyframes are hidden for performance."
                ))
                .small()
                .color(colors::TEXT_MUTED),
            );
            ui.label(
                RichText::new("Select points with the Node tool to keyframe only those points.")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
            // Still show existing keyframed geom tracks (if any) without allocating all slots.
            let existing: Vec<usize> = entry
                .geom_tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.keyframes.is_empty())
                .map(|(i, _)| i)
                .collect();
            if !existing.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} geom track(s) already have keyframes",
                        existing.len()
                    ))
                    .small()
                    .color(colors::ACCENT),
                );
            }
        }

        // Ensure we have enough keyframe tracks only for indices we will edit this frame.
        let need_tracks_upto = if path_geom_lazy {
            entry.geom_tracks.len()
        } else if is_path && !selected_point_indices.is_empty() {
            selected_point_indices
                .iter()
                .map(|p| p * 6 + 5)
                .max()
                .map(|m| m + 1)
                .unwrap_or(0)
                .min(geom_floats.len())
        } else if is_path {
            // Small paths: full geom is fine.
            geom_floats.len()
        } else {
            geom_floats.len()
        };
        while entry.geom_tracks.len() < need_tracks_upto {
            entry.geom_tracks.push(crate::app::KeyframeTrack::default());
        }

        // Gather human-readable labels and config
        let (geom_info, is_arc) = if let Some(node) = app.project.nodes.get(id) {
            let mut info = match &node.kind {
                NodeKind::Rect { .. } => vec![
                    ("Width".to_string(), 0.0, 10000.0, 1.0),
                    ("Height".to_string(), 0.0, 10000.0, 1.0),
                    ("Corner".to_string(), 0.0, 500.0, 0.5),
                ],
                NodeKind::Ellipse { .. } => vec![
                    ("RX".to_string(), 0.0, 10000.0, 1.0),
                    ("RY".to_string(), 0.0, 10000.0, 1.0),
                ],
                NodeKind::Polygon { .. } => vec![
                    ("R".to_string(), 0.0, 10000.0, 1.0),
                    ("Sides".to_string(), 3.0, 100.0, 1.0),
                    ("Rot°".to_string(), -360.0, 360.0, 1.0),
                ],
                NodeKind::Arc { .. } => vec![
                    ("R".to_string(), 0.0, 10000.0, 1.0),
                    ("Start°".to_string(), -360.0, 360.0, 1.0),
                    ("Sweep°".to_string(), -360.0, 360.0, 1.0),
                ],
                NodeKind::Path { path } => {
                    let mut v = Vec::new();
                    let num_anchors = path.anchor_positions().len();
                    // Only build labels for selected points (or all if few anchors).
                    let show_all = selected_point_indices.is_empty() && num_anchors <= 8;
                    for i in 0..num_anchors {
                        if !show_all && !selected_point_indices.contains(&i) {
                            // Placeholder so indices stay aligned; rows are skipped below.
                            for _ in 0..6 {
                                v.push((String::new(), -10000.0, 10000.0, 1.0));
                            }
                            continue;
                        }
                        // Short labels — UI shows "Pt N" with two interpolators, not long X/Y text.
                        v.push((format!("Pt {i}"), -10000.0, 10000.0, 1.0));
                        v.push((String::new(), -10000.0, 10000.0, 1.0));
                        v.push((format!("Out {i}"), -10000.0, 10000.0, 1.0));
                        v.push((String::new(), -10000.0, 10000.0, 1.0));
                        v.push((format!("In {i}"), -10000.0, 10000.0, 1.0));
                        v.push((String::new(), -10000.0, 10000.0, 1.0));
                    }
                    v
                }
                NodeKind::BrushStroke { points } => {
                    let mut v = Vec::new();
                    for i in 0..points.len() {
                        v.push((format!("Stroke {} X", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Stroke {} Y", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Stroke {} Width", i), 0.1, 500.0, 0.5));
                    }
                    v
                }
                _ => Vec::new(),
            };
            
            // Append path magic properties to info:
            if let Some(_) = app.project.document.tiling_effects.values().find(|e| e.source_id == id) {
                info.push(("Tiling Gap X".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Tiling Gap Y".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Tiling Count X".to_string(), 1.0, 1000.0, 1.0));
                info.push(("Tiling Count Y".to_string(), 1.0, 1000.0, 1.0));
                info.push(("Tiling Offset X".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Tiling Offset Y".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Tiling Row Rot".to_string(), -360.0, 360.0, 1.0));
                info.push(("Tiling Col Rot".to_string(), -360.0, 360.0, 1.0));
                info.push(("Tiling Row Scale".to_string(), -100.0, 100.0, 0.05));
                info.push(("Tiling Col Scale".to_string(), -100.0, 100.0, 0.05));
            }
            if let Some(_) = app.project.document.circular_effects.values().find(|e| e.source_id == id) {
                info.push(("Circular X".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Circular Y".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Circular Radius".to_string(), 0.0, 10000.0, 1.0));
                info.push(("Circular Copies".to_string(), 1.0, 1000.0, 1.0));
                info.push(("Circular Angle".to_string(), -360.0, 360.0, 1.0));
                info.push(("Circular Base X".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Circular Base Y".to_string(), -10000.0, 10000.0, 1.0));
            }
            if let Some(_) = app.project.document.path_effects.values().find(|e| e.source_id == id) {
                info.push(("Path Gap".to_string(), 0.1, 10000.0, 1.0));
                info.push(("Path Count".to_string(), 1.0, 1000.0, 1.0));
                info.push(("Path Offset".to_string(), -10000.0, 10000.0, 1.0));
                info.push(("Path End Scale".to_string(), 0.0, 10.0, 0.05));
                info.push(("Path End Opacity".to_string(), 0.0, 1.0, 0.02));
            }

            let is_arc = matches!(node.kind, NodeKind::Arc { .. });
            (info, is_arc)
        } else {
            (Vec::new(), false)
        };

                for i in 0..geom_floats.len() {
                    if path_geom_lazy {
                        break; // message already shown; do not paint thousands of rows
                    }
                    if let Some(node) = app.project.nodes.get(id) {
                        if matches!(&node.kind, NodeKind::Path { .. }) {
                            let pt_idx = i / 6;
                            let few = path_anchor_count <= 8 && selected_point_indices.is_empty();
                            if !few && !selected_point_indices.contains(&pt_idx) {
                                continue;
                            }
                        }
                    }
                    let (label, min, max, speed) = if i < geom_info.len() {
                        geom_info[i].clone()
                    } else {
                        (format!("Property {}", i), -10000.0, 10000.0, 1.0)
                    };

                    let is_polygon = app.project.nodes.get(id).map_or(false, |n| matches!(n.kind, NodeKind::Polygon { .. }));
                    let is_angle = (is_arc && (i == 1 || i == 2))
                        || (is_polygon && i == 2);

                    if let Some(node) = app.project.nodes.get(id) {
                        if matches!(&node.kind, NodeKind::Path { .. }) {
                            let sub = i % 6;
                            if sub == 0 || sub == 2 || sub == 4 {
                                // One short label + two interpolators (X/Y) — no "Pt 11 (X/Y)" overflow.
                                let pt_idx = i / 6;
                                let base_label = match sub {
                                    0 => format!("Pt {pt_idx}"),
                                    2 => format!("Out {pt_idx}"),
                                    4 => format!("In {pt_idx}"),
                                    _ => label.clone(),
                                };
                                let mut t1 = entry.geom_tracks[i].clone();
                                let mut t2 = if i + 1 < entry.geom_tracks.len() {
                                    entry.geom_tracks[i + 1].clone()
                                } else {
                                    crate::app::KeyframeTrack::default()
                                };
                                let current1 = geom_floats[i];
                                let current2 = if i + 1 < geom_floats.len() {
                                    geom_floats[i + 1]
                                } else {
                                    0.0
                                };
                                // Compact two-value row that stays inside the panel width.
                                ui.horizontal(|ui| {
                                    let row_w = ui.available_width().max(80.0);
                                    ui.set_max_width(row_w);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&base_label).strong().small(),
                                        )
                                        .truncate(),
                                    );
                                    let drag_w = ((row_w - 56.0) * 0.5).clamp(40.0, 72.0);

                                    let has1 = t1.keyframes.iter().any(|kf| kf.frame == frame);
                                    let val1 = t1.interpolate(frame).unwrap_or(current1);
                                    let mut v1 = val1;
                                    let d1 = ui
                                        .add(
                                            egui::DragValue::new(&mut v1)
                                                .range(min..=max)
                                                .speed(speed)
                                                .prefix("x ")
                                                .min_decimals(1)
                                                .max_decimals(2),
                                        )
                                        .on_hover_text("X");
                                    let _ = drag_w;
                                    if d1.changed() {
                                        t1.insert(frame, v1);
                                        entry_changed = true;
                                        geom_floats[i] = v1;
                                        app.set_node_geom_floats(id, &geom_floats);
                                    }
                                    if has1 {
                                        if ui
                                            .small_button("×")
                                            .on_hover_text("Delete X keyframe")
                                            .clicked()
                                        {
                                            t1.keyframes.retain(|kf| kf.frame != frame);
                                            entry_changed = true;
                                        }
                                    } else if ui
                                        .small_button("+")
                                        .on_hover_text("Add X keyframe")
                                        .clicked()
                                    {
                                        t1.insert(frame, val1);
                                        entry_changed = true;
                                    }

                                    let has2 = t2.keyframes.iter().any(|kf| kf.frame == frame);
                                    let val2 = t2.interpolate(frame).unwrap_or(current2);
                                    let mut v2 = val2;
                                    let d2 = ui
                                        .add(
                                            egui::DragValue::new(&mut v2)
                                                .range(min..=max)
                                                .speed(speed)
                                                .prefix("y ")
                                                .min_decimals(1)
                                                .max_decimals(2),
                                        )
                                        .on_hover_text("Y");
                                    if d2.changed() {
                                        t2.insert(frame, v2);
                                        entry_changed = true;
                                        if i + 1 < geom_floats.len() {
                                            geom_floats[i + 1] = v2;
                                            app.set_node_geom_floats(id, &geom_floats);
                                        }
                                    }
                                    if has2 {
                                        if ui
                                            .small_button("×")
                                            .on_hover_text("Delete Y keyframe")
                                            .clicked()
                                        {
                                            t2.keyframes.retain(|kf| kf.frame != frame);
                                            entry_changed = true;
                                        }
                                    } else if ui
                                        .small_button("+")
                                        .on_hover_text("Add Y keyframe")
                                        .clicked()
                                    {
                                        t2.insert(frame, val2);
                                        entry_changed = true;
                                    }
                                });
                                entry.geom_tracks[i] = t1;
                                if i + 1 < entry.geom_tracks.len() {
                                    entry.geom_tracks[i + 1] = t2;
                                }
                                continue;
                            }
                        }
                    }
                    
                    let mut track_geom = entry.geom_tracks[i].clone();
                    
                    // Adjust defaults/values for radian <-> degree conversion
                    let current_val = if is_angle {
                        geom_floats[i].to_degrees()
                    } else {
                        geom_floats[i]
                    };

                    // In order to use render_prop_row correctly, we convert values in track_geom to degrees temporarily if it's an angle track
                    if is_angle {
                        for kf in &mut track_geom.keyframes {
                            kf.value = kf.value.to_degrees();
                        }
                    }

                    let (changed_geom, val_geom) = render_prop_row(
                        ui,
                        &label,
                        &mut track_geom,
                        current_val,
                        min,
                        max,
                        speed,
                    );

                    if changed_geom {
                        // If value changed or deleted, convert back to radians if necessary
                        if is_angle {
                            for kf in &mut track_geom.keyframes {
                                kf.value = kf.value.to_radians();
                            }
                        }
                        entry.geom_tracks[i] = track_geom;
                        entry_changed = true;

                        if let Some(vg) = val_geom {
                            let rad_vg = if is_angle { vg.to_radians() } else { vg };
                            geom_floats[i] = rad_vg;
                            app.set_node_geom_floats(id, &geom_floats);
                        }
                    }
                }
        }); // end Geometry Properties collapsing header
        // Propagate entry_changed from inner edits (already set on entry_changed flag).
        let _ = geom_body;
    }
    // Selected keyframe panel inside Action Bar > Animation Tab
    let mut delete_kf_target = None; // (track, frame)
    if let Some((sel_node_id, ref sel_track_lbl, sel_frame)) = app.anim_selected_keyframe.clone() {
        if sel_node_id == id {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("SELECTED KEYFRAME").strong().color(colors::ACCENT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(icons::CLOSE).font(nerd_font_id(12.0))).clicked() {
                            app.anim_selected_keyframe = None;
                        }
                    });
                });
                ui.add_space(4.0);
                
                if let Some(track) = entry.get_track_mut(&sel_track_lbl) {
                    if let Some(idx) = track.keyframes.iter().position(|k| k.frame == sel_frame) {
                        let next_kf_val = track.keyframes.iter()
                            .find(|k| k.frame > sel_frame)
                            .map(|k| (k.frame, k.value));
                        
                        let kf = &mut track.keyframes[idx];
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Track: {}", sel_track_lbl)).color(colors::TEXT_MUTED));
                            ui.add_space(10.0);
                            ui.label(RichText::new(format!("Frame: {}", kf.frame)).color(colors::TEXT_MUTED));
                        });
                        ui.add_space(4.0);
                        
                        ui.horizontal(|ui| {
                            ui.label("Value:");
                            let drag = ui.add(egui::DragValue::new(&mut kf.value).speed(0.1));
                            if drag.changed() {
                                entry_changed = true;
                            }
                        });
                        ui.add_space(4.0);
                        
                        ui.horizontal(|ui| {
                            ui.label("Interpolation:");
                            let mut interp = kf.interpolation;
                            let _combo = egui::ComboBox::from_id_salt("act_kf_interp_combo")
                                .selected_text(match interp {
                                    crate::app::InterpolationMode::Linear => "Linear",
                                    crate::app::InterpolationMode::Bezier => "Bezier/Smooth",
                                })
                                .show_ui(ui, |ui| {
                                    if ui.selectable_value(&mut interp, crate::app::InterpolationMode::Linear, "Linear").clicked() {
                                        kf.interpolation = crate::app::InterpolationMode::Linear;
                                        entry_changed = true;
                                    }
                                    if ui.selectable_value(&mut interp, crate::app::InterpolationMode::Bezier, "Bezier/Smooth").clicked() {
                                        kf.interpolation = crate::app::InterpolationMode::Bezier;
                                        if let Some((next_frame, next_value)) = next_kf_val {
                                            kf.handle_right = (
                                                (next_frame - kf.frame) as f64 * 0.5,
                                                (next_value - kf.value) * 0.5
                                            );
                                        } else {
                                            kf.handle_right = (5.0, 0.0);
                                        }
                                        entry_changed = true;
                                    }
                                });
                        });
                        
                        ui.add_space(8.0);
                        if ui.button(RichText::new("🗑 Delete Keyframe").color(egui::Color32::from_rgb(230, 80, 80))).clicked() {
                            delete_kf_target = Some((sel_track_lbl.clone(), sel_frame));
                        }
                    }
                }
            });
        }
    }

    // Only write the cloned entry back when the user edited something. Always inserting
    // would stomp concurrent timeline/keyframe updates with a stale clone.
    if entry_changed {
        app.project.anim_timeline.nodes.insert(id, entry);
    }
    
    if let Some((track, frame)) = delete_kf_target {
        app.delete_keyframe(id, &track, frame);
    } else if entry_changed {
        let after_timeline = app.project.anim_timeline.clone();
        app.history.push(
            &mut app.project,
            crate::history::ProjectEdit::PatchTimeline { before: before_timeline, after: after_timeline },
        );
        app.apply_animation_for_frame(app.anim_current_frame);
    }
}
