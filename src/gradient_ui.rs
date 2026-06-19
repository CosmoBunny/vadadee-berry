//! Multi-stop gradient strip, angle dial, and gradient flow line editor.
use egui::{Color32, Id, Key, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

use crate::document::{
    linear_angle_from_line, linear_line_spanning_bbox, set_linear_line_angle, translate_linear_line,
    FillKind, GradientStop, Paint, sample_stops,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientLineHandle {
    LinearEnd0,
    LinearEnd1,
    LinearMid,
    RadialFocal,
}
use crate::theme::colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientEditorFocus {
    #[default]
    None,
    Fill,
    Stroke,
}

pub struct GradientStripResult {
    pub changed: bool,
    pub focus: GradientEditorFocus,
}

pub fn gradient_strip_editor(
    ui: &mut Ui,
    strip_id: Id,
    which: GradientEditorFocus,
    stops: &mut Vec<GradientStop>,
    selected: &mut usize,
) -> GradientStripResult {
    let mut changed = false;
    let mut focus = GradientEditorFocus::None;

    if stops.len() < 2 {
        stops.extend_from_slice(&crate::document::default_gradient_stops());
        changed = true;
    }

    let h = 32.0;
    let mut drag_i: Option<usize> = None;
    ui.push_id(strip_id, |ui| {
        let (rect, _bar_response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        paint_smooth_gradient_bar(ui, rect, stops);
        let painter = ui.painter_at(rect);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, colors::BORDER), egui::StrokeKind::Inside);

        for (i, stop) in stops.iter_mut().enumerate() {
            let x = rect.left() + rect.width() * stop.pos;
            let handle = Rect::from_center_size(
                Pos2::new(x, rect.center().y),
                Vec2::new(10.0, h - 4.0),
            );
            let r = ui.interact(handle, ui.id().with(i), Sense::click_and_drag());
            if r.clicked() {
                *selected = i;
                focus = which;
            }
            if r.dragged() {
                if let Some(pos) = r.interact_pointer_pos() {
                    stop.pos = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    drag_i = Some(i);
                    changed = true;
                }
            }
            let sel = *selected == i;
            painter.rect_filled(
                handle,
                2.0,
                if sel {
                    colors::ACCENT
                } else {
                    Color32::WHITE
                },
            );
            painter.rect_stroke(
                handle,
                2.0,
                Stroke::new(1.0, colors::BORDER),
                egui::StrokeKind::Outside,
            );
        }

        if let Some(i) = drag_i {
            *selected = i;
            stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap());
            focus = which;
        }
    });

    if *selected >= stops.len() {
        *selected = stops.len().saturating_sub(1);
    }

    ui.push_id(strip_id.with("toolbar"), |ui| {
        ui.horizontal(|ui| {
            if ui.small_button("+ stop").clicked() {
                let t = 0.5;
                let color = sample_stops(stops, t);
                stops.push(GradientStop::new(t, color));
                crate::document::normalize_stops(stops);
                changed = true;
                focus = which;
            }
            if *selected < stops.len() {
                ui.label(format!(
                    "Stop {} @ {:.0}%",
                    selected.saturating_add(1),
                    stops[*selected].pos * 100.0
                ));
            }
        });
    });

    if *selected < stops.len() {
        ui.push_id(strip_id.with("color"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Color");
                let mut c = stops[*selected].color.to_egui();
                if ui.color_edit_button_srgba(&mut c).changed() {
                    stops[*selected].color = Paint {
                        rgba: [
                            c.r() as f32 / 255.0,
                            c.g() as f32 / 255.0,
                            c.b() as f32 / 255.0,
                            c.a() as f32 / 255.0,
                        ],
                    };
                    changed = true;
                    focus = which;
                }
            });
        });
    }

    if focus == which
        && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
        && stops.len() > 2
        && *selected < stops.len()
    {
        stops.remove(*selected);
        crate::document::normalize_stops(stops);
        *selected = (*selected).min(stops.len().saturating_sub(1));
        changed = true;
    }

    GradientStripResult { changed, focus }
}

/// Circular dial to set linear gradient angle (rotates the flow line around its midpoint).
pub fn linear_gradient_angle_dial(ui: &mut Ui, dial_id: Id, angle_deg: &mut f32) -> bool {
    let mut changed = false;
    let size = 80.0;
    ui.push_id(dial_id, |ui| {
        ui.label("Angle");
        let (rect, response) =
            ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
        let center = rect.center();
        let radius = size * 0.38;
        let painter = ui.painter_at(rect);
        painter.circle_stroke(center, radius, Stroke::new(1.5, colors::BORDER));
        painter.circle_filled(center, 3.0, colors::BORDER);
        let rad = angle_deg.to_radians();
        let knob = center + Vec2::new(rad.cos(), rad.sin()) * radius;
        painter.line_segment([center, knob], Stroke::new(2.0, colors::ACCENT));
        painter.circle_filled(knob, 6.0, colors::ACCENT);
        painter.circle_stroke(knob, 6.0, Stroke::new(1.0, Color32::WHITE));
        if response.dragged() || response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let d = pos - center;
                if d.length_sq() > 2.0 {
                    *angle_deg = d.y.atan2(d.x).to_degrees() as f32;
                    changed = true;
                }
            }
        }
        ui.label(format!("{:.0}°", *angle_deg));
    });
    changed
}

fn line_to_screen(rect: Rect, x0: f32, y0: f32, x1: f32, y1: f32) -> (Pos2, Pos2, Pos2) {
    let a = Pos2::new(rect.left() + rect.width() * x0, rect.top() + rect.height() * y0);
    let b = Pos2::new(rect.left() + rect.width() * x1, rect.top() + rect.height() * y1);
    let mid = Pos2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    (a, b, mid)
}

/// Edit the virtual line the gradient flows along (endpoints + draggable midpoint).
pub fn gradient_flow_line_editor(
    ui: &mut Ui,
    editor_id: Id,
    kind: FillKind,
    line_x0: &mut f32,
    line_y0: &mut f32,
    line_x1: &mut f32,
    line_y1: &mut f32,
    radial_cx: &mut f32,
    radial_cy: &mut f32,
    aspect: f32,
) -> bool {
    if !matches!(kind, FillKind::LinearGradient | FillKind::RadialGradient) {
        return false;
    }
    let mut changed = false;
    let w = ui.available_width().min(200.0);
    let h = (w / aspect.max(0.25)).clamp(48.0, 120.0);
    ui.push_id(editor_id, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(35));
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, colors::BORDER), egui::StrokeKind::Inside);

        match kind {
            FillKind::LinearGradient => {
                let (a, b, mid) = line_to_screen(rect, *line_x0, *line_y0, *line_x1, *line_y1);
                painter.line_segment([a, b], Stroke::new(2.5, colors::ACCENT));
                painter.circle_filled(a, 5.0, Color32::WHITE);
                painter.circle_filled(b, 5.0, Color32::WHITE);
                painter.circle_stroke(a, 5.0, Stroke::new(1.5, colors::ACCENT));
                painter.circle_stroke(b, 5.0, Stroke::new(1.5, colors::ACCENT));
                painter.circle_filled(mid, 5.0, colors::ACCENT);
                painter.circle_stroke(mid, 5.0, Stroke::new(1.5, Color32::WHITE));

                let drag_end0 = ui.interact(
                    Rect::from_center_size(a, Vec2::splat(14.0)),
                    editor_id.with("e0"),
                    Sense::click_and_drag(),
                );
                let drag_end1 = ui.interact(
                    Rect::from_center_size(b, Vec2::splat(14.0)),
                    editor_id.with("e1"),
                    Sense::click_and_drag(),
                );
                let drag_mid = ui.interact(
                    Rect::from_center_size(mid, Vec2::splat(14.0)),
                    editor_id.with("mid"),
                    Sense::click_and_drag(),
                );

                let mut line = (*line_x0, *line_y0, *line_x1, *line_y1);
                let mid_store = editor_id.with("mid_drag");
                if drag_mid.drag_started() {
                    let start_norm = drag_mid
                        .interact_pointer_pos()
                        .map(|p| norm_in_rect(rect, p))
                        .unwrap_or((0.5, 0.5));
                    ui.data_mut(|d| {
                        d.insert_temp(mid_store, (line, start_norm));
                    });
                }
                if drag_end0.dragged() {
                    if let Some(p) = drag_end0.interact_pointer_pos() {
                        let (nx, ny) = norm_in_rect(rect, p);
                        line.0 = nx;
                        line.1 = ny;
                        changed = true;
                    }
                }
                if drag_end1.dragged() {
                    if let Some(p) = drag_end1.interact_pointer_pos() {
                        let (nx, ny) = norm_in_rect(rect, p);
                        line.2 = nx;
                        line.3 = ny;
                        changed = true;
                    }
                }
                if drag_mid.dragged() {
                    if let Some((start_line, start_norm)) = ui
                        .data(|d| d.get_temp::<((f32, f32, f32, f32), (f32, f32))>(mid_store))
                    {
                        if let Some(p) = drag_mid.interact_pointer_pos() {
                            let (nx, ny) = norm_in_rect(rect, p);
                            let dx = nx - start_norm.0;
                            let dy = ny - start_norm.1;
                            line = start_line;
                            translate_linear_line(&mut line, dx, dy);
                            changed = true;
                        }
                    }
                }
                if changed {
                    *line_x0 = line.0;
                    *line_y0 = line.1;
                    *line_x1 = line.2;
                    *line_y1 = line.3;
                }
            }
            FillKind::RadialGradient => {
                let px = rect.left() + rect.width() * *radial_cx;
                let py = rect.top() + rect.height() * *radial_cy;
                let focal = Pos2::new(px, py);
                painter.circle_filled(focal, 7.0, colors::ACCENT);
                painter.circle_stroke(focal, 7.0, Stroke::new(1.5, Color32::WHITE));
                let drag = ui.interact(
                    Rect::from_center_size(focal, Vec2::splat(18.0)),
                    editor_id.with("radial"),
                    Sense::click_and_drag(),
                );
                if drag.dragged() {
                    if let Some(pos) = drag.interact_pointer_pos() {
                        *radial_cx = (pos.x - rect.left()) / rect.width();
                        *radial_cy = (pos.y - rect.top()) / rect.height();
                        changed = true;
                    }
                }
            }
            FillKind::Solid => {}
        }
    });
    changed
}

fn norm_in_rect(rect: Rect, pos: Pos2) -> (f32, f32) {
    (
        (pos.x - rect.left()) / rect.width(),
        (pos.y - rect.top()) / rect.height(),
    )
}

/// After the angle dial changes, set flow line to span the bbox along that angle.
pub fn apply_angle_to_flow_line(angle_deg: f32, line: &mut (f32, f32, f32, f32)) {
    *line = linear_line_spanning_bbox(angle_deg);
}

/// Keep angle field in sync when endpoints were edited.
pub fn sync_angle_from_flow_line(line: (f32, f32, f32, f32)) -> f32 {
    linear_angle_from_line(line.0, line.1, line.2, line.3)
}

fn paint_smooth_gradient_bar(ui: &mut Ui, rect: Rect, stops: &[GradientStop]) {
    let cols = (rect.width().round() as usize).clamp(64, 512);
    let mut mesh = Mesh::default();
    for i in 0..=cols {
        let t = i as f32 / cols as f32;
        let c = sample_stops(stops, t).to_egui();
        let x = rect.left() + rect.width() * t;
        mesh.colored_vertex(Pos2::new(x, rect.top()), c);
        mesh.colored_vertex(Pos2::new(x, rect.bottom()), c);
    }
    for i in 0..cols {
        let v = (i as u32) * 2;
        mesh.add_triangle(v, v + 1, v + 2);
        mesh.add_triangle(v + 1, v + 3, v + 2);
    }
    ui.painter_at(rect).add(Shape::mesh(mesh));
}

/// Color picker for solid fill/stroke (updates all stops to the same paint).
pub fn solid_color_editor(ui: &mut Ui, stops: &mut Vec<GradientStop>) -> bool {
    if stops.len() < 2 {
        stops.extend_from_slice(&crate::document::default_gradient_stops());
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Color");
        let mut c = stops[0].color.to_egui();
        if ui.color_edit_button_srgba(&mut c).changed() {
            let paint = Paint {
                rgba: [
                    c.r() as f32 / 255.0,
                    c.g() as f32 / 255.0,
                    c.b() as f32 / 255.0,
                    c.a() as f32 / 255.0,
                ],
            };
            for s in stops.iter_mut() {
                s.color = paint;
            }
            changed = true;
        }
    });
    changed
}

pub fn paint_kind_selector(ui: &mut Ui, kind: &mut FillKind) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ch |= ui.selectable_value(kind, FillKind::Solid, "Solid").clicked();
        ch |= ui
            .selectable_value(kind, FillKind::LinearGradient, "Linear")
            .clicked();
        ch |= ui
            .selectable_value(kind, FillKind::RadialGradient, "Radial")
            .clicked();
    });
    ch
}