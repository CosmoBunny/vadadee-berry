//! Modern dark UI theme (Figma / Inkscape-adjacent).
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Painter, Pos2,
    Rect, Stroke, TextStyle, Visuals,
};

use crate::icons;

pub mod colors {
    use egui::Color32;
    pub const BG_DEEP: Color32 = Color32::from_rgb(15, 17, 23);
    pub const BG_PANEL: Color32 = Color32::from_rgb(22, 26, 35);
    pub const BG_ELEVATED: Color32 = Color32::from_rgb(32, 38, 52);
    pub const BG_HOVER: Color32 = Color32::from_rgb(42, 50, 68);
    pub const BORDER: Color32 = Color32::from_rgb(48, 56, 76);
    pub const TEXT: Color32 = Color32::from_rgb(232, 236, 245);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 150, 175);
    pub const ACCENT: Color32 = Color32::from_rgb(99, 130, 255);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(60, 78, 160);
    pub const SUCCESS: Color32 = Color32::from_rgb(72, 199, 142);
    pub const ALERT: Color32 = Color32::from_rgb(255, 95, 110);
    pub const CANVAS_BG: Color32 = Color32::from_rgb(12, 14, 18);
    pub const SELECTION: Color32 = Color32::from_rgb(99, 130, 255);
    pub const POWERLINE_A: Color32 = Color32::from_rgb(99, 130, 255);
    pub const POWERLINE_B: Color32 = Color32::from_rgb(72, 199, 142);
    pub const POWERLINE_C: Color32 = Color32::from_rgb(255, 180, 80);
}

pub const CHROME_GAP: i8 = 8;
/// Must match `status` panel `exact_size` in `ui.rs`.
pub const STATUS_BAR_HEIGHT: f32 = 30.0;
/// Extra clearance so `Order::Foreground` floaters do not paint over the status bar.
pub const FLOATING_ABOVE_STATUS_GAP: f32 = 10.0;
pub const FLOAT_RADIUS: u8 = 12;
pub const CHROME_RADIUS: u8 = 10;
pub const TOOLBAR_WIDTH: f32 = 72.0;
pub const STATUS_PAD: f32 = 12.0;
pub const STATUS_CHIP_RADIUS: u8 = 6;
pub const STATUS_SEP: &str = ">";

pub fn apply(ctx: &egui::Context) {
    load_nerd_font(ctx);

    ctx.options_mut(|o| {
        o.input_options.zoom_modifier =
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    });

    ctx.tessellation_options_mut(|t| {
        t.feathering = true;
        t.feathering_size_in_pixels = 1.25;
    });

    use colors::{
        ACCENT, ACCENT_DIM, BG_DEEP, BG_ELEVATED, BG_HOVER, BG_PANEL, BORDER, TEXT, TEXT_MUTED,
    };
    let mut visuals = Visuals::dark();

    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_ELEVATED;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = BG_ELEVATED;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.bg_fill = BG_ELEVATED;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    visuals.widgets.inactive.bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, TEXT);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.open.bg_fill = ACCENT_DIM;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = Color32::from_rgb(255, 180, 80);
    visuals.error_fg_color = Color32::from_rgb(255, 95, 110);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.indent = 16.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(15.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    ctx.set_global_style(style);
}

fn load_nerd_font(ctx: &egui::Context) {
    let bytes = include_bytes!("../assets/DaddyTimeMonoNerdFont-Regular.ttf").to_vec();
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        icons::FONT_NAME.to_owned(),
        FontData::from_owned(bytes).into(),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, icons::FONT_NAME.to_owned());
    fonts
        .families
        .entry(FontFamily::Name(icons::FONT_NAME.into()))
        .or_default()
        .push(icons::FONT_NAME.to_owned());
    ctx.set_fonts(fonts);
}

pub fn accent_button(
    ui: &mut egui::Ui,
    selected: bool,
    text: &str,
    tip: &str,
    highlight: f32,
) -> egui::Response {
    let active_fill = colors::ACCENT.gamma_multiply(0.55);
    let base_t = if selected { 1.0 } else { 0.0 };
    let t = (base_t + highlight * 0.35).clamp(0.0, 1.0);
    let fill = crate::animation::lerp_color(colors::BG_ELEVATED, active_fill, t);
    let stroke_color = crate::animation::lerp_color(colors::BORDER, colors::ACCENT, t);
    let stroke_w = 1.0 + t * 0.5;
    let size = 40.0 + highlight * 2.0;
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .font(icons::nerd_font_id(18.0))
            .color(colors::TEXT),
    )
    .fill(fill)
    .stroke(Stroke::new(stroke_w, stroke_color))
    .min_size(egui::vec2(size, size));
    ui.add(btn).on_hover_text(tip)
}

pub fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(colors::TEXT_MUTED)
            .size(11.0),
    );
    ui.add_space(2.0);
}

pub fn panel_frame() -> egui::Frame {
    floating_card_frame(1.0)
}

pub fn chrome_gap() -> f32 {
    CHROME_GAP as f32
}

/// Transparent layout slot — rounded card is drawn inside with inset so corners are not clipped.
pub fn layout_slot_frame() -> egui::Frame {
    egui::Frame::NONE
}

/// Floating sticky card: fully rounded on all four corners.
pub fn floating_card_frame(alpha: f32) -> egui::Frame {
    let alpha = alpha.clamp(0.0, 1.0);
    egui::Frame::new()
        .fill(colors::BG_PANEL.gamma_multiply(alpha))
        .stroke(Stroke::new(1.0, colors::BORDER.gamma_multiply(alpha)))
        .corner_radius(CornerRadius::same(FLOAT_RADIUS))
        .inner_margin(Margin::symmetric(8, 10))
}

/// True overlay: floats above the canvas without reserving side columns.
pub fn show_overlay_area(
    ctx: &egui::Context,
    id: &'static str,
    rect: Rect,
    alpha: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<Rect> {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.004 {
        return None;
    }
    Some(show_overlay_area_inner(
        ctx,
        id,
        rect,
        alpha,
        true,
        true,
        None,
        egui::Order::Foreground,
        add_contents,
    ))
}

/// Action bar: frame + contents share one opacity; no interaction when faded out.
pub fn show_action_bar_area(
    ctx: &egui::Context,
    id: &'static str,
    rect: Rect,
    alpha: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<Rect> {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.004 {
        return None;
    }
    // Must slide fully off-screen; default Area constrain keeps the right edge glued to content_rect.
    Some(show_overlay_area_inner(
        ctx,
        id,
        rect,
        alpha,
        alpha > 0.35,
        false,
        None,
        egui::Order::Foreground,
        add_contents,
    ))
}

/// Bottom-anchored panel (video timeline): clipped above status bar, below tooltips.
pub fn show_floating_panel_area(
    ctx: &egui::Context,
    id: &'static str,
    rect: Rect,
    alpha: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<Rect> {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.004 {
        return None;
    }
    Some(show_overlay_area_inner(
        ctx,
        id,
        rect,
        alpha,
        alpha > 0.35,
        false,
        Some(above_status_clip_rect(ctx)),
        egui::Order::Middle,
        add_contents,
    ))
}

/// Bottom panel that **slides** (full opacity); no fade. Wider clip while `slide_active`.
pub fn show_bottom_slide_panel(
    ctx: &egui::Context,
    id: &'static str,
    rect: Rect,
    open_t: f32,
    slide_active: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<Rect> {
    if open_t <= 0.001 {
        return None;
    }
    let clip = if slide_active || open_t < 0.999 {
        Some(ctx.viewport_rect())
    } else {
        Some(above_status_clip_rect(ctx))
    };
    Some(show_overlay_area_inner(
        ctx,
        id,
        rect,
        1.0,
        true,
        false,
        clip,
        egui::Order::Middle,
        add_contents,
    ))
}

/// Canvas work rect for bottom floaters — keeps them out of the status bar band.
pub fn floater_work_rect(canvas_work: Rect) -> Rect {
    let gap = chrome_gap();
    let bottom_trim = STATUS_BAR_HEIGHT + FLOATING_ABOVE_STATUS_GAP + gap;
    Rect::from_min_max(
        canvas_work.min + egui::vec2(gap, gap),
        egui::pos2(
            canvas_work.max.x - gap,
            (canvas_work.max.y - bottom_trim).max(canvas_work.min.y + gap),
        ),
    )
}

pub fn above_status_clip_rect(ctx: &egui::Context) -> Rect {
    let vp = ctx.viewport_rect();
    let bottom = vp.max.y - STATUS_BAR_HEIGHT - FLOATING_ABOVE_STATUS_GAP;
    Rect::from_min_max(vp.min, egui::pos2(vp.max.x, bottom.max(vp.min.y)))
}

/// Slide up from below the viewport; docked with bottom at `work.max.y` when `open_t=1`.
pub fn bottom_floater_slide_rect(
    ctx: &egui::Context,
    work: Rect,
    left: f32,
    width: f32,
    height: f32,
    open_t: f32,
) -> Rect {
    let t = open_t.clamp(0.0, 1.0);
    let gap = chrome_gap() as f32;
    let vp = ctx.viewport_rect();
    let open_bottom = work.max.y;
    let closed_bottom = vp.max.y + height + gap;
    let bottom = closed_bottom + (open_bottom - closed_bottom) * t;
    let top = bottom - height;
    Rect::from_min_size(Pos2::new(left, top), egui::vec2(width, height))
}

fn show_overlay_area_inner(
    ctx: &egui::Context,
    id: &'static str,
    rect: Rect,
    alpha: f32,
    interactable: bool,
    constrain_to_content: bool,
    clip_rect: Option<Rect>,
    layer: egui::Order,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Rect {
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return rect;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let mut area = egui::Area::new(egui::Id::new(id))
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .interactable(interactable)
        .movable(false)
        .order(layer);
    if !constrain_to_content {
        area = area.constrain(false);
    }
    if let Some(clip) = clip_rect {
        area = area.constrain_to(clip);
    }
    let response = area.show(ctx, |ui| {
            ui.set_width(rect.width());
            ui.set_height(rect.height());
            floating_card_frame(alpha).show(ui, |ui| {
                ui.set_opacity(alpha);
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                add_contents(ui);
            });
        });
    response.response.rect
}

pub fn overlay_work_rect(work: Rect) -> Rect {
    let gap = chrome_gap();
    Rect::from_min_max(
        work.min + egui::vec2(gap, gap),
        work.max - egui::vec2(gap, gap),
    )
}

/// Full-bleed bars (menubar, status) — square corners, no outer gap.
pub fn bar_frame(alpha: f32) -> egui::Frame {
    let alpha = alpha.clamp(0.0, 1.0);
    egui::Frame::new()
        .fill(colors::BG_PANEL.gamma_multiply(alpha))
        .stroke(Stroke::new(1.0, colors::BORDER.gamma_multiply(alpha)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(12, 4))
        .outer_margin(Margin::ZERO)
}

pub fn canvas_frame(alpha: f32) -> egui::Frame {
    let alpha = alpha.clamp(0.0, 1.0);
    egui::Frame::new()
        .fill(colors::CANVAS_BG.gamma_multiply(alpha))
        .stroke(Stroke::new(1.0, colors::BORDER.gamma_multiply(alpha * 0.65)))
        .corner_radius(CornerRadius::same(FLOAT_RADIUS))
        .inner_margin(Margin::ZERO)
}

/// Black scroll track behind action tabs (tab chips sit on top).
pub fn action_tab_track_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::BLACK)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin {
            left: 6,
            right: 6,
            top: 5,
            bottom: 10,
        })
        .outer_margin(Margin::ZERO)
}

/// One action-bar tab chip (distinct from the black track).
pub fn action_tab_chip(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    label_alpha: f32,
) -> egui::Response {
    let text_color = if selected {
        colors::TEXT.gamma_multiply(label_alpha)
    } else {
        colors::TEXT_MUTED.gamma_multiply(label_alpha.max(0.85))
    };
    let fill = if selected {
        colors::BG_ELEVATED
    } else {
        colors::BG_PANEL
    };
    let stroke = if selected {
        Stroke::new(1.0, colors::ACCENT.gamma_multiply(label_alpha))
    } else {
        Stroke::new(1.0, colors::BORDER)
    };
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .font(icons::nerd_font_id(12.0))
                .color(text_color),
        )
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6))
        .min_size(egui::vec2(0.0, 24.0)),
    )
}

/// Dark inset for action bar tab content (tabs stay on panel chrome).
pub fn action_content_frame() -> egui::Frame {
    action_content_frame_alpha(1.0)
}

pub fn action_content_frame_alpha(alpha: f32) -> egui::Frame {
    let alpha = alpha.clamp(0.0, 1.0);
    egui::Frame::new()
        .fill(colors::BG_DEEP.gamma_multiply(alpha))
        .stroke(Stroke::new(1.0, colors::BORDER.gamma_multiply(alpha)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 10))
        .outer_margin(egui::Margin::ZERO)
}

/// Grouped constraint block inside the geometry / appearance panels.
pub fn constraint_block(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(colors::BG_ELEVATED)
        .stroke(Stroke::new(1.0, colors::BORDER))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
    ui.add_space(6.0);
}

pub fn text_on_background(bg: Color32) -> Color32 {
    let r = bg.r() as f32;
    let g = bg.g() as f32;
    let b = bg.b() as f32;
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    if lum > 150.0 {
        Color32::from_rgb(16, 20, 28)
    } else {
        Color32::from_rgb(244, 247, 255)
    }
}

fn paint_status_chip(painter: &Painter, rect: Rect, fill: Color32, alpha: f32) {
    let fill = fill.gamma_multiply(alpha);
    painter.rect(
        rect,
        CornerRadius::same(STATUS_CHIP_RADIUS),
        fill,
        Stroke::NONE,
        egui::StrokeKind::Inside,
    );
}

fn paint_status_separator(painter: &Painter, center: Pos2, alpha: f32) {
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        STATUS_SEP,
        FontId::new(12.0, FontFamily::Proportional),
        colors::TEXT_MUTED.gamma_multiply(alpha),
    );
}

pub fn measure_status_label(ui: &egui::Ui, text: &str) -> f32 {
    let font = FontId::new(11.0, FontFamily::Proportional);
    let text_w = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, colors::TEXT)
        .size()
        .x;
    (text_w + STATUS_PAD * 2.0).max(48.0)
}

fn paint_sliding_label(
    painter: &Painter,
    rect: Rect,
    outgoing: &str,
    incoming: &str,
    out_offset: f32,
    in_offset: f32,
    color: Color32,
) {
    let clip = rect.shrink2(egui::vec2(4.0, 1.0));
    let clipped = painter.with_clip_rect(clip);
    let font = FontId::new(11.0, FontFamily::Proportional);
    let out_pos = Pos2::new(clip.left() + out_offset, clip.center().y);
    let in_pos = Pos2::new(clip.left() + in_offset, clip.center().y);
    clipped.text(
        out_pos,
        egui::Align2::LEFT_CENTER,
        outgoing,
        font.clone(),
        color,
    );
    clipped.text(in_pos, egui::Align2::LEFT_CENTER, incoming, font, color);
}

/// Status order: mode › action › cursor › zoom (powerline chips).
pub fn paint_powerline_status(
    ui: &mut egui::Ui,
    tool_out: &str,
    tool_in: &str,
    tool_width: f32,
    msg_out: &str,
    msg_in: &str,
    msg_width: f32,
    coords_out: &str,
    coords_in: &str,
    coords_width: f32,
    zoom: f32,
    tool_slide_out: f32,
    tool_slide_in: f32,
    msg_slide_out: f32,
    msg_slide_in: f32,
    coords_slide_out: f32,
    coords_slide_in: f32,
    alpha: f32,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.01 {
        return;
    }
    let row = ui.available_rect_before_wrap();
    let h = row.height().max(18.0);
    let y = row.top();
    let painter = ui.painter();
    let font = FontId::new(11.0, FontFamily::Proportional);
    const SEP_W: f32 = 14.0;

    let w_tool = tool_width.max(48.0);
    let w_msg = msg_width.max(48.0);
    let zoom_text = format!("Zoom {:.0}%", zoom * 100.0);
    let w_zoom = measure_status_label(ui, &zoom_text);
    // coords_width is precomputed and animated from caller
    let w_coords = coords_width.max(48.0);

    let tool_bg = colors::POWERLINE_A;
    let msg_bg = colors::BG_ELEVATED;
    let coords_bg = colors::ACCENT_DIM;
    let zoom_bg = colors::POWERLINE_B;

    let mut x = row.left() + 6.0;

    let seg_tool = Rect::from_min_size(Pos2::new(x, y), egui::vec2(w_tool, h));
    paint_status_chip(painter, seg_tool, tool_bg, alpha);
    paint_sliding_label(
        painter,
        seg_tool,
        tool_out,
        tool_in,
        tool_slide_out,
        tool_slide_in,
        text_on_background(tool_bg).gamma_multiply(alpha),
    );
    x = seg_tool.right() + SEP_W;
    paint_status_separator(
        painter,
        Pos2::new(seg_tool.right() + SEP_W * 0.5, seg_tool.center().y),
        alpha,
    );

    let seg_msg = Rect::from_min_size(Pos2::new(x, y), egui::vec2(w_msg, h));
    paint_status_chip(painter, seg_msg, msg_bg, alpha);
    paint_sliding_label(
        painter,
        seg_msg,
        msg_out,
        msg_in,
        msg_slide_out,
        msg_slide_in,
        text_on_background(msg_bg).gamma_multiply(alpha),
    );
    x = seg_msg.right() + SEP_W;
    paint_status_separator(
        painter,
        Pos2::new(seg_msg.right() + SEP_W * 0.5, seg_msg.center().y),
        alpha,
    );

    let seg_coords = Rect::from_min_size(Pos2::new(x, y), egui::vec2(w_coords, h));
    if w_coords > 4.0 {
        paint_status_chip(painter, seg_coords, coords_bg, alpha);
        paint_sliding_label(
            painter,
            seg_coords,
            coords_out,
            coords_in,
            coords_slide_out,
            coords_slide_in,
            text_on_background(coords_bg).gamma_multiply(alpha),
        );
        x = seg_coords.right() + SEP_W;
        paint_status_separator(
            painter,
            Pos2::new(seg_coords.right() + SEP_W * 0.5, seg_coords.center().y),
            alpha,
        );
    }

    let seg_zoom = Rect::from_min_size(Pos2::new(x, y), egui::vec2(w_zoom, h));
    paint_status_chip(painter, seg_zoom, zoom_bg, alpha);
    painter.text(
        Pos2::new(seg_zoom.left() + STATUS_PAD, seg_zoom.center().y),
        egui::Align2::LEFT_CENTER,
        zoom_text,
        font,
        text_on_background(zoom_bg).gamma_multiply(alpha),
    );

    ui.allocate_rect(row, egui::Sense::hover());
}