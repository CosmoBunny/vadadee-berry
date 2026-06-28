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
    video_export_progress_window(app, ui.ctx());
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
                    ui.separator();
                    ui.menu_button("Flip", |ui| {
                        if ui.button("⟺  Flip Horizontal").clicked() {
                            app.flip_selection(true);
                            ui.close();
                        }
                        if ui.button("⟻  Flip Vertical").clicked() {
                            app.flip_selection(false);
                            ui.close();
                        }
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut app.viewport.show_grid, "Show grid");
                    ui.checkbox(&mut app.viewport.snap_grid, "Snap to grid");
                    ui.checkbox(&mut app.snap_magnet, "Magnetic snap");
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

        // Backend
        ui.horizontal(|ui| {
            ui.label("Backend");
            egui::ComboBox::from_id_salt("video_backend_combo")
                .selected_text(app.video_export.backend.label())
                .width(110.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut app.video_export.backend,
                        crate::app::VideoBackend::Ffmpeg,
                        "FFmpeg",
                    );
                    ui.selectable_value(
                        &mut app.video_export.backend,
                        crate::app::VideoBackend::Gstreamer,
                        "GStreamer",
                    );
                });
        });

        // Frame rate
        ui.horizontal(|ui| {
            ui.label("Frame rate");
            let mut fps = app.video_export.fps;
            egui::ComboBox::from_id_salt("video_fps_combo")
                .selected_text(format!("{} fps", fps))
                .width(90.0)
                .show_ui(ui, |ui| {
                    for &f in &[24u32, 25, 30, 50, 60] {
                        ui.selectable_value(&mut fps, f, format!("{} fps", f));
                    }
                });
            app.video_export.fps = fps;
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
            app.request_video_export();
        }
    });
}

fn video_export_progress_window(app: &mut VadadeeBerryApp, ctx: &egui::Context) {
    if !app.video_export.progress_visible {
        return;
    }
    let Some(prog) = app.video_export.progress else {
        return;
    };
    egui::Window::new("Render to Video")
        .id(egui::Id::new("video_progress_dlg"))
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(&app.video_export.status_msg)
                    .color(colors::TEXT_MUTED)
                    .italics(),
            );
            ui.add_space(8.0);
            let pb = egui::ProgressBar::new(prog)
                .show_percentage()
                .animate(app.video_export.rendering)
                .desired_width(ui.available_width());
            ui.add(pb);
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
                    let btn_timeline = ui.button(RichText::new(timeline_btn_icon).font(nerd_font_id(12.0)));
                    if btn_timeline.clicked() {
                        app.anim_show_timeline_window = !app.anim_show_timeline_window;
                    }
                    btn_timeline.on_hover_text(timeline_btn_tooltip);

                    ui.add_space(4.0);

                    // playback controls
                    let play_icon = if app.anim_is_playing { "" } else { "" };
                    let play_tooltip = if app.anim_is_playing { "Pause" } else { "Play" };
                    
                    let btn_next = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_next.clicked() {
                        app.anim_current_frame = (app.anim_current_frame + 1) % 101;
                    }
                    btn_next.on_hover_text("Forward (1 frame)");

                    let btn_play = ui.button(RichText::new(play_icon).font(nerd_font_id(12.0)));
                    if btn_play.clicked() {
                        app.anim_is_playing = !app.anim_is_playing;
                    }
                    btn_play.on_hover_text(play_tooltip);

                    let btn_prev = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_prev.clicked() {
                        app.anim_current_frame = if app.anim_current_frame == 0 { 100 } else { app.anim_current_frame - 1 };
                    }
                    btn_prev.on_hover_text("Backward (1 frame)");

                    let btn_rewind = ui.button(RichText::new("").font(nerd_font_id(12.0)));
                    if btn_rewind.clicked() {
                        app.anim_current_frame = 0;
                        app.anim_is_playing = false;
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
    ui.horizontal(|ui| {
        ui.label("Page Color");
        let mut col = app.project.document.page_color;
        if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
            app.project.document.page_color = col;
        }
    });
}

fn layers_section(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        if ui.button("+ New Layer").clicked() {
            app.add_layer("Layer");
        }
        if ui.button("+ Video Layer").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Video", &["mp4", "mkv", "avi", "mov", "webm"])
                .pick_file()
            {
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                app.add_video_layer(&name, path.to_string_lossy().into_owned());
            }
        }
    });
    ui.add_space(4.0);
    
    let layer_count = app.project.document.layers.len();
    for i in 0..layer_count {
        let active = app.project.document.active_layer_index == i;
        let (name, visible, locked, is_video) = {
            let l = &app.project.document.layers[i];
            (l.name.clone(), l.visible, l.locked, l.kind == crate::document::LayerKind::Video)
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
            let icon = if is_video { "🎥 " } else { "🖼 " };
            ui.label(icon);
            if ui.text_edit_singleline(&mut edit_name).changed() {
                app.rename_layer(i, edit_name);
            }
        });
    }

    // Active Layer settings (Renderer/Non-renderer and Video details)
    if let Some(l) = app.project.document.active_layer_mut() {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        theme::constraint_block(ui, |ui| {
            ui.label(RichText::new("Layer Properties").strong());
            ui.add_space(4.0);

            ui.checkbox(&mut l.is_renderer, "Export Renderer Layer").on_hover_text("If unchecked, this layer will not render during export");

            ui.horizontal(|ui| {
                ui.label("Type:");
                let mut is_vid = l.kind == crate::document::LayerKind::Video;
                if ui.selectable_label(!is_vid, "🖼 Image").clicked() {
                    l.kind = crate::document::LayerKind::Image;
                }
                if ui.selectable_label(is_vid, "🎥 Video").clicked() {
                    l.kind = crate::document::LayerKind::Video;
                }
            });

            if l.kind == crate::document::LayerKind::Video {
                ui.horizontal(|ui| {
                    ui.label("Path:");
                    ui.text_edit_singleline(&mut l.video_path);
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Video", &["mp4", "mkv", "avi", "mov", "webm"])
                            .pick_file()
                        {
                            l.video_path = path.to_string_lossy().into_owned();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Volume:");
                    ui.add(egui::Slider::new(&mut l.volume, 0.0..=1.0));
                });
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
    // List any Video Layers first as "Video Objects"
    let video_layers: Vec<(uuid::Uuid, String)> = app.project.document.layers
        .iter()
        .filter(|l| l.kind == crate::document::LayerKind::Video)
        .map(|l| (l.id, l.name.clone()))
        .collect();
    for (layer_id, layer_name) in video_layers {
        let selected = app.selection.contains(&layer_id);
        let label = RichText::new(format!("🎥 {}", layer_name)).font(nerd_font_id(13.0));
        ui.horizontal(|ui| {
            if ui.selectable_label(selected, label).clicked() {
                app.set_selection(vec![layer_id]);
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
                    if let Some(pos) = app.project.document.layers.iter().position(|l| l.id == layer_id) {
                        app.delete_layer(pos);
                    }
                }
                delete_btn.on_hover_text("Delete video layer");
            });
        });
    }

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
    if app.selection.len() == 1 {
        let id = app.selection[0];
        if let Some(pos) = app.project.document.layers.iter().position(|l| l.id == id) {
            let layer = &mut app.project.document.layers[pos];
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("🎥 Color Controls").strong().color(colors::ACCENT));
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
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new("🎵 Audio Equalizer").strong().color(colors::ACCENT));
                ui.add_space(4.0);
                
                ui.horizontal(|ui| {
                    ui.label("Bass:");
                    ui.add(egui::Slider::new(&mut layer.eq_bass, -10.0..=10.0).suffix(" dB"));
                });
                ui.horizontal(|ui| {
                    ui.label("Middle:");
                    ui.add(egui::Slider::new(&mut layer.eq_mid, -10.0..=10.0).suffix(" dB"));
                });
                ui.horizontal(|ui| {
                    ui.label("Treble:");
                    ui.add(egui::Slider::new(&mut layer.eq_treble, -10.0..=10.0).suffix(" dB"));
                });
                ui.horizontal(|ui| {
                    ui.label("Volume:");
                    ui.add(egui::Slider::new(&mut layer.volume, 0.0..=1.0));
                });
            });
            
            // Name editing
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut layer.name);
            });
            return;
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
            theme::constraint_block(ui, |ui| {
                ui.label(RichText::new(format!("🎥 Video Layer: {}", layer.name)).strong().color(colors::ACCENT));
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
    edit_mode: bool,
    dragged_keyframe: &mut Option<(crate::document::NodeId, String, usize)>,
    selected_keyframe: &mut Option<(crate::document::NodeId, String, usize)>,
    graph_editor_track: &mut Option<(crate::document::NodeId, String)>,
    graph_editor_target_track: &mut Option<(crate::document::NodeId, String)>,
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
        
        // Mouse wheel scroll to pan timeline (horizontal scroll only!)
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        if scroll_delta.x != 0.0 && response.hovered() {
            *timeline_scroll = (*timeline_scroll - scroll_delta.x * 0.1).max(0.0);
        }
        
        // Find if a specific plot's keyframe is hovered
        let mut hovered_plot_lbl = None;
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

        let mut curr_frame = app.anim_current_frame;
        let mut scroll = app.anim_timeline_scroll;

        // --- HORIZONTAL TIMELINE SCROLL & RULER ---
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scroll:").color(colors::TEXT_MUTED));
            ui.add(egui::Slider::new(&mut scroll, 0.0..=500.0).show_value(false));
            ui.add_space(8.0);
            
            // Frame number indicator
            ui.label(RichText::new(format!("Current: Frame {}", curr_frame)).color(colors::TEXT));
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
            let visible_frames = 100.0;
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
            
            // Handle scrubbing/clicking to change frame
            if response.clicked() || response.dragged() {
                if let Some(mpos) = response.interact_pointer_pos() {
                    let frac = ((mpos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let target_frame = (start_frame + frac * visible_frames).round() as usize;
                    curr_frame = target_frame;
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

        let selected_node_id = app.selection.first().copied();
        let edit_mode = app.anim_edit_mode;
        
        let mut dragged = app.anim_dragged_keyframe.clone();
        
        if let Some(node_id) = selected_node_id {
            let mut temp_selected_kf = app.anim_selected_keyframe.clone();
            let mut temp_graph_track = app.anim_graph_editor_track.clone();
            let mut temp_target_track = app.anim_graph_editor_target_track.clone();
            let geom_floats = app.get_node_geom_floats(node_id);

            if let Some(anim) = app.project.anim_timeline.nodes.get_mut(&node_id) {
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
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
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
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
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
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
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
                                &mut temp_selected_kf,
                                &mut temp_graph_track,
                                &mut temp_target_track,
                            );
                        }

                        let has_geom = anim.geom_tracks.iter().any(|t| !t.keyframes.is_empty());
                        if has_geom {
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
                            for (i, track) in anim.geom_tracks.iter_mut().enumerate() {
                                if track.keyframes.is_empty() {
                                    continue;
                                }
                                let label = if i < GEOM_LABELS.len() {
                                    GEOM_LABELS[i]
                                } else {
                                    "geom_unknown"
                                };
                                let default_val = if i < geom_floats.len() { geom_floats[i] } else { 0.0 };
                                
                                let track_name = if let Some(node) = app.project.nodes.get(node_id) {
                                    match &node.kind {
                                        NodeKind::Rect { .. } => match i {
                                            0 => "Width".to_string(),
                                            1 => "Height".to_string(),
                                            2 => "Corner Rad".to_string(),
                                            _ => format!("Geom {}", i),
                                        },
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
                                        NodeKind::Path { .. } => {
                                            let pt_idx = i / 6;
                                            match i % 6 {
                                                0 => format!("Pt {} X", pt_idx),
                                                1 => format!("Pt {} Y", pt_idx),
                                                2 => format!("Pt {} Out X", pt_idx),
                                                3 => format!("Pt {} Out Y", pt_idx),
                                                4 => format!("Pt {} In X", pt_idx),
                                                5 => format!("Pt {} In Y", pt_idx),
                                                _ => unreachable!(),
                                            }
                                        }
                                        NodeKind::BrushStroke { .. } => {
                                            let pt_idx = i / 3;
                                            match i % 3 {
                                                0 => format!("Stroke {} X", pt_idx),
                                                1 => format!("Stroke {} Y", pt_idx),
                                                _ => format!("Stroke {} W", pt_idx),
                                            }
                                        }
                                        _ => format!("Geom {}", i),
                                    }
                                } else {
                                    format!("Geom {}", i)
                                };

                                let mut plots = vec![TrackPlotInfo {
                                    label,
                                    track,
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
                                    edit_mode,
                                    &mut dragged,
                                    &mut temp_selected_kf,
                                    &mut temp_graph_track,
                                    &mut temp_target_track,
                                );
                            }
                        }
                    });
            }
            app.anim_selected_keyframe = temp_selected_kf;
            app.anim_graph_editor_track = temp_graph_track;
            app.anim_graph_editor_target_track = temp_target_track;
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
    let track_count = if let Some(node_id) = app.selection.first().copied() {
        if let Some(anim) = app.project.anim_timeline.nodes.get(&node_id) {
            let has_pos = !anim.pos_x.keyframes.is_empty() || !anim.pos_y.keyframes.is_empty();
            let has_rot = !anim.rotation.keyframes.is_empty();
            let has_op = !anim.opacity.keyframes.is_empty();
            let has_col = !anim.color_r.keyframes.is_empty() 
                || !anim.color_g.keyframes.is_empty() 
                || !anim.color_b.keyframes.is_empty() 
                || !anim.color_a.keyframes.is_empty();
            let geom_count = anim.geom_tracks.iter().filter(|t| !t.keyframes.is_empty()).count();
            
            (if has_pos { 1 } else { 0 })
                + (if has_rot { 1 } else { 0 })
                + (if has_op { 1 } else { 0 })
                + (if has_col { 1 } else { 0 })
                + geom_count
        } else {
            0
        }
    } else {
        0
    };

    let card_h = match track_count {
        0 => 56.0,
        1 => 92.0,
        _ => 130.0,
    };
    
    let left = inset.left() + gap;
    let open_top = inset.bottom() - card_h - gap - 30.0;
    let travel = card_h + gap + 30.0;
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

    theme::show_action_bar_area(ctx, "floating_timeline", rect, opacity, |ui| {
        timeline_interior(app, ui);
    });
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
        "color_r" | "color_g" | "color_b" | "color_a" => "Color".to_string(),
        _ if track_lbl.starts_with("geom_") => {
            if let Ok(idx) = track_lbl["geom_".len()..].parse::<usize>() {
                app.get_node_geom_track_name(node_id, idx)
            } else {
                track_lbl.clone()
            }
        }
        _ => track_lbl.clone(),
    };
    
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("GRAPH EDITOR: {}", track_name)).strong().color(colors::ACCENT));
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new(icons::CLOSE).font(icons::nerd_font_id(12.0))).clicked() {
                    app.anim_graph_editor_track = None;
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
        } else {
            0.0
        };

        let Some(anim) = app.project.anim_timeline.nodes.get(&node_id) else {
            return;
        };
        
        // Resolve all tracks to display in this graph plot
        let mut tracks_to_draw = Vec::new(); // Vec<(String, Color32, &KeyframeTrack, f64)>
        
        if track_lbl == "pos_x" || track_lbl == "pos_y" {
            tracks_to_draw.push(("pos_x".to_string(), egui::Color32::from_rgb(0, 200, 0), &anim.pos_x, node_pos.0));
            tracks_to_draw.push(("pos_y".to_string(), egui::Color32::from_rgb(200, 0, 0), &anim.pos_y, node_pos.1));
        } else if track_lbl.starts_with("color_") {
            tracks_to_draw.push(("color_r".to_string(), egui::Color32::from_rgb(255, 100, 100), &anim.color_r, node_col[0] as f64));
            tracks_to_draw.push(("color_g".to_string(), egui::Color32::from_rgb(100, 255, 100), &anim.color_g, node_col[1] as f64));
            tracks_to_draw.push(("color_b".to_string(), egui::Color32::from_rgb(100, 100, 255), &anim.color_b, node_col[2] as f64));
        } else if track_lbl.starts_with("geom_") {
            if let Ok(idx) = track_lbl["geom_".len()..].parse::<usize>() {
                // Check if this belongs to a 2D pair (like Path X/Y, or BrushStroke X/Y)
                let mut grouped = false;
                if let Some(node) = app.project.nodes.get(node_id) {
                    let base_len = node.get_geom_floats().len();
                    if idx < base_len {
                        match &node.kind {
                            NodeKind::Path { .. } => {
                                let anchor_idx = idx / 6;
                                let sub_idx = idx % 6;
                                if sub_idx == 0 || sub_idx == 1 {
                                    // Pt X and Pt Y
                                    let idx_x = anchor_idx * 6;
                                    let idx_y = anchor_idx * 6 + 1;
                                    let lbl_x = format!("geom_{}", idx_x);
                                    let lbl_y = format!("geom_{}", idx_y);
                                    let def_x = geom_floats.get(idx_x).copied().unwrap_or(0.0);
                                    let def_y = geom_floats.get(idx_y).copied().unwrap_or(0.0);
                                    if let Some(t_x) = anim.get_track(&lbl_x) {
                                        tracks_to_draw.push((lbl_x, egui::Color32::from_rgb(0, 200, 0), t_x, def_x));
                                    }
                                    if let Some(t_y) = anim.get_track(&lbl_y) {
                                        tracks_to_draw.push((lbl_y, egui::Color32::from_rgb(200, 0, 0), t_y, def_y));
                                    }
                                    grouped = true;
                                } else if sub_idx == 2 || sub_idx == 3 {
                                    // Pt Out X and Pt Out Y
                                    let idx_x = anchor_idx * 6 + 2;
                                    let idx_y = anchor_idx * 6 + 3;
                                    let lbl_x = format!("geom_{}", idx_x);
                                    let lbl_y = format!("geom_{}", idx_y);
                                    let def_x = geom_floats.get(idx_x).copied().unwrap_or(0.0);
                                    let def_y = geom_floats.get(idx_y).copied().unwrap_or(0.0);
                                    if let Some(t_x) = anim.get_track(&lbl_x) {
                                        tracks_to_draw.push((lbl_x, egui::Color32::from_rgb(0, 200, 200), t_x, def_x));
                                    }
                                    if let Some(t_y) = anim.get_track(&lbl_y) {
                                        tracks_to_draw.push((lbl_y, egui::Color32::from_rgb(200, 0, 200), t_y, def_y));
                                    }
                                    grouped = true;
                                } else if sub_idx == 4 || sub_idx == 5 {
                                    // Pt In X and Pt In Y
                                    let idx_x = anchor_idx * 6 + 4;
                                    let idx_y = anchor_idx * 6 + 5;
                                    let lbl_x = format!("geom_{}", idx_x);
                                    let lbl_y = format!("geom_{}", idx_y);
                                    let def_x = geom_floats.get(idx_x).copied().unwrap_or(0.0);
                                    let def_y = geom_floats.get(idx_y).copied().unwrap_or(0.0);
                                    if let Some(t_x) = anim.get_track(&lbl_x) {
                                        tracks_to_draw.push((lbl_x, egui::Color32::from_rgb(100, 200, 100), t_x, def_x));
                                    }
                                    if let Some(t_y) = anim.get_track(&lbl_y) {
                                        tracks_to_draw.push((lbl_y, egui::Color32::from_rgb(200, 100, 200), t_y, def_y));
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
                                    let lbl_x = format!("geom_{}", idx_x);
                                    let lbl_y = format!("geom_{}", idx_y);
                                    let def_x = geom_floats.get(idx_x).copied().unwrap_or(0.0);
                                    let def_y = geom_floats.get(idx_y).copied().unwrap_or(0.0);
                                    if let Some(t_x) = anim.get_track(&lbl_x) {
                                        tracks_to_draw.push((lbl_x, egui::Color32::from_rgb(0, 200, 0), t_x, def_x));
                                    }
                                    if let Some(t_y) = anim.get_track(&lbl_y) {
                                        tracks_to_draw.push((lbl_y, egui::Color32::from_rgb(200, 0, 0), t_y, def_y));
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
                        tracks_to_draw.push((track_lbl.clone(), colors::ACCENT, t, default_val));
                    }
                }
            }
        } else {
            if let Some(t) = anim.get_track(&track_lbl) {
                tracks_to_draw.push((track_lbl.clone(), colors::ACCENT, t, default_val));
            }
        }
        
        if tracks_to_draw.is_empty() {
            return;
        }
        
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width() - 8.0, 136.0),
            egui::Sense::click_and_drag()
        );
        let painter = ui.painter_at(rect);
        
        painter.rect_filled(rect, egui::CornerRadius::same(4), colors::BG_DEEP);
        painter.rect_stroke(rect, egui::CornerRadius::same(4), egui::Stroke::new(1.0, colors::BORDER), egui::StrokeKind::Inside);
        
        let padding = 12.0;
        
        // Find min/max values across all resolved tracks
        let mut val_min = f64::MAX;
        let mut val_max = f64::MIN;
        let mut has_keyframes = false;
        for (_, _, track, default_val) in &tracks_to_draw {
            for kf in &track.keyframes {
                val_min = val_min.min(kf.value);
                val_max = val_max.max(kf.value);
                if kf.interpolation == crate::app::InterpolationMode::Bezier {
                    val_min = val_min.min(kf.value + kf.handle_right.1);
                    val_max = val_max.max(kf.value + kf.handle_right.1);
                    val_min = val_min.min(kf.value + kf.handle_left.1);
                    val_max = val_max.max(kf.value + kf.handle_left.1);
                }
                has_keyframes = true;
            }
            if track.keyframes.is_empty() {
                val_min = val_min.min(*default_val);
                val_max = val_max.max(*default_val);
            }
        }
        
        if !has_keyframes {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No keyframes on this track.").color(colors::TEXT_MUTED));
            });
            return;
        }
        
        if val_min >= val_max {
            val_min -= 1.0;
            val_max += 1.0;
        } else {
            let span = val_max - val_min;
            val_min -= span * 0.25;
            val_max += span * 0.25;
        }
        
        // Draw grid
        for f in (0..=100).step_by(10) {
            let x = rect.left() + (f as f32 / 100.0) * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, colors::BORDER.gamma_multiply(0.2)),
            );
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
        
        // Setup screen/graph space mapping closures
        let to_screen = |f: f64, v: f64| -> egui::Pos2 {
            let x = rect.left() + (f as f32 / 100.0) * rect.width();
            let frac_y = (v - val_min) / (val_max - val_min);
            let y = rect.bottom() - padding - (frac_y as f32) * (rect.height() - 2.0 * padding);
            egui::pos2(x, y)
        };
        
        let to_graph = |pos: egui::Pos2| -> (f64, f64) {
            let frame = (((pos.x - rect.left()) / rect.width()) * 100.0) as f64;
            let target_frac_y = (rect.bottom() - padding - pos.y) / (rect.height() - 2.0 * padding);
            let value = val_min + (target_frac_y as f64) * (val_max - val_min);
            (frame, value)
        };
        
        // Draw graph curves and detect segment clicks
        let mut clicked_segment: Option<(String, usize, usize, egui::Pos2)> = None; // (track_lbl, left_frame, right_frame, click_pos)
        for (lbl, color, track, default_val) in &tracks_to_draw {
            let track_lbl_str = lbl.to_string();
            let mut curve_pts: Vec<(usize, egui::Pos2)> = Vec::new(); // (frame, screen_pos)
            for f in 0..=100usize {
                let val = track.interpolate(f).unwrap_or(*default_val);
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
        if let Some((seg_lbl, lf, rf, _)) = clicked_segment {
            app.anim_graph_selected_segment = Some((seg_lbl, lf, rf));
            app.anim_selected_keyframe = None;
        }

        // Draw selected-segment highlight
        if let Some((ref seg_lbl, lf, rf)) = app.anim_graph_selected_segment.clone() {
            for (lbl, color, track, default_val) in &tracks_to_draw {
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
                    let target_frame = (((mpos.x - rect.left()) / rect.width()) * 100.0).clamp(0.0, 100.0).round() as usize;
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
        let playhead_x = rect.left() + (app.anim_current_frame as f32 / 100.0) * rect.width();
        painter.line_segment(
            [egui::pos2(playhead_x, rect.top()), egui::pos2(playhead_x, rect.bottom())],
            egui::Stroke::new(1.0, colors::ACCENT.gamma_multiply(0.4)),
        );
    });

    // Segment-selected: apply bezier on the span between two keyframes (no extra keyframe).
    if let Some((ref seg_lbl, lf, rf)) = app.anim_graph_selected_segment.clone() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Segment [{} – {}] selected", lf, rf))
                    .color(colors::TEXT_MUTED)
                    .italics(),
            );
            ui.add_space(8.0);
            let add_btn = ui.add(
                egui::Button::new(
                    RichText::new("+ Apply Bezier")
                        .color(egui::Color32::from_rgb(80, 200, 120))
                )
                .fill(colors::BG_DEEP),
            );
            if add_btn.clicked() {
                let before_timeline = app.project.anim_timeline.clone();
                if let Some(anim_mut) = app.project.anim_timeline.nodes.get_mut(&node_id) {
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
                        if let Some(lk) = track.keyframes.iter_mut().find(|k| k.frame == lf) {
                            lk.interpolation = crate::app::InterpolationMode::Bezier;
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
                    crate::history::ProjectEdit::PatchTimeline { before: before_timeline, after: after_timeline },
                );
                app.anim_graph_selected_segment = None;
                app.apply_animation_for_frame(app.anim_current_frame);
            }
            ui.add_space(4.0);
            if ui
                .button(
                    RichText::new("x Deselect")
                        .color(colors::TEXT_MUTED),
                )
                .clicked()
            {
                app.anim_graph_selected_segment = None;
            }
        });
    }
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
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} points selected", multi.len())).strong());
                    if ui.button("Smooth selected").clicked() {
                        app.smooth_selected_path_points();
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

    // Handle geometry tracks
    let mut geom_floats = {
        let Some(_) = app.project.nodes.get(id) else {
            return;
        };
        app.get_node_geom_floats(id)
    };
    
    if !geom_floats.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Geometry Properties").strong().color(colors::POWERLINE_C));
        ui.separator();
        ui.add_space(4.0);

        // Ensure we have enough keyframe tracks for each geometry float
        while entry.geom_tracks.len() < geom_floats.len() {
            entry.geom_tracks.push(crate::app::KeyframeTrack::default());
        }

        // Gather human-readable labels and config
        let (geom_info, is_arc) = if let Some(node) = app.project.nodes.get(id) {
            let mut info = match &node.kind {
                NodeKind::Rect { .. } => vec![
                    ("Width".to_string(), 0.0, 10000.0, 1.0),
                    ("Height".to_string(), 0.0, 10000.0, 1.0),
                    ("Corner Radius".to_string(), 0.0, 500.0, 0.5),
                ],
                NodeKind::Ellipse { .. } => vec![
                    ("Radius X".to_string(), 0.0, 10000.0, 1.0),
                    ("Radius Y".to_string(), 0.0, 10000.0, 1.0),
                ],
                NodeKind::Polygon { .. } => vec![
                    ("Radius".to_string(), 0.0, 10000.0, 1.0),
                    ("Sides".to_string(), 3.0, 100.0, 1.0),
                ],
                NodeKind::Arc { .. } => vec![
                    ("Radius".to_string(), 0.0, 10000.0, 1.0),
                    ("Start Angle (deg)".to_string(), -360.0, 360.0, 1.0),
                    ("Sweep Angle (deg)".to_string(), -360.0, 360.0, 1.0),
                ],
                NodeKind::Path { path } => {
                    let mut v = Vec::new();
                    let num_anchors = path.anchor_positions().len();
                    for i in 0..num_anchors {
                        v.push((format!("Pt {} X", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Pt {} Y", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Pt {} Out X", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Pt {} Out Y", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Pt {} In X", i), -10000.0, 10000.0, 1.0));
                        v.push((format!("Pt {} In Y", i), -10000.0, 10000.0, 1.0));
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
            let (label, min, max, speed) = if i < geom_info.len() {
                geom_info[i].clone()
            } else {
                (format!("Property {}", i), -10000.0, 10000.0, 1.0)
            };

            let is_arc_angle = is_arc && (i == 1 || i == 2);
            
            let mut track_geom = entry.geom_tracks[i].clone();
            
            // Adjust defaults/values for radian <-> degree conversion
            let current_val = if is_arc_angle {
                geom_floats[i].to_degrees()
            } else {
                geom_floats[i]
            };

            // In order to use render_prop_row correctly, we convert values in track_geom to degrees temporarily if it's an angle track
            if is_arc_angle {
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
                if is_arc_angle {
                    for kf in &mut track_geom.keyframes {
                        kf.value = kf.value.to_radians();
                    }
                }
                entry.geom_tracks[i] = track_geom;
                entry_changed = true;

                if let Some(vg) = val_geom {
                    let rad_vg = if is_arc_angle { vg.to_radians() } else { vg };
                    geom_floats[i] = rad_vg;
                    app.set_node_geom_floats(id, &geom_floats);
                }
            }
        }
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

    app.project.anim_timeline.nodes.insert(id, entry);
    
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
