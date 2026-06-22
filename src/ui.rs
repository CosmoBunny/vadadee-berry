use egui::{scroll_area::ScrollBarVisibility, Context, FontFamily, FontId, Rect, RichText, ScrollArea, Ui};

use crate::animation::action_bar_overlay_rect;
use crate::app::{KeyframeTrack, VadadeeBerryApp};
use crate::document::{
    compute_whole_object_bounds, compute_tiling_whole_bounds, compute_circular_whole_bounds, default_loft_gap_for_node, find_effect_for_pair, ArcJoin, FillKind, GeometryProfile, LineCap,
    LineJoin, NodeKind, OnPathMode, PathData, TextStyle, A4_HEIGHT_PX, A4_WIDTH_PX,
};
use crate::gradient_ui::{
    apply_angle_to_flow_line, gradient_flow_line_editor, gradient_strip_editor,
    linear_gradient_angle_dial, paint_kind_selector, solid_color_editor, sync_angle_from_flow_line,
    GradientEditorFocus,
};
use crate::icons::{self, nerd_font_id};
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
}

impl ActionTab {
    pub fn all_tabs() -> Vec<Self> {
        vec![
            Self::Export,
            Self::Layer,
            Self::ColorStroke,
            Self::Objects,
            Self::Geometry,
            Self::PathMagic,
            Self::Animation,
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
        }
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
        app.tools.active,
        app.action_tab,
        &action_text,
        msg_width,
        tool_width,
        &coords_text,
        coords_width,
    );
    app.ui_anim.advance_action_bar_slide(ui.ctx());
    app.ui_anim.advance_timeline_slide(ui.ctx());
    app.ui_anim.tick(ui.ctx());
    status_bar(app, ui);

    let canvas_alpha = app.ui_anim.canvas_alpha();
    egui::CentralPanel::default()
        .frame(theme::canvas_frame(canvas_alpha))
        .show_inside(ui, |ui| {
            let work = ui.available_rect_before_wrap();
            app.canvas_ui(ui);
            app.tools.handle_shortcuts(ui);
            let ctx = ui.ctx().clone();
            floating_toolbar(app, &ctx, work);
            floating_action_bar(app, &ctx, work);
            floating_timeline_window(app, &ctx, work);
        });

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
                    if ui.button("Open SVG…   Ctrl+O").clicked() {
                        app.request_open_svg();
                        ui.close();
                    }
                    if ui.button("Import Image…").clicked() {
                        app.request_import_image();
                        ui.close();
                    }
                    if ui.button("Save project…   Ctrl+S").clicked() {
                        app.request_save_project();
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
                    if ui.button("Raise").clicked() {
                        app.nudge_z_order(1);
                        ui.close();
                    }
                    if ui.button("Lower").clicked() {
                        app.nudge_z_order(-1);
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut app.viewport.show_grid, "Show grid");
                    ui.checkbox(&mut app.viewport.snap_grid, "Snap to grid");
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

    let collapsed_inner_w = btn_size;
    let collapsed_inner_h = btn_size;

    let expanded_inner_w = 3.0 * btn_size + 2.0 * spacing;
    let expanded_inner_h = 5.0 * btn_size + 4.0 * spacing;

    // Use egui's built-in bool animator for smooth transitions
    let expand_t = ctx.animate_bool(egui::Id::new("toolbar_expanded_anim"), app.toolbar_expanded);

    let inner_w = egui::lerp(collapsed_inner_w..=expanded_inner_w, expand_t);
    let inner_h = egui::lerp(collapsed_inner_h..=expanded_inner_h, expand_t);

    let rect = Rect::from_min_size(
        inset.min,
        egui::vec2(inner_w + 2.0 * margin_x, inner_h + 2.0 * margin_y),
    );

    // Tools list
    let tools = [
        ToolKind::Select,
        ToolKind::Node,
        ToolKind::Pen,
        ToolKind::Rectangle,
        ToolKind::Circle,
        ToolKind::Ellipse,
        ToolKind::Line,
        ToolKind::Polygon,
        ToolKind::Arc,
        ToolKind::Text,
        ToolKind::Brush,
        ToolKind::Eyedropper,
    ];

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

        // Draw ColorPicker at index 12
        if expand_t > 0.01 {
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
                        let selected = app.action_tab == tab;
                        let tab_alpha = app.ui_anim.tab_label_alpha(selected);
                        let label = format!("{} {}", tab.icon(), tab.label());
                        let resp = theme::action_tab_chip(ui, selected, &label, tab_alpha)
                            .on_hover_text(tab.label());
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
    ui.add_space(4.0);

    let has_points_panel = app.tools.select.selected_path_segment.is_some()
        || !app.tools.select.selected_path_points.is_empty();
    if has_points_panel {
        path_magic_card(ui, app, "Path points", |ui, app| {
            if let Some((id, from, to)) = app.tools.select.selected_path_segment {
                if app.selection.contains(&id) {
                    ui.label(
                        RichText::new(format!("Segment · points {from} & {to}"))
                            .color(colors::TEXT_MUTED),
                    );
                }
            } else {
                let multi: Vec<_> = app
                    .tools
                    .select
                    .selected_path_points
                    .iter()
                    .filter(|(id, _)| app.selection.contains(id))
                    .copied()
                    .collect();
                if multi.len() > 1 {
                    ui.label(
                        RichText::new(format!("{} points selected", multi.len()))
                            .color(colors::TEXT_MUTED),
                    );
                    if ui.button("Smooth selected").clicked() {
                        app.smooth_selected_path_points();
                    }
                } else if let Some((id, point_idx)) = multi.first().copied() {
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
        });
    }

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
            ui.label("Gap X");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_gap_x).speed(1.0)).changed();
            ui.label("Y");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_gap_y).speed(1.0)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Offset X");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_offset_x).speed(1.0)).changed();
            ui.label("Y");
            changed |= ui.add(decimal_drag(&mut app.ui_tiling_offset_y).speed(1.0)).changed();
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
        ui.separator();
        ui.label(RichText::new("CircularClone").strong());
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Copies");
            changed |= ui.add(decimal_drag(&mut app.ui_circular_copies).range(3..=32)).changed();
            ui.label("Angle °");
            changed |= ui.add(decimal_drag(&mut app.ui_circular_angle_offset).speed(1.0)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Origin X");
            changed |= ui.add(decimal_drag(&mut app.ui_circular_origin_x).speed(1.0)).changed();
            ui.label("Y");
            changed |= ui.add(decimal_drag(&mut app.ui_circular_origin_y).speed(1.0)).changed();
        });
        ui.horizontal(|ui| {
            if ui.button("Bake as group").clicked() {
                app.bake_circular();
            }
            if ui.button("Remove").clicked() {
                app.remove_circular_effect();
                ui.ctx().request_repaint();
            }
        });
        if changed {
            app.update_circular_effects_live();
            ui.ctx().request_repaint();
        }
    }

    if path_ids.is_empty() && app.object_on_path_panel_context().is_none() {
        // Show Tiling and CircularClone apply when only facial objects (e.g. Circle) selected, and not yet enabled
        let facial_objects: Vec<_> = app.selection.iter().filter(|&&id| {
            app.project.nodes.get(id).map_or(false, |n| !matches!(&n.kind, NodeKind::Path { .. } | NodeKind::Group { .. }))
        }).cloned().collect();
        let has_t_or_c = app.selection_has_tiling_effect() || app.selection_has_circular_effect();
        if !facial_objects.is_empty() && !has_t_or_c {
            ui.label(RichText::new("Path Magic (separate traits)").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Tiling (size gap)").clicked() {
                    app.apply_tiling_magic();
                }
                if ui.button("CircularClone (6 sides)").clicked() {
                    app.apply_circular_clone_magic();
                }
            });
            ui.add_space(8.0);
        }
        if !has_t_or_c {
            ui.label(
                RichText::new("Select path(s) or one path + object(s).")
                    .color(colors::TEXT_MUTED),
            );
        }
        return;
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
}

fn status_bar(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    let alpha = app.ui_anim.status_alpha();
    let tool_slide_out = app.ui_anim.status_tool_slide_out(120.0);
    let tool_slide_in = app.ui_anim.status_tool_slide_in(120.0);
    let msg_slide_out = app.ui_anim.status_slide_out();
    let msg_slide_in = app.ui_anim.status_slide_in();
    let tool_width = app.ui_anim.status_tool_seg_width();
    let msg_width = app.ui_anim.status_message_seg_width();
    egui::Panel::bottom("status")
        .frame(theme::bar_frame(alpha))
        .exact_size(30.0)
        .resizable(false)
        .show_inside(ui, |ui| {
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
                    // timeline toggle
                    let timeline_btn_icon = if app.anim_show_timeline_window { "" } else { "" };
                    let timeline_btn_tooltip = if app.anim_show_timeline_window { "Hide timeline" } else { "Show timeline" };
                    let btn_timeline = ui.button(RichText::new(timeline_btn_icon).font(nerd_font_id(12.0)));
                    if btn_timeline.clicked() {
                        app.anim_show_timeline_window = !app.anim_show_timeline_window;
                    }
                    btn_timeline.on_hover_text(timeline_btn_tooltip);

                    ui.add_space(4.0);

                    // playback controls
                    let play_icon = if app.anim_is_playing { "" } else { "" };
                    let play_tooltip = if app.anim_is_playing { "Pause" } else { "Play" };
                    
                    let btn_rewind = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_rewind.clicked() {
                        app.anim_current_frame = 0;
                        app.anim_is_playing = false;
                    }
                    btn_rewind.on_hover_text("Back to start");

                    let btn_prev = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_prev.clicked() {
                        app.anim_current_frame = if app.anim_current_frame == 0 { 100 } else { app.anim_current_frame - 1 };
                    }
                    btn_prev.on_hover_text("Backward (1 frame)");

                    let btn_play = ui.button(RichText::new(play_icon).font(nerd_font_id(12.0)));
                    if btn_play.clicked() {
                        app.anim_is_playing = !app.anim_is_playing;
                    }
                    btn_play.on_hover_text(play_tooltip);

                    let btn_next = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_next.clicked() {
                        app.anim_current_frame = (app.anim_current_frame + 1) % 101;
                    }
                    btn_next.on_hover_text("Forward (1 frame)");

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
        ui.label("Preset");
        if ui.button("A4").clicked() {
            app.set_page_size(A4_WIDTH_PX, A4_HEIGHT_PX);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Size");
        let mut w = app.project.document.width as f32;
        let mut h = app.project.document.height as f32;
        let ch = ui.add(decimal_drag(&mut w).range(64.0..=8192.0).suffix("w"));
        let ch2 = ui.add(decimal_drag(&mut h).range(64.0..=8192.0).suffix("h"));
        if ch.changed() || ch2.changed() {
            app.set_page_size(w as f64, h as f64);
        }
    });
}

fn layers_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    if ui.button("+ New layer").clicked() {
        app.add_layer("Layer");
    }
    let layer_count = app.project.document.layers.len();
    for i in 0..layer_count {
        let active = app.project.document.active_layer_index == i;
        let (name, visible, locked) = {
            let l = &app.project.document.layers[i];
            (l.name.clone(), l.visible, l.locked)
        };
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
            let mut edit_name = name;
            if ui.text_edit_singleline(&mut edit_name).changed() {
                app.rename_layer(i, edit_name);
            }
        });
    }
}

fn objects_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(format!("{} selected", app.selection.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("▲").on_hover_text("Raise").clicked() {
                app.nudge_z_order(1);
            }
            if ui.small_button("▼").on_hover_text("Lower").clicked() {
                app.nudge_z_order(-1);
            }
            if ui.small_button("⧉").on_hover_text("Duplicate").clicked() {
                app.duplicate_selection();
            }
        });
    });
    let object_ids: Vec<_> = app
        .project
        .document
        .active_layer()
        .map(|l| l.nodes.clone())
        .unwrap_or_default();
    for id in object_ids.iter().rev() {
        let Some(node) = app.project.nodes.get(*id) else {
            continue;
        };
        let selected = app.selection.contains(id);
        let icon = node_icon(&node.kind);
        let label = RichText::new(format!("{icon} {}", node.name)).font(nerd_font_id(13.0));
        ui.horizontal(|ui| {
            if ui.selectable_label(selected, label).clicked() {
                app.set_selection(vec![*id]);
            }
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
                    app.delete_nodes(&[*id]);
                }
                delete_btn.on_hover_text("Delete object");
            });
        });
    }
}

fn appearance_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
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

fn geometry_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    if app.tools.active == ToolKind::Brush {
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
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.tools.brush.brush_type, crate::tools::BrushType::Standard, "Standard");
                    ui.selectable_value(&mut app.tools.brush.brush_type, crate::tools::BrushType::Pen, "Pen");
                });
            
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut app.tools.brush.size, 1.0..=100.0).text("Size"));
            ui.add(egui::Slider::new(&mut app.tools.brush.smoothness, 0.0..=1.0).text("Smoothness"));
            ui.add(egui::Slider::new(&mut app.tools.brush.heavy, 0.0..=1.0).text("Heavybrush"));

            if app.tools.brush.brush_type == crate::tools::BrushType::Pen {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(RichText::new("Pen Tip 3D Preview").strong());
                ui.add_space(4.0);
                
                let is_drawing = !app.tools.brush.points.is_empty();
                let active_width = if is_drawing {
                    app.tools.brush.points.last().map(|&(_, _, w)| w).unwrap_or(app.tools.brush.size)
                } else {
                    app.tools.brush.size
                };
                draw_3d_pen_tip(ui, active_width, is_drawing);
            }
        });
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
            if ui.button("Smooth selected points").clicked() {
                app.smooth_selected_path_points();
            }
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
        GeometryProfile::ClosedPath { vertices, cyclic } => {
            ui.label(RichText::new("Closed path").strong());
            ui.label(format!("Vertices: {vertices}"));
            ui.label(format!("Cyclic: {cyclic}"));
            ui.label(
                RichText::new("Fill enabled — drag points with the node tool (N)")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        GeometryProfile::OpenPath { vertices, cyclic } => {
            ui.label(RichText::new("Open path").strong());
            ui.label(format!("Vertices: {vertices}"));
            ui.label(format!("Cyclic: {cyclic}"));
            ui.label(
                RichText::new("Not cyclic — close the path in Color & stroke to apply fill")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
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
        NodeKind::Arc { .. } => icons::ARC,
        NodeKind::BrushStroke { .. } => icons::BRUSH,
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
    edit_mode: bool,
    dragged_keyframe: &mut Option<(crate::document::NodeId, String, usize)>,
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
        let visible_frames = 100.0;
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
                    
                    let kf_color = if is_hovered || is_being_dragged {
                        colors::ACCENT
                    } else {
                        plot.color
                    };
                    
                    painter.circle(
                        center,
                        4.5,
                        kf_color,
                        egui::Stroke::new(1.0, colors::BG_PANEL),
                    );
                    
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
        
        // Mouse wheel scroll to pan timeline (horizontal scroll only!)
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        if scroll_delta.x != 0.0 && response.hovered() {
            *timeline_scroll = (*timeline_scroll - scroll_delta.x * 0.1).max(0.0);
        }
        
        // Drag interaction
        if dragged_keyframe.is_some() {
            // Dragging keyframe: do not scrub playhead
        } else if response.dragged_by(egui::PointerButton::Primary) || response.clicked_by(egui::PointerButton::Primary) {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let relative_x = mouse_pos.x - rect.left();
                let raw_frame = start_frame + (relative_x / rect.width() * visible_frames);
                *current_frame = raw_frame.round().max(0.0) as usize;
            }
        } else if response.dragged_by(egui::PointerButton::Secondary) {
            let delta_x = ui.input(|i| i.pointer.delta().x);
            let frames_pan = delta_x / rect.width() * visible_frames;
            *timeline_scroll = (*timeline_scroll - frames_pan).max(0.0);
        }
    });
}

fn timeline_interior(app: &mut VadadeeBerryApp, ui: &mut Ui) {
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
            });
        });
        
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let selected_node_id = app.selection.first().copied();
        let mut curr_frame = app.anim_current_frame;
        let mut scroll = app.anim_timeline_scroll;
        let edit_mode = app.anim_edit_mode;
        
        let mut dragged = app.anim_dragged_keyframe.clone();
        
        if let Some(node_id) = selected_node_id {
            if let Some(anim) = app.anim_timeline.nodes.get_mut(&node_id) {
                // Determine which tracks have keyframes
                let has_pos = !anim.pos_x.keyframes.is_empty() || !anim.pos_y.keyframes.is_empty();
                let has_rot = !anim.rotation.keyframes.is_empty();
                let has_op = !anim.opacity.keyframes.is_empty();
                let has_col = !anim.color_r.keyframes.is_empty() 
                    || !anim.color_g.keyframes.is_empty() 
                    || !anim.color_b.keyframes.is_empty() 
                    || !anim.color_a.keyframes.is_empty();
                
                ScrollArea::vertical()
                    .id_salt("timeline_tracks_scroll")
                    .max_height(80.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        
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
                                "Position",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                edit_mode,
                                &mut dragged,
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
                                "Rotation",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                edit_mode,
                                &mut dragged,
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
                                edit_mode,
                                &mut dragged,
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
                                "Color",
                                Some(node_id),
                                &mut plots,
                                &mut curr_frame,
                                &mut scroll,
                                edit_mode,
                                &mut dragged,
                            );
                        }
                    });
            }
        }
        
        app.anim_dragged_keyframe = dragged;

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
    let card_w = inset.width() - 2.0 * gap - width_reduction;
    let card_h = 120.0;
    
    let left = inset.left() + gap;
    let open_top = inset.bottom() - card_h - gap - 30.0;
    let travel = card_h + gap + 30.0;
    let top = open_top + (1.0 - open_t) * travel;
    
    let rect = Rect::from_min_size(egui::pos2(left, top), egui::vec2(card_w, card_h));
    let opacity = egui::emath::easing::cubic_out(open_t);

    theme::show_action_bar_area(ctx, "floating_timeline", rect, opacity, |ui| {
        timeline_interior(app, ui);
    });
}

fn animation_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    if app.selection.is_empty() {
        ui.label(RichText::new("Select one object to edit animation properties").color(colors::TEXT_MUTED));
        return;
    }
    let id = app.selection[0];
    
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
    
    ui.label(RichText::new(format!("Animation for {}", name)).strong().color(colors::ACCENT));
    ui.add_space(4.0);
    ui.label(RichText::new(format!("Current Frame: {}", app.anim_current_frame)).strong());
    ui.separator();
    ui.add_space(4.0);

    let mut entry = app.anim_timeline.nodes.entry(id).or_default().clone();
    let frame = app.anim_current_frame;

    let mut render_prop_row = |ui: &mut Ui, label: &str, track: &mut KeyframeTrack, default_val: f64, min: f64, max: f64, speed: f64| -> (bool, Option<f64>) {
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
                ui.label(RichText::new(format!("{:.2} (interpolated)", val)).color(colors::TEXT_MUTED));
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
        entry.rotation = track_rot;
        entry_changed = true;
        if let Some(vrot) = val_rot {
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
            ui.label(RichText::new(" (interpolated)").color(colors::TEXT_MUTED));
            if ui.button("+").on_hover_text("Add color keyframe").clicked() {
                entry.color_r.insert(frame, r as f64);
                entry.color_g.insert(frame, g as f64);
                entry.color_b.insert(frame, b as f64);
                entry.color_a.insert(frame, a as f64);
                entry_changed = true;
            }
        }
    });

    app.anim_timeline.nodes.insert(id, entry);
}
