//! Left collaboration dock (chat + settings), one sliding panel at a time.

use egui::scroll_area::ScrollBarVisibility;
use egui::{Context, FontId, Rect, RichText, ScrollArea, Ui};

use crate::animation::left_dock_panel_rect;
use crate::app::VadadeeBerryApp;
use crate::collab::RemotePeer;
use crate::icons;
use crate::theme::{self, colors};
use crate::tools::ToolKind;

pub const PANEL_WIDTH: f32 = 340.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftDockPanel {
    Chat,
    Collab,
}

#[derive(Clone)]
pub struct ChatToast {
    pub username: String,
    pub text: String,
    pub born: f64,
}

pub struct LeftDockState {
    pub active: Option<LeftDockPanel>,
    pub chat_draft: String,
    pub chat_scroll_to_end: bool,
    pub chat_focus_pending: bool,
    pub game_chat_notifications: bool,
    pub chat_toasts: Vec<ChatToast>,
}

impl LeftDockState {
    pub fn push_chat_toast(&mut self, username: String, text: String) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.chat_toasts.push(ChatToast {
            username,
            text,
            born: now,
        });
        if self.chat_toasts.len() > 8 {
            self.chat_toasts.remove(0);
        }
    }

    pub fn tick_toasts(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.chat_toasts.retain(|t| now - t.born < 6.0);
    }
}

impl LeftDockState {
    pub fn toggle(&mut self, panel: LeftDockPanel) {
        if self.active == Some(panel) {
            self.active = None;
        } else {
            self.active = Some(panel);
            if panel == LeftDockPanel::Chat {
                self.chat_focus_pending = true;
            }
        }
    }
}

impl Default for LeftDockState {
    fn default() -> Self {
        Self {
            active: None,
            chat_draft: String::new(),
            chat_scroll_to_end: false,
            chat_focus_pending: false,
            game_chat_notifications: true,
            chat_toasts: Vec::new(),
        }
    }
}

pub fn show(app: &mut VadadeeBerryApp, ctx: &Context, canvas_work: Rect) {
    #[cfg(target_os = "android")]
    {
        let _ = (app, ctx, canvas_work);
        return;
    }

    let open_t = app.ui_anim.left_dock_open_t();
    let animating = app.ui_anim.left_dock_running;
    let opacity = app.ui_anim.left_dock_opacity();
    if app.left_dock.active.is_none() && !animating && open_t <= 0.001 {
        return;
    }
    if opacity <= 0.004 && !animating && app.left_dock.active.is_none() {
        return;
    }

    let inset = theme::overlay_work_rect(canvas_work);
    let toolbar_right = app
        .toolbar_outer_rect
        .map(|r| r.max.x)
        .unwrap_or(inset.left() + theme::TOOLBAR_WIDTH);
    let rect = left_dock_panel_rect(canvas_work, PANEL_WIDTH, open_t, toolbar_right);

    theme::show_action_bar_area(ctx, "left_collab_dock", rect, opacity, |ui| {
        let tab_offset = app.ui_anim.tab_content_offset();
        let tab_alpha = app.ui_anim.tab_content_alpha();
        ui.label(RichText::new("Live").strong().color(colors::TEXT));
        ui.separator();
        ui.add_space(tab_offset);
        theme::action_content_frame_alpha(tab_alpha).show(ui, |ui| {
            let panel = app.left_dock.active.unwrap_or(LeftDockPanel::Chat);
            match panel {
                LeftDockPanel::Chat => chat_body(app, ui),
                LeftDockPanel::Collab => collab_body(app, ui),
            }
        });
    });
}

fn chat_body(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.label(RichText::new("Chat").strong());
    ui.checkbox(
        &mut app.left_dock.game_chat_notifications,
        "Game-style popups (bottom-left)",
    );
    ui.separator();
    let scroll = ScrollArea::vertical()
        .id_salt("collab_chat_scroll")
        .max_height(140.0)
        .auto_shrink([false, true])
        .stick_to_bottom(app.left_dock.chat_scroll_to_end)
        .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    app.left_dock.chat_scroll_to_end = false;
    scroll.show(ui, |ui| {
        ui.set_max_width(ui.available_width());
        for line in app.collab.chat_log() {
            ui.label(
                RichText::new(format!("[{}]: {}", line.username, line.text)).small(),
            );
        }
    });
    ui.separator();
    let draft_id = ui.make_persistent_id("collab_chat_draft");
    if app.left_dock.chat_focus_pending {
        ui.memory_mut(|m| m.request_focus(draft_id));
        app.left_dock.chat_focus_pending = false;
    }
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let draft = ui.add(
            egui::TextEdit::singleline(&mut app.left_dock.chat_draft)
                .id(draft_id)
                .hint_text("Message… (Enter to send)")
                .desired_width(ui.available_width() - 56.0),
        );
        let draft_focused = draft.has_focus();
        if (ui.button("Send").clicked() || (enter && draft_focused))
            && !app.left_dock.chat_draft.trim().is_empty()
        {
            let msg = app.left_dock.chat_draft.trim().to_string();
            app.left_dock.chat_draft.clear();
            app.collab.send_chat(msg);
            app.left_dock.chat_scroll_to_end = true;
        }
    });
}

fn collab_body(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.label(RichText::new("Collaboration").strong());
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut app.collab.ui_config.role,
            crate::collab::CollabRole::Server,
            "Server",
        );
        ui.selectable_value(
            &mut app.collab.ui_config.role,
            crate::collab::CollabRole::Client,
            "Client",
        );
    });
    ui.horizontal(|ui| {
        ui.label("Host");
        ui.text_edit_singleline(&mut app.collab.ui_config.host);
        ui.label("Port");
        ui.add(
            egui::DragValue::new(&mut app.collab.ui_config.port)
                .range(1..=65535)
                .speed(1),
        );
    });
    ui.label("Room");
    ui.text_edit_singleline(&mut app.collab.ui_config.room_id);
    ui.label("Username");
    ui.text_edit_singleline(&mut app.collab.ui_config.username);
    ui.label("Secret");
    ui.add(
        egui::TextEdit::singleline(&mut app.collab.ui_config.secret_key).password(true),
    );
    ui.checkbox(
        &mut app.collab.ui_config.live_canvas_sync,
        "Live canvas sync",
    );
    ui.horizontal(|ui| {
        let start = match app.collab.ui_config.role {
            crate::collab::CollabRole::Server => "Start server",
            crate::collab::CollabRole::Client => "Connect",
        };
        if ui.button(start).clicked() {
            app.collab.start();
        }
        if ui.button("Disconnect").clicked() {
            app.collab.disconnect();
        }
    });
    collab_status_label(ui, app);
    if app.collab.decrypt_warning_count() > 0 {
        ui.colored_label(
            egui::Color32::from_rgb(220, 140, 60),
            format!(
                "Skipped {} encrypted packet(s)",
                app.collab.decrypt_warning_count()
            ),
        );
    }
    #[cfg(not(target_os = "android"))]
    {
        if app.collab.ui_config.role == crate::collab::CollabRole::Server
            && app.collab.is_connected()
        {
            ui.separator();
            server_setup_panel(ui, app);
        }
    }
    ui.separator();
    peers_panel(app, ui);
    #[cfg(not(target_os = "android"))]
    {
        if app.collab.ui_config.role != crate::collab::CollabRole::Server
            || !app.collab.is_connected()
        {
            ui.separator();
            ui.collapsing("AI (MCP)", |ui| {
                ui.collapsing("Vision preview", |ui| {
                    let ctx = ui.ctx().clone();
                    mcp_preview_panel(app, ui, &ctx);
                });
                mcp_setup_hint(ui);
            });
        }
    }
}

#[cfg(not(target_os = "android"))]
fn server_setup_panel(ui: &mut Ui, app: &mut VadadeeBerryApp) {
    ui.label(RichText::new("Share with clients").strong());
    let cfg = &app.collab.ui_config;
    let client_text = format!(
        "Role: Client\nHost: {}\nPort: {}\nRoom: {}\nSecret: {}\nUsername: {}",
        cfg.host.trim(),
        cfg.port,
        cfg.room_id.trim(),
        cfg.secret_key,
        cfg.username.trim(),
    );
    copyable_config_block(ui, "client_connect_cfg", &client_text, "Copy client settings");

    ui.add_space(6.0);
    ui.label(RichText::new("AI (MCP) setup").strong());
    ui.collapsing("Vision preview", |ui| {
        let ctx = ui.ctx().clone();
        mcp_preview_panel(app, ui, &ctx);
    });
    mcp_setup_hint(ui);
    let mcp_json = mcp_cursor_config_json();
    copyable_config_block(ui, "mcp_cursor_cfg", &mcp_json, "Copy MCP config");
}

#[cfg(not(target_os = "android"))]
fn mcp_preview_panel(app: &mut VadadeeBerryApp, ui: &mut Ui, ctx: &Context) {
    if app.mcp_preview.width == 0 || app.mcp_preview.height == 0 || app.mcp_preview.rgba.is_empty() {
        ui.label(RichText::new("No MCP preview yet — call capture_canvas_raster").small().color(colors::TEXT_MUTED));
        return;
    }
    let w = app.mcp_preview.width;
    let h = app.mcp_preview.height;
    if app.mcp_preview.texture.is_none() {
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &app.mcp_preview.rgba);
        app.mcp_preview.texture = Some(ctx.load_texture(
            format!("mcp_preview_{}", app.mcp_preview.updated_at.to_bits()),
            img,
            egui::TextureOptions::LINEAR,
        ));
    }
    if let Some(tex) = &app.mcp_preview.texture {
        let b = &app.mcp_preview.bounds;
        ui.label(
            RichText::new(format!(
                "MCP preview {}×{} @ {}%  crop ({:.0},{:.0},{:.0},{:.0})",
                w, h, app.mcp_preview.resolution_percent as i32, b[0], b[1], b[2], b[3]
            ))
            .small(),
        );
        let max_w = ui.available_width().max(80.0);
        let aspect = h as f32 / w as f32;
        let disp_w = max_w;
        let disp_h = (max_w * aspect).min(220.0);
        ui.image((tex.id(), egui::vec2(disp_w, disp_h)));
    }
}

fn mcp_setup_hint(ui: &mut Ui) {
    ui.label(
        RichText::new(format!(
            "TCP bridge: 127.0.0.1:{} (while app is open)",
            crate::mcp::DEFAULT_MCP_PORT
        ))
        .small()
        .color(colors::TEXT_MUTED),
    );
    ui.label(
        RichText::new("Stdio command: vadadee-mcp-stdio")
            .small()
            .color(colors::TEXT_MUTED),
    );
}

#[cfg(not(target_os = "android"))]
fn mcp_cursor_config_json() -> String {
    serde_json::json!({
        "mcpServers": {
            "vadadee-berry": {
                "command": "vadadee-mcp-stdio",
                "args": [],
                "env": {
                    "VADADEE_MCP_PORT": crate::mcp::DEFAULT_MCP_PORT.to_string(),
                    "VADADEE_MCP_HOST": "127.0.0.1"
                }
            }
        }
    })
    .to_string()
}

fn copyable_config_block(ui: &mut Ui, id: &str, text: &str, copy_label: &str) {
    let avail = ui.available_width().max(120.0);
    ScrollArea::vertical()
        .id_salt(id)
        .max_height(140.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_max_width(avail);
            ui.add(
                egui::Label::new(RichText::new(text).monospace().small())
                    .selectable(true)
                    .wrap(),
            );
        });
    if ui.button(copy_label).clicked() {
        ui.ctx().copy_text(text.to_string());
    }
}

fn peers_panel(app: &mut VadadeeBerryApp, ui: &mut Ui) {
    ui.label(RichText::new("Connected").strong());
    if let Some(ms) = app.collab.connection_latency_ms() {
        ui.label(
            RichText::new(format!("Room latency: {ms} ms"))
                .small()
                .color(colors::TEXT_MUTED),
        );
    }
    let peers: Vec<RemotePeer> = app.collab.peers_sorted();
    let panel_w = ui.available_width().max(120.0);
    if peers.is_empty() {
        ui.label(RichText::new("No remote peers yet").small().color(colors::TEXT_MUTED));
        return;
    }
    ScrollArea::vertical()
        .id_salt("collab_peers_scroll")
        .max_height(160.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_width(panel_w);
            for peer in peers {
                theme::floating_card_frame(0.92).show(ui, |ui| {
                    ui.set_max_width(panel_w);
                    ui.horizontal(|ui| {
                        ui.set_max_width(panel_w);
                        let c = egui::Color32::from_rgb(
                            peer.color_rgb[0],
                            peer.color_rgb[1],
                            peer.color_rgb[2],
                        );
                        ui.colored_label(c, "●");
                        let name = truncate_fit(&peer.username, 18);
                        ui.label(RichText::new(name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let lat = peer_latency_label(&peer, app.collab.connection_latency_ms());
                            ui.label(
                                RichText::new(lat)
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.set_max_width(panel_w);
                        if ui.small_button("Go to cursor").clicked() {
                            if let Some((x, y)) = peer.cursor_doc {
                                app.focus_viewport_on_peer(x, y);
                            }
                        }
                    });
                    ui.add_space(4.0);
                });
                ui.add_space(6.0);
            }
        });
}

fn truncate_fit(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn peer_latency_label(peer: &RemotePeer, room_ms: Option<u32>) -> String {
    if let Some(idle) = peer.idle_ms {
        if idle > 10_000 {
            return "away".into();
        }
        if idle > 5000 {
            return "idle".into();
        }
    }
    room_ms.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "…".into())
}

/// Bottom-left chat toasts above the status bar (slide up + fade).
pub fn show_chat_toasts(app: &mut VadadeeBerryApp, ctx: &Context) {
    app.left_dock.tick_toasts();
    if app.left_dock.chat_toasts.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    let dock = theme::STATUS_BAR_HEIGHT + theme::FLOATING_ABOVE_STATUS_GAP + 8.0;
    let base_y = screen.max.y - dock;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut y_stack = 0.0_f32;
    for toast in app.left_dock.chat_toasts.iter().rev() {
        let age = (now - toast.born).max(0.0) as f32;
        let life = 5.5_f32;
        let t = (age / life).min(1.0);
        let slide = egui::emath::easing::cubic_out(t) * 48.0;
        let alpha = (1.0 - t).max(0.0);
        if alpha <= 0.02 {
            continue;
        }
        let h = 36.0;
        let w = 280.0_f32;
        let pos = egui::pos2(screen.min.x + 12.0, base_y - h - y_stack - slide);
        y_stack += h + 6.0;
        egui::Area::new(egui::Id::new(("chat_toast", toast.born.to_bits())))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                theme::floating_card_frame(alpha).show(ui, |ui| {
                    ui.set_max_width(w);
                    ui.label(
                        RichText::new(format!(
                            "[{}]: {}",
                            truncate_fit(&toast.username, 12),
                            truncate_fit(&toast.text, 48)
                        ))
                        .small(),
                    );
                });
            });
    }
}

fn collab_status_label(ui: &mut Ui, app: &VadadeeBerryApp) {
    match app.collab.status() {
        crate::collab::CollabStatus::Error(e) => {
            ui.colored_label(colors::ACCENT, format!("Error: {e}"));
        }
        crate::collab::CollabStatus::Hosting(url) => {
            ui.label(
                RichText::new(format!("Hosting {url}"))
                    .color(egui::Color32::from_rgb(100, 200, 140)),
            );
        }
        other => {
            let label = match other {
                crate::collab::CollabStatus::Disconnected => "Disconnected",
                crate::collab::CollabStatus::Connecting => "Connecting…",
                crate::collab::CollabStatus::Connected => "Connected (E2EE)",
                crate::collab::CollabStatus::Hosting(_) | crate::collab::CollabStatus::Error(_) => {
                    unreachable!()
                }
            };
            ui.label(RichText::new(label).color(colors::TEXT_MUTED));
        }
    }
}

const CURSOR_BUBBLE_MAX_W: f32 = 200.0;

pub fn draw_local_cursor_bubble(app: &mut VadadeeBerryApp, ui: &mut Ui, origin: egui::Pos2) {
    #[cfg(target_os = "android")]
    {
        let _ = (app, ui, origin);
        return;
    }
    let Some((dx, dy)) = app.cursor_doc else {
        return;
    };
    let pos = app.viewport.doc_to_screen((dx, dy), origin);
    let color = collab_display_color(app.collab.local_color_rgb);
    if !app.cursor_bubble_text.is_empty() {
        draw_cursor_bubble_label(
            ui.painter(),
            pos + egui::vec2(0.0, -30.0),
            &app.cursor_bubble_text,
            color,
        );
    }
    if !app.cursor_bubble_edit {
        return;
    }
    let anchor = pos + egui::vec2(0.0, -56.0);
    let input_id = egui::Id::new("cursor_bubble_input");
    if app.cursor_bubble_focus_pending {
        ui.ctx().memory_mut(|m| m.request_focus(input_id));
    }
    egui::Area::new(egui::Id::new("local_cursor_bubble"))
        .fixed_pos(anchor)
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            ui.set_max_width(CURSOR_BUBBLE_MAX_W);
            theme::floating_card_frame(0.95).show(ui, |ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut app.cursor_bubble_text)
                        .id(input_id)
                        .hint_text("Type at cursor…")
                        .desired_width(CURSOR_BUBBLE_MAX_W - 16.0),
                );
                if resp.has_focus() || resp.clicked() {
                    app.cursor_bubble_focus_pending = false;
                } else if app.cursor_bubble_focus_pending {
                    resp.request_focus();
                }
            });
        });
    if app.cursor_bubble_edit {
        ui.ctx().request_repaint();
    }
}

pub fn draw_remote_cursors(app: &VadadeeBerryApp, ui: &mut Ui, origin: egui::Pos2) {
    #[cfg(target_os = "android")]
    {
        let _ = (app, ui, origin);
        return;
    }
    if !app.collab.is_connected() {
        return;
    }
    let painter = ui.painter();
    for peer in app.collab.peers_sorted() {
        let Some((dx, dy)) = peer.cursor_doc else {
            continue;
        };
        let pos = app.viewport.doc_to_screen((dx, dy), origin);
        let color = collab_display_color(peer.color_rgb);
        let drawing = peer
            .tool_label
            .as_deref()
            .and_then(tool_kind_from_label)
            .is_some_and(|t| t.is_drawing_tool());
        if drawing {
            let icon = peer
                .tool_label
                .as_deref()
                .map(collab_tool_icon)
                .unwrap_or(icons::PEN);
            draw_outlined_icon(
                &painter,
                pos + egui::vec2(8.0, 8.0),
                icon,
                color,
            );
        } else {
            draw_pointer_cursor(&painter, pos, color);
        }
        draw_outlined_text(
            &painter,
            pos + egui::vec2(12.0, 18.0),
            &truncate_fit(&peer.username, 14),
            color,
        );
        if let Some(ref bubble) = peer.cursor_bubble {
            if !bubble.is_empty() {
                draw_cursor_bubble_label(&painter, pos + egui::vec2(0.0, -30.0), bubble, color);
            }
        }
    }
}

fn collab_display_color(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn draw_cursor_bubble_label(
    painter: &egui::Painter,
    top_left: egui::Pos2,
    text: &str,
    accent: egui::Color32,
) {
    let font_id = egui::FontId::proportional(12.0);
    let text_color = egui::Color32::from_rgb(18, 22, 32);
    let galley = painter.layout(
        text.to_owned(),
        font_id,
        text_color,
        CURSOR_BUBBLE_MAX_W,
    );
    let pad = egui::vec2(6.0, 4.0);
    let rect = egui::Rect::from_min_size(top_left, galley.size() + pad * 2.0);
    painter.rect_filled(rect, 6.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 235));
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(2.0, accent),
        egui::StrokeKind::Outside,
    );
    painter.galley(top_left + pad, galley, text_color);
}

fn draw_pointer_cursor(painter: &egui::Painter, tip: egui::Pos2, color: egui::Color32) {
    let a = tip;
    let b = tip + egui::vec2(0.0, 14.0);
    let c = tip + egui::vec2(9.0, 10.0);
    let outline = egui::Stroke::new(2.5, egui::Color32::WHITE);
    let stroke = egui::Stroke::new(1.25, egui::Color32::from_black_alpha(200));
    painter.add(egui::Shape::convex_polygon(
        vec![a, b, c],
        color,
        outline,
    ));
    painter.add(egui::Shape::convex_polygon(vec![a, b, c], color, stroke));
}

fn draw_outlined_text(painter: &egui::Painter, pos: egui::Pos2, text: &str, color: egui::Color32) {
    let font = egui::FontId::proportional(11.0);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        painter.text(
            pos + egui::vec2(dx, dy),
            egui::Align2::LEFT_TOP,
            text,
            font.clone(),
            egui::Color32::WHITE,
        );
    }
    painter.text(pos, egui::Align2::LEFT_TOP, text, font, color);
}

fn draw_outlined_icon(painter: &egui::Painter, pos: egui::Pos2, icon: &str, color: egui::Color32) {
    let font = FontId::new(16.0, egui::FontFamily::Name(icons::FONT_NAME.into()));
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        painter.text(
            pos + egui::vec2(dx, dy),
            egui::Align2::LEFT_TOP,
            icon,
            font.clone(),
            egui::Color32::WHITE,
        );
    }
    painter.text(pos, egui::Align2::LEFT_TOP, icon, font, color);
}

fn tool_kind_from_label(label: &str) -> Option<ToolKind> {
    match label {
        "Select" => Some(ToolKind::Select),
        "Edit" => Some(ToolKind::Node),
        "Rectangle" => Some(ToolKind::Rectangle),
        "Circle" => Some(ToolKind::Circle),
        "Ellipse" => Some(ToolKind::Ellipse),
        "Line" => Some(ToolKind::Line),
        "Polygon" => Some(ToolKind::Polygon),
        "Pen" => Some(ToolKind::Pen),
        "Text" => Some(ToolKind::Text),
        "Arc" => Some(ToolKind::Arc),
        "Plotter" => Some(ToolKind::Plotter),
        "Brush" => Some(ToolKind::Brush),
        "Paint" => Some(ToolKind::RasterBrush),
        "Eraser" => Some(ToolKind::Eraser),
        "Fill" => Some(ToolKind::BucketFill),
        "Eyedropper" => Some(ToolKind::Eyedropper),
        _ => None,
    }
}

trait DrawingTool {
    fn is_drawing_tool(self) -> bool;
}

impl DrawingTool for ToolKind {
    fn is_drawing_tool(self) -> bool {
        matches!(
            self,
            ToolKind::Pen
                | ToolKind::Brush
                | ToolKind::RasterBrush
                | ToolKind::Eraser
                | ToolKind::BucketFill
                | ToolKind::Rectangle
                | ToolKind::Circle
                | ToolKind::Ellipse
                | ToolKind::Line
                | ToolKind::Polygon
                | ToolKind::Arc
                | ToolKind::Text
                | ToolKind::Node
        )
    }
}

fn collab_tool_icon(label: &str) -> &'static str {
    match tool_kind_from_label(label) {
        Some(ToolKind::Select) => icons::SELECT,
        Some(ToolKind::Node) => icons::NODE,
        Some(ToolKind::Pen) => icons::PEN,
        Some(ToolKind::Rectangle) => icons::RECT,
        Some(ToolKind::Circle) => icons::CIRCLE,
        Some(ToolKind::Ellipse) => icons::ELLIPSE,
        Some(ToolKind::Line) => icons::LINE,
        Some(ToolKind::Polygon) => icons::POLY,
        Some(ToolKind::Arc) => icons::ARC,
        Some(ToolKind::Plotter) => icons::PLOTTER,
        Some(ToolKind::Text) => icons::TEXT,
        Some(ToolKind::Brush) => icons::BRUSH,
        Some(ToolKind::RasterBrush) => icons::RASTER_BRUSH,
        Some(ToolKind::Eraser) => icons::ERASER,
        Some(ToolKind::BucketFill) => icons::BUCKET,
        Some(ToolKind::Eyedropper) => icons::EYE_DROPPER,
        None => icons::SELECT,
    }
}