//! AV / Media timeline UI: crop handles, DAW clips, piano roll, toolbar actions.

use egui::{Color32, Context, Rect, RichText, Ui};
use uuid::Uuid;

use crate::app::VadadeeBerryApp;
use crate::document::{AvClip, Layer, LayerKind, MusicClip};
use crate::icons;


/// Wider handles so trim start/end are easy to grab.
const TRIM_HANDLE_W: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvClipHit {
    #[default]
    None,
    TrimStart,
    TrimEnd,
    Body,
    MusicBody(Uuid),
    MusicTrimStart(Uuid),
    MusicTrimEnd(Uuid),
}

/// Sticky gesture so clip/trim drag does not "slip" into timeline scroll mid-drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AvDragMode {
    Move,
    TrimStart,
    TrimEnd,
}

#[derive(Debug, Clone, Copy)]
pub struct AvTimelineDrag {
    pub layer_idx: usize,
    pub clip_id: Uuid,
    pub is_music: bool,
    pub mode: AvDragMode,
    /// Clip timeline start (or DAW start) when the gesture began.
    pub origin_start_sec: f32,
    /// Play length / duration when the gesture began.
    pub origin_len_sec: f32,
    /// Source in-point when the gesture began (media only).
    pub origin_offset_sec: f32,
    /// Pointer X at press (screen).
    pub origin_pointer_x: f32,
    /// Track width at press (for sec-per-pixel).
    pub origin_track_w: f32,
    /// Visible time span in seconds at press.
    pub origin_visible_sec: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoTool {
    #[default]
    Add,
    Remove,
    Grab,
}

/// One row in the AV timeline = one ActionBar layer (`LayerKind::AV`).
/// Clips (video / audio / DAW) are a **queue on that row**, not separate rows.
#[derive(Clone)]
pub struct AvTimelineRow {
    pub layer_idx: usize,
    pub layer_id: Uuid,
    /// Layer name from the Layers tab (truncated for labels).
    pub layer_name: String,
    pub row_label: String,
    pub av_role: crate::document::AvRole,
    /// All media clips on this layer (queue order).
    pub av_clip_ids: Vec<Uuid>,
    /// All DAW / music clips on this layer.
    pub music_clip_ids: Vec<Uuid>,
}

pub fn collect_timeline_rows(layers: &[Layer]) -> Vec<AvTimelineRow> {
    let mut rows = Vec::new();
    for (idx, layer) in layers.iter().enumerate() {
        if layer.kind != LayerKind::AV {
            continue;
        }
        let mut layer = layer.clone();
        layer.ensure_av_clips();
        let role_tag = match layer.av_role {
            crate::document::AvRole::Video => "Video",
            crate::document::AvRole::Audio => "Audio",
            crate::document::AvRole::Daw => "DAW",
        };
        let n_media = layer.av_clips.len();
        let n_daw = layer.music_clips.len();
        let suffix = if n_media + n_daw == 0 {
            "(empty)".into()
        } else {
            format!("{n_media}+{n_daw} clips")
        };
        rows.push(AvTimelineRow {
            layer_idx: idx,
            layer_id: layer.id,
            layer_name: layer.name.clone(),
            row_label: format!("{} · {} · {}", role_tag, layer.name, suffix),
            av_role: layer.av_role,
            av_clip_ids: layer.av_clips.iter().map(|c| c.id).collect(),
            music_clip_ids: layer.music_clips.iter().map(|c| c.id).collect(),
        });
    }
    rows
}

/// Next timeline start for a new media clip: end of the queue on this layer.
pub fn queue_append_start_sec(layer: &Layer) -> f32 {
    let mut end = 0.0f32;
    for c in &layer.av_clips {
        end = end.max(c.timeline_end_secs());
    }
    for c in &layer.music_clips {
        end = end.max(c.end_sec());
    }
    end.max(0.0)
}

/// AV-only tools live on the main floating toolbar when an AV layer is selected.
/// Kept as no-op here so interior editor no longer shows misplaced Split/DAW buttons.
pub fn av_toolbar(_app: &mut VadadeeBerryApp, _ui: &mut Ui) {
    // Intentionally empty — Split / DAW are on the main toolbar when AV is active.
}

pub fn paint_trim_caps(
    painter: &egui::Painter,
    clip_rect: Rect,
    hovered: AvClipHit,
    color: Color32,
    stroke: Color32,
) {
    let cap_h = clip_rect.height();
    let left_cap = Rect::from_min_size(clip_rect.min, egui::vec2(TRIM_HANDLE_W, cap_h));
    let right_cap = Rect::from_min_size(
        egui::pos2(clip_rect.max.x - TRIM_HANDLE_W, clip_rect.min.y),
        egui::vec2(TRIM_HANDLE_W, cap_h),
    );

    let draw_cap = |r: Rect, active: bool| {
        if !active {
            return;
        }
        painter.rect(
            r,
            egui::CornerRadius {
                nw: 6,
                sw: 6,
                ne: 0,
                se: 0,
            },
            color.gamma_multiply(1.25),
            egui::Stroke::new(1.5, stroke),
            egui::StrokeKind::Inside,
        );
    };

    draw_cap(
        left_cap,
        matches!(hovered, AvClipHit::TrimStart | AvClipHit::MusicTrimStart(_)),
    );
    let right_radius = egui::CornerRadius {
        nw: 0,
        sw: 0,
        ne: 6,
        se: 6,
    };
    if matches!(hovered, AvClipHit::TrimEnd | AvClipHit::MusicTrimEnd(_)) {
        painter.rect(
            right_cap,
            right_radius,
            color.gamma_multiply(1.25),
            egui::Stroke::new(1.5, stroke),
            egui::StrokeKind::Inside,
        );
    }
}

pub fn hit_test_clip(clip_rect: Rect, pos: egui::Pos2, music_id: Option<Uuid>) -> AvClipHit {
    // Inflate slightly so short clips / edges are still hittable.
    let hit_rect = clip_rect.expand2(egui::vec2(2.0, 2.0));
    if !hit_rect.contains(pos) {
        return AvClipHit::None;
    }
    let handle = TRIM_HANDLE_W.min(clip_rect.width() * 0.35).max(10.0);
    let left = Rect::from_min_size(clip_rect.min, egui::vec2(handle, clip_rect.height())).expand2(egui::vec2(2.0, 2.0));
    let right = Rect::from_min_size(
        egui::pos2(clip_rect.max.x - handle, clip_rect.min.y),
        egui::vec2(handle, clip_rect.height()),
    )
    .expand2(egui::vec2(2.0, 2.0));
    let m_start = |id: Uuid| AvClipHit::MusicTrimStart(id);
    let m_end = |id: Uuid| AvClipHit::MusicTrimEnd(id);
    let m_body = |id: Uuid| AvClipHit::MusicBody(id);
    // Prefer trim handles when near edges (even if body also covers them).
    if left.contains(pos) {
        return if let Some(id) = music_id {
            m_start(id)
        } else {
            AvClipHit::TrimStart
        };
    }
    if right.contains(pos) {
        return if let Some(id) = music_id {
            m_end(id)
        } else {
            AvClipHit::TrimEnd
        };
    }
    if !clip_rect.contains(pos) {
        return AvClipHit::None;
    }
    if let Some(id) = music_id {
        m_body(id)
    } else {
        AvClipHit::Body
    }
}

/// Map sticky drag to a new start / length / offset.
pub fn apply_sticky_drag(
    drag: &AvTimelineDrag,
    pointer_x: f32,
) -> (f32, f32, f32) {
    let track_w = drag.origin_track_w.max(1.0);
    let dx_sec = (pointer_x - drag.origin_pointer_x) / track_w * drag.origin_visible_sec;
    match drag.mode {
        AvDragMode::Move => {
            let start = (drag.origin_start_sec + dx_sec).max(0.0);
            (start, drag.origin_len_sec, drag.origin_offset_sec)
        }
        AvDragMode::TrimStart => {
            // Positive dx → later start (shorter from left); negative → earlier.
            let mut start = drag.origin_start_sec + dx_sec;
            let mut len = drag.origin_len_sec - dx_sec;
            let mut offset = drag.origin_offset_sec + dx_sec;
            if start < 0.0 {
                len += start; // start is negative
                offset -= start; // undo offset for clamped region
                start = 0.0;
            }
            if offset < 0.0 {
                start -= offset;
                len += offset;
                offset = 0.0;
            }
            len = len.max(0.1);
            (start.max(0.0), len, offset.max(0.0))
        }
        AvDragMode::TrimEnd => {
            let len = (drag.origin_len_sec + dx_sec).max(0.1);
            (drag.origin_start_sec, len, drag.origin_offset_sec)
        }
    }
}

pub fn av_clip_rect(
    track_rect: Rect,
    clip: &AvClip,
    start_frame: f32,
    visible_frames: f32,
    fps: f32,
) -> Rect {
    let clip_start_frame = clip.video_timeline_start * fps;
    let clip_end_frame = clip.timeline_end_secs() * fps;
    let clip_start_x =
        track_rect.left() + ((clip_start_frame - start_frame) / visible_frames) * track_rect.width();
    let clip_end_x =
        track_rect.left() + ((clip_end_frame - start_frame) / visible_frames) * track_rect.width();
    Rect::from_min_max(
        egui::pos2(clip_start_x, track_rect.top() + 4.0),
        egui::pos2(clip_end_x.max(clip_start_x + 10.0), track_rect.bottom() - 4.0),
    )
}

pub fn piano_roll_panel(app: &mut VadadeeBerryApp, ui: &mut Ui, ctx: &Context) {
    let Some(clip_id) = app.piano_roll_clip else {
        return;
    };

    // Search all layers — DAW clips live on DAW-role layers, not only the active one.
    let Some((layer_idx, clip_name)) = app
        .project
        .document
        .layers
        .iter()
        .enumerate()
        .find_map(|(i, l)| {
            l.music_clips
                .iter()
                .find(|c| c.id == clip_id)
                .map(|c| (i, c.name.clone()))
        })
    else {
        app.piano_roll_clip = None;
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} DAW Piano — {}", icons::MUSIC, clip_name)).strong());
        ui.separator();
        if ui.selectable_label(app.piano_tool == PianoTool::Add, format!("{} Add", icons::ADD)).clicked() {
            app.piano_tool = PianoTool::Add;
        }
        if ui
            .selectable_label(app.piano_tool == PianoTool::Remove, format!("{} Remove", icons::REMOVE))
            .clicked()
        {
            app.piano_tool = PianoTool::Remove;
        }
        if ui.selectable_label(app.piano_tool == PianoTool::Grab, format!("{} Grab", icons::GRAB)).clicked() {
            app.piano_tool = PianoTool::Grab;
        }
        ui.label(RichText::new("Ctrl+Scroll = zoom | Scroll = pitch").small().weak());
        if ui.button(format!("{} Close", icons::CLOSE)).clicked() {
            app.piano_roll_clip = None;
        }
    });

    let clip_duration = app
        .project
        .document
        .layers
        .get(layer_idx)
        .and_then(|l| l.music_clips.iter().find(|c| c.id == clip_id))
        .map(|c| c.duration_sec)
        .unwrap_or(1.0);
    let ticks_visible = (clip_duration * app.anim_fps as f32 * app.piano_zoom).max(16.0) as u32;
    let row_h = 14.0;
    let keys_visible: i32 = 24;
    let grid_w = ui.available_width().max(200.0);
    let viewport_h = 160.0;

    egui::ScrollArea::vertical()
        .max_height(viewport_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (resp, painter) =
                ui.allocate_painter(egui::vec2(grid_w, keys_visible as f32 * row_h + 18.0), egui::Sense::click_and_drag());
            let rect = resp.rect;
            painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 20, 28));

            let grid_top = rect.top() + 18.0;
            let grid_rect = Rect::from_min_max(egui::pos2(rect.left() + 36.0, grid_top), rect.right_bottom());
            let pitch_base = 60 + app.piano_pitch_scroll as i32;

            for i in 0..=ticks_visible.min(64) {
                let x = grid_rect.left() + (i as f32 / ticks_visible as f32) * grid_rect.width();
                painter.line_segment(
                    [egui::pos2(x, grid_rect.top()), egui::pos2(x, grid_rect.bottom())],
                    egui::Stroke::new(1.0, Color32::from_gray(40)),
                );
            }
            for k in 0..keys_visible {
                let y = grid_rect.top() + k as f32 * row_h;
                let pitch = pitch_base + (keys_visible - 1 - k);
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                let key_bg = if is_black {
                    Color32::from_rgb(28, 30, 38)
                } else {
                    Color32::from_rgb(220, 222, 230)
                };
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(rect.left() + 2.0, y), egui::pos2(rect.left() + 34.0, y + row_h)),
                    1.0,
                    key_bg,
                );
                let label_col = if is_black {
                    Color32::from_gray(180)
                } else {
                    Color32::from_gray(40)
                };
                painter.text(
                    egui::pos2(rect.left() + 4.0, y + row_h * 0.5),
                    egui::Align2::LEFT_CENTER,
                    format!("{}", pitch.clamp(0, 127)),
                    egui::FontId::proportional(9.0),
                    label_col,
                );
                painter.line_segment(
                    [egui::pos2(grid_rect.left(), y), egui::pos2(grid_rect.right(), y)],
                    egui::Stroke::new(1.0, Color32::from_gray(35)),
                );
            }

            let clip_notes: Vec<_> = app
                .project
                .document
                .layers
                .get(layer_idx)
                .and_then(|l| l.music_clips.iter().find(|c| c.id == clip_id))
                .map(|c| c.notes.clone())
                .unwrap_or_default();
            for note in &clip_notes {
                let row = keys_visible - 1 - (note.pitch as i32 - pitch_base);
                if row < 0 || row >= keys_visible {
                    continue;
                }
                let x0 = grid_rect.left()
                    + (note.start_tick as f32 / ticks_visible as f32) * grid_rect.width();
                let x1 = grid_rect.left()
                    + ((note.start_tick + note.duration_ticks) as f32 / ticks_visible as f32) * grid_rect.width();
                let y_final = grid_rect.top() + row as f32 * row_h;
                let nr = Rect::from_min_max(
                    egui::pos2(x0, y_final + 1.0),
                    egui::pos2(x1.max(x0 + 4.0), y_final + row_h - 1.0),
                );
                painter.rect_filled(nr, 2.0, Color32::from_rgb(180, 90, 255));
            }

            if resp.dragged() && app.piano_tool == PianoTool::Grab {
                app.piano_scroll_offset += resp.drag_delta().x;
            }

            let scroll = ui.input(|i| i.smooth_scroll_delta);
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            if scroll.y != 0.0 && resp.hovered() {
                if ctrl {
                    app.piano_zoom = (app.piano_zoom * (1.0 - scroll.y * 0.002)).clamp(0.25, 8.0);
                } else {
                    app.piano_pitch_scroll = (app.piano_pitch_scroll - scroll.y * 0.05).clamp(0.0, 88.0);
                }
                ctx.request_repaint();
            }

            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    if grid_rect.contains(pos) {
                        match app.piano_tool {
                            PianoTool::Add => {
                                let rel_x = ((pos.x - grid_rect.left()) / grid_rect.width()).clamp(0.0, 1.0);
                                let tick = (rel_x * ticks_visible as f32) as u32;
                                let row = ((pos.y - grid_rect.top()) / row_h).floor() as i32;
                                let pitch = (pitch_base + keys_visible - 1 - row).clamp(0, 127) as u8;
                                if let Some(clip) = app
                                    .project
                                    .document
                                    .layers
                                    .get_mut(layer_idx)
                                    .and_then(|l| l.music_clips.iter_mut().find(|c| c.id == clip_id))
                                {
                                    clip.notes.push(crate::document::MusicNote::new(pitch, tick, 4));
                                }
                            }
                            PianoTool::Remove => {
                                if let Some(clip) = app
                                    .project
                                    .document
                                    .layers
                                    .get_mut(layer_idx)
                                    .and_then(|l| l.music_clips.iter_mut().find(|c| c.id == clip_id))
                                {
                                    clip.notes.retain(|n| {
                                        let row = keys_visible - 1 - (n.pitch as i32 - pitch_base);
                                        if row < 0 || row >= keys_visible {
                                            return true;
                                        }
                                        let y_final = grid_rect.top() + row as f32 * row_h;
                                        let y1 = y_final + row_h;
                                        !(pos.y >= y_final && pos.y < y1)
                                    });
                                }
                            }
                            PianoTool::Grab => {}
                        }
                    }
                }
            }
        });
}

pub fn music_clip_rect(
    track_rect: Rect,
    clip: &MusicClip,
    start_frame: f32,
    visible_frames: f32,
    fps: f32,
) -> Rect {
    let clip_start_frame = clip.timeline_start_sec * fps;
    let clip_end_frame = clip.end_sec() * fps;
    let clip_start_x =
        track_rect.left() + ((clip_start_frame - start_frame) / visible_frames) * track_rect.width();
    let clip_end_x =
        track_rect.left() + ((clip_end_frame - start_frame) / visible_frames) * track_rect.width();
    Rect::from_min_max(
        egui::pos2(clip_start_x, track_rect.top() + 4.0),
        egui::pos2(clip_end_x.max(clip_start_x + 10.0), track_rect.bottom() - 4.0),
    )
}