use serde::{Deserialize, Serialize};
use uuid::Uuid;

const AUDIO_EXTS: &[&str] = &["mp3", "wav", "aac", "m4a", "flac", "ogg", "opus", "wma"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvClip {
    pub id: Uuid,
    pub name: String,
    pub media_path: String,
    pub video_start_offset: f32,
    pub video_play_length: f32,
    pub video_timeline_start: f32,
    #[serde(default)]
    pub media_source_duration: Option<f32>,
    /// Sub-track row inside the parent AV layer (avoids overlap on the same row).
    #[serde(default)]
    pub track_row: u32,
}

impl AvClip {
    pub fn new_from_media(name: impl Into<String>, path: impl Into<String>, timeline_start: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            media_path: path.into(),
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: timeline_start,
            media_source_duration: None,
            track_row: 0,
        }
    }

    pub fn new_empty(name: impl Into<String>, timeline_start: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            media_path: String::new(),
            video_start_offset: 0.0,
            video_play_length: 1.0,
            video_timeline_start: timeline_start,
            media_source_duration: None,
            track_row: 0,
        }
    }

    pub fn from_legacy(
        id: Uuid,
        name: String,
        media_path: String,
        video_start_offset: f32,
        video_play_length: f32,
        video_timeline_start: f32,
        media_source_duration: Option<f32>,
    ) -> Self {
        Self {
            id,
            name,
            media_path,
            video_start_offset,
            video_play_length,
            video_timeline_start,
            media_source_duration,
            track_row: 0,
        }
    }

    pub fn path_is_audio_only(path: &str) -> bool {
        if path.is_empty() {
            return true;
        }
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        AUDIO_EXTS.iter().any(|a| *a == ext)
    }

    pub fn is_audio_only(&self) -> bool {
        Self::path_is_audio_only(&self.media_path)
    }

    /// True when playhead time is inside this clip's timeline span [start, end).
    pub fn contains_timeline_sec(&self, t: f32) -> bool {
        t >= self.video_timeline_start && t < self.timeline_end_secs()
    }

    pub fn timeline_play_secs(&self) -> f32 {
        let source_cap = self
            .media_source_duration
            .unwrap_or(self.video_play_length)
            .max(0.0);
        // Media available after in-point (trim start).
        let remaining = (source_cap - self.video_start_offset.max(0.0)).max(0.0);
        if self.video_play_length >= 3599.0 {
            return remaining;
        }
        self.video_play_length.min(remaining).max(0.0)
    }

    pub fn timeline_end_secs(&self) -> f32 {
        self.video_timeline_start + self.timeline_play_secs()
    }
}

/// Pick the lowest sub-track row with no time overlap against existing clips.
pub fn assign_free_track_row(
    av_clips: &[AvClip],
    music_clips: &[super::MusicClip],
    start_sec: f32,
    end_sec: f32,
) -> u32 {
    let mut row = 0u32;
    loop {
        let av_overlap = av_clips.iter().any(|c| {
            c.track_row == row && ranges_overlap(start_sec, end_sec, c.video_timeline_start, c.timeline_end_secs())
        });
        let music_overlap = music_clips.iter().any(|c| {
            c.track_row == row && ranges_overlap(start_sec, end_sec, c.timeline_start_sec, c.end_sec())
        });
        if !av_overlap && !music_overlap {
            return row;
        }
        row += 1;
    }
}

fn ranges_overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> bool {
    a0 < b1 && b0 < a1
}