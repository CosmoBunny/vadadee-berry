//! Video API (Phase 10): AV clip orchestration on existing media.
//!
//! Lua shape:
//! ```lua
//! local clips = vblua.video.clips()           -- [{id, name, kind, start, length, row}]
//! vblua.video.move(id, 12.5)                  -- retime on the timeline
//! vblua.video.trim(id, { offset = 2.0, length = 8.0 })
//! local right = vblua.video.split(id, 14.0)   -- -> second clip id
//! vblua.video.remove(id) / rename(id, "Intro")
//! ```
//!
//! Deliberate limits (sandbox + honesty):
//! - `add_clip(path)` is DENIED: new media references need the Asset API
//!   (Phase 11) + platform picker (Phase 12), never Lua path strings.
//! - `media_path` is never exposed (host path layout stays host-side);
//!   scripts see `kind` (`video`/`audio`/`image`) + cached `duration`.
//! - No speed/reverse/transitions: the host `AvClip` has no such fields —
//!   faking them would lie. They arrive with the host model.
//! - Lua orchestrates (times/rows/splits); Rust decodes (untouched here).

use crate::document::{Document, LayerKind};

/// Timeline bounds guard (10h — matches the host's 3600s "full length" idiom).
pub const MAX_TIME_SEC: f32 = 36_000.0;
pub const MIN_CLIP_SEC: f32 = 0.1;

/// Read-only clip row. `media_path` is intentionally absent.
#[derive(Debug, Clone)]
pub struct ClipSnapshot {
    pub id: String,
    pub layer_id: String,
    pub name: String,
    /// `video` | `audio` | `image` | `empty`.
    pub kind: String,
    pub timeline_start: f32,
    pub play_length: f32,
    pub start_offset: f32,
    pub track_row: u32,
    /// Cached source duration, if the host has probed it.
    pub duration: Option<f32>,
    pub muted: bool,
    pub locked: bool,
}

/// Read-only timeline marker row.
#[derive(Debug, Clone)]
pub struct MarkerSnapshot {
    pub id: String,
    pub name: String,
    pub time: f32,
}

/// AV layers and their clips.
#[derive(Debug, Clone, Default)]
pub struct VideoSnapshot {
    pub layers: Vec<VideoLayerSnapshot>,
    pub active_layer_id: Option<String>,
    pub markers: Vec<MarkerSnapshot>,
}

/// Read-only per-layer clip list.
#[derive(Debug, Clone, Default)]
pub struct VideoLayerSnapshot {
    pub layer_id: String,
    pub layer_name: String,
    pub clips: Vec<ClipSnapshot>,
}

impl VideoSnapshot {
    pub fn capture(doc: &Document) -> Self {
        let layers = doc
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::AV)
            .map(|l| VideoLayerSnapshot {
                layer_id: l.id.to_string(),
                layer_name: l.name.clone(),
                clips: l
                    .av_clips
                    .iter()
                    .map(|c| ClipSnapshot {
                        id: c.id.to_string(),
                        layer_id: l.id.to_string(),
                        name: c.name.clone(),
                        kind: if c.media_path.is_empty() {
                            "empty"
                        } else if c.is_audio_only() {
                            "audio"
                        } else if c.is_still_image() {
                            "image"
                        } else {
                            "video"
                        }
                        .to_string(),
                        timeline_start: c.video_timeline_start,
                        play_length: c.video_play_length,
                        start_offset: c.video_start_offset,
                        track_row: c.track_row,
                        duration: c.media_source_duration,
                        muted: c.muted,
                        locked: c.locked,
                    })
                    .collect(),
            })
            .collect();
        Self {
            layers,
            active_layer_id: doc
                .layers
                .get(doc.active_layer_index)
                .map(|l| l.id.to_string()),
            markers: doc
                .timeline_markers
                .iter()
                .map(|m| MarkerSnapshot {
                    id: m.id.to_string(),
                    name: m.name.clone(),
                    time: m.time_sec,
                })
                .collect(),
        }
    }

    pub fn find_clip(&self, id: &str) -> Option<(usize, usize)> {
        for (li, l) in self.layers.iter().enumerate() {
            for (ci, c) in l.clips.iter().enumerate() {
                if c.id == id {
                    return Some((li, ci));
                }
            }
        }
        None
    }

    pub fn find_marker(&self, id: &str) -> Option<usize> {
        self.markers.iter().position(|m| m.id == id)
    }

    pub(crate) fn sort_markers(&mut self) {
        self.markers.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn clamp_time(t: f32) -> f32 {
    if !t.is_finite() {
        0.0
    } else {
        t.clamp(0.0, MAX_TIME_SEC)
    }
}

fn clamp_len(l: f32) -> f32 {
    if !l.is_finite() {
        MIN_CLIP_SEC
    } else {
        l.clamp(MIN_CLIP_SEC, MAX_TIME_SEC)
    }
}

/// One staged clip mutation.
#[derive(Debug, Clone)]
pub enum VideoCommand {
    Move {
        clip_id: uuid::Uuid,
        timeline_start: f32,
    },
    Trim {
        clip_id: uuid::Uuid,
        start_offset: f32,
        play_length: f32,
    },
    SetRow {
        clip_id: uuid::Uuid,
        row: u32,
    },
    Rename {
        clip_id: uuid::Uuid,
        name: String,
    },
    SetMuted {
        clip_id: uuid::Uuid,
        muted: bool,
    },
    SetLocked {
        clip_id: uuid::Uuid,
        locked: bool,
    },
    Duplicate {
        clip_id: uuid::Uuid,
        new_id: uuid::Uuid,
    },
    RippleDelete {
        clip_id: uuid::Uuid,
    },
    Remove {
        clip_id: uuid::Uuid,
    },
    Split {
        clip_id: uuid::Uuid,
        new_id: uuid::Uuid,
        at_sec: f32,
    },
    AddMarker {
        marker_id: uuid::Uuid,
        name: String,
        time: f32,
    },
    RenameMarker {
        marker_id: uuid::Uuid,
        name: String,
    },
    MoveMarker {
        marker_id: uuid::Uuid,
        time: f32,
    },
    RemoveMarker {
        marker_id: uuid::Uuid,
    },
}

impl VideoCommand {
    /// Apply one command; `true` when the document changed.
    pub fn apply_to(&self, doc: &mut Document) -> bool {
        match self {
            VideoCommand::Move {
                clip_id,
                timeline_start,
            } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        if c.locked {
                            return false;
                        }
                        c.video_timeline_start = clamp_time(*timeline_start);
                        layer.sync_legacy_from_primary_clip();
                        return true;
                    }
                }
                false
            }
            VideoCommand::Trim {
                clip_id,
                start_offset,
                play_length,
            } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        if c.locked {
                            return false;
                        }
                        c.video_start_offset = start_offset.max(0.0);
                        c.video_play_length = clamp_len(*play_length);
                        layer.sync_legacy_from_primary_clip();
                        return true;
                    }
                }
                false
            }
            VideoCommand::SetRow { clip_id, row } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        c.track_row = (*row).min(64);
                        return true;
                    }
                }
                false
            }
            VideoCommand::Rename { clip_id, name } => {
                if name.is_empty() {
                    return false;
                }
                let name: String = name.chars().take(128).collect();
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        c.name = name.clone();
                        return true;
                    }
                }
                false
            }
            VideoCommand::SetMuted { clip_id, muted } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        c.muted = *muted;
                        return true;
                    }
                }
                false
            }
            VideoCommand::SetLocked { clip_id, locked } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(c) = layer.av_clips.iter_mut().find(|c| c.id == *clip_id) {
                        c.locked = *locked;
                        return true;
                    }
                }
                false
            }
            VideoCommand::Duplicate { clip_id, new_id } => {
                for layer in doc.layers.iter_mut() {
                    let Some(pos) = layer.av_clips.iter().position(|c| c.id == *clip_id) else {
                        continue;
                    };
                    if layer.av_clips[pos].locked {
                        return false;
                    }
                    let mut copy = layer.av_clips[pos].clone();
                    copy.id = *new_id;
                    copy.name =
                        format!("{} copy", copy.name.chars().take(100).collect::<String>());
                    copy.video_timeline_start = copy.timeline_end_secs();
                    layer.av_clips.insert(pos + 1, copy);
                    layer.sync_legacy_from_primary_clip();
                    return true;
                }
                false
            }
            VideoCommand::RippleDelete { clip_id } => {
                for layer in doc.layers.iter_mut() {
                    let Some(pos) =
                        layer.av_clips.iter().position(|c| c.id == *clip_id)
                    else {
                        continue;
                    };
                    if layer.av_clips[pos].locked {
                        return false;
                    }
                    let gone = layer.av_clips[pos].clone();
                    let span = gone.timeline_play_secs().max(0.0);
                    let cut_at = gone.video_timeline_start;
                    layer.av_clips.remove(pos);
                    for c in layer.av_clips.iter_mut() {
                        if c.track_row == gone.track_row
                            && c.video_timeline_start >= cut_at
                        {
                            c.video_timeline_start =
                                (c.video_timeline_start - span).max(0.0);
                        }
                    }
                    if layer.av_clips.is_empty() {
                        layer.video_path.clear();
                        layer.media_source_duration = None;
                    } else {
                        layer.sync_legacy_from_primary_clip();
                    }
                    return true;
                }
                false
            }
            VideoCommand::Remove { clip_id } => {
                for layer in doc.layers.iter_mut() {
                    if layer.av_clips.iter().any(|c| c.id == *clip_id && c.locked) {
                        return false;
                    }
                    let before = layer.av_clips.len();
                    layer.av_clips.retain(|c| c.id != *clip_id);
                    if layer.av_clips.len() != before {
                        layer.sync_legacy_from_primary_clip();
                        return true;
                    }
                }
                false
            }
            VideoCommand::Split {
                clip_id,
                new_id,
                at_sec,
            } => {
                for layer in doc.layers.iter_mut() {
                    let pos = layer.av_clips.iter().position(|c| c.id == *clip_id);
                    let Some(idx) = pos else { continue };
                    let clip = layer.av_clips[idx].clone();
                    if clip.locked {
                        return false;
                    }
                    // Mirror the host split guard: strictly inside the span.
                    if !(*at_sec > clip.video_timeline_start + MIN_CLIP_SEC
                        && *at_sec < clip.timeline_end_secs() - MIN_CLIP_SEC)
                    {
                        return false;
                    }
                    let left_len = at_sec - clip.video_timeline_start;
                    let right_len = clip.timeline_play_secs() - left_len;
                    let mut right = clip.clone();
                    right.id = *new_id;
                    right.name = format!("{} (split)", clip.name.chars().take(100).collect::<String>());
                    right.video_timeline_start = *at_sec;
                    right.video_start_offset += left_len;
                    right.video_play_length = right_len.max(MIN_CLIP_SEC);
                    if let Some(c) = layer.av_clips.get_mut(idx) {
                        c.video_play_length = left_len.max(MIN_CLIP_SEC);
                    }
                    layer.av_clips.insert(idx + 1, right);
                    layer.sync_legacy_from_primary_clip();
                    return true;
                }
                false
            }
            VideoCommand::AddMarker { marker_id, name, time } => {
                if doc.timeline_markers.iter().any(|m| m.id == *marker_id) {
                    return false;
                }
                doc.timeline_markers.push(
                    crate::document::TimelineMarker::with_id(*marker_id, name.clone(), *time),
                );
                doc.sort_timeline_markers();
                true
            }
            VideoCommand::RenameMarker { marker_id, name } => {
                doc.rename_timeline_marker(*marker_id, name)
            }
            VideoCommand::MoveMarker { marker_id, time } => {
                doc.move_timeline_marker(*marker_id, *time)
            }
            VideoCommand::RemoveMarker { marker_id } => {
                doc.remove_timeline_marker(*marker_id)
            }
        }
    }
}

/// Snapshot-side split preview: validates + returns the derived halves so the
/// Lua call gets immediate errors and the optimistic snapshot stays exact.
pub fn preview_split(
    clip: &ClipSnapshot,
    at_sec: f32,
) -> Result<(f32, f32, f32, f32), String> {
    let t = clamp_time(at_sec);
    if !(t > clip.timeline_start + MIN_CLIP_SEC) {
        return Err("split point must be inside the clip (after its start)".to_string());
    }
    // End from snapshot fields (play_length capped like the host accessor).
    let end = clip.timeline_start + clip.play_length;
    if !(t < end - MIN_CLIP_SEC) {
        return Err("split point must be inside the clip (before its end)".to_string());
    }
    let left_len = (t - clip.timeline_start).max(MIN_CLIP_SEC);
    let right_len = (end - t).max(MIN_CLIP_SEC);
    Ok((left_len, t, clip.start_offset + left_len, right_len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AvClip;
    fn av_doc() -> Document {
        let mut doc = Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![crate::document::Layer::new_av_layer(
                uuid::Uuid::new_v4(),
                "AV".into(),
                String::new(),
            )],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        };
        doc.layers[0].av_clips.push(AvClip {
            id: uuid::Uuid::new_v4(),
            name: "Clip".into(),
            media_path: "take.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: 20.0,
            video_timeline_start: 5.0,
            media_source_duration: Some(30.0),
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        });
        doc
    }

    #[test]
    fn snapshot_hides_media_path() {
        let doc = av_doc();
        let snap = VideoSnapshot::capture(&doc);
        assert_eq!(snap.layers.len(), 1);
        let c = &snap.layers[0].clips[0];
        assert_eq!(c.kind, "video");
        assert_eq!(c.duration, Some(30.0));
    }

    #[test]
    fn move_trim_split_remove_round_trip() {
        let mut doc = av_doc();
        let id = doc.layers[0].av_clips[0].id;
        assert!(VideoCommand::Move {
            clip_id: id,
            timeline_start: 10.0,
        }
        .apply_to(&mut doc));
        assert!(VideoCommand::Trim {
            clip_id: id,
            start_offset: 2.0,
            play_length: 8.0,
        }
        .apply_to(&mut doc));
        let c = &doc.layers[0].av_clips[0];
        assert_eq!((c.video_timeline_start, c.video_start_offset), (10.0, 2.0));
        let right = uuid::Uuid::new_v4();
        assert!(VideoCommand::Split {
            clip_id: id,
            new_id: right,
            at_sec: 14.0,
        }
        .apply_to(&mut doc));
        assert_eq!(doc.layers[0].av_clips.len(), 2);
        // Outside-span split is a no-op.
        assert!(!VideoCommand::Split {
            clip_id: id,
            new_id: uuid::Uuid::new_v4(),
            at_sec: 500.0,
        }
        .apply_to(&mut doc));
        assert!(VideoCommand::Remove {
            clip_id: right,
        }
        .apply_to(&mut doc));
        assert_eq!(doc.layers[0].av_clips.len(), 1);
    }
}
