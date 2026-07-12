//! Node Editor dialog: infinite grid, pan/zoom, Add/Edit/View toolbar, typed nodes & wires.

use egui::{Color32, Context, Pos2, Rect, RichText, Sense, Ui, Vec2};
use uuid::Uuid;

use crate::app::VadadeeBerryApp;
use crate::document::{
    GraphNodeKind, GraphParam, GraphParamKind, LayerKind, NodeGraph, PortDir, PortType,
};
use crate::icons::{self, nerd_font_id};
use crate::theme::colors;

const NODE_W_MIN: f32 = 148.0;
const NODE_W_MAX: f32 = 300.0;
const NODE_H_BASE: f32 = 56.0;
const PORT_R: f32 = 6.0;
const PORT_GAP: f32 = 18.0;

/// Card width grows with title length (no "…" in titles).
fn node_width(n: &crate::document::GraphNode) -> f32 {
    let title = format!("{} · {}", n.kind.category_label(), n.name);
    // ~7.2px per glyph at base 11pt; padding for trash chip + margins.
    let w = 36.0 + title.chars().count() as f32 * 7.4;
    w.clamp(NODE_W_MIN, NODE_W_MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeEditorToolMode {
    /// Pan / look only — no move, wire, or edit.
    #[default]
    Idle,
    /// Opens Add catalog overlay.
    Add,
    /// Toggle: when selected, move/edit/wire allowed (no overlay).
    Edit,
    /// Opens View zoom/fit overlay.
    View,
}

/// UI state for the node editor (not serialized into the project).
#[derive(Debug, Clone)]
pub struct NodeEditorUiState {
    pub open_layer_id: Option<Uuid>,
    pub tool: NodeEditorToolMode,
    pub selected: Option<Uuid>,
    /// Selected wire / connector (`GraphLink.id`).
    pub selected_link: Option<Uuid>,
    /// Wire drag: (from_node, from_port).
    pub wire_drag: Option<(Uuid, String)>,
    pub wire_cursor: Option<Pos2>,
    /// Pending "add node" menu after wire drop on empty space.
    pub add_menu_at: Option<(Pos2, Option<(Uuid, String)>)>,
    /// Sticky node move: (node_id, graph-space grab offset from node origin).
    pub node_drag: Option<(Uuid, Vec2)>,
    /// Node whose image stream is shown in the floating preview popup.
    pub preview_node: Option<Uuid>,
    /// Last user-chosen window size (prevents content from “sticking” full height).
    pub window_size: Vec2,
}

impl Default for NodeEditorUiState {
    fn default() -> Self {
        Self {
            open_layer_id: None,
            tool: NodeEditorToolMode::default(),
            selected: None,
            selected_link: None,
            wire_drag: None,
            wire_cursor: None,
            add_menu_at: None,
            node_drag: None,
            preview_node: None,
            window_size: Vec2::new(920.0, 560.0),
        }
    }
}

impl NodeEditorUiState {
    pub fn open(&mut self, layer_id: Uuid) {
        self.open_layer_id = Some(layer_id);
        self.tool = NodeEditorToolMode::Idle;
        self.preview_node = None;
        self.selected_link = None;
    }

    pub fn close(&mut self) {
        self.open_layer_id = None;
        self.wire_drag = None;
        self.add_menu_at = None;
        self.selected = None;
        self.selected_link = None;
        self.node_drag = None;
        self.preview_node = None;
        self.tool = NodeEditorToolMode::Idle;
    }

    /// True when Edit mode is active (move / edit fields / connect).
    pub fn can_edit(&self) -> bool {
        self.tool == NodeEditorToolMode::Edit
    }
}

pub fn show_node_editor_dialog(app: &mut VadadeeBerryApp, ctx: &Context) {
    let Some(layer_id) = app.node_editor_ui.open_layer_id else {
        return;
    };
    let layer_idx = app
        .project
        .document
        .layers
        .iter()
        .position(|l| l.id == layer_id);
    let Some(layer_idx) = layer_idx else {
        app.node_editor_ui.close();
        return;
    };
    if app.project.document.layers[layer_idx].kind != LayerKind::NodeEditor {
        app.node_editor_ui.close();
        return;
    }
    app.project.document.layers[layer_idx].ensure_node_graph();

    // Mode shortcuts / Esc → Idle. Never closes the window on Esc.
    node_editor_mode_keys(app, ctx);

    let mut open = true;
    let title = {
        let name = &app.project.document.layers[layer_idx].name;
        format!("{} Node Editor — {}", icons::NODE_EDITOR, name)
    };

    // Resizable window with *user-controlled* size. Node drag must never grow it:
    // canvas is painter-only + hard clip; no ui.interact off the canvas rect.
    let max_h = ctx.content_rect().height() * 0.92;
    let max_w = ctx.content_rect().width() * 0.96;
    let mut win_size = app.node_editor_ui.window_size;
    win_size.x = win_size.x.clamp(480.0, max_w);
    win_size.y = win_size.y.clamp(280.0, max_h);

    let win_resp = egui::Window::new(title)
        // v4: toolbar always allocated first with fixed strip height.
        .id(egui::Id::new(("node_editor_dlg_v4", layer_id)))
        .open(&mut open)
        .default_size(win_size)
        .min_width(520.0)
        .min_height(320.0)
        .max_height(max_h)
        .max_width(max_w)
        .resizable(true)
        .collapsible(false)
        .constrain(true)
        .show(ctx, |ui| {
            let outer = ui.max_rect();
            ui.set_clip_rect(outer);

            // Fixed-height mode strip so Idle/Add/Edit/View never get clipped by canvas.
            const TOOLBAR_H: f32 = 36.0;
            let toolbar_rect = Rect::from_min_size(
                outer.min,
                Vec2::new(outer.width(), TOOLBAR_H),
            );
            // scope_builder (not allocate_new_ui) — avoids "overflow grows parent".
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(toolbar_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(toolbar_rect);
                    ui.set_max_size(toolbar_rect.size());
                    ui.painter().rect_filled(
                        toolbar_rect,
                        0.0,
                        Color32::from_rgb(40, 44, 58),
                    );
                    node_editor_toolbar(app, ui, layer_idx);
                },
            );

            // Canvas = rest of the window under the toolbar.
            let canvas_rect = Rect::from_min_max(
                Pos2::new(outer.min.x, toolbar_rect.max.y),
                outer.max,
            );
            if canvas_rect.height() > 8.0 && canvas_rect.width() > 8.0 {
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(canvas_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        ui.set_clip_rect(canvas_rect);
                        ui.set_max_size(canvas_rect.size());
                        node_editor_canvas(app, ui, layer_idx);
                    },
                );
            }
        });

    // Sync size from the window frame so *user* resize is kept; ignore content inflation
    // by clamping to max and never growing from a drag that is not on the resize border.
    if let Some(inner) = win_resp {
        let r = inner.response.rect;
        let new_size = r.size();
        // Accept size changes only when they stay within bounds (user resize grip).
        // If something tried to blow past max, snap back next frame via default_size.
        app.node_editor_ui.window_size = Vec2::new(
            new_size.x.clamp(480.0, max_w),
            new_size.y.clamp(280.0, max_h),
        );
    }

    // Only the window [x] / hide control closes the editor — not Esc.
    if !open {
        app.node_editor_ui.close();
    }
}

/// Esc → Idle (never close). A → Add, E → Edit, V → View.
/// While typing in Value/Expr, only Esc unfocuses; letter keys go to the text field.
fn node_editor_mode_keys(app: &mut VadadeeBerryApp, ctx: &Context) {
    let text_focused = ctx.wants_keyboard_input();
    if text_focused {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
        return;
    }

    let (esc, a, e, v) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::A),
            i.key_pressed(egui::Key::E),
            i.key_pressed(egui::Key::V),
        )
    });

    if esc {
        app.node_editor_ui.tool = NodeEditorToolMode::Idle;
        app.node_editor_ui.node_drag = None;
        app.node_editor_ui.wire_drag = None;
        app.node_editor_ui.add_menu_at = None;
        app.node_editor_ui.wire_cursor = None;
        return;
    }
    if a {
        app.node_editor_ui.tool = NodeEditorToolMode::Add;
        app.node_editor_ui.node_drag = None;
        app.node_editor_ui.wire_drag = None;
        return;
    }
    if e {
        app.node_editor_ui.tool = if app.node_editor_ui.tool == NodeEditorToolMode::Edit {
            NodeEditorToolMode::Idle
        } else {
            NodeEditorToolMode::Edit
        };
        if app.node_editor_ui.tool != NodeEditorToolMode::Edit {
            app.node_editor_ui.node_drag = None;
            app.node_editor_ui.wire_drag = None;
        }
        return;
    }
    if v {
        app.node_editor_ui.tool = NodeEditorToolMode::View;
        app.node_editor_ui.node_drag = None;
        app.node_editor_ui.wire_drag = None;
    }
}

fn node_editor_toolbar(app: &mut VadadeeBerryApp, ui: &mut Ui, layer_idx: usize) {
    let mut add_btn_rect = Rect::NOTHING;
    let mut view_btn_rect = Rect::NOTHING;
    let tool = app.node_editor_ui.tool;

    // Mode buttons as explicit selectable buttons (always readable labels).
    let mode_btn = |ui: &mut Ui, on: bool, label: &str| -> egui::Response {
        ui.add(
            egui::Button::new(RichText::new(label).strong().color(if on {
                Color32::from_rgb(40, 44, 58)
            } else {
                Color32::from_rgb(220, 225, 235)
            }))
            .fill(if on {
                Color32::from_rgb(120, 170, 255)
            } else {
                Color32::from_rgb(55, 60, 78)
            })
            .stroke(egui::Stroke::new(
                1.0,
                if on {
                    Color32::from_rgb(160, 200, 255)
                } else {
                    Color32::from_rgb(80, 88, 110)
                },
            ))
            .min_size(Vec2::new(56.0, 26.0)),
        )
    };

    ui.add_space(6.0);
    if mode_btn(ui, tool == NodeEditorToolMode::Idle, "Idle")
        .on_hover_text("Idle — pan only (Esc)")
        .clicked()
    {
        app.node_editor_ui.tool = NodeEditorToolMode::Idle;
        app.node_editor_ui.node_drag = None;
        app.node_editor_ui.wire_drag = None;
    }
    let add_r = mode_btn(ui, tool == NodeEditorToolMode::Add, "Add");
    add_btn_rect = add_r.rect;
    if add_r.on_hover_text("Add nodes — catalog (A)").clicked() {
        app.node_editor_ui.tool = if tool == NodeEditorToolMode::Add {
            NodeEditorToolMode::Idle
        } else {
            NodeEditorToolMode::Add
        };
    }
    let edit_on = tool == NodeEditorToolMode::Edit;
    if mode_btn(ui, edit_on, "Edit")
        .on_hover_text("Edit — move / wire / values (E)")
        .clicked()
    {
        app.node_editor_ui.tool = if edit_on {
            NodeEditorToolMode::Idle
        } else {
            NodeEditorToolMode::Edit
        };
        if app.node_editor_ui.tool != NodeEditorToolMode::Edit {
            app.node_editor_ui.node_drag = None;
            app.node_editor_ui.wire_drag = None;
        }
    }
    let view_r = mode_btn(ui, tool == NodeEditorToolMode::View, "View");
    view_btn_rect = view_r.rect;
    if view_r.on_hover_text("View — zoom / fit (V)").clicked() {
        app.node_editor_ui.tool = if tool == NodeEditorToolMode::View {
            NodeEditorToolMode::Idle
        } else {
            NodeEditorToolMode::View
        };
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(6.0);
        if ui
            .button(RichText::new("Hide").color(Color32::from_rgb(200, 200, 210)))
            .on_hover_text("Hide node editor")
            .clicked()
        {
            app.node_editor_ui.close();
        }
        ui.label(
            RichText::new(match tool {
                NodeEditorToolMode::Idle => "mode: Idle",
                NodeEditorToolMode::Add => "mode: Add",
                NodeEditorToolMode::Edit => "mode: Edit",
                NodeEditorToolMode::View => "mode: View",
            })
            .small()
            .color(Color32::from_rgb(160, 170, 190)),
        );
    });

    // Only Add / View use floating overlays. Edit never does.
    match app.node_editor_ui.tool {
        NodeEditorToolMode::Add => {
            let anchor = Pos2::new(add_btn_rect.left(), ui.max_rect().bottom() + 4.0);
            show_tool_overlay(ui.ctx(), "ne_add_overlay", anchor, |ui| {
                add_menu_strip(app, ui, layer_idx);
            });
        }
        NodeEditorToolMode::View => {
            let anchor = Pos2::new(view_btn_rect.left(), view_btn_rect.bottom() + 6.0);
            show_tool_overlay(ui.ctx(), "ne_view_overlay", anchor, |ui| {
                view_menu_strip(app, ui, layer_idx);
            });
        }
        NodeEditorToolMode::Idle | NodeEditorToolMode::Edit => {}
    }
}

fn show_tool_overlay(
    ctx: &Context,
    id: &'static str,
    anchor: Pos2,
    add_contents: impl FnOnce(&mut Ui),
) {
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(anchor)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(Color32::from_rgb(32, 36, 48))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(70, 78, 100)))
                .inner_margin(egui::Margin::same(10))
                .corner_radius(8.0)
                .show(ui, |ui| {
                    add_contents(ui);
                });
        });
}

fn add_menu_strip(app: &mut VadadeeBerryApp, ui: &mut Ui, layer_idx: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Add:").strong());
        let mut spawn: Option<GraphNodeKind> = None;
        ui.menu_button("Object ▾", |ui| {
            if ui.button("Image").clicked() {
                spawn = Some(GraphNodeKind::ObjectImage {
                    path: String::new(),
                });
                ui.close();
            }
            if ui.button("Video").clicked() {
                spawn = Some(GraphNodeKind::ObjectVideo {
                    path: String::new(),
                });
                ui.close();
            }
            if ui.button("Audio").clicked() {
                spawn = Some(GraphNodeKind::ObjectAudio {
                    path: String::new(),
                });
                ui.close();
            }
            if ui.button("Object from Application").clicked() {
                let ids = app.selection.clone();
                spawn = Some(GraphNodeKind::ObjectFromApp { node_ids: ids });
                ui.close();
            }
            if ui.button("Output Object").clicked() {
                spawn = Some(GraphNodeKind::OutputObject);
                ui.close();
            }
        });
        ui.menu_button("Algebra ▾", |ui| {
            if ui.button("Value").clicked() {
                spawn = Some(GraphNodeKind::Value { value: 0.0 });
                ui.close();
            }
            if ui.button("Expr").clicked() {
                spawn = Some(GraphNodeKind::Expr {
                    expr: "x".into(),
                });
                ui.close();
            }
            if ui.button("Frame").clicked() {
                spawn = Some(GraphNodeKind::Frame);
                ui.close();
            }
            if ui.button("Time").clicked() {
                spawn = Some(GraphNodeKind::Time);
                ui.close();
            }
        });
        ui.menu_button("Effect ▾", |ui| {
            if ui.button("Brightness").clicked() {
                spawn = Some(GraphNodeKind::Brightness);
                ui.close();
            }
            if ui.button("Color Changer").clicked() {
                spawn = Some(GraphNodeKind::ColorChanger);
                ui.close();
            }
            if ui.button("Linear Blur").clicked() {
                spawn = Some(GraphNodeKind::LinearBlur);
                ui.close();
            }
            if ui.button("Equalizer").clicked() {
                spawn = Some(GraphNodeKind::Equalizer);
                ui.close();
            }
            if ui.button("Speed").clicked() {
                spawn = Some(GraphNodeKind::Speed);
                ui.close();
            }
        });
        ui.menu_button("Geometry ▾", |ui| {
            for (label, k) in [
                ("Size", GraphNodeKind::GeoSize),
                ("Placement", GraphNodeKind::GeoPlacement),
                ("Rotate", GraphNodeKind::GeoRotate),
                ("Trapezoid", GraphNodeKind::GeoTrapezoid),
                ("Mirror", GraphNodeKind::GeoMirror),
                ("Add", GraphNodeKind::GeoAdd),
            ] {
                if ui.button(label).clicked() {
                    spawn = Some(k);
                    ui.close();
                }
            }
        });
        ui.menu_button("Parameter ▾", |ui| {
            if ui.button("Real").clicked() {
                if let Some(g) = app.project.document.layers[layer_idx]
                    .node_graph
                    .as_mut()
                {
                    let p = GraphParam::new_real(format!("Real {}", g.parameters.len() + 1), 0.0);
                    let id = p.id;
                    g.parameters.push(p);
                    spawn = Some(GraphNodeKind::ParamReal { param_id: id });
                }
                ui.close();
            }
            if ui.button("Color").clicked() {
                if let Some(g) = app.project.document.layers[layer_idx]
                    .node_graph
                    .as_mut()
                {
                    let p = GraphParam::new_color(
                        format!("Color {}", g.parameters.len() + 1),
                        1.0,
                        1.0,
                        1.0,
                    );
                    let id = p.id;
                    g.parameters.push(p);
                    spawn = Some(GraphNodeKind::ParamColor { param_id: id });
                }
                ui.close();
            }
            if ui.button("Position").clicked() {
                if let Some(g) = app.project.document.layers[layer_idx]
                    .node_graph
                    .as_mut()
                {
                    let p =
                        GraphParam::new_position(format!("Pos {}", g.parameters.len() + 1), 0.0, 0.0);
                    let id = p.id;
                    g.parameters.push(p);
                    spawn = Some(GraphNodeKind::ParamPosition { param_id: id });
                }
                ui.close();
            }
        });

        if let Some(kind) = spawn {
            spawn_node_centered(app, layer_idx, kind);
        }
    });
}

fn view_menu_strip(app: &mut VadadeeBerryApp, ui: &mut Ui, layer_idx: usize) {
    ui.horizontal(|ui| {
        if ui.button("Zoom +").on_hover_text("Ctrl +").clicked() {
            if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
                g.view.zoom = (g.view.zoom * 1.15).clamp(0.15, 8.0);
            }
        }
        if ui.button("Zoom −").on_hover_text("Ctrl -").clicked() {
            if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
                g.view.zoom = (g.view.zoom / 1.15).clamp(0.15, 8.0);
            }
        }
        if ui.button("Fit").clicked() {
            fit_graph_view(app, layer_idx, false);
        }
        if ui.button("Fit selection").clicked() {
            fit_graph_view(app, layer_idx, true);
        }
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_ref() {
            ui.label(
                RichText::new(format!("zoom {:.0}%", g.view.zoom * 100.0))
                    .small()
                    .weak(),
            );
        }
    });
}

fn spawn_node_centered(app: &mut VadadeeBerryApp, layer_idx: usize, kind: GraphNodeKind) {
    let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() else {
        return;
    };
    // Place near view center.
    let x = -g.view.pan_x / g.view.zoom + 80.0;
    let y = -g.view.pan_y / g.view.zoom + 60.0
        + (g.nodes.len() as f32 % 5.0) * 24.0;
    let id = g.add_node(kind, x, y);
    app.node_editor_ui.selected = Some(id);
    app.status_message = "Node added".into();
}

fn fit_graph_view(app: &mut VadadeeBerryApp, layer_idx: usize, selection_only: bool) {
    let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() else {
        return;
    };
    let sel = app.node_editor_ui.selected;
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut any = false;
    for n in g.nodes.values() {
        if selection_only && sel != Some(n.id) {
            continue;
        }
        any = true;
        min_x = min_x.min(n.x);
        min_y = min_y.min(n.y);
        max_x = max_x.max(n.x + node_width(n));
        max_y = max_y.max(n.y + node_height(n, false));
    }
    if !any {
        g.view.pan_x = 0.0;
        g.view.pan_y = 0.0;
        g.view.zoom = 1.0;
        return;
    }
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let w = (max_x - min_x).max(120.0);
    let h = (max_y - min_y).max(80.0);
    // Assume ~800x500 viewport interior.
    let zx = 700.0 / w;
    let zy = 420.0 / h;
    g.view.zoom = zx.min(zy).clamp(0.2, 3.0);
    g.view.pan_x = 400.0 - cx * g.view.zoom;
    g.view.pan_y = 240.0 - cy * g.view.zoom;
}

fn node_height(n: &crate::document::GraphNode, preview_open: bool) -> f32 {
    let ports = n.ports().len().max(1) as f32;
    let mut h = NODE_H_BASE + (ports - 1.0).max(0.0) * PORT_GAP;
    // Room for Browse / Prev row inside the card.
    if matches!(
        n.kind,
        GraphNodeKind::ObjectImage { .. }
            | GraphNodeKind::ObjectVideo { .. }
            | GraphNodeKind::ObjectAudio { .. }
            | GraphNodeKind::ObjectFromApp { .. }
    ) {
        h = h.max(92.0);
    }
    let (has_in, has_out) = NodeGraph::image_port_dirs(&n.kind);
    if preview_open && (has_in || has_out) && !matches!(n.kind, GraphNodeKind::ObjectAudio { .. }) {
        h = h.max(148.0);
    } else if has_in || has_out {
        // Bottom strip for flat Prev control.
        if !matches!(
            n.kind,
            GraphNodeKind::ObjectImage { .. }
                | GraphNodeKind::ObjectVideo { .. }
                | GraphNodeKind::ObjectAudio { .. }
                | GraphNodeKind::ObjectFromApp { .. }
        ) {
            h = h.max(h + 4.0).max(72.0);
        }
    }
    h
}

/// Flat text control (no filled button chrome).
fn flat_text_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .small()
                .color(Color32::from_rgb(140, 180, 255)),
        )
        .frame(false)
        .min_size(Vec2::new(0.0, 14.0)),
    )
}

fn node_editor_canvas(app: &mut VadadeeBerryApp, ui: &mut Ui, layer_idx: usize) {
    let avail = ui.available_size();
    // Fixed canvas footprint — must equal available only (no min grow).
    let (rect, response) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    // Hard clip for every paint call — labels (Prev/Browse) must not leak outside.
    let canvas_clip = rect.intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(canvas_clip);

    // Background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(22, 24, 30));

    // Ctrl +/- zoom
    let (ctrl, plus, minus, scroll) = ui.input(|i| {
        (
            i.modifiers.ctrl || i.modifiers.command,
            i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus),
            i.key_pressed(egui::Key::Minus),
            i.smooth_scroll_delta,
        )
    });
    // Precompute whether press started on a node (for pan vs move).
    let preview_nid = app.node_editor_ui.preview_node;
    let press_on_node = ui.input(|i| i.pointer.press_origin()).is_some_and(|o| {
        app.project.document.layers[layer_idx]
            .node_graph
            .as_ref()
            .map(|gg| {
                gg.nodes.values().any(|n| {
                    let prev = preview_nid == Some(n.id);
                    let r = graph_to_screen(
                        n.x,
                        n.y,
                        node_width(n),
                        node_height(n, prev),
                        rect,
                        &gg.view,
                    );
                    r.contains(o)
                })
            })
            .unwrap_or(false)
    });
    let can_edit = app.node_editor_ui.can_edit();
    let allow_pan = app.node_editor_ui.wire_drag.is_none()
        && app.node_editor_ui.node_drag.is_none()
        && (!can_edit || !press_on_node);

    if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
        if ctrl && plus {
            g.view.zoom = (g.view.zoom * 1.12).clamp(0.15, 8.0);
        }
        if ctrl && minus {
            g.view.zoom = (g.view.zoom / 1.12).clamp(0.15, 8.0);
        }
        if ctrl && scroll.y != 0.0 {
            let f = if scroll.y > 0.0 { 1.08 } else { 1.0 / 1.08 };
            g.view.zoom = (g.view.zoom * f).clamp(0.15, 8.0);
        }
        let pan_drag = response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged() && allow_pan);
        if pan_drag {
            let d = response.drag_delta();
            g.view.pan_x += d.x;
            g.view.pan_y += d.y;
        }
    }

    // Grid
    if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_ref() {
        paint_grid(&painter, rect, g.view.pan_x, g.view.pan_y, g.view.zoom);
    }

    // Interaction + draw need mutable graph + ui state carefully.
    let pointer = response.interact_pointer_pos();
    let edit = app.node_editor_ui.can_edit();

    // Collect node rects in screen space for hit testing
    let mut node_screen: Vec<(Uuid, Rect)> = Vec::new();
    if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_ref() {
        for n in g.nodes.values() {
            let prev = preview_nid == Some(n.id);
            let r = graph_to_screen(n.x, n.y, node_width(n), node_height(n, prev), rect, &g.view);
            node_screen.push((n.id, r));
        }
    }

    // Wire preview
    if let Some((from_n, from_p)) = app.node_editor_ui.wire_drag.clone() {
        if let Some(mp) = pointer {
            app.node_editor_ui.wire_cursor = Some(mp);
            if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_ref() {
                if let Some(node) = g.nodes.get(&from_n) {
                    if let Some(start) = port_screen_pos(
                        node,
                        &from_p,
                        PortDir::Output,
                        rect,
                        &g.view,
                        preview_nid == Some(from_n),
                    ) {
                        paint_wire_flowchart(
                            &painter,
                            start,
                            mp,
                            Color32::from_rgb(120, 200, 140),
                            2.0,
                            &[],
                            rect,
                            &g.view,
                        );
                    }
                }
            }
        }
    }

    // Draw existing links + hit-test for connector select.
    let mut wire_hit: Option<Uuid> = None;
    let mut wire_hit_dist = f32::MAX;
    if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_ref() {
        let sel_node = app.node_editor_ui.selected;
        let sel_link = app.node_editor_ui.selected_link;
        for link in &g.links {
            let Some(a) = g.nodes.get(&link.from_node) else {
                continue;
            };
            let Some(b) = g.nodes.get(&link.to_node) else {
                continue;
            };
            let Some(p0) = port_screen_pos(
                a,
                &link.from_port,
                PortDir::Output,
                rect,
                &g.view,
                preview_nid == Some(link.from_node),
            ) else {
                continue;
            };
            let Some(p1) = port_screen_pos(
                b,
                &link.to_port,
                PortDir::Input,
                rect,
                &g.view,
                preview_nid == Some(link.to_node),
            ) else {
                continue;
            };
            let link_selected = sel_link == Some(link.id);
            let endpoint_sel =
                sel_node == Some(link.from_node) || sel_node == Some(link.to_node);
            let col = if link_selected {
                Color32::from_rgb(255, 200, 80)
            } else if endpoint_sel {
                Color32::from_rgb(180, 160, 90)
            } else {
                Color32::from_rgb(90, 100, 120)
            };
            let thick = if link_selected {
                3.4
            } else if endpoint_sel {
                2.8
            } else {
                1.6
            };
            let obs: Vec<kurbo::Rect> = g
                .nodes
                .values()
                .filter(|n| n.id != link.from_node && n.id != link.to_node)
                .map(|n| {
                    let prev = preview_nid == Some(n.id);
                    let h = node_height(n, prev) as f64;
                    kurbo::Rect::new(
                        n.x as f64 - 12.0,
                        n.y as f64 - 12.0,
                        (n.x + node_width(n)) as f64 + 12.0,
                        n.y as f64 + h + 12.0,
                    )
                })
                .collect();
            let pts = wire_screen_points(p0, p1, &obs, rect, &g.view);
            paint_polyline_curved(&painter, &pts, col, thick);
            if link_selected {
                painter.circle_filled(p0, 4.0, Color32::from_rgb(80, 200, 100));
                painter.circle_filled(p1, 4.0, Color32::from_rgb(220, 80, 80));
            }
            // Hit-test wire (pick closest within 10 px).
            if let Some(mp) = pointer {
                let d = dist_point_to_polyline(mp, &pts);
                if d < 10.0 && d < wire_hit_dist {
                    wire_hit_dist = d;
                    wire_hit = Some(link.id);
                }
            }
        }
    }

    // Draw nodes + handle interactions
    let mut delete_id: Option<Uuid> = None;
    let mut delete_link: Option<Uuid> = None;
    let mut start_wire: Option<(Uuid, String)> = None;
    let mut finish_wire: Option<(Uuid, String)> = None;
    let mut click_select: Option<Uuid> = None;
    let mut value_edits: Vec<(Uuid, f64)> = Vec::new();
    let mut expr_edits: Vec<(Uuid, String)> = Vec::new();
    let mut path_edits: Vec<(Uuid, String)> = Vec::new();
    let mut begin_node_drag: Option<(Uuid, Vec2)> = None;

    // Clone ids for iteration
    let node_ids: Vec<Uuid> = app.project.document.layers[layer_idx]
        .node_graph
        .as_ref()
        .map(|g| g.nodes.keys().copied().collect())
        .unwrap_or_default();

    for nid in node_ids {
        let (node_clone, view) = {
            let g = app.project.document.layers[layer_idx]
                .node_graph
                .as_ref()
                .unwrap();
            let n = g.nodes.get(&nid).unwrap().clone();
            (n, g.view.clone())
        };
        let preview_open = app.node_editor_ui.preview_node == Some(nid);
        let r = graph_to_screen(
            node_clone.x,
            node_clone.y,
            node_width(&node_clone),
            node_height(&node_clone, preview_open),
            rect,
            &view,
        );
        // Outside the canvas: still hit-test for drag if started, but never allocate widgets
        // off-canvas (that was expanding the Node Editor window).
        let on_canvas = r.intersects(rect);
        let selected = app.node_editor_ui.selected == Some(nid);
        let live_real = app.project.document.layers[layer_idx]
            .node_graph
            .as_ref()
            .and_then(|g| g.last_real_out(nid));
        if !on_canvas {
            // Fully off-canvas: no widgets (would expand the window). Sticky drag still works.
            continue;
        }
        paint_node_card(
            &painter,
            r,
            &node_clone,
            selected,
            live_real,
            &view,
        );

        // Title bar: show delete while pointer is over the title strip (incl. trash hit target).
        let title_h = (22.0 * view.zoom).clamp(18.0, 28.0);
        let title_rect = Rect::from_min_size(r.min, Vec2::new(r.width(), title_h));
        let ptr = pointer.unwrap_or(Pos2::new(-99999.0, -99999.0));
        let del_size = (18.0 * view.zoom.max(0.85)).max(16.0);
        let del_rect = Rect::from_center_size(
            Pos2::new(r.max.x - del_size * 0.7, r.min.y + title_h * 0.5),
            Vec2::splat(del_size),
        );
        // Delete via canvas pointer only — never ui.interact (expands layout / window).
        let del_hit = del_rect.intersect(canvas_clip);
        let del_hover = del_hit.width() > 1.0
            && del_hit.height() > 1.0
            && del_hit.contains(ptr);
        let title_hover = title_rect.intersect(canvas_clip).contains(ptr) || del_hover;
        if title_hover && canvas_clip.contains(del_rect.center()) {
            painter.text(
                del_rect.center(),
                egui::Align2::CENTER_CENTER,
                icons::DELETE,
                nerd_font_id(12.0 * view.zoom.max(0.85)),
                Color32::from_rgb(255, 100, 110),
            );
            if del_hover {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if response.clicked() && del_hover {
            delete_id = Some(nid);
        }

        // Port hits
        let ports = node_clone.ports();
        for (pi, port) in ports.iter().enumerate() {
            let pr = port_rect_for(&node_clone, port, pi, rect, &view, preview_open);
            let hot = pointer.map(|p| pr.expand(4.0).contains(p)).unwrap_or(false);
            let col = match port.ty {
                PortType::RawImage => Color32::from_rgb(100, 160, 255),
                PortType::RawSound => Color32::from_rgb(200, 140, 255),
                PortType::Real => Color32::from_rgb(240, 200, 80),
                PortType::Color => Color32::from_rgb(255, 120, 160),
                PortType::Position => Color32::from_rgb(120, 220, 180),
            };
            let pr_r = (PORT_R * view.zoom.max(0.7)).clamp(4.0, 10.0);
            painter.circle_filled(pr.center(), pr_r, col);
            painter.circle_stroke(
                pr.center(),
                pr_r,
                egui::Stroke::new(1.0, Color32::from_gray(40)),
            );
            // Port name labels — only when pin center is inside canvas (no bleed).
            let label_font =
                egui::FontId::proportional((9.0 * view.zoom).clamp(7.0, 15.0));
            let label_col = Color32::from_rgb(160, 168, 184);
            if canvas_clip.contains(pr.center()) {
                match port.dir {
                    PortDir::Input => {
                        painter.text(
                            pr.center() + Vec2::new(8.0 * view.zoom.max(0.7), 0.0),
                            egui::Align2::LEFT_CENTER,
                            &port.name,
                            label_font.clone(),
                            label_col,
                        );
                    }
                    PortDir::Output => {
                        let out_label = if port.ty == PortType::Real {
                            if let Some(v) = live_real {
                                format!("{}={:.3}", port.name, v)
                            } else {
                                port.name.clone()
                            }
                        } else {
                            port.name.clone()
                        };
                        painter.text(
                            pr.center() - Vec2::new(8.0 * view.zoom.max(0.7), 0.0),
                            egui::Align2::RIGHT_CENTER,
                            out_label,
                            label_font,
                            if port.ty == PortType::Real {
                                Color32::from_rgb(240, 210, 100)
                            } else {
                                label_col
                            },
                        );
                    }
                }
            }
            // Half-cut unconnected look: dark wedge
            let connected = app.project.document.layers[layer_idx]
                .node_graph
                .as_ref()
                .map(|g| {
                    g.links.iter().any(|l| match port.dir {
                        PortDir::Input => l.to_node == nid && l.to_port == port.id,
                        PortDir::Output => l.from_node == nid && l.from_port == port.id,
                    })
                })
                .unwrap_or(false);
            if !connected {
                let cut = match port.dir {
                    PortDir::Input => Rect::from_min_max(
                        Pos2::new(pr.min.x, pr.min.y),
                        Pos2::new(pr.center().x, pr.max.y),
                    ),
                    PortDir::Output => Rect::from_min_max(
                        Pos2::new(pr.center().x, pr.min.y),
                        Pos2::new(pr.max.x, pr.max.y),
                    ),
                };
                painter.rect_filled(cut, 0.0, Color32::from_rgb(22, 24, 30));
                painter.circle_stroke(
                    pr.center(),
                    PORT_R,
                    egui::Stroke::new(1.2, col.gamma_multiply(0.8)),
                );
            }

            if edit && hot && response.drag_started() && port.dir == PortDir::Output {
                start_wire = Some((nid, port.id.clone()));
            }
            if edit
                && hot
                && response.drag_stopped()
                && port.dir == PortDir::Input
                && app.node_editor_ui.wire_drag.is_some()
            {
                finish_wire = Some((nid, port.id.clone()));
            }
        }

        // Sticky node drag: primary press on body (not port, not delete) → grab until release.
        // Later nodes in iteration win (drawn on top).
        if edit
            && app.node_editor_ui.wire_drag.is_none()
            && response.drag_started()
            && app.node_editor_ui.node_drag.is_none()
        {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                let on_del = del_rect.expand(2.0).contains(origin);
                let on_port = ports.iter().enumerate().any(|(pi, port)| {
                    let pr = port_rect_for(&node_clone, port, pi, rect, &view, preview_open);
                    pr.expand(6.0).contains(origin)
                });
                if r.contains(origin) && !on_del && !on_port {
                    let (gx, gy) = screen_to_graph(origin, rect, &view);
                    begin_node_drag =
                        Some((nid, Vec2::new(gx - node_clone.x, gy - node_clone.y)));
                }
            }
        }

        // Select on body click, but not when trash was clicked.
        if response.clicked()
            && r.contains(pointer.unwrap_or(Pos2::ZERO))
            && !del_rect.contains(pointer.unwrap_or(Pos2::ZERO))
            && delete_id.is_none()
        {
            click_select = Some(nid);
        }

        // Painter-only node chrome (no allocate_ui_at_rect — that was resizing the window).
        {
            let z = view.zoom.max(0.5);
            let title_h = (22.0 * z).clamp(16.0, 32.0);
            let pad = 5.0 * z;
            let bottom_h = (15.0 * z).clamp(12.0, 22.0);
            let fs = (10.0 * z).clamp(8.0, 16.0);
            let font = egui::FontId::proportional(fs);
            let link_col = Color32::from_rgb(140, 180, 255);
            let muted = Color32::from_rgb(160, 170, 190);

            let bottom = Rect::from_min_max(
                Pos2::new(r.min.x + pad, r.max.y - pad - bottom_h),
                Pos2::new(r.max.x - pad, r.max.y - pad),
            );
            let body = Rect::from_min_max(
                Pos2::new(r.min.x + pad, r.min.y + title_h + 2.0),
                Pos2::new(r.max.x - pad, bottom.min.y - 2.0),
            );

            let is_media = matches!(
                node_clone.kind,
                GraphNodeKind::ObjectImage { .. }
                    | GraphNodeKind::ObjectVideo { .. }
                    | GraphNodeKind::ObjectAudio { .. }
            );
            let is_from_app = matches!(node_clone.kind, GraphNodeKind::ObjectFromApp { .. });
            let (has_in, has_out) = NodeGraph::image_port_dirs(&node_clone.kind);
            let can_preview = (has_in || has_out)
                && !matches!(node_clone.kind, GraphNodeKind::ObjectAudio { .. });

            // Hit targets (clipped)
            let browse_rect = Rect::from_min_size(
                Pos2::new(body.min.x, body.min.y + fs + 2.0),
                Vec2::new(body.width() * 0.45, fs + 4.0),
            )
            .intersect(canvas_clip);
            let prev_rect = bottom.intersect(canvas_clip);
            let usesel_rect = Rect::from_min_size(
                Pos2::new(body.min.x, body.min.y + fs + 2.0),
                Vec2::new(body.width() * 0.55, fs + 4.0),
            )
            .intersect(canvas_clip);

            let click = response.clicked()
                && pointer.is_some_and(|p| canvas_clip.contains(p));
            let ptr = pointer.unwrap_or(Pos2::new(-99999.0, -99999.0));

            if is_media {
                let path = match &node_clone.kind {
                    GraphNodeKind::ObjectImage { path }
                    | GraphNodeKind::ObjectVideo { path }
                    | GraphNodeKind::ObjectAudio { path } => path.clone(),
                    _ => String::new(),
                };
                let display = truncate_middle(
                    if path.is_empty() {
                        "(no file)"
                    } else {
                        std::path::Path::new(&path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(path.as_str())
                    },
                    (12.0 * z).clamp(8.0, 18.0) as usize,
                );
                painter.text(
                    Pos2::new(body.min.x, body.min.y + fs * 0.5),
                    egui::Align2::LEFT_CENTER,
                    display,
                    font.clone(),
                    muted,
                );
                if (edit || path.is_empty())
                    && browse_rect.width() > 20.0
                    && canvas_clip.contains(browse_rect.left_center())
                {
                    painter.text(
                        browse_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        "Browse",
                        font.clone(),
                        link_col,
                    );
                    if click && browse_rect.contains(ptr) {
                        let mut dlg = rfd::FileDialog::new();
                        dlg = match &node_clone.kind {
                            GraphNodeKind::ObjectImage { .. } => dlg
                                .add_filter(
                                    "Images",
                                    &["png", "jpg", "jpeg", "webp", "gif", "bmp"],
                                )
                                .add_filter("All", &["*"]),
                            GraphNodeKind::ObjectVideo { .. } => dlg
                                .add_filter(
                                    "Video",
                                    &["mp4", "webm", "mov", "mkv", "avi", "m4v"],
                                )
                                .add_filter("All", &["*"]),
                            GraphNodeKind::ObjectAudio { .. } => dlg
                                .add_filter(
                                    "Audio",
                                    &["mp3", "wav", "ogg", "flac", "m4a", "aac"],
                                )
                                .add_filter("All", &["*"]),
                            _ => dlg,
                        };
                        if let Some(p) = dlg.pick_file() {
                            path_edits.push((nid, p.to_string_lossy().into_owned()));
                        }
                    }
                }
            } else if is_from_app {
                let n = match &node_clone.kind {
                    GraphNodeKind::ObjectFromApp { node_ids } => node_ids.len(),
                    _ => 0,
                };
                painter.text(
                    Pos2::new(body.min.x, body.min.y + fs * 0.5),
                    egui::Align2::LEFT_CENTER,
                    format!("{n} obj"),
                    font.clone(),
                    muted,
                );
                if edit {
                    painter.text(
                        usesel_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        "Use sel",
                        font.clone(),
                        link_col,
                    );
                    if click && usesel_rect.contains(ptr) {
                        if let Some(g) =
                            app.project.document.layers[layer_idx].node_graph.as_mut()
                        {
                            if let Some(node) = g.nodes.get_mut(&nid) {
                                if let GraphNodeKind::ObjectFromApp { node_ids } = &mut node.kind {
                                    *node_ids = app.selection.clone();
                                }
                            }
                        }
                    }
                }
            }

            // In-node preview image (base texture only — never heavy bake here).
            if preview_open && can_preview && body.height() > 20.0 {
                let img_rect = Rect::from_min_max(
                    Pos2::new(body.min.x, body.min.y + fs * 2.2),
                    Pos2::new(body.max.x, bottom.min.y - 2.0),
                )
                .intersect(canvas_clip);
                if img_rect.width() > 8.0 && img_rect.height() > 8.0 {
                    let preview_eval = app.project.document.layers[layer_idx]
                        .node_graph
                        .as_ref()
                        .map(|g| {
                            if has_out {
                                g.resolve_node_image_out(nid)
                            } else {
                                let mut found = crate::document::GraphOutputEval::default();
                                for p in node_clone.ports() {
                                    if p.dir == PortDir::Input && p.ty == PortType::RawImage {
                                        if let Some(src) = g.input_source_node(nid, &p.id) {
                                            found = g.resolve_node_image_out(src);
                                            if !matches!(
                                                found.image,
                                                crate::document::GraphImageSource::Empty
                                            ) {
                                                break;
                                            }
                                        }
                                    }
                                }
                                found
                            }
                        });
                    if let Some(eval) = preview_eval {
                        let tid = match &eval.image {
                            crate::document::GraphImageSource::FilePath(path) => {
                                let _ = app.ensure_graph_path_texture(path, ui.ctx());
                                app.graph_path_texture_id(path)
                            }
                            crate::document::GraphImageSource::AppObjects(ids) => {
                                ids.iter().find_map(|id| app.image_texture_id(*id))
                            }
                            crate::document::GraphImageSource::Empty => None,
                        };
                        if let Some(tid) = tid {
                            let uv = egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            );
                            painter.image(tid, img_rect, uv, Color32::WHITE);
                        } else {
                            painter.text(
                                img_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no image",
                                font.clone(),
                                Color32::from_rgb(120, 128, 140),
                            );
                        }
                    }
                }
            }

            // Prev only when the control is fully usable inside the canvas —
            // no floating "Prev" when the node is clipped at any edge.
            if can_preview
                && prev_rect.width() > 18.0
                && prev_rect.height() > 10.0
                && canvas_clip.contains(prev_rect.center())
            {
                let plabel = if preview_open { "Hide" } else { "Prev" };
                painter.text(
                    prev_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    plabel,
                    font.clone(),
                    link_col,
                );
                if click && prev_rect.contains(ptr) {
                    app.node_editor_ui.preview_node =
                        if preview_open { None } else { Some(nid) };
                }
            }

            // Live algebra readouts (scaled with zoom).
            if matches!(
                node_clone.kind,
                GraphNodeKind::Frame | GraphNodeKind::Time | GraphNodeKind::ParamReal { .. }
            ) {
                if let Some(v) = live_real {
                    let label = match node_clone.kind {
                        GraphNodeKind::Frame => format!("f={v:.0}"),
                        GraphNodeKind::Time => format!("t={v:.3}s"),
                        GraphNodeKind::ParamReal { .. } => format!("p={v:.3}"),
                        _ => format!("{v:.3}"),
                    };
                    painter.text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::monospace((11.0 * z).clamp(8.0, 18.0)),
                        Color32::from_rgb(240, 210, 100),
                    );
                }
            }
            if let GraphNodeKind::Expr { ref expr } = node_clone.kind {
                if !edit {
                    if let Some(v) = live_real {
                        painter.text(
                            body.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{}={v:.2}", truncate_middle(expr, 10)),
                            egui::FontId::monospace((10.0 * z).clamp(8.0, 16.0)),
                            Color32::from_rgb(240, 210, 100),
                        );
                    }
                }
            }
            if let GraphNodeKind::Value { value } = node_clone.kind {
                if !edit {
                    painter.text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("v={value:.3}"),
                        egui::FontId::monospace((11.0 * z).clamp(8.0, 18.0)),
                        Color32::from_rgb(240, 210, 100),
                    );
                }
            }
            let _ = edit; // Value/Expr inline editors only when selected+edit below
        }

        // Value / Expr: only when the field fully fits inside the canvas (no edge bleed,
        // no Area outside the window that fights layout).
        if edit && app.node_editor_ui.selected == Some(nid) {
            if matches!(node_clone.kind, GraphNodeKind::Value { .. }) {
                let edit_rect = Rect::from_min_size(
                    Pos2::new(r.min.x + 10.0 * view.zoom.max(0.7), r.min.y + 26.0 * view.zoom.max(0.7)),
                    Vec2::new((r.width() - 20.0).max(20.0), (18.0 * view.zoom).clamp(16.0, 28.0)),
                );
                if canvas_clip.contains(edit_rect.min)
                    && canvas_clip.contains(edit_rect.max - Vec2::splat(0.5))
                {
                    let mut v = match node_clone.kind {
                        GraphNodeKind::Value { value } => value,
                        _ => 0.0,
                    };
                    // Painter readout only when partially clipped; full editor when fully inside.
                    let resp = egui::Area::new(egui::Id::new(("ne_val_area", nid)))
                        .order(egui::Order::Foreground)
                        .fixed_pos(edit_rect.min)
                        .constrain_to(canvas_clip)
                        .interactable(true)
                        .show(ui.ctx(), |ui| {
                            ui.set_max_size(edit_rect.size());
                            ui.add_sized(
                                edit_rect.size(),
                                egui::DragValue::new(&mut v).speed(0.1).prefix("v="),
                            )
                        })
                        .inner;
                    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        resp.surrender_focus();
                    }
                    if resp.changed() {
                        value_edits.push((nid, v));
                    }
                }
            }
            if let GraphNodeKind::Expr { ref expr } = node_clone.kind {
                let edit_rect = Rect::from_min_size(
                    Pos2::new(r.min.x + 8.0 * view.zoom.max(0.7), r.min.y + 26.0 * view.zoom.max(0.7)),
                    Vec2::new((r.width() - 16.0).max(20.0), (18.0 * view.zoom).clamp(16.0, 28.0)),
                );
                if canvas_clip.contains(edit_rect.min)
                    && canvas_clip.contains(edit_rect.max - Vec2::splat(0.5))
                {
                    let mut e = expr.clone();
                    let resp = egui::Area::new(egui::Id::new(("ne_expr_area", nid)))
                        .order(egui::Order::Foreground)
                        .fixed_pos(edit_rect.min)
                        .constrain_to(canvas_clip)
                        .interactable(true)
                        .show(ui.ctx(), |ui| {
                            ui.set_max_size(edit_rect.size());
                            ui.add_sized(
                                edit_rect.size(),
                                egui::TextEdit::singleline(&mut e)
                                    .id(egui::Id::new(("ne_expr", nid)))
                                    .desired_width(edit_rect.width())
                                    .font(egui::TextStyle::Monospace),
                            )
                        })
                        .inner;
                    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        resp.surrender_focus();
                    }
                    if resp.changed() {
                        expr_edits.push((nid, e));
                    }
                }
            }
        }
    }

    // Hover affordance for connector pick.
    if wire_hit.is_some() && app.node_editor_ui.wire_drag.is_none() {
        response.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    // Click empty / wire: select connector (prefer wire over deselect).
    if response.clicked() && app.node_editor_ui.wire_drag.is_none() {
        if let Some(lid) = wire_hit {
            app.node_editor_ui.selected_link = Some(lid);
            app.node_editor_ui.selected = None;
            click_select = None;
            app.status_message = "Connector selected — Delete/Backspace to remove".into();
        } else if click_select.is_none() {
            // Clicked empty canvas (not a node) → clear wire selection.
            let on_any_node = pointer.is_some_and(|mp| {
                node_screen.iter().any(|(_, nr)| nr.contains(mp))
            });
            if !on_any_node {
                app.node_editor_ui.selected_link = None;
            }
        }
    }
    if click_select.is_some() {
        // Selecting a node clears link selection.
        app.node_editor_ui.selected_link = None;
    }

    // Keyboard: Delete / Backspace — selected wire first, else selected node.
    let text_focused = ui.ctx().wants_keyboard_input();
    let key_delete = !text_focused
        && ui.input(|i| {
            !i.modifiers.ctrl
                && !i.modifiers.command
                && (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        });
    if key_delete {
        if let Some(lid) = app.node_editor_ui.selected_link {
            delete_link = Some(lid);
        } else if let Some(id) = app.node_editor_ui.selected {
            delete_id = Some(id);
        }
    }

    // Apply mutations
    if let Some(lid) = delete_link {
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
            g.links.retain(|l| l.id != lid);
        }
        app.node_editor_ui.selected_link = None;
        app.status_message = "Connector deleted".into();
        click_select = None;
    }
    if let Some(id) = delete_id {
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
            g.remove_node(id);
        }
        if app.node_editor_ui.selected == Some(id) {
            app.node_editor_ui.selected = None;
        }
        app.node_editor_ui.selected_link = None;
        app.node_editor_ui.node_drag = None;
        app.node_editor_ui.wire_drag = None;
        app.status_message = "Node deleted".into();
        click_select = None;
    }
    if let Some(grab) = begin_node_drag {
        app.node_editor_ui.node_drag = Some(grab);
        app.node_editor_ui.selected = Some(grab.0);
        app.node_editor_ui.selected_link = None;
    }
    // Sticky drag: follow pointer every frame while held.
    if let Some((id, grab_off)) = app.node_editor_ui.node_drag {
        if response.dragged() || ui.input(|i| i.pointer.primary_down()) {
            if let Some(mp) = pointer.or_else(|| ui.input(|i| i.pointer.interact_pos())) {
                if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
                    let (gx, gy) = screen_to_graph(mp, rect, &g.view);
                    if let Some(n) = g.nodes.get_mut(&id) {
                        n.x = gx - grab_off.x;
                        n.y = gy - grab_off.y;
                    }
                }
            }
        }
        if response.drag_stopped() || ui.input(|i| i.pointer.primary_released()) {
            app.node_editor_ui.node_drag = None;
        }
    }
    if let Some(w) = start_wire {
        app.node_editor_ui.wire_drag = Some(w);
        app.node_editor_ui.node_drag = None; // don't move while wiring
    }
    if response.drag_stopped() {
        if let Some((from_n, from_p)) = app.node_editor_ui.wire_drag.take() {
            if let Some((to_n, to_p)) = finish_wire {
                if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
                    match g.try_add_link(from_n, &from_p, to_n, &to_p) {
                        Ok(()) => app.status_message = "Connected".into(),
                        Err(e) => app.status_message = e,
                    }
                }
            } else if let Some(mp) = pointer {
                // Drop on empty → add menu
                let on_node = node_screen.iter().any(|(_, r)| r.contains(mp));
                if !on_node {
                    app.node_editor_ui.add_menu_at = Some((mp, Some((from_n, from_p))));
                }
            }
        }
        app.node_editor_ui.wire_cursor = None;
        app.node_editor_ui.node_drag = None;
    }
    if delete_id.is_none() {
        if let Some(id) = click_select {
            app.node_editor_ui.selected = Some(id);
        } else if response.clicked()
            && app.node_editor_ui.wire_drag.is_none()
            && app.node_editor_ui.node_drag.is_none()
        {
            // Click empty canvas → deselect.
            if let Some(mp) = pointer {
                let on_any = node_screen.iter().any(|(_, r)| r.contains(mp));
                if !on_any {
                    app.node_editor_ui.selected = None;
                }
            }
        }
    }
    for (id, v) in value_edits {
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
            if let Some(n) = g.nodes.get_mut(&id) {
                if let GraphNodeKind::Value { value } = &mut n.kind {
                    *value = v;
                }
            }
        }
    }
    for (id, e) in expr_edits {
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
            if let Some(n) = g.nodes.get_mut(&id) {
                if let GraphNodeKind::Expr { expr } = &mut n.kind {
                    *expr = e;
                }
            }
        }
    }
    for (id, path) in path_edits {
        if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
            if let Some(n) = g.nodes.get_mut(&id) {
                match &mut n.kind {
                    GraphNodeKind::ObjectImage { path: p }
                    | GraphNodeKind::ObjectVideo { path: p }
                    | GraphNodeKind::ObjectAudio { path: p } => {
                        *p = path.clone();
                        app.status_message = "Media path set".into();
                        // Streaming playback — do not full-decode into RAM on Browse
                        // (that OOM'd machines for long MP3s / thread storms).
                        if matches!(n.kind, GraphNodeKind::ObjectAudio { .. }) && !path.is_empty() {
                            app.status_message = "Audio path set (streams on Play)".into();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Root error banner
    if let Some(err) = app.project.document.layers[layer_idx]
        .node_graph
        .as_ref()
        .and_then(|g| g.root_error.clone())
    {
        painter.text(
            rect.left_top() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            err,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(255, 120, 120),
        );
    }

    // Drop-add menu (type-filtered when dropping a wire).
    if let Some((pos, wire_from)) = app.node_editor_ui.add_menu_at.clone() {
        let from_ty = wire_from.as_ref().and_then(|(fnid, fpid)| {
            app.project.document.layers[layer_idx]
                .node_graph
                .as_ref()
                .and_then(|g| g.port_type(*fnid, fpid))
        });
        let choices: Vec<GraphNodeKind> = if let Some(ty) = from_ty {
            NodeGraph::catalog_kinds_accepting(ty)
        } else {
            vec![
                GraphNodeKind::Value { value: 0.0 },
                GraphNodeKind::Expr {
                    expr: "x".into(),
                },
                GraphNodeKind::Brightness,
                GraphNodeKind::ObjectFromApp {
                    node_ids: app.selection.clone(),
                },
            ]
        };
        egui::Area::new(egui::Id::new("node_add_drop_menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    let header = if let Some(ty) = from_ty {
                        format!("Add (accepts {})", ty.label())
                    } else {
                        "Add node".into()
                    };
                    ui.label(RichText::new(header).strong());
                    if choices.is_empty() {
                        ui.label(RichText::new("No compatible nodes").small().weak());
                    }
                    let mut choice: Option<GraphNodeKind> = None;
                    for kind in &choices {
                        let label = format!("{} · {}", kind.category_label(), kind.default_title());
                        if ui.button(label).clicked() {
                            choice = Some(kind.clone());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        app.node_editor_ui.add_menu_at = None;
                    }
                    if let Some(kind) = choice {
                        let (gx, gy) = screen_to_graph(
                            pos,
                            rect,
                            &app.project.document.layers[layer_idx]
                                .node_graph
                                .as_ref()
                                .map(|g| g.view.clone())
                                .unwrap_or_default(),
                        );
                        if let Some(g) = app.project.document.layers[layer_idx]
                            .node_graph
                            .as_mut()
                        {
                            let id = g.add_node(kind, gx, gy);
                            if let Some((from_n, from_p)) = wire_from {
                                let from_ty = g.port_type(from_n, &from_p);
                                if let Some(ports) = g.nodes.get(&id).map(|n| n.ports()) {
                                    // Prefer first compatible input (type match).
                                    for p in ports {
                                        if p.dir != PortDir::Input {
                                            continue;
                                        }
                                        if let Some(fty) = from_ty {
                                            if !PortType::can_connect(fty, p.ty) {
                                                continue;
                                            }
                                        }
                                        let _ = g.try_add_link(from_n, &from_p, id, &p.id);
                                        break;
                                    }
                                }
                            }
                            app.node_editor_ui.selected = Some(id);
                        }
                        app.node_editor_ui.add_menu_at = None;
                    }
                });
            });
    }

    response.context_menu(|ui| {
        if ui.button("Add Value here").clicked() {
            if let Some(mp) = pointer {
                let view = app.project.document.layers[layer_idx]
                    .node_graph
                    .as_ref()
                    .map(|g| g.view.clone())
                    .unwrap_or_default();
                let (gx, gy) = screen_to_graph(mp, rect, &view);
                if let Some(g) = app.project.document.layers[layer_idx].node_graph.as_mut() {
                    let id = g.add_node(GraphNodeKind::Value { value: 0.0 }, gx, gy);
                    app.node_editor_ui.selected = Some(id);
                }
            }
            ui.close();
        }
    });

}

fn truncate_middle(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    if max_chars < 5 {
        return chars.into_iter().take(max_chars).collect();
    }
    let keep = max_chars - 1; // room for …
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars.iter().take(head).collect();
    out.push('…');
    out.extend(chars.iter().rev().take(tail).rev());
    out
}



fn paint_grid(painter: &egui::Painter, rect: Rect, pan_x: f32, pan_y: f32, zoom: f32) {
    let step = 24.0 * zoom;
    if step < 4.0 {
        return;
    }
    let col = Color32::from_rgba_unmultiplied(55, 60, 75, 90);
    let origin = rect.min + Vec2::new(pan_x.rem_euclid(step), pan_y.rem_euclid(step));
    let mut x = origin.x - step;
    while x < rect.max.x + step {
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            egui::Stroke::new(1.0, col),
        );
        x += step;
    }
    let mut y = origin.y - step;
    while y < rect.max.y + step {
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            egui::Stroke::new(1.0, col),
        );
        y += step;
    }
}

fn graph_to_screen(x: f32, y: f32, w: f32, h: f32, canvas: Rect, view: &crate::document::GraphView) -> Rect {
    let z = view.zoom;
    let min = canvas.min + Vec2::new(x * z + view.pan_x, y * z + view.pan_y);
    Rect::from_min_size(min, Vec2::new(w * z, h * z))
}

fn screen_to_graph(p: Pos2, canvas: Rect, view: &crate::document::GraphView) -> (f32, f32) {
    let z = view.zoom.max(0.01);
    let lx = (p.x - canvas.min.x - view.pan_x) / z;
    let ly = (p.y - canvas.min.y - view.pan_y) / z;
    (lx, ly)
}

fn port_screen_pos(
    node: &crate::document::GraphNode,
    port_id: &str,
    dir: PortDir,
    canvas: Rect,
    view: &crate::document::GraphView,
    preview_open: bool,
) -> Option<Pos2> {
    let ports = node.ports();
    let idx = ports.iter().position(|p| p.id == port_id && p.dir == dir)?;
    let r = port_rect_for(node, &ports[idx], idx, canvas, view, preview_open);
    Some(r.center())
}

fn port_rect_for(
    node: &crate::document::GraphNode,
    port: &crate::document::PortDef,
    index_among_all: usize,
    canvas: Rect,
    view: &crate::document::GraphView,
    preview_open: bool,
) -> Rect {
    let h = node_height(node, preview_open);
    let body = graph_to_screen(node.x, node.y, node_width(node), h, canvas, view);
    let inputs: Vec<_> = node.ports().into_iter().filter(|p| p.dir == PortDir::Input).collect();
    let outputs: Vec<_> = node.ports().into_iter().filter(|p| p.dir == PortDir::Output).collect();
    let (side_index, total) = match port.dir {
        PortDir::Input => (
            inputs.iter().position(|p| p.id == port.id).unwrap_or(0),
            inputs.len().max(1),
        ),
        PortDir::Output => (
            outputs.iter().position(|p| p.id == port.id).unwrap_or(0),
            outputs.len().max(1),
        ),
    };
    let _ = index_among_all;
    let y_frac = (side_index as f32 + 0.5) / total as f32;
    let y = body.min.y + 24.0 * view.zoom + (body.height() - 28.0 * view.zoom) * y_frac;
    let x = match port.dir {
        PortDir::Input => body.min.x,
        PortDir::Output => body.max.x,
    };
    Rect::from_center_size(Pos2::new(x, y), Vec2::splat(PORT_R * 2.0 * view.zoom.max(0.7)))
}

fn paint_node_card(
    painter: &egui::Painter,
    r: Rect,
    node: &crate::document::GraphNode,
    selected: bool,
    live_real: Option<f64>,
    view: &crate::document::GraphView,
) {
    let fill = Color32::from_rgb(36, 40, 52);
    let stroke = if selected {
        Color32::from_rgb(90, 150, 255)
    } else if node.error.is_some() {
        Color32::from_rgb(220, 80, 80)
    } else {
        Color32::from_rgb(70, 78, 96)
    };
    painter.rect(
        r,
        egui::CornerRadius::same(8),
        fill,
        egui::Stroke::new(if selected { 2.0 } else { 1.2 }, stroke),
        egui::StrokeKind::Inside,
    );
    // Title bar
    let title_h = 22.0_f32.min(r.height() * 0.4);
    let title_r = Rect::from_min_size(r.min, Vec2::new(r.width(), title_h));
    painter.rect_filled(
        title_r,
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        Color32::from_rgb(48, 54, 70),
    );
    let z = view.zoom.max(0.5);
    // Full title — card width is sized via node_width() to fit (no "…").
    let title = format!("{} · {}", node.kind.category_label(), node.name);
    painter.text(
        title_r.left_center() + Vec2::new(6.0 * z, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional((11.0 * z).clamp(8.0, 18.0)),
        colors::TEXT,
    );
    // Live Real chip on title bar (compact).
    if let Some(v) = live_real {
        if matches!(
            node.kind,
            GraphNodeKind::Value { .. }
                | GraphNodeKind::Expr { .. }
                | GraphNodeKind::Frame
                | GraphNodeKind::Time
                | GraphNodeKind::ParamReal { .. }
        ) {
            let chip = format!("{v:.2}");
            painter.text(
                title_r.right_center() - Vec2::new(20.0 * z, 0.0),
                egui::Align2::RIGHT_CENTER,
                chip,
                egui::FontId::monospace((10.0 * z).clamp(8.0, 16.0)),
                Color32::from_rgb(200, 180, 80),
            );
        }
    }
    if let Some(err) = &node.error {
        painter.text(
            r.center_bottom() - Vec2::new(0.0, 4.0 * z),
            egui::Align2::CENTER_BOTTOM,
            truncate_middle(err, 24),
            egui::FontId::proportional((9.0 * z).clamp(7.0, 14.0)),
            Color32::from_rgb(255, 140, 140),
        );
    }
}

/// Screen-space polyline for a graph wire (for paint + hit-test).
fn wire_screen_points(
    a_screen: Pos2,
    b_screen: Pos2,
    obstacles_graph: &[kurbo::Rect],
    canvas: Rect,
    view: &crate::document::GraphView,
) -> Vec<Pos2> {
    let (ax, ay) = screen_to_graph(a_screen, canvas, view);
    let (bx, by) = screen_to_graph(b_screen, canvas, view);
    let start = (ax as f64, ay as f64);
    let end = (bx as f64, by as f64);
    let start_n = (1.0_f64, 0.0);
    let end_n = (-1.0_f64, 0.0);
    let routed = crate::document::flowchart::route_orthogonal_with_normals(
        start,
        end,
        start_n,
        end_n,
        obstacles_graph,
    );
    routed
        .into_iter()
        .map(|(x, y)| {
            let r = graph_to_screen(x as f32, y as f32, 0.0, 0.0, canvas, view);
            r.min
        })
        .collect()
}

fn dist_point_to_polyline(p: Pos2, pts: &[Pos2]) -> f32 {
    if pts.len() < 2 {
        return f32::MAX;
    }
    let mut best = f32::MAX;
    for i in 0..pts.len() - 1 {
        best = best.min(dist_point_to_segment(p, pts[i], pts[i + 1]));
    }
    best
}

fn dist_point_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-8 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    p.distance(proj)
}

/// Route like flowchart: stubs out of ports + obstacle-aware orthogonal mid path.
/// Corners use quarter-circle fillets (no dots).
fn paint_wire_flowchart(
    painter: &egui::Painter,
    a_screen: Pos2,
    b_screen: Pos2,
    color: Color32,
    thickness: f32,
    obstacles_graph: &[kurbo::Rect],
    canvas: Rect,
    view: &crate::document::GraphView,
) {
    let pts = wire_screen_points(a_screen, b_screen, obstacles_graph, canvas, view);
    paint_polyline_curved(painter, &pts, color, thickness);
}

/// Draw orthogonal polyline with true quarter-circle corner fillets (no dots).
fn paint_polyline_curved(
    painter: &egui::Painter,
    pts: &[Pos2],
    color: Color32,
    thickness: f32,
) {
    if pts.len() < 2 {
        return;
    }
    let stroke = egui::Stroke::new(thickness, color);
    let max_r = 14.0_f32;

    // Build shortened straight runs + arc corners between them.
    for i in 0..pts.len() - 1 {
        let p0 = pts[i];
        let p1 = pts[i + 1];
        let has_prev = i > 0;
        let has_next = i + 1 < pts.len() - 1;

        let d = (p1 - p0).length().max(1e-3);
        let dir = (p1 - p0) / d;

        // Corner radius limited by half of adjacent segment lengths.
        let r_in = if has_prev {
            let prev_len = (p0 - pts[i - 1]).length();
            max_r.min(prev_len * 0.45).min(d * 0.45)
        } else {
            0.0
        };
        let r_out = if has_next {
            let next_len = (pts[i + 2] - p1).length();
            max_r.min(next_len * 0.45).min(d * 0.45)
        } else {
            0.0
        };

        let start = if r_in > 0.5 {
            p0 + dir * r_in
        } else {
            p0
        };
        let end = if r_out > 0.5 {
            p1 - dir * r_out
        } else {
            p1
        };

        if (end - start).length_sq() > 0.25 {
            painter.line_segment([start, end], stroke);
        }

        // Arc at the bend into the next segment (at p1).
        if has_next && r_out > 0.5 {
            let p2 = pts[i + 2];
            let d2 = (p2 - p1).length().max(1e-3);
            let dir2 = (p2 - p1) / d2;
            let r = r_out;
            let enter = p1 - dir * r;
            let leave = p1 + dir2 * r;
            paint_quarter_arc(painter, enter, p1, leave, r, stroke);
        }
    }
}

/// Approximate a rounded orthogonal corner from `enter` → through `corner` → `leave`
/// with sampled arc (no center-dot).
fn paint_quarter_arc(
    painter: &egui::Painter,
    enter: Pos2,
    corner: Pos2,
    leave: Pos2,
    radius: f32,
    stroke: egui::Stroke,
) {
    // Inward normals for orthogonal edges.
    let v0 = (enter - corner).normalized();
    let v1 = (leave - corner).normalized();
    // Arc center is offset from corner along both unit vectors.
    let center = corner + v0 * radius + v1 * radius;

    let a0 = (enter - center).angle();
    let a1 = (leave - center).angle();
    // Shortest turn matching the corner (quarter turn expected).
    let mut delta = a1 - a0;
    while delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    while delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }

    let steps = 10_usize.max(((delta.abs() / std::f32::consts::FRAC_PI_2) * 8.0) as usize);
    let mut prev = enter;
    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let ang = a0 + delta * t;
        let p = center + Vec2::angled(ang) * radius;
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
    // Ensure we land on leave.
    if (prev - leave).length_sq() > 0.25 {
        painter.line_segment([prev, leave], stroke);
    }
}

/// Parameter tab body for active Node Editor layer.
pub fn parameter_tab_ui(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    // Prefer active layer; if selection is a Node Editor layer, switch active to it.
    if let Some(sel_id) = app.selection.first().copied() {
        if let Some(i) = app
            .project
            .document
            .layers
            .iter()
            .position(|l| l.id == sel_id && l.kind == LayerKind::NodeEditor)
        {
            if app.project.document.active_layer_index != i {
                app.project.document.active_layer_index = i;
            }
        }
    }
    let idx = app.project.document.active_layer_index;
    let is_ne = app
        .project
        .document
        .layers
        .get(idx)
        .is_some_and(|l| l.kind == LayerKind::NodeEditor);
    if !is_ne {
        ui.label(
            RichText::new("Select a Node Editor layer (Layer tab) to edit parameters.")
                .small()
                .weak(),
        );
        ui.label(
            RichText::new("Tip: create or activate a Node Editor layer — the Parameter tab appears in the action bar.")
                .small()
                .weak(),
        );
        return;
    }
    app.project.document.layers[idx].ensure_node_graph();
    // Parameters list = only entries that still have a Param* node in the graph.
    if let Some(g) = app.project.document.layers[idx].node_graph.as_mut() {
        g.sync_parameters_with_nodes();
    }
    let layer_id = app.project.document.layers[idx].id;
    let frame = app.anim_current_frame;
    let mut remove: Option<usize> = None;
    let mut kf_ops: Vec<(String, f64)> = Vec::new();
    let mut open_anim = false;

    {
        let Some(g) = app.project.document.layers[idx].node_graph.as_mut() else {
            return;
        };

        ui.label(RichText::new("Parameters").strong());
        ui.label(
            RichText::new("From graph Param nodes only. Add: Node Editor → Parameter ▾")
                .small()
                .weak(),
        );
        ui.add_space(6.0);

        if g.parameters.is_empty() {
            ui.label(
                RichText::new("No parameter nodes yet. Use Add → Parameter → Real / Color / Position.")
                    .small()
                    .weak(),
            );
        }

        for (i, p) in g.parameters.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(100.0));
                    match p.kind {
                        GraphParamKind::Real => {
                            ui.add(egui::DragValue::new(&mut p.v0).speed(0.05));
                        }
                        GraphParamKind::Color => {
                            let mut c = [
                                p.v0 as f32,
                                p.v1 as f32,
                                p.v2 as f32,
                                p.v3 as f32,
                            ];
                            if ui.color_edit_button_rgba_unmultiplied(&mut c).changed() {
                                p.v0 = c[0] as f64;
                                p.v1 = c[1] as f64;
                                p.v2 = c[2] as f64;
                                p.v3 = c[3] as f64;
                            }
                        }
                        GraphParamKind::Position => {
                            ui.label("x");
                            ui.add(egui::DragValue::new(&mut p.v0).speed(0.5));
                            ui.label("y");
                            ui.add(egui::DragValue::new(&mut p.v1).speed(0.5));
                        }
                    }
                    if ui
                        .button(RichText::new(icons::DELETE).font(nerd_font_id(12.0)))
                        .on_hover_text("Delete parameter and its graph node(s)")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("frame {frame}"))
                            .small()
                            .weak(),
                    );
                    if ui
                        .small_button("+ KF")
                        .on_hover_text("Insert keyframe(s) at current frame")
                        .clicked()
                    {
                        for lbl in p.anim_track_labels() {
                            let colon_parts = lbl.matches(':').count();
                            let v = if colon_parts == 1 {
                                p.v0
                            } else {
                                match lbl.rsplit(':').next().and_then(|s| s.parse::<usize>().ok()) {
                                    Some(0) | None => p.v0,
                                    Some(1) => p.v1,
                                    Some(2) => p.v2,
                                    Some(3) => p.v3,
                                    _ => p.v0,
                                }
                            };
                            kf_ops.push((lbl, v));
                        }
                    }
                    if ui
                        .small_button("Anim")
                        .on_hover_text("Open Animation tab")
                        .clicked()
                    {
                        open_anim = true;
                    }
                });
            });
        }
        if let Some(i) = remove {
            let pid = g.parameters[i].id;
            g.parameters.remove(i);
            let dead: Vec<Uuid> = g
                .nodes
                .iter()
                .filter(|(_, n)| match n.kind {
                    GraphNodeKind::ParamReal { param_id }
                    | GraphNodeKind::ParamColor { param_id }
                    | GraphNodeKind::ParamPosition { param_id } => param_id == pid,
                    _ => false,
                })
                .map(|(id, _)| *id)
                .collect();
            for id in dead {
                // remove_node also syncs parameters list
                g.remove_node(id);
            }
            g.sync_parameters_with_nodes();
        }
    } // drop graph borrow before touching anim_timeline

    if !kf_ops.is_empty() {
        let entry = app
            .project
            .anim_timeline
            .nodes
            .entry(layer_id)
            .or_default();
        for (lbl, v) in kf_ops {
            entry.ensure_track(&lbl);
            if let Some(t) = entry.get_track_mut(&lbl) {
                t.insert(frame, v);
            }
        }
        app.apply_animation_for_frame(frame);
        app.status_message = format!("Keyframe at frame {frame}");
    }
    if open_anim {
        app.action_tab = crate::ui::ActionTab::Animation;
        app.selection = vec![layer_id];
    }
}
